//! Deterministic Rollback Netcode Engine for 2-Player P2P Mecha Boxing Ring (T-099).
//!
//! Provides zero-input-delay client prediction, ring-buffered action queues,
//! and instantaneous speculative rollback resimulation across simulated network latency,
//! jitter, and packet loss.

use crate::boxing::{ActionKind, BoxingRingEngine};

pub const DEFAULT_BOXING_BUFFER_SIZE: usize = 64;
pub const MAX_SUPPORTED_BOXING_PLAYERS: usize = 2;

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
}
