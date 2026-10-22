//! 100,000-entity zero-allocation-per-tick spatial acceleration index (`SpatialIndex`).
//!
//! T-068: Resolves range / collision / visibility (FOV) queries for massive unit counts on a
//! single core using only integer arithmetic and a handful of flat SoA slabs allocated once.
//!
//! Layout (defaults: 16,384 buckets x 100,000 entities):
//! - `heads`      : bucket head table, 64KB (L2-resident, one `fill` per tick to reset)
//! - `next`       : intrusive chain links, 400KB
//! - `entity_ids` : caller identifiers, 400KB
//! - `cell_keys`  : exact packed cell ownership, 800KB (hash-collision disambiguation)
//! - `positions`  : fixed-point coordinates, 1.6MB
//!
//! Every query is bounded by the cells overlapping the query shape plus an exact integer
//! distance / containment filter. Negative coordinates are supported (Euclidean division).

extern crate alloc;

use alloc::boxed::Box;

use crate::fixed::{Fixed, FixedVec2};

/// Default number of hash buckets (power of two; 16,384 * 4B = 64KB head table).
pub const DEFAULT_SPATIAL_BUCKETS: usize = 16_384;
/// Default entity capacity of the spatial index.
pub const DEFAULT_SPATIAL_MAX_ENTITIES: usize = 100_000;
/// Sentinel marking an empty bucket head / end of an entity chain.
pub const EMPTY_INDEX: u32 = u32::MAX;

/// Large odd multiplicative constants for 2D cell -> bucket hashing (integer only).
const HASH_PRIME_X: u64 = 0x9E37_79B9_7F4A_7C15;
const HASH_PRIME_Y: u64 = 0xC2B2_AE3D_27D4_EB4F;

/// Heap-allocates a zero-initialized `T` without ever materializing it on the stack.
///
/// # Safety
/// `T` must be valid when all of its bytes are zero (plain integer arrays, `FixedVec2`).
unsafe fn boxed_zeroed<T>() -> Box<T> {
    let layout = core::alloc::Layout::new::<T>();
    if layout.size() == 0 {
        return Box::from_raw(core::ptr::NonNull::<T>::dangling().as_ptr());
    }
    let ptr = alloc::alloc::alloc_zeroed(layout) as *mut T;
    if ptr.is_null() {
        alloc::alloc::handle_alloc_error(layout);
    }
    Box::from_raw(ptr)
}

/// Deterministic integer square root (floor) of a `u128`.
#[inline(always)]
pub(crate) fn isqrt_u128(v: u128) -> u128 {
    if v < 2 {
        return v;
    }
    // Newton-Raphson on integers; starts above the root and converges monotonically.
    let mut x = 1u128 << ((128 - v.leading_zeros()).div_ceil(2));
    loop {
        let y = (x + v / x) >> 1;
        if y >= x {
            return x;
        }
        x = y;
    }
}

/// Zero-allocation-per-tick spatial hash index for up to `MAX_ENTITIES` entities.
///
/// Entities are chained per bucket through `next` (intrusive singly-linked list inside a
/// flat array), so [`insert`](Self::insert) is O(1), [`clear`](Self::clear) is a single
/// sequential fill of the head table, and no heap traffic happens after construction.
#[repr(C, align(64))]
pub struct SpatialIndex<
    const BUCKETS: usize = DEFAULT_SPATIAL_BUCKETS,
    const MAX_ENTITIES: usize = DEFAULT_SPATIAL_MAX_ENTITIES,
> {
    /// Number of entities currently inserted (also the next free slot).
    pub count: usize,
    /// World units per cell edge (> 0).
    pub cell_size: i64,
    /// Bucket head table: slot index of the first entity in each bucket, or `EMPTY_INDEX`.
    pub heads: Box<[u32; BUCKETS]>,
    /// Intrusive chain link: slot index of the next entity in the same bucket.
    pub next: Box<[u32; MAX_ENTITIES]>,
    /// Caller-provided entity identifier per slot.
    pub entity_ids: Box<[u32; MAX_ENTITIES]>,
    /// Packed `(cell_x as u32) << 32 | (cell_y as u32)` per slot (exact cell ownership).
    pub cell_keys: Box<[u64; MAX_ENTITIES]>,
    /// Fixed-point world position per slot.
    pub positions: Box<[FixedVec2; MAX_ENTITIES]>,
}

