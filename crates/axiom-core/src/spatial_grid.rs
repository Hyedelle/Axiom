//! # Deterministic 3D Spatial Hashing & Broadphase Collision Acceleration (`axiom-core::spatial_grid`)
//!
//! T-189: High-Performance 0-Float, 64-Byte L1 Cache Line Aligned 3D Spatial Hashing Grid
//! and AABB Broadphase Collision Engine.
//!
//! ## Core Architectural Guarantees:
//! - **0-Float in Core & Euclidean Division**: All coordinates in integer millimeters (`mm: i32`),
//!   using `div_euclid` to eliminate negative coordinate skew.
//! - **64B L1 Cache Line Alignment**: `AabbSlab64` and `SpatialCell64` are exactly 64 bytes
//!   (`#[repr(C, align(64))]`), preventing multi-core false sharing.
//! - **0-Heap Flat Array Topology**: Fixed-capacity compile-time generic arrays (`SpatialHashGrid<TABLE_CAP, CELL_CAP>`).
//! - **Lexicographical Minimum Overlap Rule**: Mathematically guarantees exact 1-time collision pair emission
//!   with zero dynamic memory allocation and zero hashset overhead.
//! - **JobSystem & DeterministicEventBus Ready**: N-batch parallel partitioning and physics event emission.

use core::hash::Hasher;
use core::mem::{align_of, size_of};
use crate::hash::Fnv1aHasher;

/// Flag bit: Entity is static terrain / prop (does not move).
pub const COLLISION_FLAG_STATIC: u32 = 1 << 0;
/// Flag bit: Entity is dynamic kinematic / rigid body.
pub const COLLISION_FLAG_DYNAMIC: u32 = 1 << 1;
/// Flag bit: Entity is a sensor / trigger volume (no physical response, generates event).
pub const COLLISION_FLAG_TRIGGER: u32 = 1 << 2;
/// Flag bit: Entity is sleeping / stationary.
pub const COLLISION_FLAG_SLEEPING: u32 = 1 << 3;

/// Default capacity for hash grid table buckets (must be power of two).
pub const DEFAULT_GRID_TABLE_CAP: usize = 1024;
const _: () = assert!(DEFAULT_GRID_TABLE_CAP.is_power_of_two());

/// Default entity capacity per 64-byte spatial cell bucket.
pub const DEFAULT_CELL_CAPACITY: usize = 14;

/// Exact 64-Byte L1 Cache Line Aligned 3D AABB Bounding Box Slab.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(C, align(64))]
pub struct AabbSlab64 {
    /// 8B: 64-bit component/layer bitmask (maps directly to ComponentMask64)
    pub layer_mask: u64,
    /// 8B: User-defined tag or payload pointer
    pub user_data: u64,
    /// 4B: Min X coordinate in integer millimeters
    pub min_x: i32,
    /// 4B: Min Y coordinate in integer millimeters
    pub min_y: i32,
    /// 4B: Min Z coordinate in integer millimeters
    pub min_z: i32,
    /// 4B: Max X coordinate in integer millimeters
    pub max_x: i32,
    /// 4B: Max Y coordinate in integer millimeters
    pub max_y: i32,
    /// 4B: Max Z coordinate in integer millimeters
    pub max_z: i32,
    /// 4B: Unique entity handle identifier (0 = empty / inactive)
    pub entity_id: u32,
    /// 4B: ABA prevention generation counter
    pub generation: u32,
    /// 4B: Entity physics flags (`COLLISION_FLAG_*`)
    pub flags: u32,
    /// 12B: Exact padding ensuring 64-byte L1 cache line geometry
    pub _reserved: [u8; 12],
}

const _: () = assert!(size_of::<AabbSlab64>() == 64);
const _: () = assert!(align_of::<AabbSlab64>() == 64);

impl Default for AabbSlab64 {
    fn default() -> Self {
        Self::empty()
    }
}

impl AabbSlab64 {
    /// Creates an empty inactive AABB slab.
    pub const fn empty() -> Self {
        Self {
            layer_mask: 0,
            user_data: 0,
            min_x: 0,
            min_y: 0,
            min_z: 0,
            max_x: 0,
            max_y: 0,
            max_z: 0,
            entity_id: 0,
            generation: 0,
            flags: 0,
            _reserved: [0u8; 12],
        }
    }

