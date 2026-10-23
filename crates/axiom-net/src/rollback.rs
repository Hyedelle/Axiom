//! 16-Player Distributed P2P Rollback Netcode Engine (T-161).
//!
//! Features:
//! - 0-Float, 64-Byte L1 cache aligned structures.
//! - 8-Frame sliding window redundant input delivery (loss-tolerant).
//! - Ring-buffered input history with heuristic repeat prediction.
//! - Instantaneous speculative rollback resimulation upon delayed/divergent remote inputs.
//! - FNV-1a state consensus hash tree verification with real-time desync detection.

use crate::packet::{NetPacket64, INPUT_WINDOW_FRAMES, MAX_PEERS, NET_WIRE_MAGIC};
use crate::telemetry::NetTelemetry64;

pub const DEFAULT_NET_BUFFER_FRAMES: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetError {
    InvalidMagic,
    InvalidSenderId,
    BufferOverflow,
    DesyncDetected { frame: u32, local_hash: u64, remote_hash: u64 },
    NatPunchBlocked { sender_id: u8 },
}

/// Result metrics returned after a simulation step or packet delivery.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RollbackStepResult {
    pub current_frame: u32,
    pub rolled_back: bool,
    pub rollback_depth: u32,
    pub active_peers: u16,
    pub current_state_hash: u64,
}

/// 16-Player Distributed P2P Rollback Netcode Session.
#[derive(Debug, Clone)]
pub struct DistributedRollbackSession<const BUFFER_FRAMES: usize = DEFAULT_NET_BUFFER_FRAMES> {
    pub local_peer_id: usize,
    pub current_frame: u32,
    pub min_confirmed_frame: u32,
    pub has_confirmed_frame: bool,
    pub active_peers_mask: u16,
    pub expected_peers_mask: u16,
    pub seq_counter: u32,

    // Input ring buffers: [peer_id][slot]
    pub input_matrix: [[u16; BUFFER_FRAMES]; MAX_PEERS],
    pub input_frame_tags: [[u32; BUFFER_FRAMES]; MAX_PEERS],
    pub is_confirmed: [[bool; BUFFER_FRAMES]; MAX_PEERS],
    pub last_confirmed_frames: [u32; MAX_PEERS],

    // State hash ring buffer: [slot] -> 64-bit FNV-1a hash
    pub state_hashes: [u64; BUFFER_FRAMES],

    pub telemetry: NetTelemetry64,
    pub nat_session: Option<crate::nat::NatTraversalSession<MAX_PEERS>>,
}

impl<const BUFFER_FRAMES: usize> DistributedRollbackSession<BUFFER_FRAMES> {
    pub fn new(local_peer_id: usize) -> Self {
        assert!(local_peer_id < MAX_PEERS, "local_peer_id must be < 16");

        let mut session = Self {
            local_peer_id,
            current_frame: 0,
            min_confirmed_frame: 0,
            has_confirmed_frame: false,
            active_peers_mask: 1 << local_peer_id,
            expected_peers_mask: 0,
            seq_counter: 0,
            input_matrix: [[0u16; BUFFER_FRAMES]; MAX_PEERS],
            input_frame_tags: [[u32::MAX; BUFFER_FRAMES]; MAX_PEERS],
            is_confirmed: [[false; BUFFER_FRAMES]; MAX_PEERS],
            last_confirmed_frames: [0u32; MAX_PEERS],
            state_hashes: [0u64; BUFFER_FRAMES],
            telemetry: NetTelemetry64::default(),
            nat_session: None,
        };
        session.telemetry.active_peers = 1;
        session
    }

    /// Creates a session with an explicit set of expected participating peers.
    pub fn with_expected_peers(local_peer_id: usize, expected_peers_mask: u16) -> Self {
        let mut session = Self::new(local_peer_id);
        session.expected_peers_mask = expected_peers_mask;
        session
    }

