//! Deterministic Rollback Netcode Engine for 2-Player P2P Mecha Boxing Ring (T-099).
//!
//! Provides zero-input-delay client prediction, ring-buffered action queues,
//! and instantaneous speculative rollback resimulation across simulated network latency,
//! jitter, and packet loss.

use crate::boxing::{ActionKind, BoxingRingEngine};

pub const DEFAULT_BOXING_BUFFER_SIZE: usize = 64;
pub const MAX_SUPPORTED_BOXING_PLAYERS: usize = 2;
pub const NETBOX_WIRE_MAGIC: u8 = 0x58; // 'X' (Axiom wire protocol marker)

/// 18-byte packed POD binary packet for WebRTC DataChannel (Unreliable / Unordered).
/// Fits within a single 64B L1 cache line (3 packets per cache line) with zero allocations.
#[repr(C, packed)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NetboxWirePacket {
    pub magic: u8,              // 0x58 ('X')
    pub sender_id: u8,          // 0 or 1
    pub sequence: u16,          // Packet sequence counter
    pub current_frame: u32,     // Sender's current simulation frame
    pub ack_frame: u32,         // Confirmed frame of the remote peer
    pub window_inputs: [u8; 4], // 4-frame sliding window: [F, F-1, F-2, F-3] as ActionKind u8
    pub checksum: u16,          // Lower 16 bits of FNV-1a state hash
}

impl NetboxWirePacket {
    pub const SIZE: usize = core::mem::size_of::<Self>();

    #[inline]
    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        unsafe { core::mem::transmute_copy(self) }
    }

    #[inline]
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < Self::SIZE {
            return None;
        }
        let pkt: Self = unsafe { core::ptr::read_unaligned(bytes.as_ptr() as *const Self) };
        if pkt.magic != NETBOX_WIRE_MAGIC {
            return None;
        }
        Some(pkt)
    }
}

/// Telemetry stats returned when advancing a rollback frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BoxingFrameStats {
    pub current_frame: u64,
    pub rolled_back: bool,
    pub rollback_frames_count: u32,
    pub predicted_inputs_used: u32,
    pub state_hash: u64,
}

/// Fixed-capacity action ring buffer for a single boxer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BoxingInputQueue<const FRAMES: usize = DEFAULT_BOXING_BUFFER_SIZE> {
    pub inputs: [ActionKind; FRAMES],
    pub frame_tags: [u64; FRAMES],
    pub is_confirmed: [bool; FRAMES],
    pub last_confirmed_frame: u64,
}

impl<const FRAMES: usize> Default for BoxingInputQueue<FRAMES> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const FRAMES: usize> BoxingInputQueue<FRAMES> {
    pub const fn new() -> Self {
        Self {
            inputs: [ActionKind::Idle; FRAMES],
            frame_tags: [u64::MAX; FRAMES],
            is_confirmed: [false; FRAMES],
            last_confirmed_frame: 0,
        }
    }

    /// Sets a confirmed input from local client or network packet.
    pub fn set_confirmed(&mut self, frame: u64, input: ActionKind) {
        let slot = (frame as usize) % FRAMES;
        self.inputs[slot] = input;
        self.frame_tags[slot] = frame;
        self.is_confirmed[slot] = true;
        if frame > self.last_confirmed_frame {
            self.last_confirmed_frame = frame;
        }
    }

    /// Predicts an input for a frame when network packet has not yet arrived.
    pub fn predict_for_frame(&mut self, frame: u64) -> ActionKind {
        let slot = (frame as usize) % FRAMES;
        if self.frame_tags[slot] == frame && self.is_confirmed[slot] {
            return self.inputs[slot];
        }

        // Boxing prediction heuristic:
        // Continuous actions (Guard) stay held; discrete strikes/evasions default to Idle.
        let prev_slot = (frame.saturating_sub(1) as usize) % FRAMES;
        let prev = self.inputs[prev_slot];
        let predicted = if prev == ActionKind::Guard {
            ActionKind::Guard
        } else {
            ActionKind::Idle
        };

        self.inputs[slot] = predicted;
        self.frame_tags[slot] = frame;
        self.is_confirmed[slot] = false;
        predicted
    }