    /// Creates a new active AABB slab.
    pub fn new(
        entity_id: u32,
        generation: u32,
        layer_mask: u64,
        flags: u32,
        min: (i32, i32, i32),
        max: (i32, i32, i32),
    ) -> Self {
        assert!(min.0 <= max.0, "min_x must be <= max_x");
        assert!(min.1 <= max.1, "min_y must be <= max_y");
        assert!(min.2 <= max.2, "min_z must be <= max_z");

        Self {
            layer_mask,
            user_data: 0,
            min_x: min.0,
            min_y: min.1,
            min_z: min.2,
            max_x: max.0,
            max_y: max.1,
            max_z: max.2,
            entity_id,
            generation,
            flags,
            _reserved: [0u8; 12],
        }
    }

    #[inline(always)]
    pub fn is_active(&self) -> bool {
        self.entity_id != 0
    }

    /// Evaluates exact 3D AABB overlap intersection.
    #[inline(always)]
    pub fn intersects(&self, other: &Self) -> bool {
        self.min_x <= other.max_x
            && self.max_x >= other.min_x
            && self.min_y <= other.max_y
            && self.max_y >= other.min_y
            && self.min_z <= other.max_z
            && self.max_z >= other.min_z
    }

    /// Checks if two entities are permitted to collide based on layer mask and physics flags.
    #[inline(always)]
    pub fn can_collide_with(&self, other: &Self) -> bool {
        if self.entity_id == other.entity_id || !self.is_active() || !other.is_active() {
            return false;
        }
        // At least one common layer bitmask
        if (self.layer_mask & other.layer_mask) == 0 {
            return false;
        }
        // At least one must be dynamic (or trigger)
        let dynamic_mask = COLLISION_FLAG_DYNAMIC | COLLISION_FLAG_TRIGGER;
        ((self.flags | other.flags) & dynamic_mask) != 0
    }
}

/// Exact 64-Byte L1 Cache Line Aligned Spatial Cell Bucket Slab.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(C, align(64))]
pub struct SpatialCell64<const CAP: usize = DEFAULT_CELL_CAPACITY> {
    /// 2B: Number of valid entity IDs currently in this cell
    pub count: u16,
    /// 2B: Cell status / flags
    pub flags: u16,
    /// 4B: Hash verification tag or cell sequence
    pub cell_hash: u32,
    /// 56B: Flat inline entity IDs (14 * 4B = 56B when CAP = 14)
    pub entities: [u32; CAP],
}

const _: () = assert!(size_of::<SpatialCell64<14>>() == 64);
const _: () = assert!(align_of::<SpatialCell64<14>>() == 64);

impl<const CAP: usize> Default for SpatialCell64<CAP> {
    fn default() -> Self {
        Self::empty()
    }
}

impl<const CAP: usize> SpatialCell64<CAP> {
    pub const fn empty() -> Self {
        Self {
            count: 0,
            flags: 0,
            cell_hash: 0,
            entities: [0u32; CAP],
        }
    }

    #[inline(always)]
    pub fn is_full(&self) -> bool {
        (self.count as usize) >= CAP
    }

    #[inline(always)]
    pub fn push(&mut self, entity_id: u32) -> bool {
        let idx = self.count as usize;
        if idx < CAP {
            self.entities[idx] = entity_id;
            self.count += 1;
            true
        } else {
            false
        }
    }

    #[inline(always)]
    pub fn clear(&mut self) {
        self.count = 0;
        self.flags = 0;
        self.cell_hash = 0;
    }
}

/// 3-Prime Coprime Multipliers + Wang Finalizer 3D Spatial Hash Function.
/// Maps 3D discrete grid coordinates to `0..table_mask`.
#[inline(always)]
pub fn spatial_hash_3d(cx: i32, cy: i32, cz: i32, table_mask: usize) -> usize {
    let ux = cx as u32;
    let uy = cy as u32;
    let uz = cz as u32;

    let h = ux.wrapping_mul(0x8DA6_B343)
        ^ uy.wrapping_mul(0xD816_3841)
        ^ uz.wrapping_mul(0xCB1A_B31F);

    let mixed = h ^ (h >> 16);
    let mixed = mixed.wrapping_mul(0x7FEB_352D);
    let mixed = mixed ^ (mixed >> 15);

    (mixed as usize) & table_mask
}

/// Representation of a detected collision pair.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
#[repr(C)]
pub struct CollisionPair64 {
    pub entity_a: u32,
    pub entity_b: u32,
    pub pair_key: u64,
}

