//! Fixed-size, zero-heap contiguous Entity Storage (EntityTable).
//!
//! Stores up to 1,024 64-byte entities in contiguous memory aligned to 64 bytes.
//! Guarantees zero cache-line splits and 100% sequential hardware prefetching.

use crate::entity::Entity;
use crate::fixed::FixedVec2;

pub const DEFAULT_TABLE_CAPACITY: usize = 1024;

/// Fixed-size entity table allocated contiguously on the stack or BSS without heap allocation.
///
/// Designed to reside within CPU cache hierarchies with zero pointer chasing:
/// - 1,024 entities * 64 bytes = 65,536 bytes (64 KB).
/// - Every element is strictly aligned to 64-byte cache-line boundaries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(C, align(64))]
pub struct EntityTable<const CAP: usize = DEFAULT_TABLE_CAPACITY> {
    /// Contiguous array of 64-byte aligned entity slots
    pub entities: [Entity; CAP],
    /// Number of currently active entities
    active_count: usize,
    /// Next hint index for O(1) sequential free slot searches
    free_cursor: usize,
}

impl<const CAP: usize> Default for EntityTable<CAP> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const CAP: usize> EntityTable<CAP> {
    /// Creates a new, completely zeroed entity table with zero heap allocation.
    pub const fn new() -> Self {
        Self {
            entities: [Entity::empty(); CAP],
            active_count: 0,
            free_cursor: 0,
        }
    }

    /// Maximum entity capacity of this table.
    #[inline(always)]
    pub const fn capacity(&self) -> usize {
        CAP
    }

    /// Number of active entities currently alive in the table.
    #[inline(always)]
    pub const fn len(&self) -> usize {
        self.active_count
    }

    /// Returns true if no active entities exist in the table.
    #[inline(always)]
    pub const fn is_empty(&self) -> bool {
        self.active_count == 0
    }

    /// Returns true if the table has reached maximum capacity.
    #[inline(always)]
    pub const fn is_full(&self) -> bool {
        self.active_count >= CAP
    }

    /// Spawns an entity into the first available slot, returning its slot index.
    ///
    /// Preserves strict determinism and zero allocation.
    pub fn spawn(&mut self, id: u64, pos: FixedVec2, vel: FixedVec2) -> Option<usize> {
        if self.is_full() || id == 0 {
            return None;
        }

        // Search starting from free_cursor for O(1) amortized linear allocation
        for offset in 0..CAP {
            let slot = (self.free_cursor + offset) % CAP;
            if !self.entities[slot].is_active() {
                self.entities[slot] = Entity {
                    id,
                    pos,
                    vel,
                    flags: 1,
                    user_data: 0,
                    _reserved: [0; 8],
                };
                self.active_count += 1;
                self.free_cursor = (slot + 1) % CAP;
                return Some(slot);
            }
        }

        None
    }

    /// Despawns an entity at the given slot. Returns true if an entity was removed.
    pub fn despawn(&mut self, slot: usize) -> bool {
        if slot < CAP && self.entities[slot].is_active() {
            self.entities[slot] = Entity::empty();
            self.active_count = self.active_count.saturating_sub(1);
            self.free_cursor = slot;
            true
        } else {
            false
        }
    }

    /// Borrows the entity at the given slot if active.
    #[inline(always)]
    pub fn get(&self, slot: usize) -> Option<&Entity> {
        if slot < CAP && self.entities[slot].is_active() {
            Some(&self.entities[slot])
        } else {
            None
        }
    }

    /// Mutably borrows the entity at the given slot if active.
    #[inline(always)]
    pub fn get_mut(&mut self, slot: usize) -> Option<&mut Entity> {
        if slot < CAP && self.entities[slot].is_active() {
            Some(&mut self.entities[slot])
        } else {
            None
        }
    }

    /// Direct slice access to the underlying 64-byte aligned contiguous array.
    #[inline(always)]
    pub fn as_slice(&self) -> &[Entity; CAP] {
        &self.entities
    }