    /// Attaches a NAT traversal session to filter wire packets based on direct hole-punched connectivity.
    pub fn enable_nat_traversal(&mut self, local_candidate: crate::nat::NatCandidate64) {
        self.nat_session = Some(crate::nat::NatTraversalSession::new(self.local_peer_id as u8, local_candidate));
    }

    /// Returns true if direct P2P connectivity with the given peer has been established.
    pub fn is_peer_p2p_ready(&self, peer_id: usize) -> bool {
        if let Some(ref nat) = self.nat_session {
            nat.is_peer_connected(peer_id)
        } else {
            true // Direct mode assumes ready
        }
    }

    /// Submits local input for current frame and generates wire packet.
    pub fn submit_local_input(&mut self, input: u16) -> NetPacket64 {
        let frame = self.current_frame;
        let slot = (frame as usize) % BUFFER_FRAMES;

        self.input_matrix[self.local_peer_id][slot] = input;
        self.input_frame_tags[self.local_peer_id][slot] = frame;
        self.is_confirmed[self.local_peer_id][slot] = true;
        self.last_confirmed_frames[self.local_peer_id] = frame;

        // Build 8-frame sliding window of redundant inputs: [F, F-1, F-2, ..., F-7]
        let mut window = [0u16; INPUT_WINDOW_FRAMES];
        for i in 0..INPUT_WINDOW_FRAMES {
            if frame >= i as u32 {
                let past_frame = frame - (i as u32);
                let p_slot = (past_frame as usize) % BUFFER_FRAMES;
                window[i] = self.input_matrix[self.local_peer_id][p_slot];
            }
        }

        self.seq_counter = self.seq_counter.wrapping_add(1);
        self.telemetry.total_packets_sent += 1;

        self.update_min_confirmed_frame();

        let (ack_frame, state_hash) = if self.has_confirmed_frame {
            let state_slot = (self.min_confirmed_frame as usize) % BUFFER_FRAMES;
            (self.min_confirmed_frame, self.state_hashes[state_slot])
        } else {
            (0, 0)
        };

        let mut peer_acks = [0u8; MAX_PEERS];
        for p in 0..MAX_PEERS {
            let offset = frame.saturating_sub(self.last_confirmed_frames[p]);
            peer_acks[p] = offset.min(255) as u8;
        }

        NetPacket64 {
            magic: NET_WIRE_MAGIC,
            sender_id: self.local_peer_id as u8,
            packet_flags: 0,
            sequence: self.seq_counter,
            current_frame: frame,
            ack_frame,
            state_hash,
            rtt_timestamp: frame,
            loss_permille: self.telemetry.observed_packet_loss_permille,
            active_peers_mask: self.active_peers_mask,
            window_inputs: window,
            peer_acks,
        }
    }