    /// Retrieves the recorded or predicted action for a frame.
    #[inline(always)]
    pub fn get_input(&self, frame: u64) -> ActionKind {
        let slot = (frame as usize) % FRAMES;
        if self.frame_tags[slot] == frame {
            self.inputs[slot]
        } else {
            ActionKind::Idle
        }
    }

    #[inline(always)]
    pub fn is_frame_confirmed(&self, frame: u64) -> bool {
        let slot = (frame as usize) % FRAMES;
        self.frame_tags[slot] == frame && self.is_confirmed[slot]
    }
}

/// Peer-to-peer rollback simulation session for competitive mecha boxing.
#[derive(Debug, Clone)]
pub struct BoxingRollbackSession<const FRAMES: usize = DEFAULT_BOXING_BUFFER_SIZE> {
    pub local_player_id: usize, // 0: Player 0 (Red), 1: Player 1 (Blue), 2: Spectator
    pub current_frame: u64,
    pub confirmed_frame: u64,
    pub input_queues: [BoxingInputQueue<FRAMES>; MAX_SUPPORTED_BOXING_PLAYERS],
    pub saved_states: [BoxingRingEngine; FRAMES],
    pub current_state: BoxingRingEngine,
    pub total_rollbacks: u64,
    pub total_frames_resimulated: u64,
    pub last_rollback_frame: u64,
}

impl<const FRAMES: usize> BoxingRollbackSession<FRAMES> {
    pub fn new(local_player_id: usize, initial_state: BoxingRingEngine) -> Self {
        let mut session = Self {
            local_player_id,
            current_frame: 0,
            confirmed_frame: 0,
            input_queues: [BoxingInputQueue::new(), BoxingInputQueue::new()],
            saved_states: [initial_state.clone(); FRAMES],
            current_state: initial_state.clone(),
            total_rollbacks: 0,
            total_frames_resimulated: 0,
            last_rollback_frame: 0,
        };
        session.saved_states[0] = initial_state;
        session
    }

    /// Advances the simulation by 1 frame using local input and remote prediction.
    pub fn advance_frame(&mut self, local_input: ActionKind) -> BoxingFrameStats {
        let frame = self.current_frame;

        // 1. Record local input (if participant)
        if self.local_player_id < MAX_SUPPORTED_BOXING_PLAYERS {
            self.input_queues[self.local_player_id].set_confirmed(frame, local_input);
        }

        // 2. Predict remote input if not yet confirmed
        let mut predicted_count = 0;
        for p in 0..MAX_SUPPORTED_BOXING_PLAYERS {
            if !self.input_queues[p].is_frame_confirmed(frame) {
                self.input_queues[p].predict_for_frame(frame);
                predicted_count += 1;
            }
        }

        // 3. Save state before stepping forward
        let slot = (frame as usize) % FRAMES;
        self.saved_states[slot] = self.current_state.clone();

        // 4. Step forward with inputs for both players
        let p0_act = self.input_queues[0].get_input(frame);
        let p1_act = self.input_queues[1].get_input(frame);
        self.current_state.step_pvp(p0_act, p1_act);

        self.current_frame += 1;
        self.confirmed_frame = self.input_queues[0]
            .last_confirmed_frame
            .min(self.input_queues[1].last_confirmed_frame);

        BoxingFrameStats {
            current_frame: self.current_frame,
            rolled_back: false,
            rollback_frames_count: 0,
            predicted_inputs_used: predicted_count,
            state_hash: self.current_state.compute_hash(),
        }
    }