impl CollisionPair64 {
    #[inline(always)]
    pub fn new(id_a: u32, id_b: u32) -> Self {
        let min_id = id_a.min(id_b);
        let max_id = id_a.max(id_b);
        Self {
            entity_a: min_id,
            entity_b: max_id,
            pair_key: ((min_id as u64) << 32) | (max_id as u64),
        }
    }
}

/// 0-Heap Fixed-Capacity Spatial Hash Grid Broadphase Acceleration Structure.
#[derive(Clone, Copy, Debug)]
#[repr(C, align(64))]
pub struct SpatialHashGrid<
    const TABLE_CAP: usize = DEFAULT_GRID_TABLE_CAP,
    const CELL_CAP: usize = DEFAULT_CELL_CAPACITY,
> {
    pub cells: [SpatialCell64<CELL_CAP>; TABLE_CAP],
    pub cell_size_mm: i32,
    pub table_mask: usize,
    pub total_insertions: u32,
    pub overflow_count: u32,
}

const _: () = assert!(align_of::<SpatialHashGrid<1024, 14>>() == 64);

impl<const TABLE_CAP: usize, const CELL_CAP: usize> Default for SpatialHashGrid<TABLE_CAP, CELL_CAP> {
    fn default() -> Self {
        Self::new(1000) // Default 1,000mm (1.0m) cell edge
    }
}

impl<const TABLE_CAP: usize, const CELL_CAP: usize> SpatialHashGrid<TABLE_CAP, CELL_CAP> {
    /// Creates a new spatial hash grid with a designated cell size in millimeters.
    pub const fn new(cell_size_mm: i32) -> Self {
        assert!(TABLE_CAP.is_power_of_two(), "TABLE_CAP must be a power of two");
        Self {
            cells: [SpatialCell64::empty(); TABLE_CAP],
            cell_size_mm: if cell_size_mm > 0 { cell_size_mm } else { 1000 },
            table_mask: TABLE_CAP - 1,
            total_insertions: 0,
            overflow_count: 0,
        }
    }

    /// Resets all cells in the grid.
    #[inline(always)]
    pub fn clear(&mut self) {
        for cell in self.cells.iter_mut() {
            cell.clear();
        }
        self.total_insertions = 0;
        self.overflow_count = 0;
    }

    /// Inserts an entity AABB into the grid across all intersecting cells.
    /// Returns the number of cells populated.
    pub fn insert(&mut self, slab: &AabbSlab64) -> usize {
        if !slab.is_active() {
            return 0;
        }

        let min_cx = slab.min_x.div_euclid(self.cell_size_mm);
        let max_cx = slab.max_x.div_euclid(self.cell_size_mm);
        let min_cy = slab.min_y.div_euclid(self.cell_size_mm);
        let max_cy = slab.max_y.div_euclid(self.cell_size_mm);
        let min_cz = slab.min_z.div_euclid(self.cell_size_mm);
        let max_cz = slab.max_z.div_euclid(self.cell_size_mm);

        let mut inserted = 0;
        for cz in min_cz..=max_cz {
            for cy in min_cy..=max_cy {
                for cx in min_cx..=max_cx {
                    let bucket = spatial_hash_3d(cx, cy, cz, self.table_mask);
                    if self.cells[bucket].push(slab.entity_id) {
                        inserted += 1;
                        self.total_insertions += 1;
                    } else {
                        self.overflow_count += 1;
                    }
                }
            }
        }
        inserted
    }

    /// Queries all entity IDs overlapping with `query_aabb`.
    /// Writes matching IDs to `out_entities` and returns the count written.
    pub fn query_aabb_candidates(
        &self,
        query_aabb: &AabbSlab64,
        out_entities: &mut [u32],
    ) -> usize {
        if !query_aabb.is_active() || out_entities.is_empty() {
            return 0;
        }

        let min_cx = query_aabb.min_x.div_euclid(self.cell_size_mm);
        let max_cx = query_aabb.max_x.div_euclid(self.cell_size_mm);
        let min_cy = query_aabb.min_y.div_euclid(self.cell_size_mm);
        let max_cy = query_aabb.max_y.div_euclid(self.cell_size_mm);
        let min_cz = query_aabb.min_z.div_euclid(self.cell_size_mm);
        let max_cz = query_aabb.max_z.div_euclid(self.cell_size_mm);

        let mut written = 0;
        let limit = out_entities.len();

        for cz in min_cz..=max_cz {
            for cy in min_cy..=max_cy {
                for cx in min_cx..=max_cx {
                    let bucket = spatial_hash_3d(cx, cy, cz, self.table_mask);
                    let cell = &self.cells[bucket];
                    for i in 0..(cell.count as usize) {
                        let id = cell.entities[i];
                        if id != query_aabb.entity_id {
                            // Check for duplicates in output buffer
                            let mut already_present = false;
                            for k in 0..written {
                                if out_entities[k] == id {
                                    already_present = true;
                                    break;
                                }
                            }
                            if !already_present {
                                if written < limit {
                                    out_entities[written] = id;
                                    written += 1;
                                } else {
                                    return written;
                                }
                            }
                        }
                    }
                }
            }
        }
        written
    }

