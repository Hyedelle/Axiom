//! # Deterministic Infinite Chunk Streaming & Spatial Terrain Indexer
//!
//! Provides fixed-size 32x32 procedural chunks, zero-floating-point terrain noise,
//! and fixed-slab chunk streaming (`ChunkManager`) for open-world infinite exploration.

use crate::fixed::FixedVec2;
use crate::hash::deterministic_hash;
use crate::pcg::TileType;

pub const DEFAULT_CHUNK_SIZE: usize = 32;
pub const DEFAULT_MAX_CHUNKS: usize = 32;
pub const DEFAULT_VIEW_RADIUS: i32 = 2; // (2*2 + 1)^2 = 25 chunks in view

/// 2D discrete chunk coordinate in the infinite world.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(C)]
pub struct ChunkCoord {
    pub x: i32,
    pub y: i32,
}

impl ChunkCoord {
    #[inline(always)]
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }

    /// Chebyshev distance (max(|dx|, |dy|)) between two chunk coordinates.
    #[inline(always)]
    pub fn chebyshev_dist(self, other: Self) -> i32 {
        let dx = (self.x - other.x).abs();
        let dy = (self.y - other.y).abs();
        if dx > dy {
            dx
        } else {
            dy
        }
    }

    /// Converts world tile coordinates `(x, y)` to chunk coordinates.
    #[inline(always)]
    pub fn from_world_tile(tile_x: i64, tile_y: i64, chunk_size: usize) -> (Self, usize, usize) {
        let cs = chunk_size as i64;
        let cx = tile_x.div_euclid(cs) as i32;
        let cy = tile_y.div_euclid(cs) as i32;
        let lx = tile_x.rem_euclid(cs) as usize;
        let ly = tile_y.rem_euclid(cs) as usize;
        (Self::new(cx, cy), lx, ly)
    }

    /// Converts world fixed-point position to chunk coordinate and local tile offsets.
    #[inline(always)]
    pub fn from_world_pos(pos: FixedVec2, chunk_size: usize) -> (Self, usize, usize) {
        let tx = pos.x.to_int();
        let ty = pos.y.to_int();
        Self::from_world_tile(tx, ty, chunk_size)
    }
}

/// Fixed-size terrain chunk holding 32x32 tiles.
///
/// 64-byte aligned for optimal CPU cache-line hardware transfer.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[repr(C, align(64))]
pub struct Chunk<const S: usize = DEFAULT_CHUNK_SIZE> {
    pub coord: ChunkCoord,
    pub tiles: [[TileType; S]; S],
    pub dirty: bool,
    pub generation_tick: u64,
}

impl<const S: usize> Default for Chunk<S> {
    fn default() -> Self {
        Self {
            coord: ChunkCoord::new(0, 0),
            tiles: [[TileType::Plains; S]; S],
            dirty: false,
            generation_tick: 0,
        }
    }
}

impl<const S: usize> Chunk<S> {
    /// Deterministically generates chunk terrain from world seed and chunk coordinate.
    pub fn generate(world_seed: u64, coord: ChunkCoord, tick: u64) -> Self {
        let mut chunk = Self {
            coord,
            tiles: [[TileType::Plains; S]; S],
            dirty: false,
            generation_tick: tick,
        };

        let origin_x = (coord.x as i64).wrapping_mul(S as i64);
        let origin_y = (coord.y as i64).wrapping_mul(S as i64);

        for ly in 0..S {
            for lx in 0..S {
                let wx = (origin_x + lx as i64) as i32;
                let wy = (origin_y + ly as i64) as i32;
                chunk.tiles[ly][lx] = sample_world_tile(wx, wy, world_seed);
            }
        }

        chunk
    }

    /// Retrieves tile at local coordinates `(lx, ly)`.
    #[inline(always)]
    pub fn tile_at(&self, lx: usize, ly: usize) -> Option<TileType> {
        if lx < S && ly < S {
            Some(self.tiles[ly][lx])
        } else {
            None
        }
    }

    /// Sets tile at local coordinates `(lx, ly)` and marks chunk dirty.
    #[inline(always)]
    pub fn set_tile(&mut self, lx: usize, ly: usize, tile: TileType) -> bool {
        if lx < S && ly < S {
            self.tiles[ly][lx] = tile;
            self.dirty = true;
            true
        } else {
            false
        }
    }