    /// Receives a remote wire packet and integrates redundant inputs.
    /// Returns Ok(Some(earliest_invalidated_frame)) if rollback is required, Ok(None) if not.
    pub fn receive_remote_packet(&mut self, pkt: &NetPacket64) -> Result<Option<u32>, NetError> {
        if pkt.magic != NET_WIRE_MAGIC {
            return Err(NetError::InvalidMagic);
        }
        let sender = pkt.sender_id as usize;
        if sender >= MAX_PEERS || sender == self.local_peer_id {
            return Err(NetError::InvalidSenderId);
        }

        if let Some(ref nat) = self.nat_session {
            if !nat.is_peer_connected(sender) {
                return Err(NetError::NatPunchBlocked { sender_id: pkt.sender_id });
            }
        }

        self.telemetry.total_packets_received += 1;
        self.active_peers_mask |= 1 << sender;
        self.telemetry.active_peers = self.active_peers_mask.count_ones() as u16;

        let mut earliest_divergence: Option<u32> = None;

        // Ingest the 8-frame sliding window
        for i in 0..INPUT_WINDOW_FRAMES {
            if pkt.current_frame < i as u32 {
                break;
            }
            let frame = pkt.current_frame - (i as u32);
            let input = pkt.window_inputs[i];
            let slot = (frame as usize) % BUFFER_FRAMES;

            let already_confirmed = self.is_confirmed[sender][slot] && self.input_frame_tags[sender][slot] == frame;

            if !already_confirmed {
                // If frame <= current_frame, check if our predicted input differed
                if frame <= self.current_frame {
                    let old_predicted = self.input_matrix[sender][slot];
                    if old_predicted != input {
                        earliest_divergence = Some(match earliest_divergence {
                            Some(e) => e.min(frame),
                            None => frame,
                        });
                    }
                }

                self.input_matrix[sender][slot] = input;
                self.input_frame_tags[sender][slot] = frame;
                self.is_confirmed[sender][slot] = true;
                if frame > self.last_confirmed_frames[sender] {
                    self.last_confirmed_frames[sender] = frame;
                }
            }
        }

        self.update_min_confirmed_frame();

        // Check state hash consensus for confirmed frame within active ring buffer window
        let frame_age = self.current_frame.saturating_sub(pkt.ack_frame);
        if self.has_confirmed_frame
            && pkt.ack_frame <= self.min_confirmed_frame
            && frame_age < BUFFER_FRAMES as u32
            && pkt.state_hash != 0
            && (earliest_divergence.is_none() || earliest_divergence.unwrap() > pkt.ack_frame)
        {
            let chk_slot = (pkt.ack_frame as usize) % BUFFER_FRAMES;
            let local_hash = self.state_hashes[chk_slot];
            if local_hash != 0 && local_hash != pkt.state_hash {
                self.telemetry.total_desyncs += 1;
                return Err(NetError::DesyncDetected {
                    frame: pkt.ack_frame,
                    local_hash,
                    remote_hash: pkt.state_hash,
                });
            }
        }

        Ok(earliest_divergence)
    }

    /// Verifies remote state hash consensus for an acknowledged frame after resimulation.
    pub fn verify_remote_consensus(&mut self, pkt: &NetPacket64) -> Result<(), NetError> {
        let frame_age = self.current_frame.saturating_sub(pkt.ack_frame);
        if self.has_confirmed_frame
            && pkt.ack_frame <= self.min_confirmed_frame
            && frame_age < BUFFER_FRAMES as u32
            && pkt.state_hash != 0
        {
            let chk_slot = (pkt.ack_frame as usize) % BUFFER_FRAMES;
            let local_hash = self.state_hashes[chk_slot];
            if local_hash != 0 && local_hash != pkt.state_hash {
                self.telemetry.total_desyncs += 1;
                return Err(NetError::DesyncDetected {
                    frame: pkt.ack_frame,
                    local_hash,
                    remote_hash: pkt.state_hash,
                });
            }
        }
        Ok(())
    }

    /// Advances 1 local simulation frame with heuristic input prediction and rollback support.
    pub fn advance_frame<F>(&mut self, mut sim_step: F) -> RollbackStepResult
    where
        F: FnMut(u32, &[u16; MAX_PEERS]) -> u64,
    {
        let frame = self.current_frame;

        // Gather all inputs for `frame` (using actual confirmed or heuristic prediction)
        let mut frame_inputs = [0u16; MAX_PEERS];
        let mask = if self.expected_peers_mask != 0 {
            self.expected_peers_mask
        } else {
            self.active_peers_mask
        };
        for p in 0..MAX_PEERS {
            if (mask & (1 << p)) == 0 {
                continue;
            }
            let slot = (frame as usize) % BUFFER_FRAMES;
            if self.is_confirmed[p][slot] && self.input_frame_tags[p][slot] == frame {
                frame_inputs[p] = self.input_matrix[p][slot];
            } else {
                // Heuristic prediction: repeat last confirmed frame's input
                let last_f = self.last_confirmed_frames[p];
                let prev_slot = (last_f as usize) % BUFFER_FRAMES;
                let predicted = self.input_matrix[p][prev_slot];
                self.input_matrix[p][slot] = predicted;
                self.input_frame_tags[p][slot] = frame;
                self.is_confirmed[p][slot] = false;
                frame_inputs[p] = predicted;
            }
        }

        // Execute deterministic simulation step
        let hash = sim_step(frame, &frame_inputs);
        let slot = (frame as usize) % BUFFER_FRAMES;
        self.state_hashes[slot] = hash;

        self.current_frame += 1;
        self.update_min_confirmed_frame();

        RollbackStepResult {
            current_frame: self.current_frame,
            rolled_back: false,
            rollback_depth: 0,
            active_peers: self.active_peers_mask,
            current_state_hash: hash,
        }
    }

