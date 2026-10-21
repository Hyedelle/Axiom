//! # 100,000 NPC Background Ecology Simulation (Phase 4 Prototype)
//!
//! Provides 64-byte cache-line aligned NPC structures and contiguous DOD slab pools (`NpcCrowdPool`)
//! capable of simulating 100k background NPCs at 60Hz with zero floating-point arithmetic.

use crate::fixed::{Fixed, FixedVec2};
use crate::hash::deterministic_hash;

pub const DEFAULT_NPC_TABLE_CAPACITY: usize = 1024;
pub const TICKS_PER_DAY: u64 = 1440; // 1 tick = 1 in-game minute, 1440 ticks = 24-hour cycle

/// NPC daily activity behavior state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u8)]
pub enum NpcActivity {
    #[default]
    Idle = 0,
    Commute = 1,
    Work = 2,
    Rest = 3,
    Socialize = 4,
}

/// 64-byte cache-line aligned NPC entity for high-density background simulation.
///
/// Guaranteed zero false sharing and 100% sequential hardware prefetching.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(C, align(64))]
pub struct Npc {
    /// World position (Q32.16 fixed-point)
    pub pos: FixedVec2,
    /// Current target waypoint (Q32.16 fixed-point)
    pub target: FixedVec2,
    /// Home residence tile coordinate
    pub home: (i32, i32),
    /// Workplace / farm / mine tile coordinate
    pub work: (i32, i32),
    /// Unique NPC identity (0 = inactive / dead)
    pub id: u32,
    /// Current physical energy (0 ~ 1000)
    pub energy: u16,
    /// Current health pool (0 ~ 1000)
    pub health: u16,
    /// Current behavioral state
    pub activity: NpcActivity,
    /// Work shift offset (0 = day shift, 1 = evening shift, 2 = night shift)
    pub schedule_shift: u8,
    /// Movement speed in Q8.8 fixed-point (256 = 1.0 tile/tick)
    pub speed_q8: u16,
    /// Padding to enforce exact 64-byte alignment
    _padding: [u8; 4],
}

impl Default for Npc {
    fn default() -> Self {
        Self::empty()
    }
}

impl Npc {
    /// Creates an empty inactive NPC slot.
    pub const fn empty() -> Self {
        Self {
            pos: FixedVec2::new(Fixed::ZERO, Fixed::ZERO),
            target: FixedVec2::new(Fixed::ZERO, Fixed::ZERO),
            home: (0, 0),
            work: (0, 0),
            id: 0,
            energy: 1000,
            health: 1000,
            activity: NpcActivity::Idle,
            schedule_shift: 0,
            speed_q8: 256, // 1.0 tile per tick
            _padding: [0; 4],
        }
    }

    /// Returns true if this NPC slot is actively simulating.
    #[inline(always)]
    pub fn is_active(&self) -> bool {
        self.id != 0
    }

    /// Advances NPC circadian cycle, schedule, and movement for 1 discrete tick.
    pub fn update(&mut self, tick: u64) {
        if !self.is_active() {
            return;
        }

        // 1. Circadian schedule resolution
        let shift_offset = (self.schedule_shift as u64).wrapping_mul(480);
        let day_time = (tick.wrapping_add(shift_offset)) % TICKS_PER_DAY;

        if day_time < 360 {
            // Night (0:00 ~ 6:00): Sleeping at home
            self.activity = NpcActivity::Rest;
            self.target = FixedVec2::from_ints(self.home.0 as i64, self.home.1 as i64);
            self.energy = self.energy.saturating_add(2).min(1000);
        } else if day_time < 480 {
            // Morning Commute (6:00 ~ 8:00): Traveling to work
            self.activity = NpcActivity::Commute;
            self.target = FixedVec2::from_ints(self.work.0 as i64, self.work.1 as i64);
        } else if day_time < 1080 {
            // Working hours (8:00 ~ 18:00): Production at work station
            self.activity = NpcActivity::Work;
            self.target = FixedVec2::from_ints(self.work.0 as i64, self.work.1 as i64);
            self.energy = self.energy.saturating_sub(1);
        } else if day_time < 1200 {
            // Evening Commute (18:00 ~ 20:00): Returning home
            self.activity = NpcActivity::Commute;
            self.target = FixedVec2::from_ints(self.home.0 as i64, self.home.1 as i64);
        } else {
            // Leisure / Socialize (20:00 ~ 24:00): Free time near home
            self.activity = NpcActivity::Socialize;
            self.target = FixedVec2::from_ints(self.home.0 as i64, self.home.1 as i64);
        }

        // 2. Goal-seeking kinematic integration (Integer fixed-point)
        let speed = Fixed::from_raw((self.speed_q8 as i64) << 8); // Convert Q8.8 to Q32.16
        let dx = self.target.x - self.pos.x;
        let dy = self.target.y - self.pos.y;

        if dx.0.abs() <= speed.0 && dy.0.abs() <= speed.0 {
            self.pos = self.target;
        } else {
            // Step along X
            if dx.0 > speed.0 {
                self.pos.x += speed;
            } else if dx.0 < -speed.0 {
                self.pos.x -= speed;
            } else {
                self.pos.x = self.target.x;
            }

            // Step along Y
            if dy.0 > speed.0 {
                self.pos.y += speed;
            } else if dy.0 < -speed.0 {
                self.pos.y -= speed;
            } else {
                self.pos.y = self.target.y;
            }
        }
    }
}