    /// Computes deterministic 64-bit hash of this chunk's coordinate and terrain content.
    pub fn compute_hash(&self) -> u64 {
        deterministic_hash(&(self.coord, &self.tiles))
    }
}

/// Deterministic 2D integer hash producing a 32-bit pseudo-random value.
#[inline(always)]
pub fn hash2d(x: i32, y: i32, seed: u64) -> u32 {
    let mut h = seed;
    h = h.wrapping_add((x as i64 as u64).wrapping_mul(0x9E3779B97F4A7C15));
    h = (h ^ (h >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
    h = h.wrapping_add((y as i64 as u64).wrapping_mul(0x94D049BB133111EB));
    h = (h ^ (h >> 27)).wrapping_mul(0x9E3779B97F4A7C15);
    (h ^ (h >> 31)) as u32
}

/// Bilinear integer noise at a power-of-two scale.
#[inline]
fn sample_value_noise(x: i32, y: i32, scale: i32, seed: u64) -> u32 {
    let gx0 = x.div_euclid(scale);
    let gy0 = y.div_euclid(scale);
    let gx1 = gx0 + 1;
    let gy1 = gy0 + 1;

    let fx = x.rem_euclid(scale);
    let fy = y.rem_euclid(scale);

    let v00 = (hash2d(gx0, gy0, seed) & 0xFFFF) as i64;
    let v10 = (hash2d(gx1, gy0, seed) & 0xFFFF) as i64;
    let v01 = (hash2d(gx0, gy1, seed) & 0xFFFF) as i64;
    let v11 = (hash2d(gx1, gy1, seed) & 0xFFFF) as i64;

    let s = scale as i64;
    let ix0 = v00 + ((v10 - v00) * fx as i64) / s;
    let ix1 = v01 + ((v11 - v01) * fx as i64) / s;

    let val = ix0 + ((ix1 - ix0) * fy as i64) / s;
    val as u32
}

/// Determines the terrain tile at world coordinate `(wx, wy)` with zero floating point.
#[inline]
pub fn sample_world_tile(wx: i32, wy: i32, seed: u64) -> TileType {
    // Multi-octave elevation
    let elev_low = sample_value_noise(wx, wy, 32, seed);
    let elev_high = sample_value_noise(wx, wy, 8, seed.wrapping_add(101));
    let elevation = (elev_low * 3 + elev_high) / 4;

    // Moisture map
    let moisture = sample_value_noise(wx, wy, 16, seed.wrapping_add(54321));

    if elevation < 18_000 {
        TileType::Water
    } else if elevation > 48_000 {
        TileType::Rock
    } else if moisture > 36_000 {
        TileType::Forest
    } else {
        TileType::Plains
    }
}

/// Slot in the ChunkManager's flat slab pool.
#[derive(Debug, Clone, PartialEq, Eq)]
#[repr(C, align(64))]
pub struct ChunkSlot<const S: usize = DEFAULT_CHUNK_SIZE> {
    pub chunk: Chunk<S>,
    pub active: bool,
    pub last_accessed_tick: u64,
}

impl<const S: usize> Default for ChunkSlot<S> {
    fn default() -> Self {
        Self {
            chunk: Chunk::default(),
            active: false,
            last_accessed_tick: 0,
        }
    }
}

/// Streaming metrics returned on each spatial update.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ChunkStreamStats {
    pub loaded_count: usize,
    pub generated_chunks: u32,
    pub evicted_chunks: u32,
}

/// Fixed-slab manager for procedural chunk streaming and spatial querying.
///
/// Guaranteed zero dynamic allocations during world exploration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkManager<const S: usize = DEFAULT_CHUNK_SIZE, const MAX_CHUNKS: usize = DEFAULT_MAX_CHUNKS> {
    pub slots: [ChunkSlot<S>; MAX_CHUNKS],
    pub world_seed: u64,
    pub center_chunk: ChunkCoord,
    pub view_radius: i32,
    pub total_generated: u64,
    pub total_evicted: u64,
}

impl<const S: usize, const MAX_CHUNKS: usize> ChunkManager<S, MAX_CHUNKS> {
    /// Creates a new uninitialized chunk manager.
    pub fn new(world_seed: u64, view_radius: i32) -> Self {
        // SAFETY: Memory layout matches ChunkSlot<S> for default S
        let mut mgr = Self {
            slots: unsafe { core::mem::zeroed() },
            world_seed,
            center_chunk: ChunkCoord::new(i32::MIN, i32::MIN), // force initial load
            view_radius,
            total_generated: 0,
            total_evicted: 0,
        };

        // Ensure active flags are false
        for slot in &mut mgr.slots {
            slot.active = false;
        }

        mgr
    }