    /// Direct mutable slice access to the underlying 64-byte aligned contiguous array.
    #[inline(always)]
    pub fn as_mut_slice(&mut self) -> &mut [Entity; CAP] {
        &mut self.entities
    }

    /// Iterates through all active entities sequentially.
    #[inline]
    pub fn iter_active(&self) -> impl Iterator<Item = (usize, &Entity)> {
        self.entities
            .iter()
            .enumerate()
            .filter(|(_, entity)| entity.is_active())
    }

    /// Mutably iterates through all active entities sequentially.
    #[inline]
    pub fn iter_active_mut(&mut self) -> impl Iterator<Item = (usize, &mut Entity)> {
        self.entities
            .iter_mut()
            .enumerate()
            .filter(|(_, entity)| entity.is_active())
    }

    /// Integrates kinematics across the entire contiguous array in a single linear pass.
    ///
    /// CPU hardware prefetcher reads sequential 64-byte cache lines with zero branching on pointers.
    #[inline]
    pub fn update_kinematics(&mut self) {
        for entity in self.entities.iter_mut() {
            if entity.is_active() {
                entity.pos += entity.vel;
            }
        }
    }

    /// Clears all entities in the table.
    pub fn clear(&mut self) {
        self.entities = [Entity::empty(); CAP];
        self.active_count = 0;
        self.free_cursor = 0;
    }
}

/// Multi-slab entity pool managing contiguous 64KB `EntityTable` blocks.
///
/// Designed to scale up to 10,000 ~ 100,000 entities while maintaining
/// strict L1/L2 cache locality per slab during sequential kinematic integration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(C, align(64))]
pub struct EntitySlabPool<const SLABS: usize = 10> {
    /// Array of 64KB contiguous entity slabs (10 slabs = 10,240 entities)
    pub slabs: [EntityTable<DEFAULT_TABLE_CAPACITY>; SLABS],
    /// Total active entities across all slabs
    active_count: usize,
}

impl<const SLABS: usize> Default for EntitySlabPool<SLABS> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const SLABS: usize> EntitySlabPool<SLABS> {
    /// Creates a new, completely zeroed entity slab pool.
    pub const fn new() -> Self {
        Self {
            slabs: [EntityTable::new(); SLABS],
            active_count: 0,
        }
    }

    /// Total entity capacity across all slabs (e.g. 10 * 1024 = 10,240).
    #[inline(always)]
    pub const fn capacity(&self) -> usize {
        SLABS * DEFAULT_TABLE_CAPACITY
    }

    /// Total active entity count across all slabs.
    #[inline(always)]
    pub const fn len(&self) -> usize {
        self.active_count
    }

    /// Returns true if no entities are active in the pool.
    #[inline(always)]
    pub const fn is_empty(&self) -> bool {
        self.active_count == 0
    }

    /// Spawns an entity into the first available slab with capacity.
    /// Returns a flat global handle ID (`u32`).
    pub fn spawn(&mut self, id: u64, pos: FixedVec2, vel: FixedVec2) -> Option<u32> {
        if id == 0 {
            return None;
        }

        for (slab_idx, slab) in self.slabs.iter_mut().enumerate() {
            if !slab.is_full() {
                if let Some(slot) = slab.spawn(id, pos, vel) {
                    self.active_count += 1;
                    return Some((slab_idx as u32 * DEFAULT_TABLE_CAPACITY as u32) + slot as u32);
                }
            }
        }
        None
    }

    /// Despawns an entity by its global handle ID.
    pub fn despawn(&mut self, global_id: u32) -> bool {
        let slab_idx = (global_id as usize) / DEFAULT_TABLE_CAPACITY;
        let slot = (global_id as usize) % DEFAULT_TABLE_CAPACITY;

        if slab_idx < SLABS && self.slabs[slab_idx].despawn(slot) {
            self.active_count = self.active_count.saturating_sub(1);
            true
        } else {
            false
        }
    }