impl<const BUCKETS: usize, const MAX_ENTITIES: usize> Default
    for SpatialIndex<BUCKETS, MAX_ENTITIES>
{
    fn default() -> Self {
        Self::new(32)
    }
}

impl<const BUCKETS: usize, const MAX_ENTITIES: usize> SpatialIndex<BUCKETS, MAX_ENTITIES> {
    /// Allocates all slabs once (zeroed, heap) and resets the bucket table.
    pub fn new(cell_size: i64) -> Self {
        assert!(BUCKETS > 0, "SpatialIndex requires at least one bucket");
        // SAFETY: all slab element types are plain integers / integer structs for which
        // the all-zero bit pattern is a valid value.
        let mut index = unsafe {
            Self {
                count: 0,
                cell_size: if cell_size > 0 { cell_size } else { 32 },
                heads: boxed_zeroed::<[u32; BUCKETS]>(),
                next: boxed_zeroed::<[u32; MAX_ENTITIES]>(),
                entity_ids: boxed_zeroed::<[u32; MAX_ENTITIES]>(),
                cell_keys: boxed_zeroed::<[u64; MAX_ENTITIES]>(),
                positions: boxed_zeroed::<[FixedVec2; MAX_ENTITIES]>(),
            }
        };
        index.heads.fill(EMPTY_INDEX);
        index
    }

    /// Number of entities currently inserted.
    #[inline(always)]
    pub fn len(&self) -> usize {
        self.count
    }

    /// `true` when no entities are inserted.
    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Maximum number of entities this index can hold.
    #[inline(always)]
    pub const fn capacity(&self) -> usize {
        MAX_ENTITIES
    }

    /// Resets the index for the next tick: one sequential head-table fill, no deallocation.
    #[inline]
    pub fn clear(&mut self) {
        self.heads.fill(EMPTY_INDEX);
        self.count = 0;
    }

    /// Converts a world coordinate (integer part) to a cell coordinate (Euclidean division).
    #[inline(always)]
    fn cell_coord(&self, world: Fixed) -> i64 {
        world.to_int().div_euclid(self.cell_size)
    }

    /// Packs a cell coordinate pair into a single exact ownership key.
    #[inline(always)]
    const fn pack_cell(cx: i64, cy: i64) -> u64 {
        ((cx as u32 as u64) << 32) | (cy as u32 as u64)
    }

    /// Hashes a cell coordinate pair into a bucket slot (pure integer mixing).
    #[inline(always)]
    fn bucket_of(cx: i64, cy: i64) -> usize {
        let hx = (cx as u64).wrapping_mul(HASH_PRIME_X);
        let hy = (cy as u64).wrapping_mul(HASH_PRIME_Y);
        let mixed = hx ^ hy.rotate_left(29);
        let folded = mixed ^ (mixed >> 32);
        if BUCKETS.is_power_of_two() {
            (folded as usize) & (BUCKETS - 1)
        } else {
            (folded % (BUCKETS as u64)) as usize
        }
    }

    /// Inserts an entity at `pos`. Returns `false` when the index is full.
    #[inline]
    pub fn insert(&mut self, entity_id: u32, pos: FixedVec2) -> bool {
        if self.count >= MAX_ENTITIES {
            return false;
        }
        let cx = self.cell_coord(pos.x);
        let cy = self.cell_coord(pos.y);
        let bucket = Self::bucket_of(cx, cy);
        let slot = self.count;

        self.entity_ids[slot] = entity_id;
        self.positions[slot] = pos;
        self.cell_keys[slot] = Self::pack_cell(cx, cy);
        self.next[slot] = self.heads[bucket];
        self.heads[bucket] = slot as u32;
        self.count += 1;
        true
    }

    /// Squared distance between two positions in raw fixed-point units (overflow-free).
    #[inline(always)]
    fn dist_sq_raw(a: FixedVec2, b: FixedVec2) -> u128 {
        let dx = (a.x.raw() - b.x.raw()) as i128;
        let dy = (a.y.raw() - b.y.raw()) as i128;
        (dx * dx + dy * dy) as u128
    }