    /// Rewinds and resimulates frames from `from_frame` up to `self.current_frame`.
    pub fn rollback_and_resimulate<F>(&mut self, from_frame: u32, mut sim_step: F) -> RollbackStepResult
    where
        F: FnMut(u32, &[u16; MAX_PEERS]) -> u64,
    {
        let depth = self.current_frame.saturating_sub(from_frame);
        if depth == 0 {
            let slot = (self.current_frame.saturating_sub(1) as usize) % BUFFER_FRAMES;
            return RollbackStepResult {
                current_frame: self.current_frame,
                rolled_back: false,
                rollback_depth: 0,
                active_peers: self.active_peers_mask,
                current_state_hash: self.state_hashes[slot],
            };
        }

        self.telemetry.total_rollbacks_triggered += 1;
        self.telemetry.total_resimulated_frames += depth;
        self.telemetry.max_rollback_depth = self.telemetry.max_rollback_depth.max(depth as u16);

        let mut last_hash = 0u64;
        for f in from_frame..self.current_frame {
            let mut inputs = [0u16; MAX_PEERS];
            let slot = (f as usize) % BUFFER_FRAMES;
            for p in 0..MAX_PEERS {
                if !self.is_confirmed[p][slot] || self.input_frame_tags[p][slot] != f {
                    let prev_slot = (f.saturating_sub(1) as usize) % BUFFER_FRAMES;
                    let predicted = self.input_matrix[p][prev_slot];
                    self.input_matrix[p][slot] = predicted;
                    self.input_frame_tags[p][slot] = f;
                    self.is_confirmed[p][slot] = false;
                }
                inputs[p] = self.input_matrix[p][slot];
            }
            last_hash = sim_step(f, &inputs);
            self.state_hashes[slot] = last_hash;
        }

        self.update_min_confirmed_frame();

        RollbackStepResult {
            current_frame: self.current_frame,
            rolled_back: true,
            rollback_depth: depth,
            active_peers: self.active_peers_mask,
            current_state_hash: last_hash,
        }
    }

    fn update_min_confirmed_frame(&mut self) {
        if self.current_frame == 0 {
            return;
        }

        let mask = if self.expected_peers_mask != 0 {
            self.expected_peers_mask
        } else {
            if self.active_peers_mask.count_ones() < 2 {
                return;
            }
            self.active_peers_mask
        };

        if !self.has_confirmed_frame {
            let mut all_frame0 = true;
            for p in 0..MAX_PEERS {
                if (mask & (1 << p)) != 0 {
                    let slot0 = 0 % BUFFER_FRAMES;
                    if !(self.is_confirmed[p][slot0] && self.input_frame_tags[p][slot0] == 0) {
                        all_frame0 = false;
                        break;
                    }
                }
            }
            if all_frame0 {
                self.has_confirmed_frame = true;
                self.min_confirmed_frame = 0;
            } else {
                return;
            }
        }

        while self.min_confirmed_frame + 1 < self.current_frame {
            let next_f = self.min_confirmed_frame + 1;
            let next_slot = (next_f as usize) % BUFFER_FRAMES;
            let mut all_next = true;
            for p in 0..MAX_PEERS {
                if (mask & (1 << p)) != 0 {
                    if !(self.is_confirmed[p][next_slot] && self.input_frame_tags[p][next_slot] == next_f) {
                        all_next = false;
                        break;
                    }
                }
            }
            if all_next {
                self.min_confirmed_frame = next_f;
            } else {
                break;
            }
        }
    }
}
