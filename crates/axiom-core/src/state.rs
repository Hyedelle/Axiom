//! Minimal deterministic Tick State structure and pure transition function.

use crate::entity::Entity;
use crate::fixed::{Fixed, FixedVec2};
use crate::hash::deterministic_hash;
use crate::input::InputState;
use crate::tick::Tick;

pub const DEFAULT_MAX_ENTITIES: usize = 32;

/// Deterministic, cache-resident game world snapshot at a discrete tick.
///
/// Guaranteed pure state representation with zero heap allocation:
/// $$NextState = f(CurrentState, Input)$$
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TickState<const N: usize = DEFAULT_MAX_ENTITIES> {
    /// Monotonic tick index of this state
    pub tick: Tick,
    /// Number of active entities
    pub entity_count: usize,
    /// Contiguous array of cache-line resident entities
    pub entities: [Entity; N],
}

impl<const N: usize> Default for TickState<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> TickState<N> {
    /// Creates an empty initial state at Tick 0.
    pub const fn new() -> Self {
        Self {
            tick: Tick::ZERO,
            entity_count: 0,
            entities: [Entity::empty(); N],
        }
    }

    /// Spawns a new entity in the state, returning its slot index if space permits.
    pub fn spawn(&mut self, id: u64, pos: FixedVec2, vel: FixedVec2) -> Option<usize> {
        for (idx, entity) in self.entities.iter_mut().enumerate() {
            if !entity.is_active() {
                *entity = Entity {
                    id,
                    pos,
                    vel,
                    flags: 1,
                    user_data: 0,
                    _reserved: [0; 8],
                };
                self.entity_count += 1;
                return Some(idx);
            }
        }
        None
    }

    /// Pure transition function: returns the exact next state given an input.
    ///
    /// Preserves absolute determinism:
    /// - No floats
    /// - No system clocks
    /// - No external I/O
    #[must_use]
    pub fn step(&self, input: &InputState) -> Self {
        let mut next = *self;
        next.tick = self.tick.next();

        // Control entity 0 (if active) using discrete input
        if next.entity_count > 0 && next.entities[0].is_active() {
            // Apply input acceleration/speed directly in fixed-point
            next.entities[0].vel.x = Fixed::from_int(input.move_x as i64 * 4);
            next.entities[0].vel.y = Fixed::from_int(input.move_y as i64 * 4);
        }

        // Integrate kinematics for all active entities: pos = pos + vel
        for entity in next.entities.iter_mut() {
            if entity.is_active() {
                entity.pos += entity.vel;
            }
        }

        next
    }

    /// Computes the cross-platform, deterministic 64-bit hash of this state.
    #[inline]
    pub fn compute_hash(&self) -> u64 {
        deterministic_hash(self)
    }
}