    /// Visits every slot whose cell lies within the inclusive cell rectangle.
    ///
    /// `visit` returns `false` to stop early. Falls back to a flat linear scan when the
    /// rectangle covers more cells than there are entities (a sequential pass over the SoA
    /// slabs is cheaper than walking that many buckets).
    #[inline(always)]
    fn for_each_in_cells<F: FnMut(usize) -> bool>(
        &self,
        min_cx: i64,
        min_cy: i64,
        max_cx: i64,
        max_cy: i64,
        mut visit: F,
    ) {
        let span_x = (max_cx - min_cx + 1) as u128;
        let span_y = (max_cy - min_cy + 1) as u128;
        if span_x * span_y > self.count as u128 {
            for slot in 0..self.count {
                let key = self.cell_keys[slot];
                let cx = (key >> 32) as u32 as i32 as i64;
                let cy = key as u32 as i32 as i64;
                if cx >= min_cx && cx <= max_cx && cy >= min_cy && cy <= max_cy && !visit(slot)
                {
                    return;
                }
            }
            return;
        }

        for cy in min_cy..=max_cy {
            for cx in min_cx..=max_cx {
                let key = Self::pack_cell(cx, cy);
                let mut slot = self.heads[Self::bucket_of(cx, cy)];
                while slot != EMPTY_INDEX {
                    let s = slot as usize;
                    if self.cell_keys[s] == key && !visit(s) {
                        return;
                    }
                    slot = self.next[s];
                }
            }
        }
    }

    /// Collects entity IDs within `radius` of `center` (inclusive, exact squared-distance test).
    ///
    /// Writes into `out_buffer` and returns the number written (truncated at buffer length).
    pub fn query_radius(&self, center: FixedVec2, radius: Fixed, out_buffer: &mut [u32]) -> usize {
        if out_buffer.is_empty() || self.count == 0 {
            return 0;
        }
        let radius = if radius.raw() < 0 { Fixed::ZERO } else { radius };
        let min = FixedVec2::new(center.x - radius, center.y - radius);
        let max = FixedVec2::new(center.x + radius, center.y + radius);
        let r_sq = (radius.raw() as i128 * radius.raw() as i128) as u128;

        let mut written = 0usize;
        self.for_each_in_cells(
            self.cell_coord(min.x),
            self.cell_coord(min.y),
            self.cell_coord(max.x),
            self.cell_coord(max.y),
            |slot| {
                if Self::dist_sq_raw(self.positions[slot], center) <= r_sq {
                    out_buffer[written] = self.entity_ids[slot];
                    written += 1;
                }
                written < out_buffer.len()
            },
        );
        written
    }

    /// Collects entity IDs whose position lies inside the inclusive AABB `[min, max]`.
    ///
    /// Writes into `out_buffer` and returns the number written (truncated at buffer length).
    pub fn query_aabb(&self, min: FixedVec2, max: FixedVec2, out_buffer: &mut [u32]) -> usize {
        if out_buffer.is_empty() || self.count == 0 || min.x > max.x || min.y > max.y {
            return 0;
        }
        let mut written = 0usize;
        self.for_each_in_cells(
            self.cell_coord(min.x),
            self.cell_coord(min.y),
            self.cell_coord(max.x),
            self.cell_coord(max.y),
            |slot| {
                let p = self.positions[slot];
                if p.x >= min.x && p.x <= max.x && p.y >= min.y && p.y <= max.y {
                    out_buffer[written] = self.entity_ids[slot];
                    written += 1;
                }
                written < out_buffer.len()
            },
        );
        written
    }