    /// Delivers confirmed remote input from the network link.
    /// Triggers immediate speculative rollback and resimulation if prediction differed.
    pub fn receive_remote_input(
        &mut self,
        remote_player_id: usize,
        frame: u64,
        input: ActionKind,
    ) -> bool {
        if remote_player_id >= MAX_SUPPORTED_BOXING_PLAYERS
            || (self.local_player_id < MAX_SUPPORTED_BOXING_PLAYERS
                && remote_player_id == self.local_player_id)
        {
            return false;
        }

        if frame >= self.current_frame {
            self.input_queues[remote_player_id].set_confirmed(frame, input);
            return false;
        }

        if self.current_frame.saturating_sub(frame) >= FRAMES as u64 {
            return false;
        }

        let slot = (frame as usize) % FRAMES;
        let was_confirmed = self.input_queues[remote_player_id].frame_tags[slot] == frame
            && self.input_queues[remote_player_id].is_confirmed[slot];
        let previous_input = self.input_queues[remote_player_id].get_input(frame);

        // Store confirmed input
        self.input_queues[remote_player_id].set_confirmed(frame, input);

        if was_confirmed && previous_input == input {
            return false;
        }

        // Misprediction! Rollback to frame and resimulate forward!
        self.current_state = self.saved_states[slot].clone();

        for f in frame..self.current_frame {
            let f_slot = (f as usize) % FRAMES;
            self.saved_states[f_slot] = self.current_state.clone();
            let p0_act = self.input_queues[0].get_input(f);
            let p1_act = self.input_queues[1].get_input(f);
            self.current_state.step_pvp(p0_act, p1_act);
        }

        let rolled_frames = (self.current_frame - frame) as u32;
        self.total_rollbacks += 1;
        self.total_frames_resimulated += rolled_frames as u64;
        self.last_rollback_frame = frame;
        true
    }

    /// Constructs an outbound 4-frame redundant wire packet for WebRTC DataChannel transmission.
    pub fn create_wire_packet(&self, local_input: ActionKind, sequence: u16) -> NetboxWirePacket {
        let sender_id = if self.local_player_id < MAX_SUPPORTED_BOXING_PLAYERS {
            self.local_player_id as u8
        } else {
            0
        };
        let curr_f = self.current_frame;
        let mut window = [0u8; 4];
        window[0] = local_input.to_u8();

        for i in 1..4 {
            if curr_f >= i {
                let past_f = curr_f - i;
                window[i as usize] = self.input_queues[sender_id as usize].get_input(past_f).to_u8();
            } else {
                window[i as usize] = ActionKind::Idle.to_u8();
            }
        }

        let remote_id = (sender_id as usize) ^ 1;
        let ack_frame = self.input_queues[remote_id].last_confirmed_frame as u32;
        let checksum = (self.current_state.compute_hash() & 0xFFFF) as u16;

        NetboxWirePacket {
            magic: NETBOX_WIRE_MAGIC,
            sender_id,
            sequence,
            current_frame: curr_f as u32,
            ack_frame,
            window_inputs: window,
            checksum,
        }
    }

    /// Delivers a 4-frame sliding window redundant packet from remote peer.
    /// Recovers up to 3 previously dropped consecutive packets without desync,
    /// and performs a single atomic rollback and resimulation to the earliest mismatch.
    pub fn receive_redundant_packet(&mut self, pkt: &NetboxWirePacket) -> (bool, u32) {
        let sender = pkt.sender_id as usize;
        if sender >= MAX_SUPPORTED_BOXING_PLAYERS
            || (self.local_player_id < MAX_SUPPORTED_BOXING_PLAYERS && sender == self.local_player_id)
        {
            return (false, 0);
        }

        let curr_f = pkt.current_frame as u64;
        let mut earliest_mismatch: Option<u64> = None;

        // Inspect window [F, F-1, F-2, F-3]
        for (i, &raw_act) in pkt.window_inputs.iter().enumerate() {
            if curr_f < i as u64 {
                break;
            }
            let frame = curr_f - i as u64;

            if self.current_frame > frame && self.current_frame - frame >= FRAMES as u64 {
                continue;
            }
            let action = ActionKind::from_u8(raw_act);

            if frame >= self.current_frame {
                self.input_queues[sender].set_confirmed(frame, action);
                continue;
            }

            let slot = (frame as usize) % FRAMES;
            let was_confirmed = self.input_queues[sender].frame_tags[slot] == frame
                && self.input_queues[sender].is_confirmed[slot];
            let prev_act = self.input_queues[sender].get_input(frame);

            self.input_queues[sender].set_confirmed(frame, action);

            if !was_confirmed || prev_act != action {
                earliest_mismatch = Some(earliest_mismatch.map_or(frame, |old| old.min(frame)));
            }
        }

        if let Some(rollback_frame) = earliest_mismatch {
            if rollback_frame < self.current_frame {
                let slot = (rollback_frame as usize) % FRAMES;
                self.current_state = self.saved_states[slot].clone();

                for f in rollback_frame..self.current_frame {
                    let f_slot = (f as usize) % FRAMES;
                    self.saved_states[f_slot] = self.current_state.clone();
                    let p0_act = self.input_queues[0].get_input(f);
                    let p1_act = self.input_queues[1].get_input(f);
                    self.current_state.step_pvp(p0_act, p1_act);
                }

                let rolled_count = (self.current_frame - rollback_frame) as u32;
                self.total_rollbacks += 1;
                self.total_frames_resimulated += rolled_count as u64;
                self.last_rollback_frame = rollback_frame;
                return (true, rolled_count);
            }
        }

        (false, 0)
    }
}