/// Contiguous 64KB table holding 1,024 64-byte aligned NPCs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C, align(64))]
pub struct NpcTable<const CAP: usize = DEFAULT_NPC_TABLE_CAPACITY> {
    pub npcs: [Npc; CAP],
    pub active_count: usize,
    free_cursor: usize,
}

impl<const CAP: usize> Default for NpcTable<CAP> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const CAP: usize> NpcTable<CAP> {
    /// Creates a zeroed NPC table with zero heap allocations.
    pub const fn new() -> Self {
        Self {
            npcs: [Npc::empty(); CAP],
            active_count: 0,
            free_cursor: 0,
        }
    }

    #[inline(always)]
    pub const fn capacity(&self) -> usize {
        CAP
    }

    #[inline(always)]
    pub const fn len(&self) -> usize {
        self.active_count
    }

    #[inline(always)]
    pub const fn is_empty(&self) -> bool {
        self.active_count == 0
    }

    #[inline(always)]
    pub const fn is_full(&self) -> bool {
        self.active_count >= CAP
    }

    /// Spawns an NPC into the table. Returns local slot index.
    pub fn spawn(
        &mut self,
        id: u32,
        home: (i32, i32),
        work: (i32, i32),
        shift: u8,
        speed_q8: u16,
    ) -> Option<usize> {
        if id == 0 || self.is_full() {
            return None;
        }

        let start = self.free_cursor;
        for i in 0..CAP {
            let idx = (start + i) % CAP;
            if !self.npcs[idx].is_active() {
                self.npcs[idx] = Npc {
                    pos: FixedVec2::from_ints(home.0 as i64, home.1 as i64),
                    target: FixedVec2::from_ints(home.0 as i64, home.1 as i64),
                    home,
                    work,
                    id,
                    energy: 1000,
                    health: 1000,
                    activity: NpcActivity::Rest,
                    schedule_shift: shift,
                    speed_q8,
                    _padding: [0; 4],
                };
                self.active_count += 1;
                self.free_cursor = (idx + 1) % CAP;
                return Some(idx);
            }
        }
        None
    }

    /// Despawns an NPC at the given local slot index.
    pub fn despawn(&mut self, slot: usize) -> bool {
        if slot < CAP && self.npcs[slot].is_active() {
            self.npcs[slot] = Npc::empty();
            self.active_count = self.active_count.saturating_sub(1);
            if slot < self.free_cursor {
                self.free_cursor = slot;
            }
            true
        } else {
            false
        }
    }

    /// Simulates all active NPCs sequentially.
    #[inline]
    pub fn update(&mut self, tick: u64) {
        for npc in &mut self.npcs {
            if npc.is_active() {
                npc.update(tick);
            }
        }
    }
}

/// Contiguous DOD pool composed of multiple 64KB NPC slabs.
///
/// Simulates up to 100,000+ NPCs with zero pointer chasing.
#[derive(Debug, Clone, PartialEq, Eq)]
#[repr(C, align(64))]
pub struct NpcCrowdPool<const SLABS: usize = 100> {
    pub slabs: [NpcTable<DEFAULT_NPC_TABLE_CAPACITY>; SLABS],
    pub active_count: usize,
}

impl<const SLABS: usize> Default for NpcCrowdPool<SLABS> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const SLABS: usize> NpcCrowdPool<SLABS> {
    /// Creates a new uninitialized NPC crowd pool.
    pub const fn new() -> Self {
        Self {
            slabs: [NpcTable::new(); SLABS],
            active_count: 0,
        }
    }

    /// Maximum total NPC capacity (e.g. 100 slabs * 1,024 = 102,400 NPCs).
    #[inline(always)]
    pub const fn capacity(&self) -> usize {
        SLABS * DEFAULT_NPC_TABLE_CAPACITY
    }

    #[inline(always)]
    pub const fn len(&self) -> usize {
        self.active_count
    }

    #[inline(always)]
    pub const fn is_empty(&self) -> bool {
        self.active_count == 0
    }