    /// Finds the closest entity to `pos` within `max_radius`.
    ///
    /// Returns `(entity_id, distance)`; ties are broken deterministically by the lowest
    /// entity ID. Searches concentric cell rings so nearby hits terminate early.
    pub fn find_closest(&self, pos: FixedVec2, max_radius: Fixed) -> Option<(u32, Fixed)> {
        if self.count == 0 || max_radius.raw() < 0 {
            return None;
        }
        let origin_cx = self.cell_coord(pos.x);
        let origin_cy = self.cell_coord(pos.y);
        let max_ring = (max_radius.to_int() / self.cell_size) + 1;
        let max_sq = (max_radius.raw() as i128 * max_radius.raw() as i128) as u128;

        let mut best_sq = u128::MAX;
        let mut best_id = u32::MAX;
        let mut found = false;

        // Cells inspected if every ring is walked; beyond the entity count a linear scan wins.
        let ring_cells = (2 * max_ring as u128 + 1) * (2 * max_ring as u128 + 1);
        if ring_cells > self.count as u128 {
            for slot in 0..self.count {
                let d = Self::dist_sq_raw(self.positions[slot], pos);
                let id = self.entity_ids[slot];
                if d <= max_sq && (d < best_sq || (d == best_sq && id < best_id)) {
                    best_sq = d;
                    best_id = id;
                    found = true;
                }
            }
        } else {
            let cell_raw = Fixed::from_int(self.cell_size).raw() as i128;
            for ring in 0..=max_ring {
                // Everything in ring `r` is at least (r - 1) * cell_size away.
                if found && ring > 0 {
                    let lower = (ring as i128 - 1) * cell_raw;
                    if (lower * lower) as u128 > best_sq {
                        break;
                    }
                }
                for cy in (origin_cy - ring)..=(origin_cy + ring) {
                    let on_edge_y = cy == origin_cy - ring || cy == origin_cy + ring;
                    let mut cx = origin_cx - ring;
                    while cx <= origin_cx + ring {
                        let key = Self::pack_cell(cx, cy);
                        let mut slot = self.heads[Self::bucket_of(cx, cy)];
                        while slot != EMPTY_INDEX {
                            let s = slot as usize;
                            if self.cell_keys[s] == key {
                                let d = Self::dist_sq_raw(self.positions[s], pos);
                                let id = self.entity_ids[s];
                                if d <= max_sq && (d < best_sq || (d == best_sq && id < best_id))
                                {
                                    best_sq = d;
                                    best_id = id;
                                    found = true;
                                }
                            }
                            slot = self.next[s];
                        }
                        // Interior rows only visit the two edge columns.
                        cx += if on_edge_y || ring == 0 { 1 } else { 2 * ring };
                    }
                }
            }
        }

        if found {
            Some((best_id, Fixed::from_raw(isqrt_u128(best_sq) as i64)))
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;

    /// Deterministic integer LCG for test placement (no `rand`).
    fn lcg(state: &mut u64) -> u64 {
        *state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        *state >> 33
    }

    #[test]
    fn test_spatial_index_layout_and_reset() {
        assert_eq!(core::mem::align_of::<SpatialIndex>(), 64);
        assert_eq!(isqrt_u128(0), 0);
        assert_eq!(isqrt_u128(1), 1);
        assert_eq!(isqrt_u128(15), 3);
        assert_eq!(isqrt_u128(16), 4);
        assert_eq!(isqrt_u128(1 << 64), 1 << 32);

        let mut index = SpatialIndex::<64, 8>::new(10);
        assert!(index.is_empty());
        assert_eq!(index.capacity(), 8);
        for i in 0..8 {
            assert!(index.insert(i, FixedVec2::from_ints(i as i64, 0)));
        }
        assert!(!index.insert(99, FixedVec2::ZERO), "capacity must be enforced");
        assert_eq!(index.len(), 8);
        index.clear();
        assert!(index.is_empty());
        assert!(index.heads.iter().all(|&h| h == EMPTY_INDEX));
        let mut out = [0u32; 8];
        assert_eq!(index.query_radius(FixedVec2::ZERO, Fixed::from_int(100), &mut out), 0);
    }

    #[test]
    fn test_spatial_index_100k_entities() {
        const N: usize = 100_000;
        let mut index = SpatialIndex::<16_384, N>::new(32);
        let mut seed = 0x5EED_u64;
        for i in 0..N {
            let x = (lcg(&mut seed) % 4096) as i64 - 2048; // negative coordinates included
            let y = (lcg(&mut seed) % 4096) as i64 - 2048;
            assert!(index.insert(i as u32, FixedVec2::from_ints(x, y)));
        }
        assert_eq!(index.len(), N);

        let centers = [
            FixedVec2::from_ints(0, 0),
            FixedVec2::from_ints(-1500, 733),
            FixedVec2::from_ints(2047, -2048),
            FixedVec2::from_ints(17, -17),
        ];
        let radius = Fixed::from_int(48);
        let r_sq = radius.raw() as i128 * radius.raw() as i128;
        let mut out = [0u32; 4096];

        for center in centers {
            let found = index.query_radius(center, radius, &mut out);
            let mut expected: Vec<u32> = (0..N)
                .filter(|&s| {
                    let p = index.positions[s];
                    let dx = (p.x.raw() - center.x.raw()) as i128;
                    let dy = (p.y.raw() - center.y.raw()) as i128;
                    dx * dx + dy * dy <= r_sq
                })
                .map(|s| index.entity_ids[s])
                .collect();
            let mut got = out[..found].to_vec();
            got.sort_unstable();
            expected.sort_unstable();
            assert_eq!(got, expected, "radius query mismatch at {:?}", center);
            assert!(found > 0, "dense 100k field must yield hits");
        }

        // Second tick: clear + reinsert produces an identical answer (determinism).
        let before = index.query_radius(centers[1], radius, &mut out);
        let mut snapshot = out[..before].to_vec();
        snapshot.sort_unstable();
        index.clear();
        let mut seed = 0x5EED_u64;
        for i in 0..N {
            let x = (lcg(&mut seed) % 4096) as i64 - 2048;
            let y = (lcg(&mut seed) % 4096) as i64 - 2048;
            index.insert(i as u32, FixedVec2::from_ints(x, y));
        }
        let after = index.query_radius(centers[1], radius, &mut out);
        let mut replay = out[..after].to_vec();
        replay.sort_unstable();
        assert_eq!(snapshot, replay);
    }

    #[test]
    fn test_spatial_index_aabb_and_closest() {
        let mut index = SpatialIndex::<256, 64>::new(10);
        index.insert(1, FixedVec2::from_ints(5, 5));
        index.insert(2, FixedVec2::from_ints(25, 5));
        index.insert(3, FixedVec2::from_ints(-15, -15));
        index.insert(4, FixedVec2::from_ints(100, 100));
        index.insert(5, FixedVec2::from_ints(8, 9));

        let mut out = [0u32; 16];

        // Inclusive AABB covering the first-quadrant cluster only.
        let n = index.query_aabb(
            FixedVec2::from_ints(0, 0),
            FixedVec2::from_ints(25, 10),
            &mut out,
        );
        let mut got = out[..n].to_vec();
        got.sort_unstable();
        assert_eq!(got, [1, 2, 5]);

        // AABB spanning negative space.
        let n = index.query_aabb(
            FixedVec2::from_ints(-20, -20),
            FixedVec2::from_ints(-10, -10),
            &mut out,
        );
        assert_eq!(&out[..n], &[3]);

        // Inverted AABB and tiny buffer edge cases.
        assert_eq!(
            index.query_aabb(FixedVec2::from_ints(5, 5), FixedVec2::from_ints(0, 0), &mut out),
            0
        );
        let mut one = [0u32; 1];
        assert_eq!(
            index.query_aabb(
                FixedVec2::from_ints(-1000, -1000),
                FixedVec2::from_ints(1000, 1000),
                &mut one
            ),
            1
        );

        // Closest to (6, 6): entity 1 at (5,5) is sqrt(2) away, entity 5 at (8,9) is sqrt(13).
        let (id, dist) = index
            .find_closest(FixedVec2::from_ints(6, 6), Fixed::from_int(50))
            .expect("hit expected");
        assert_eq!(id, 1);
        // sqrt(2) in Q32.16 == 92,681 raw (1.41421...)
        assert_eq!(dist.raw(), 92_681);

        // Radius too small: nothing.
        assert!(index
            .find_closest(FixedVec2::from_ints(60, 60), Fixed::from_int(5))
            .is_none());

        // Far query reaching only entity 4 via ring expansion.
        let (id, dist) = index
            .find_closest(FixedVec2::from_ints(90, 100), Fixed::from_int(500))
            .expect("hit expected");
        assert_eq!(id, 4);
        assert_eq!(dist, Fixed::from_int(10));

        // Deterministic tie-break: equidistant entities -> lowest ID wins.
        index.insert(7, FixedVec2::from_ints(-15, -17));
        index.insert(6, FixedVec2::from_ints(-15, -13));
        let (id, dist) = index
            .find_closest(FixedVec2::from_ints(-15, -15), Fixed::from_int(3))
            .expect("hit expected");
        assert_eq!(id, 3);
        assert_eq!(dist, Fixed::ZERO);
        let (id, dist) = index
            .find_closest(FixedVec2::from_ints(-15, -16), Fixed::from_int(3))
            .expect("hit expected");
        assert_eq!(id, 3, "tie between 3 and 7 must resolve to the lower ID");
        assert_eq!(dist, Fixed::from_int(1));
    }
}