/// Zero-float multi-peer spectator and relay session for P2P tournament broadcasting.
/// Plays verified synchronized inputs from both players without speculative rollback,
/// and re-broadcasts packets to downstream spectator peers in a P2P mesh relay tree.
#[derive(Debug, Clone)]
pub struct SpectatorRelaySession<const FRAMES: usize = DEFAULT_BOXING_BUFFER_SIZE> {
    pub engine: BoxingRingEngine,
    pub input_queues: [BoxingInputQueue<FRAMES>; MAX_SUPPORTED_BOXING_PLAYERS],
    pub current_frame: u64,
    pub relay_outbox: [Option<NetboxWirePacket>; 8],
    pub relay_count: usize,
    pub desync_detected: bool,
    pub last_state_hash: u64,
}

impl<const FRAMES: usize> Default for SpectatorRelaySession<FRAMES> {
    fn default() -> Self {
        Self::new(BoxingRingEngine::new(42))
    }
}

impl<const FRAMES: usize> SpectatorRelaySession<FRAMES> {
    pub fn new(engine: BoxingRingEngine) -> Self {
        Self {
            engine,
            input_queues: [BoxingInputQueue::new(), BoxingInputQueue::new()],
            current_frame: 0,
            relay_outbox: [None; 8],
            relay_count: 0,
            desync_detected: false,
            last_state_hash: 0,
        }
    }

    /// Ingests a wire packet from Player 0 or Player 1, and stages it for downstream relay.
    pub fn receive_packet(&mut self, pkt: NetboxWirePacket) {
        let sender = (pkt.sender_id as usize) % MAX_SUPPORTED_BOXING_PLAYERS;
        let curr_f = pkt.current_frame as u64;

        // Extract redundant sliding window inputs [F, F-1, F-2, F-3]
        for (i, &raw_act) in pkt.window_inputs.iter().enumerate() {
            if curr_f < i as u64 {
                break;
            }
            let frame = curr_f - i as u64;
            let action = ActionKind::from_u8(raw_act);
            self.input_queues[sender].set_confirmed(frame, action);
        }

        // Add to relay outbox if not full
        if self.relay_count < self.relay_outbox.len() {
            self.relay_outbox[self.relay_count] = Some(pkt);
            self.relay_count += 1;
        }
    }

    /// Advances the spectator simulation by one frame if both players' inputs are confirmed.
    pub fn step_frame(&mut self) -> Option<BoxingFrameStats> {
        let f = self.current_frame;

        let slot = (f as usize) % FRAMES;
        let p0_ready = self.input_queues[0].frame_tags[slot] == f
            && self.input_queues[0].is_confirmed[slot];
        let p1_ready = self.input_queues[1].frame_tags[slot] == f
            && self.input_queues[1].is_confirmed[slot];

        if !p0_ready || !p1_ready {
            return None; // Waiting for confirmed player inputs
        }

        let p0_act = self.input_queues[0].get_input(f);
        let p1_act = self.input_queues[1].get_input(f);

        self.engine.step_pvp(p0_act, p1_act);
        self.current_frame += 1;
        self.last_state_hash = self.engine.compute_hash();

        Some(BoxingFrameStats {
            current_frame: self.current_frame,
            rolled_back: false,
            rollback_frames_count: 0,
            predicted_inputs_used: 0,
            state_hash: self.last_state_hash,
        })
    }

    /// Drains queued relay packets into a slice to forward to downstream peers.
    pub fn drain_relay_packets(&mut self, out: &mut [NetboxWirePacket]) -> usize {
        let count = self.relay_count.min(out.len());
        for i in 0..count {
            if let Some(pkt) = self.relay_outbox[i].take() {
                out[i] = pkt;
            }
        }
        self.relay_count = 0;
        count
    }
}
