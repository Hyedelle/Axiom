//! # Deterministic Rollback Netcode Engine (Phase 3 Milestone / GGPO Architecture)
//!
//! Provides zero-input-delay client prediction, ring-buffered input queues,
//! and instantaneous speculative rollback resimulation across simulated network latency.

use crate::hash::deterministic_hash;
use crate::input::InputState;
use crate::state::{TickState, DEFAULT_MAX_ENTITIES};
use crate::tick::Tick;

pub const DEFAULT_NETCODE_BUFFER_SIZE: usize = 64;
pub const MAX_SUPPORTED_PLAYERS: usize = 2;

/// Status and statistics returned from a frame advancement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RollbackFrameStats {
    pub current_frame: u64,
    pub rolled_back: bool,
    pub rollback_frames_count: u32,
    pub predicted_inputs_used: u32,
}

/// Fixed-capacity input ring buffer for a single player.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerInputQueue<const FRAMES: usize = DEFAULT_NETCODE_BUFFER_SIZE> {
    pub inputs: [InputState; FRAMES],
    pub frame_tags: [u64; FRAMES],
    pub is_confirmed: [bool; FRAMES],
    pub last_confirmed_frame: u64,
}

impl<const FRAMES: usize> Default for PlayerInputQueue<FRAMES> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const FRAMES: usize> PlayerInputQueue<FRAMES> {
    pub const fn new() -> Self {
        Self {
            inputs: [InputState::NEUTRAL; FRAMES],
            frame_tags: [u64::MAX; FRAMES],
            is_confirmed: [false; FRAMES],
            last_confirmed_frame: 0,
        }
    }

    /// Sets a confirmed input for a specific frame.
    pub fn set_confirmed(&mut self, frame: u64, input: InputState) {
        let slot = (frame as usize) % FRAMES;
        self.inputs[slot] = input;
        self.frame_tags[slot] = frame;
        self.is_confirmed[slot] = true;
        if frame > self.last_confirmed_frame {
            self.last_confirmed_frame = frame;
        }
    }

    /// Predicts input for `frame` based on previous input heuristics.
    pub fn predict_for_frame(&mut self, frame: u64) -> InputState {
        let slot = (frame as usize) % FRAMES;
        if self.frame_tags[slot] == frame && self.is_confirmed[slot] {
            return self.inputs[slot];
        }

        // Heuristic: Repeat previous frame's input
        let prev_slot = ((frame.saturating_sub(1)) as usize) % FRAMES;
        let predicted = self.inputs[prev_slot];
        self.inputs[slot] = predicted;
        self.frame_tags[slot] = frame;
        self.is_confirmed[slot] = false;
        predicted
    }

    /// Retrieves the input recorded for a frame.
    #[inline(always)]
    pub fn get_input(&self, frame: u64) -> InputState {
        let slot = (frame as usize) % FRAMES;
        if self.frame_tags[slot] == frame {
            self.inputs[slot]
        } else {
            InputState::NEUTRAL
        }
    }

    /// Returns true if the input for a frame was confirmed from the network.
    #[inline(always)]
    pub fn is_frame_confirmed(&self, frame: u64) -> bool {
        let slot = (frame as usize) % FRAMES;
        self.frame_tags[slot] == frame && self.is_confirmed[slot]
    }
}

/// Peer-to-peer rollback simulation session for competitive multiplayer.
#[derive(Debug, Clone)]
pub struct RollbackSession<
    const FRAMES: usize = DEFAULT_NETCODE_BUFFER_SIZE,
    const ENTITIES: usize = DEFAULT_MAX_ENTITIES,
> {
    pub local_player_id: usize,
    pub current_frame: u64,
    pub confirmed_frame: u64,
    pub input_queues: [PlayerInputQueue<FRAMES>; MAX_SUPPORTED_PLAYERS],
    pub saved_states: [TickState<ENTITIES>; FRAMES],
    pub current_state: TickState<ENTITIES>,
    pub total_rollbacks: u64,
    pub total_frames_resimulated: u64,
}

impl<const FRAMES: usize, const ENTITIES: usize> RollbackSession<FRAMES, ENTITIES> {
    /// Creates a new rollback netcode session initialized with an initial state.
    pub fn new(local_player_id: usize, initial_state: TickState<ENTITIES>) -> Self {
        let mut session = Self {
            local_player_id,
            current_frame: 0,
            confirmed_frame: 0,
            input_queues: [PlayerInputQueue::new(), PlayerInputQueue::new()],
            saved_states: [TickState::new(); FRAMES],
            current_state: initial_state,
            total_rollbacks: 0,
            total_frames_resimulated: 0,
        };

        // Save initial frame 0 state
        session.saved_states[0] = initial_state;
        session
    }