    /// Retrieves an entity by its global handle ID.
    #[inline(always)]
    pub fn get(&self, global_id: u32) -> Option<&Entity> {
        let slab_idx = (global_id as usize) / DEFAULT_TABLE_CAPACITY;
        let slot = (global_id as usize) % DEFAULT_TABLE_CAPACITY;

        if slab_idx < SLABS {
            self.slabs[slab_idx].get(slot)
        } else {
            None
        }
    }

    /// Mutably retrieves an entity by its global handle ID.
    #[inline(always)]
    pub fn get_mut(&mut self, global_id: u32) -> Option<&mut Entity> {
        let slab_idx = (global_id as usize) / DEFAULT_TABLE_CAPACITY;
        let slot = (global_id as usize) % DEFAULT_TABLE_CAPACITY;

        if slab_idx < SLABS {
            self.slabs[slab_idx].get_mut(slot)
        } else {
            None
        }
    }

    /// Integrates kinematics across all slabs sequentially.
    /// Each slab stays resident within L1/L2 cache while processed linearly.
    #[inline]
    pub fn update_kinematics(&mut self) {
        for slab in self.slabs.iter_mut() {
            slab.update_kinematics();
        }
    }

    /// Clears all entities in all slabs.
    pub fn clear(&mut self) {
        for slab in self.slabs.iter_mut() {
            slab.clear();
        }
        self.active_count = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::{align_of, size_of};

    #[test]
    fn test_entity_table_geometry() {
        assert_eq!(
            align_of::<EntityTable<1024>>(),
            64,
            "EntityTable must be 64-byte aligned"
        );
        // 1024 * 64 bytes = 65,536 bytes for entities + tracking fields + alignment
        assert!(
            size_of::<EntityTable<1024>>() >= 65_536,
            "Table must store at least 64KB of entity data"
        );
    }

    #[test]
    fn test_cache_line_alignment_per_element() {
        let table = EntityTable::<16>::new();
        let base_ptr = table.entities.as_ptr() as usize;
        assert_eq!(base_ptr % 64, 0, "Base pointer must align to 64 bytes");

        for i in 0..16 {
            let elem_ptr = unsafe { table.entities.as_ptr().add(i) } as usize;
            assert_eq!(
                elem_ptr % 64,
                0,
                "Entity slot {i} must align to 64-byte cache-line boundary"
            );
        }
    }

    #[test]
    fn test_spawn_and_despawn_lifecycle() {
        let mut table = EntityTable::<4>::new();
        assert_eq!(table.len(), 0);

        let s0 = table.spawn(101, FixedVec2::ZERO, FixedVec2::ZERO).unwrap();
        let s1 = table.spawn(102, FixedVec2::ZERO, FixedVec2::ZERO).unwrap();
        assert_eq!(table.len(), 2);
        assert_eq!(s0, 0);
        assert_eq!(s1, 1);

        assert!(table.despawn(s0));
        assert_eq!(table.len(), 1);
        assert!(table.get(s0).is_none());

        // Reallocates s0
        let s2 = table.spawn(103, FixedVec2::ZERO, FixedVec2::ZERO).unwrap();
        assert_eq!(s2, 0);
        assert_eq!(table.len(), 2);
    }

    #[test]
    fn test_kinematics_sequential_iteration() {
        let mut table = EntityTable::<1024>::new();

        // Spawn 1,024 entities
        for i in 0..1024 {
            table
                .spawn(
                    (i + 1) as u64,
                    FixedVec2::from_ints(i as i64, 0),
                    FixedVec2::from_ints(1, 2),
                )
                .unwrap();
        }
        assert_eq!(table.len(), 1024);
        assert!(table.is_full());

        // Step kinematics 10 times
        for _ in 0..10 {
            table.update_kinematics();
        }

        // Verify entity 0: pos = (0 + 10, 0 + 20)
        let e0 = table.get(0).unwrap();
        assert_eq!(e0.pos, FixedVec2::from_ints(10, 20));

        // Verify entity 1023: pos = (1023 + 10, 0 + 20)
        let e_last = table.get(1023).unwrap();
        assert_eq!(e_last.pos, FixedVec2::from_ints(1033, 20));
    }
}