    /// Spawns an NPC into the first available slab.
    ///
    /// Returns global handle ID (`u32`).
    pub fn spawn(
        &mut self,
        id: u32,
        home: (i32, i32),
        work: (i32, i32),
        shift: u8,
        speed_q8: u16,
    ) -> Option<u32> {
        if id == 0 {
            return None;
        }

        for (slab_idx, slab) in self.slabs.iter_mut().enumerate() {
            if !slab.is_full() {
                if let Some(slot) = slab.spawn(id, home, work, shift, speed_q8) {
                    self.active_count += 1;
                    return Some((slab_idx as u32 * DEFAULT_NPC_TABLE_CAPACITY as u32) + slot as u32);
                }
            }
        }
        None
    }

    /// Despawns an NPC by its global handle ID.
    pub fn despawn(&mut self, global_id: u32) -> bool {
        let slab_idx = (global_id as usize) / DEFAULT_NPC_TABLE_CAPACITY;
        let slot = (global_id as usize) % DEFAULT_NPC_TABLE_CAPACITY;

        if slab_idx < SLABS && self.slabs[slab_idx].despawn(slot) {
            self.active_count = self.active_count.saturating_sub(1);
            true
        } else {
            false
        }
    }

    /// Retrieves an immutable reference to an active NPC by global handle ID.
    #[inline]
    pub fn get_npc(&self, global_id: u32) -> Option<&Npc> {
        let slab_idx = (global_id as usize) / DEFAULT_NPC_TABLE_CAPACITY;
        let slot = (global_id as usize) % DEFAULT_NPC_TABLE_CAPACITY;

        if slab_idx < SLABS && self.slabs[slab_idx].npcs[slot].is_active() {
            Some(&self.slabs[slab_idx].npcs[slot])
        } else {
            None
        }
    }

    /// Retrieves a mutable reference to an active NPC by global handle ID.
    #[inline]
    pub fn get_npc_mut(&mut self, global_id: u32) -> Option<&mut Npc> {
        let slab_idx = (global_id as usize) / DEFAULT_NPC_TABLE_CAPACITY;
        let slot = (global_id as usize) % DEFAULT_NPC_TABLE_CAPACITY;

        if slab_idx < SLABS && self.slabs[slab_idx].npcs[slot].is_active() {
            Some(&mut self.slabs[slab_idx].npcs[slot])
        } else {
            None
        }
    }

    /// Simulates all active NPCs across all slabs for 1 discrete tick.
    #[inline]
    pub fn update(&mut self, tick: u64) {
        for slab in &mut self.slabs {
            if slab.active_count > 0 {
                slab.update(tick);
            }
        }
    }

    /// Computes deterministic state hash across all active NPCs.
    pub fn compute_hash(&self) -> u64 {
        let mut h = 0xCBF29CE484222325u64;
        for slab in &self.slabs {
            if slab.active_count > 0 {
                for npc in &slab.npcs {
                    if npc.is_active() {
                        h ^= deterministic_hash(npc);
                        h = h.rotate_left(13).wrapping_mul(0x100000001B3);
                    }
                }
            }
        }
        h
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_npc_size_and_alignment() {
        assert_eq!(
            core::mem::size_of::<Npc>(),
            64,
            "Npc must be exactly 64 bytes"
        );
        assert_eq!(
            core::mem::align_of::<Npc>(),
            64,
            "Npc must be aligned to 64-byte cache line"
        );
    }

    #[test]
    fn test_npc_circadian_cycle() {
        let mut npc = Npc {
            pos: FixedVec2::from_ints(0, 0),
            target: FixedVec2::from_ints(0, 0),
            home: (0, 0),
            work: (50, 50),
            id: 1,
            energy: 500,
            health: 1000,
            activity: NpcActivity::Idle,
            schedule_shift: 0,
            speed_q8: 256,
            _padding: [0; 4],
        };

        // Tick 100 (Night, 1:40 AM) -> Rest
        npc.update(100);
        assert_eq!(npc.activity, NpcActivity::Rest);

        // Tick 400 (Morning Commute, 6:40 AM) -> Commute
        npc.update(400);
        assert_eq!(npc.activity, NpcActivity::Commute);
        assert_eq!(npc.target, FixedVec2::from_ints(50, 50));

        // Tick 700 (Work, 11:40 AM) -> Work
        npc.update(700);
        assert_eq!(npc.activity, NpcActivity::Work);

        // Tick 1100 (Evening Commute, 18:20) -> Commute
        npc.update(1100);
        assert_eq!(npc.activity, NpcActivity::Commute);
        assert_eq!(npc.target, FixedVec2::from_ints(0, 0));
    }
}