    /// Delivers a confirmed remote input arriving from the network link.
    ///
    /// If the newly received input contradicts a previously predicted input,
    /// a speculative rollback is triggered immediately.
    pub fn receive_remote_input(
        &mut self,
        remote_player_id: usize,
        frame: u64,
        input: InputState,
    ) -> bool {
        if remote_player_id >= MAX_SUPPORTED_PLAYERS || remote_player_id == self.local_player_id {
            return false;
        }

        if frame > self.current_frame {
            // Future input stored directly as confirmed
            self.input_queues[remote_player_id].set_confirmed(frame, input);
            return false;
        }

        // Check if frame falls within rollback buffer window
        if self.current_frame.saturating_sub(frame) >= FRAMES as u64 {
            // History is too old to rollback
            return false;
        }

        let slot = (frame as usize) % FRAMES;
        let was_confirmed = self.input_queues[remote_player_id].frame_tags[slot] == frame
            && self.input_queues[remote_player_id].is_confirmed[slot];
        let previous_input = if self.input_queues[remote_player_id].frame_tags[slot] == frame {
            self.input_queues[remote_player_id].inputs[slot]
        } else {
            InputState::NEUTRAL
        };

        // Set confirmed input
        self.input_queues[remote_player_id].set_confirmed(frame, input);

        // If this frame was already executed speculatively and mispredicted:
        if !was_confirmed && previous_input != input {
            // MISPREDICTION! Rollback and resimulate from `frame`
            self.rollback_and_resimulate(frame);
            return true;
        }

        false
    }

    /// Advances the simulation by 1 local frame with zero input latency.
    pub fn advance_frame(&mut self, local_input: InputState) -> RollbackFrameStats {
        let mut stats = RollbackFrameStats {
            current_frame: self.current_frame,
            rolled_back: false,
            rollback_frames_count: 0,
            predicted_inputs_used: 0,
        };

        // 1. Record confirmed local input
        self.input_queues[self.local_player_id].set_confirmed(self.current_frame, local_input);

        // 2. Predict missing remote inputs
        for p in 0..MAX_SUPPORTED_PLAYERS {
            if p != self.local_player_id {
                let slot = (self.current_frame as usize) % FRAMES;
                if !self.input_queues[p].is_confirmed[slot] {
                    self.input_queues[p].predict_for_frame(self.current_frame);
                    stats.predicted_inputs_used += 1;
                }
            }
        }

        // 3. Save current state snapshot before step
        let slot = (self.current_frame as usize) % FRAMES;
        self.saved_states[slot] = self.current_state;

        // 4. Combine inputs and integrate physics for 1 tick
        let combined_input = self.aggregate_inputs(self.current_frame);
        self.current_state = self.current_state.step(&combined_input);
        self.current_frame += 1;
        stats.current_frame = self.current_frame;

        // 5. Advance confirmed frame watermark if all players' inputs are confirmed
        self.update_confirmed_watermark();

        stats
    }

    /// Rolls back world state to `from_frame` and resimulates forward to `current_frame`.
    fn rollback_and_resimulate(&mut self, from_frame: u64) {
        let rollback_slot = (from_frame as usize) % FRAMES;
        // 1. Instantaneous state recall
        self.current_state = self.saved_states[rollback_slot];

        let resim_count = self.current_frame - from_frame;
        self.total_rollbacks += 1;
        self.total_frames_resimulated += resim_count;

        // 2. Deterministic forward re-simulation
        for frame in from_frame..self.current_frame {
            let slot = (frame as usize) % FRAMES;
            self.saved_states[slot] = self.current_state;

            let combined_input = self.aggregate_inputs(frame);
            self.current_state = self.current_state.step(&combined_input);
        }
    }

    /// Aggregates all players' inputs for a specific frame.
    fn aggregate_inputs(&self, frame: u64) -> InputState {
        let in0 = self.input_queues[0].get_input(frame);
        let in1 = self.input_queues[1].get_input(frame);
        // Player 0 controls X axis, Player 1 controls Y axis
        InputState::new(in0.move_x, in1.move_y, in0.buttons | in1.buttons)
    }

    /// Updates the earliest unconfirmed frame boundary.
    fn update_confirmed_watermark(&mut self) {
        while self.confirmed_frame < self.current_frame {
            let slot = (self.confirmed_frame as usize) % FRAMES;
            let p0_ok = self.input_queues[0].is_confirmed[slot];
            let p1_ok = self.input_queues[1].is_confirmed[slot];
            if p0_ok && p1_ok {
                self.confirmed_frame += 1;
            } else {
                break;
            }
        }
    }