    /// Generates deduplicated collision pairs from all entities using the
    /// Lexicographical Minimum Overlap Rule.
    ///
    /// For each pair $(A, B)$ that intersects, the pair is accepted if and only if
    /// the current cell $(cx, cy, cz)$ is exactly the minimum overlap cell:
    /// `cx == max(A.min_x, B.min_x).div_euclid(cell_size)`, etc.
    ///
    /// Returns the number of unique collision pairs written to `out_pairs`.
    pub fn query_all_collision_pairs<const SLAB_COUNT: usize>(
        &self,
        slabs: &[AabbSlab64; SLAB_COUNT],
        out_pairs: &mut [CollisionPair64],
    ) -> usize {
        let mut pair_count = 0;
        let pair_limit = out_pairs.len();

        for i in 0..SLAB_COUNT {
            let slab_a = &slabs[i];
            if !slab_a.is_active() {
                continue;
            }

            let min_cx = slab_a.min_x.div_euclid(self.cell_size_mm);
            let max_cx = slab_a.max_x.div_euclid(self.cell_size_mm);
            let min_cy = slab_a.min_y.div_euclid(self.cell_size_mm);
            let max_cy = slab_a.max_y.div_euclid(self.cell_size_mm);
            let min_cz = slab_a.min_z.div_euclid(self.cell_size_mm);
            let max_cz = slab_a.max_z.div_euclid(self.cell_size_mm);

            for cz in min_cz..=max_cz {
                for cy in min_cy..=max_cy {
                    for cx in min_cx..=max_cx {
                        let bucket = spatial_hash_3d(cx, cy, cz, self.table_mask);
                        let cell = &self.cells[bucket];

                        for k in 0..(cell.count as usize) {
                            let other_id = cell.entities[k];
                            // Only evaluate ordered pairs where slab_a.entity_id < other_id
                            if slab_a.entity_id >= other_id {
                                continue;
                            }

                            // Find slab_b in slabs array
                            let slab_b_opt = slabs.iter().find(|s| s.entity_id == other_id);
                            if let Some(slab_b) = slab_b_opt {
                                if slab_a.can_collide_with(slab_b) && slab_a.intersects(slab_b) {
                                    // Lexicographical Minimum Overlap Rule:
                                    // Only accept if (cx, cy, cz) is the lexicographical min cell of overlap
                                    let min_overlap_cx = slab_a.min_x.max(slab_b.min_x).div_euclid(self.cell_size_mm);
                                    let min_overlap_cy = slab_a.min_y.max(slab_b.min_y).div_euclid(self.cell_size_mm);
                                    let min_overlap_cz = slab_a.min_z.max(slab_b.min_z).div_euclid(self.cell_size_mm);

                                    if cx == min_overlap_cx && cy == min_overlap_cy && cz == min_overlap_cz {
                                        if pair_count < pair_limit {
                                            out_pairs[pair_count] = CollisionPair64::new(slab_a.entity_id, slab_b.entity_id);
                                            pair_count += 1;
                                        } else {
                                            return pair_count;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        pair_count
    }

    /// Computes a guaranteed-deterministic 64-bit FNV-1a checksum over the grid state.
    pub fn compute_consensus_hash(&self) -> u64 {
        let mut hasher = Fnv1aHasher::new();
        hasher.write_usize(self.total_insertions as usize);
        hasher.write_usize(self.overflow_count as usize);

        for (i, cell) in self.cells.iter().enumerate() {
            if cell.count > 0 {
                hasher.write_usize(i);
                hasher.write_u16(cell.count);
                for k in 0..(cell.count as usize) {
                    hasher.write_u32(cell.entities[k]);
                }
            }
        }

        hasher.finish()
    }
}
