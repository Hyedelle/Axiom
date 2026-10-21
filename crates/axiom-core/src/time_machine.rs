//! Time-travel and rollback simulation buffer.
//!
//! Provides instantaneous (O(1)) state recall and deterministic re-simulation.

use crate::input::InputState;
use crate::state::{TickState, DEFAULT_MAX_ENTITIES};
use crate::tick::Tick;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RollbackError {
    TickNotFound(Tick),
    TickInFuture(Tick),
    HistoryTooOld(Tick),
}

/// A fixed-capacity ring buffer of states and inputs for time-travel rollback.
pub struct TimeMachine<const CAPACITY: usize = 256, const ENTITIES: usize = DEFAULT_MAX_ENTITIES> {
    states: [TickState<ENTITIES>; CAPACITY],
    inputs: [InputState; CAPACITY],
    oldest_tick: Tick,
    max_recorded_tick: Tick,
    cursor_tick: Tick,
    count: usize,
}

impl<const CAPACITY: usize, const ENTITIES: usize> Default for TimeMachine<CAPACITY, ENTITIES> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const CAPACITY: usize, const ENTITIES: usize> TimeMachine<CAPACITY, ENTITIES> {
    pub const fn new() -> Self {
        Self {
            states: [TickState::new(); CAPACITY],
            inputs: [InputState::NEUTRAL; CAPACITY],
            oldest_tick: Tick::ZERO,
            max_recorded_tick: Tick::ZERO,
            cursor_tick: Tick::ZERO,
            count: 0,
        }
    }

    /// Records a (State, Input) pair into the ring buffer.
    pub fn push(&mut self, state: TickState<ENTITIES>, input: InputState) {
        let slot = (state.tick.0 as usize) % CAPACITY;
        self.states[slot] = state;
        self.inputs[slot] = input;
        self.cursor_tick = state.tick;

        if self.count == 0 {
            self.oldest_tick = state.tick;
            self.max_recorded_tick = state.tick;
            self.count = 1;
        } else {
            if state.tick > self.max_recorded_tick {
                self.max_recorded_tick = state.tick;
            }
            if self.count < CAPACITY {
                self.count += 1;
            } else {
                // Buffer wrapped around, advance oldest_tick
                self.oldest_tick = Tick(self.max_recorded_tick.0.saturating_sub(CAPACITY as u64 - 1));
            }
        }
    }

    /// Retrieves an exact state snapshot at the requested tick.
    pub fn get_state(&self, tick: Tick) -> Result<&TickState<ENTITIES>, RollbackError> {
        if self.count == 0 {
            return Err(RollbackError::TickNotFound(tick));
        }
        if tick > self.max_recorded_tick {
            return Err(RollbackError::TickInFuture(tick));
        }
        if tick < self.oldest_tick {
            return Err(RollbackError::HistoryTooOld(tick));
        }

        let slot = (tick.0 as usize) % CAPACITY;
        let state = &self.states[slot];
        if state.tick == tick {
            Ok(state)
        } else {
            Err(RollbackError::TickNotFound(tick))
        }
    }

    /// Retrieves the input recorded at the requested tick.
    pub fn get_input(&self, tick: Tick) -> Option<InputState> {
        if self.count == 0 || tick > self.max_recorded_tick || tick < self.oldest_tick {
            return None;
        }
        let slot = (tick.0 as usize) % CAPACITY;
        Some(self.inputs[slot])
    }

    /// Rolls back the timeline cursor to a specific tick and returns that state.
    pub fn rollback_to(&mut self, target_tick: Tick) -> Result<TickState<ENTITIES>, RollbackError> {
        let state = *self.get_state(target_tick)?;
        self.cursor_tick = target_tick;
        Ok(state)
    }

    /// Current timeline cursor tick.
    pub fn cursor_tick(&self) -> Tick {
        self.cursor_tick
    }

    /// Highest tick recorded in the buffer.
    pub fn max_recorded_tick(&self) -> Tick {
        self.max_recorded_tick
    }
}