    /// Current simulation tick.
    #[inline(always)]
    pub fn tick(&self) -> Tick {
        self.current_state.tick
    }

    /// Computes deterministic state hash of the current world.
    #[inline]
    pub fn compute_hash(&self) -> u64 {
        deterministic_hash(&self.current_state)
    }
}

/// Simulated lossy network channel with programmable latency and packet jitter.
#[derive(Debug, Clone)]
pub struct SimulatedNetworkPipe {
    packets: [Option<(u64, usize, u64, InputState)>; 128], // (arrive_tick, sender_id, frame, input)
    count: usize,
    pub latency_ticks: u64,
}

impl SimulatedNetworkPipe {
    pub const fn new(latency_ticks: u64) -> Self {
        Self {
            packets: [None; 128],
            count: 0,
            latency_ticks,
        }
    }

    /// Enqueues a packet to be delivered after `latency_ticks`.
    pub fn send(&mut self, current_tick: u64, sender_id: usize, frame: u64, input: InputState) -> bool {
        let arrive_tick = current_tick + self.latency_ticks;
        for slot in &mut self.packets {
            if slot.is_none() {
                *slot = Some((arrive_tick, sender_id, frame, input));
                self.count += 1;
                return true;
            }
        }
        false
    }

    /// Delivers all packets scheduled to arrive at or before `current_tick`.
    pub fn deliver_arrived<F>(&mut self, current_tick: u64, mut on_receive: F)
    where
        F: FnMut(usize, u64, InputState),
    {
        for slot in &mut self.packets {
            if let Some((arrive_tick, sender_id, frame, input)) = *slot {
                if arrive_tick <= current_tick {
                    *slot = None;
                    self.count = self.count.saturating_sub(1);
                    on_receive(sender_id, frame, input);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixed::FixedVec2;

    #[test]
    fn test_rollback_netcode_synchronization_over_latency() {
        // Initial state: Entity 1 at (0, 0)
        let mut initial_state = TickState::<16>::new();
        initial_state.spawn(1, FixedVec2::ZERO, FixedVec2::ZERO);

        let mut peer0 = RollbackSession::<64, 16>::new(0, initial_state);
        let mut peer1 = RollbackSession::<64, 16>::new(1, initial_state);

        // 6 ticks of simulated network latency (100ms at 60Hz)
        let mut net_pipe0_to_1 = SimulatedNetworkPipe::new(6);
        let mut net_pipe1_to_0 = SimulatedNetworkPipe::new(6);

        // Run 60 frames of interactive multiplayer simulation
        for tick in 0..60 {
            // Player 0 inputs: Alternating right/left
            let in0 = InputState::new(if tick % 4 < 2 { 1 } else { -1 }, 0, 0);
            // Player 1 inputs: Upward motion
            let in1 = InputState::new(0, 1, 0);

            // 1. Advance local frame immediately (Zero local input lag!)
            peer0.advance_frame(in0);
            peer1.advance_frame(in1);

            // 2. Transmit inputs through network pipes
            net_pipe0_to_1.send(tick, 0, tick, in0);
            net_pipe1_to_0.send(tick, 1, tick, in1);

            // 3. Deliver arrived packets and trigger speculative rollbacks
            net_pipe0_to_1.deliver_arrived(tick, |sender, frame, input| {
                peer1.receive_remote_input(sender, frame, input);
            });
            net_pipe1_to_0.deliver_arrived(tick, |sender, frame, input| {
                peer0.receive_remote_input(sender, frame, input);
            });
        }

        // Flush remaining in-flight network packets
        for flush_tick in 60..70 {
            net_pipe0_to_1.deliver_arrived(flush_tick, |sender, frame, input| {
                peer1.receive_remote_input(sender, frame, input);
            });
            net_pipe1_to_0.deliver_arrived(flush_tick, |sender, frame, input| {
                peer0.receive_remote_input(sender, frame, input);
            });
        }

        // Both peers must have performed rollbacks successfully
        assert!(peer0.total_rollbacks > 0);
        assert!(peer1.total_rollbacks > 0);
        assert_eq!(peer0.current_frame, 60);
        assert_eq!(peer1.current_frame, 60);

        // BIT-FOR-BIT IDENTICAL MASTER STATE HASH VERIFICATION!
        let hash0 = peer0.compute_hash();
        let hash1 = peer1.compute_hash();
        assert_eq!(
            hash0, hash1,
            "Peer 0 and Peer 1 must reach 100% bit-identical state sync despite 100ms latency!"
        );
        assert_eq!(peer0.current_state.entities[0].pos, peer1.current_state.entities[0].pos);
    }
}