    /// Finds index of loaded chunk at `coord`.
    #[inline]
    pub fn find_chunk_index(&self, coord: ChunkCoord) -> Option<usize> {
        for (i, slot) in self.slots.iter().enumerate() {
            if slot.active && slot.chunk.coord == coord {
                return Some(i);
            }
        }
        None
    }

    /// References chunk at `coord` if currently loaded.
    #[inline]
    pub fn chunk_at(&self, coord: ChunkCoord) -> Option<&Chunk<S>> {
        self.find_chunk_index(coord).map(|idx| &self.slots[idx].chunk)
    }

    /// Mutably references chunk at `coord` if currently loaded.
    #[inline]
    pub fn chunk_at_mut(&mut self, coord: ChunkCoord) -> Option<&mut Chunk<S>> {
        if let Some(idx) = self.find_chunk_index(coord) {
            Some(&mut self.slots[idx].chunk)
        } else {
            None
        }
    }

    /// Number of active loaded chunks.
    #[inline]
    pub fn loaded_count(&self) -> usize {
        self.slots.iter().filter(|s| s.active).count()
    }

    /// Updates active chunks centered at `center_coord`.
    ///
    /// Loads missing chunks within `view_radius` and unloads out-of-range chunks.
    pub fn update_center(&mut self, center_coord: ChunkCoord, tick: u64) -> ChunkStreamStats {
        let mut stats = ChunkStreamStats::default();
        self.center_chunk = center_coord;

        // 1. Evict chunks outside view_radius
        for slot in &mut self.slots {
            if slot.active && slot.chunk.coord.chebyshev_dist(center_coord) > self.view_radius {
                slot.active = false;
                self.total_evicted += 1;
                stats.evicted_chunks += 1;
            }
        }

        // 2. Load missing chunks in view window
        for dy in -self.view_radius..=self.view_radius {
            for dx in -self.view_radius..=self.view_radius {
                let target_coord = ChunkCoord::new(center_coord.x + dx, center_coord.y + dy);

                if let Some(idx) = self.find_chunk_index(target_coord) {
                    self.slots[idx].last_accessed_tick = tick;
                } else {
                    // Need to allocate slot and generate chunk
                    let free_idx = self.find_free_or_lru_slot(center_coord);
                    let chunk = Chunk::generate(self.world_seed, target_coord, tick);

                    if self.slots[free_idx].active {
                        self.total_evicted += 1;
                        stats.evicted_chunks += 1;
                    }

                    self.slots[free_idx] = ChunkSlot {
                        chunk,
                        active: true,
                        last_accessed_tick: tick,
                    };

                    self.total_generated += 1;
                    stats.generated_chunks += 1;
                }
            }
        }

        stats.loaded_count = self.loaded_count();
        stats
    }

    /// Updates chunk streaming based on a world position.
    #[inline]
    pub fn update_player_pos(&mut self, pos: FixedVec2, tick: u64) -> ChunkStreamStats {
        let (coord, _, _) = ChunkCoord::from_world_pos(pos, S);
        self.update_center(coord, tick)
    }

    /// Queries the terrain tile at world integer coordinate `(tile_x, tile_y)`.
    ///
    /// If the chunk is currently loaded, returns tile from chunk;
    /// otherwise samples procedural generation directly.
    #[inline]
    pub fn tile_at(&self, tile_x: i64, tile_y: i64) -> TileType {
        let (coord, lx, ly) = ChunkCoord::from_world_tile(tile_x, tile_y, S);
        if let Some(chunk) = self.chunk_at(coord) {
            chunk.tiles[ly][lx]
        } else {
            sample_world_tile(tile_x as i32, tile_y as i32, self.world_seed)
        }
    }

    /// Sets tile at world coordinate and marks corresponding chunk dirty.
    pub fn set_tile(&mut self, tile_x: i64, tile_y: i64, tile: TileType) -> bool {
        let (coord, lx, ly) = ChunkCoord::from_world_tile(tile_x, tile_y, S);
        if let Some(chunk) = self.chunk_at_mut(coord) {
            chunk.set_tile(lx, ly, tile)
        } else {
            false
        }
    }

    /// Checks if a world position is walkable.
    #[inline]
    pub fn is_walkable_pos(&self, pos: FixedVec2) -> bool {
        let tile = self.tile_at(pos.x.to_int(), pos.y.to_int());
        tile.is_walkable()
    }

    /// Finds an empty slot or the furthest LRU slot for eviction.
    fn find_free_or_lru_slot(&self, center: ChunkCoord) -> usize {
        // Priority 1: Inactive slot
        for (i, slot) in self.slots.iter().enumerate() {
            if !slot.active {
                return i;
            }
        }

        // Priority 2: Furthest slot by distance, break ties by LRU tick
        let mut best_idx = 0;
        let mut max_dist = -1;
        let mut oldest_tick = u64::MAX;

        for (i, slot) in self.slots.iter().enumerate() {
            let dist = slot.chunk.coord.chebyshev_dist(center);
            if dist > max_dist || (dist == max_dist && slot.last_accessed_tick < oldest_tick) {
                max_dist = dist;
                oldest_tick = slot.last_accessed_tick;
                best_idx = i;
            }
        }

        best_idx
    }

    /// Computes deterministic hash across all loaded chunks.
    pub fn compute_hash(&self) -> u64 {
        let mut h = self.world_seed;
        for slot in &self.slots {
            if slot.active {
                h ^= slot.chunk.compute_hash().wrapping_mul(0x9E3779B97F4A7C15);
            }
        }
        h
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chunk_coord_conversion() {
        // (0, 0)
        let (c, lx, ly) = ChunkCoord::from_world_tile(0, 0, 32);
        assert_eq!(c, ChunkCoord::new(0, 0));
        assert_eq!((lx, ly), (0, 0));

        // (31, 31)
        let (c, lx, ly) = ChunkCoord::from_world_tile(31, 31, 32);
        assert_eq!(c, ChunkCoord::new(0, 0));
        assert_eq!((lx, ly), (31, 31));

        // (32, 0) -> Next chunk
        let (c, lx, ly) = ChunkCoord::from_world_tile(32, 0, 32);
        assert_eq!(c, ChunkCoord::new(1, 0));
        assert_eq!((lx, ly), (0, 0));

        // (-1, -1) -> Negative chunk
        let (c, lx, ly) = ChunkCoord::from_world_tile(-1, -1, 32);
        assert_eq!(c, ChunkCoord::new(-1, -1));
        assert_eq!((lx, ly), (31, 31));
    }

    #[test]
    fn test_chunk_determinism() {
        let seed = 42;
        let coord = ChunkCoord::new(5, -3);

        let chunk1 = Chunk::<32>::generate(seed, coord, 10);
        let chunk2 = Chunk::<32>::generate(seed, coord, 10);

        assert_eq!(chunk1, chunk2);
        assert_eq!(chunk1.compute_hash(), chunk2.compute_hash());
    }

    #[test]
    fn test_chunk_manager_streaming() {
        let mut mgr = ChunkManager::<32, 32>::new(12345, 2);

        // View radius = 2 -> (2*2 + 1)^2 = 25 chunks loaded
        let stats = mgr.update_center(ChunkCoord::new(0, 0), 1);
        assert_eq!(stats.loaded_count, 25);
        assert_eq!(stats.generated_chunks, 25);
        assert_eq!(mgr.loaded_count(), 25);

        // Center chunk must be accessible
        assert!(mgr.chunk_at(ChunkCoord::new(0, 0)).is_some());
        assert!(mgr.chunk_at(ChunkCoord::new(2, 2)).is_some());
        assert!(mgr.chunk_at(ChunkCoord::new(3, 0)).is_none()); // Out of radius

        // Move center 1 chunk to the right (ChunkCoord(1, 0))
        let stats2 = mgr.update_center(ChunkCoord::new(1, 0), 2);
        assert_eq!(mgr.loaded_count(), 25);
        // Column x = -2 should be evicted (5 chunks), Column x = 3 loaded (5 chunks)
        assert_eq!(stats2.evicted_chunks, 5);
        assert_eq!(stats2.generated_chunks, 5);

        assert!(mgr.chunk_at(ChunkCoord::new(-2, 0)).is_none());
        assert!(mgr.chunk_at(ChunkCoord::new(3, 0)).is_some());
    }
}
