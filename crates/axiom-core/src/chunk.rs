//! # Deterministic Infinite Chunk Streaming & Biome PCG Pipeline (Phase 3)
//!
//! Provides fixed-size 32x32 procedural chunks carrying terrain, biome and resource
//! layers, a zero-allocation player-modification delta slab that survives chunk
//! eviction, and a fixed-slab, O(1)-indexed streaming manager (`ChunkManager`) for
//! open-world exploration over the full `i32` coordinate range.

use crate::fixed::FixedVec2;
use crate::hash::{deterministic_hash, Fnv1aHasher};
pub use crate::pcg::{hash2d, sample_value_noise, sample_world_biome, sample_world_tile};
use crate::pcg::{
    compose_biome_sample, fill_noise_row, layer_seed, BiomeKind, BiomeNoiseContext, BiomeSample,
    ResourceDeposit, TileType, NOISE_LAYER_COUNT, NOISE_LAYER_SHIFTS,
};

pub const DEFAULT_CHUNK_SIZE: usize = 32;
pub const DEFAULT_MAX_CHUNKS: usize = 64;
pub const DEFAULT_VIEW_RADIUS: i32 = 2; // (2*2 + 1)^2 = 25 chunks in view
/// Default capacity of the player-modification delta slab (entries).
pub const DEFAULT_DELTA_CAPACITY: usize = 2048;
/// Fixed bucket count of the slot index table (must exceed `MAX_CHUNKS`).
pub const CHUNK_INDEX_BUCKETS: usize = 512;
/// Empty marker inside the slot index table.
pub const CHUNK_INDEX_EMPTY: u16 = u16::MAX;
/// Largest navigation window edge accepted by [`ChunkManager::find_open_spawn`].
pub const SPAWN_WINDOW_MAX: usize = 64;
/// Default open-area requirement (tiles) for [`ChunkManager::find_open_spawn`].
pub const DEFAULT_SPAWN_MIN_OPEN_AREA: usize = 1024;

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
        let dx = (self.x as i64 - other.x as i64).abs();
        let dy = (self.y as i64 - other.y as i64).abs();
        let d = if dx > dy { dx } else { dy };
        if d > i32::MAX as i64 {
            i32::MAX
        } else {
            d as i32
        }
    }

    /// Packs the coordinate into a single 64-bit key (bit-exact, collision free).
    #[inline(always)]
    pub const fn key(self) -> u64 {
        ((self.x as u32 as u64) << 32) | (self.y as u32 as u64)
    }

    /// Deterministic bucket hash for slab indexing.
    #[inline(always)]
    pub const fn bucket_hash(self) -> u64 {
        let mut h = self.key().wrapping_mul(0x9E3779B97F4A7C15);
        h = (h ^ (h >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
        h = (h ^ (h >> 27)).wrapping_mul(0x94D049BB133111EB);
        h ^ (h >> 31)
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

/// 64-byte cache-line header describing one streamed chunk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(C, align(64))]
pub struct ChunkHeader {
    pub coord: ChunkCoord,
    /// Most frequent biome inside the chunk (lowest id wins ties).
    pub primary_biome: BiomeKind,
    /// True once any tile diverges from the pristine procedural output.
    pub dirty: bool,
    pub _reserved0: [u8; 2],
    /// Number of `set_tile` edits applied to this chunk instance.
    pub modification_count: u32,
    pub generation_tick: u64,
    /// Rolling deterministic hash: content hash at generation, mixed per edit.
    pub state_hash: u64,
    /// Tile count per biome (`BiomeKind as usize`).
    pub biome_histogram: [u16; BiomeKind::COUNT],
    pub deposit_tile_count: u16,
    pub water_tile_count: u16,
    pub walkable_tile_count: u16,
    pub _reserved1: u16,
}

impl Default for ChunkHeader {
    fn default() -> Self {
        Self {
            coord: ChunkCoord::new(0, 0),
            primary_biome: BiomeKind::TemperatePlains,
            dirty: false,
            _reserved0: [0; 2],
            modification_count: 0,
            generation_tick: 0,
            state_hash: 0,
            biome_histogram: [0; BiomeKind::COUNT],
            deposit_tile_count: 0,
            water_tile_count: 0,
            walkable_tile_count: 0,
            _reserved1: 0,
        }
    }
}

/// Packs a deposit kind (3 bits) and richness level (5 bits) into one byte.
#[inline(always)]
pub const fn pack_resource(kind: ResourceDeposit, richness: u16) -> u8 {
    let level = if richness == 0 {
        0
    } else {
        let l = (richness >> 6) as u8; // 0..=31 for richness < 2048
        if l == 0 {
            1
        } else if l > 31 {
            31
        } else {
            l
        }
    };
    ((kind as u8 & 7) << 5) | level
}

/// Unpacks a resource byte into `(kind, richness_level 0..=31)`.
#[inline(always)]
pub const fn unpack_resource(packed: u8) -> (ResourceDeposit, u8) {
    (ResourceDeposit::from_u8(packed >> 5), packed & 0x1F)
}

/// Fixed-size terrain chunk holding `S x S` tiles with biome and resource layers.
///
/// 64-byte aligned for optimal CPU cache-line hardware transfer.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[repr(C, align(64))]
pub struct Chunk<const S: usize = DEFAULT_CHUNK_SIZE> {
    pub header: ChunkHeader,
    pub tiles: [[TileType; S]; S],
    /// `BiomeKind as u8` per tile.
    pub biomes: [[u8; S]; S],
    /// Packed `(ResourceDeposit, richness level)` per tile, see [`pack_resource`].
    pub resources: [[u8; S]; S],
}

impl<const S: usize> Default for Chunk<S> {
    fn default() -> Self {
        Self {
            header: ChunkHeader::default(),
            tiles: [[TileType::Plains; S]; S],
            biomes: [[0; S]; S],
            resources: [[0; S]; S],
        }
    }
}

impl<const S: usize> Chunk<S> {
    /// Deterministically generates chunk terrain, biomes and deposits from the
    /// world seed and chunk coordinate.
    pub fn generate(world_seed: u64, coord: ChunkCoord, tick: u64) -> Self {
        let mut chunk = Self::default();
        chunk.header.coord = coord;
        chunk.header.generation_tick = tick;

        let origin_x = (coord.x as i64).wrapping_mul(S as i64);
        let origin_y = (coord.y as i64).wrapping_mul(S as i64);

        let mut ctx = BiomeNoiseContext::new();
        let mut rows = [[0u32; S]; NOISE_LAYER_COUNT];
        let mut histogram = [0u32; BiomeKind::COUNT];
        let mut deposits = 0u32;
        let mut water = 0u32;
        let mut walkable = 0u32;
        let wx0 = origin_x as i32;

        for ly in 0..S {
            let wy = (origin_y + ly as i64) as i32;

            // Bulk-sample every noise layer for this row (lattice runs, one hash per cell).
            let mut layer = 0usize;
            while layer < NOISE_LAYER_COUNT {
                fill_noise_row(
                    &mut rows[layer],
                    wx0,
                    wy,
                    NOISE_LAYER_SHIFTS[layer],
                    layer_seed(world_seed, layer),
                    &mut ctx.cells[layer],
                );
                layer += 1;
            }

            for lx in 0..S {
                let wx = wx0.wrapping_add(lx as i32);
                let mut fields = [0u32; NOISE_LAYER_COUNT];
                let mut layer = 0usize;
                while layer < NOISE_LAYER_COUNT {
                    fields[layer] = rows[layer][lx];
                    layer += 1;
                }
                let s: BiomeSample =
                    compose_biome_sample(wx, wy, world_seed, &fields, &mut ctx.veins);
                chunk.tiles[ly][lx] = s.tile;
                chunk.biomes[ly][lx] = s.biome.as_u8();
                chunk.resources[ly][lx] = pack_resource(s.deposit_kind, s.deposit_richness);
                histogram[s.biome as usize] += 1;
                if !s.deposit_kind.is_none() {
                    deposits += 1;
                }
                if s.tile == TileType::Water {
                    water += 1;
                }
                if s.tile.is_walkable() {
                    walkable += 1;
                }
            }
        }

        let mut primary = 0usize;
        for (i, &count) in histogram.iter().enumerate() {
            if count > histogram[primary] {
                primary = i;
            }
            chunk.header.biome_histogram[i] = count.min(u16::MAX as u32) as u16;
        }
        chunk.header.primary_biome = BiomeKind::from_u8(primary as u8);
        chunk.header.deposit_tile_count = deposits.min(u16::MAX as u32) as u16;
        chunk.header.water_tile_count = water.min(u16::MAX as u32) as u16;
        chunk.header.walkable_tile_count = walkable.min(u16::MAX as u32) as u16;
        chunk.header.state_hash = chunk.compute_hash();
        chunk
    }

    #[inline(always)]
    pub const fn coord(&self) -> ChunkCoord {
        self.header.coord
    }

    #[inline(always)]
    pub const fn is_dirty(&self) -> bool {
        self.header.dirty
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

    /// Retrieves biome at local coordinates `(lx, ly)`.
    #[inline(always)]
    pub fn biome_at(&self, lx: usize, ly: usize) -> Option<BiomeKind> {
        if lx < S && ly < S {
            Some(BiomeKind::from_u8(self.biomes[ly][lx]))
        } else {
            None
        }
    }

    /// Retrieves `(deposit kind, richness level 0..=31)` at local coordinates.
    #[inline(always)]
    pub fn deposit_at(&self, lx: usize, ly: usize) -> Option<(ResourceDeposit, u8)> {
        if lx < S && ly < S {
            Some(unpack_resource(self.resources[ly][lx]))
        } else {
            None
        }
    }

    /// Sets tile at local coordinates `(lx, ly)`, marks the chunk dirty and folds
    /// the edit into the rolling `state_hash`.
    #[inline]
    pub fn set_tile(&mut self, lx: usize, ly: usize, tile: TileType) -> bool {
        if lx < S && ly < S {
            self.tiles[ly][lx] = tile;
            self.header.dirty = true;
            self.header.modification_count = self.header.modification_count.wrapping_add(1);
            let edit = ((lx as u64) << 40)
                | ((ly as u64) << 24)
                | ((tile as u8 as u64) << 16)
                | (self.header.modification_count as u64 & 0xFFFF);
            self.header.state_hash ^= edit.wrapping_mul(0x9E3779B97F4A7C15).rotate_left(17);
            true
        } else {
            false
        }
    }

    /// Computes deterministic 64-bit FNV-1a hash of coordinate, terrain, biome and
    /// resource layers (flat byte sweep, no per-element dispatch).
    pub fn compute_hash(&self) -> u64 {
        use core::hash::Hasher;
        let mut hasher = Fnv1aHasher::new();
        hasher.write(&self.header.coord.x.to_le_bytes());
        hasher.write(&self.header.coord.y.to_le_bytes());
        // SAFETY: TileType is `repr(u8)`; the 2D array is a contiguous S*S byte block.
        let tile_bytes =
            unsafe { core::slice::from_raw_parts(self.tiles.as_ptr() as *const u8, S * S) };
        hasher.write(tile_bytes);
        for row in self.biomes.iter() {
            hasher.write(row);
        }
        for row in self.resources.iter() {
            hasher.write(row);
        }
        hasher.finish()
    }
}

// ---------------------------------------------------------------------------
// Player modification delta slab
// ---------------------------------------------------------------------------

/// One preserved player edit (16 bytes, POD). Survives chunk eviction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(C)]
pub struct ChunkDelta {
    /// `ChunkCoord::key()` of the owning chunk.
    pub chunk_key: u64,
    /// Local tile index `ly * S + lx`.
    pub local_index: u16,
    /// Tile value to re-apply on regeneration.
    pub tile: u8,
    /// `1` when the entry is occupied.
    pub occupied: u8,
    /// Sequence number of the most recent write (for replay ordering / debugging).
    pub sequence: u32,
}

impl ChunkDelta {
    #[inline(always)]
    pub const fn is_occupied(&self) -> bool {
        self.occupied != 0
    }

    #[inline(always)]
    pub const fn coord(&self) -> ChunkCoord {
        ChunkCoord::new((self.chunk_key >> 32) as u32 as i32, self.chunk_key as u32 as i32)
    }

    #[inline(always)]
    pub const fn tile_type(&self) -> TileType {
        TileType::from_u8(self.tile)
    }
}

/// Fixed-slab open-addressing map of player tile edits keyed by `(chunk, local tile)`.
///
/// Zero heap allocation; when full, further edits of *new* tiles are rejected
/// (`record` returns `false`) while edits of already tracked tiles always succeed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkDeltaMap<const CAP: usize = DEFAULT_DELTA_CAPACITY> {
    pub entries: [ChunkDelta; CAP],
    pub len: usize,
    pub next_sequence: u32,
    pub rejected: u64,
}

impl<const CAP: usize> Default for ChunkDeltaMap<CAP> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const CAP: usize> ChunkDeltaMap<CAP> {
    pub const fn new() -> Self {
        Self {
            entries: [ChunkDelta {
                chunk_key: 0,
                local_index: 0,
                tile: 0,
                occupied: 0,
                sequence: 0,
            }; CAP],
            len: 0,
            next_sequence: 0,
            rejected: 0,
        }
    }

    #[inline(always)]
    fn bucket(chunk_key: u64, local_index: u16) -> usize {
        let mut h = chunk_key ^ ((local_index as u64).wrapping_mul(0xD6E8_FEB8_6659_FD93));
        h = (h ^ (h >> 32)).wrapping_mul(0x9E3779B97F4A7C15);
        h = (h ^ (h >> 29)).wrapping_mul(0xBF58476D1CE4E5B9);
        ((h ^ (h >> 32)) % (CAP as u64)) as usize
    }

    #[inline(always)]
    pub const fn len(&self) -> usize {
        self.len
    }

    #[inline(always)]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    #[inline(always)]
    pub const fn capacity(&self) -> usize {
        CAP
    }

    /// Clears every entry (O(CAP), no allocation).
    pub fn clear(&mut self) {
        for e in self.entries.iter_mut() {
            *e = ChunkDelta::default();
        }
        self.len = 0;
        self.next_sequence = 0;
        self.rejected = 0;
    }

    /// Finds the slot holding `(coord, local_index)`, or the first free probe slot.
    #[inline]
    fn probe(&self, chunk_key: u64, local_index: u16) -> (usize, bool) {
        if CAP == 0 {
            return (0, false);
        }
        let mut idx = Self::bucket(chunk_key, local_index);
        for _ in 0..CAP {
            let e = &self.entries[idx];
            if !e.is_occupied() {
                return (idx, false);
            }
            if e.chunk_key == chunk_key && e.local_index == local_index {
                return (idx, true);
            }
            idx += 1;
            if idx == CAP {
                idx = 0;
            }
        }
        (idx, false)
    }

    /// Records (or overwrites) a tile edit. Returns `false` only when the slab is full.
    pub fn record(&mut self, coord: ChunkCoord, local_index: u16, tile: TileType) -> bool {
        let key = coord.key();
        let (idx, found) = self.probe(key, local_index);
        if !found {
            if self.len >= CAP {
                self.rejected = self.rejected.wrapping_add(1);
                return false;
            }
            self.len += 1;
        }
        self.next_sequence = self.next_sequence.wrapping_add(1);
        self.entries[idx] = ChunkDelta {
            chunk_key: key,
            local_index,
            tile: tile as u8,
            occupied: 1,
            sequence: self.next_sequence,
        };
        true
    }

    /// Looks up a preserved edit for `(coord, local_index)`.
    #[inline]
    pub fn get(&self, coord: ChunkCoord, local_index: u16) -> Option<TileType> {
        if self.len == 0 {
            return None;
        }
        let (idx, found) = self.probe(coord.key(), local_index);
        if found {
            Some(self.entries[idx].tile_type())
        } else {
            None
        }
    }

    /// Counts preserved edits belonging to `coord`.
    pub fn count_for_chunk(&self, coord: ChunkCoord) -> usize {
        let key = coord.key();
        self.entries
            .iter()
            .filter(|e| e.is_occupied() && e.chunk_key == key)
            .count()
    }

    /// Re-applies every preserved edit of `chunk.header.coord` onto a freshly
    /// generated chunk. Returns the number of tiles restored.
    pub fn apply_to<const S: usize>(&self, chunk: &mut Chunk<S>) -> u32 {
        if self.len == 0 {
            return 0;
        }
        let key = chunk.header.coord.key();
        let mut applied = 0u32;
        for e in self.entries.iter() {
            if !e.is_occupied() || e.chunk_key != key {
                continue;
            }
            let li = e.local_index as usize;
            let (lx, ly) = (li % S, li / S);
            if chunk.set_tile(lx, ly, e.tile_type()) {
                applied += 1;
            }
        }
        applied
    }

    /// Deterministic hash of the whole delta slab (order independent).
    pub fn compute_hash(&self) -> u64 {
        let mut h = (self.len as u64).wrapping_mul(0x9E3779B97F4A7C15);
        for e in self.entries.iter() {
            if e.is_occupied() {
                let v = deterministic_hash(&(e.chunk_key, e.local_index, e.tile));
                h ^= v.wrapping_mul(0xBF58476D1CE4E5B9);
            }
        }
        h
    }
}

// ---------------------------------------------------------------------------
// Streaming manager
// ---------------------------------------------------------------------------

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
    /// Player edits re-applied onto regenerated chunks during this update.
    pub reapplied_deltas: u32,
}

/// Fixed-slab manager for procedural chunk streaming and spatial querying.
///
/// Guaranteed zero dynamic allocations during world exploration. Chunk lookup is
/// O(1) through a fixed open-addressing index table; player edits persist across
/// eviction through [`ChunkDeltaMap`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkManager<
    const S: usize = DEFAULT_CHUNK_SIZE,
    const MAX_CHUNKS: usize = DEFAULT_MAX_CHUNKS,
    const DELTA_CAP: usize = DEFAULT_DELTA_CAPACITY,
> {
    pub slots: [ChunkSlot<S>; MAX_CHUNKS],
    /// Open-addressing table mapping `ChunkCoord` buckets to slot indices.
    pub index: [u16; CHUNK_INDEX_BUCKETS],
    pub deltas: ChunkDeltaMap<DELTA_CAP>,
    pub world_seed: u64,
    pub center_chunk: ChunkCoord,
    pub view_radius: i32,
    pub total_generated: u64,
    pub total_evicted: u64,
}

impl<const S: usize, const MAX_CHUNKS: usize, const DELTA_CAP: usize>
    ChunkManager<S, MAX_CHUNKS, DELTA_CAP>
{
    /// Creates a new uninitialized chunk manager.
    pub fn new(world_seed: u64, view_radius: i32) -> Self {
        assert!(
            MAX_CHUNKS < CHUNK_INDEX_BUCKETS,
            "MAX_CHUNKS must be smaller than CHUNK_INDEX_BUCKETS"
        );
        // SAFETY: every field of ChunkSlot<S> is valid when all-zero
        // (TileType::Plains = 0, BiomeKind::TemperatePlains = 0, bool false, integers 0).
        let mut mgr = Self {
            slots: unsafe { core::mem::zeroed() },
            index: [CHUNK_INDEX_EMPTY; CHUNK_INDEX_BUCKETS],
            deltas: ChunkDeltaMap::new(),
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

    // ---- slot index ------------------------------------------------------

    #[inline(always)]
    fn bucket_of(coord: ChunkCoord) -> usize {
        (coord.bucket_hash() % (CHUNK_INDEX_BUCKETS as u64)) as usize
    }

    /// Rebuilds the O(1) slot index from the slab. Call after mutating `slots` directly.
    pub fn rebuild_index(&mut self) {
        self.index = [CHUNK_INDEX_EMPTY; CHUNK_INDEX_BUCKETS];
        for i in 0..MAX_CHUNKS {
            if self.slots[i].active {
                let coord = self.slots[i].chunk.header.coord;
                self.index_insert(coord, i);
            }
        }
    }

    #[inline]
    fn index_insert(&mut self, coord: ChunkCoord, slot_idx: usize) {
        let mut b = Self::bucket_of(coord);
        for _ in 0..CHUNK_INDEX_BUCKETS {
            let cur = self.index[b];
            if cur == CHUNK_INDEX_EMPTY {
                self.index[b] = slot_idx as u16;
                return;
            }
            let cur_slot = cur as usize;
            if cur_slot == slot_idx
                || !self.slots[cur_slot].active
                || self.slots[cur_slot].chunk.header.coord == coord
            {
                // Reuse a stale / duplicate entry in place.
                self.index[b] = slot_idx as u16;
                return;
            }
            b += 1;
            if b == CHUNK_INDEX_BUCKETS {
                b = 0;
            }
        }
    }

    /// Finds index of loaded chunk at `coord` (O(1) expected).
    #[inline]
    pub fn find_chunk_index(&self, coord: ChunkCoord) -> Option<usize> {
        let mut b = Self::bucket_of(coord);
        for _ in 0..CHUNK_INDEX_BUCKETS {
            let cur = self.index[b];
            if cur == CHUNK_INDEX_EMPTY {
                return None;
            }
            let slot_idx = cur as usize;
            if slot_idx < MAX_CHUNKS {
                let slot = &self.slots[slot_idx];
                if slot.active && slot.chunk.header.coord == coord {
                    return Some(slot_idx);
                }
            }
            b += 1;
            if b == CHUNK_INDEX_BUCKETS {
                b = 0;
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

    // ---- streaming -------------------------------------------------------

    /// Updates active chunks centered at `center_coord`.
    ///
    /// Loads missing chunks within `view_radius` (re-applying preserved player
    /// edits) and unloads out-of-range chunks.
    pub fn update_center(&mut self, center_coord: ChunkCoord, tick: u64) -> ChunkStreamStats {
        let mut stats = ChunkStreamStats::default();
        self.center_chunk = center_coord;

        // 1. Evict chunks outside view_radius
        let mut evicted_any = false;
        for slot in &mut self.slots {
            if slot.active && slot.chunk.header.coord.chebyshev_dist(center_coord) > self.view_radius
            {
                slot.active = false;
                self.total_evicted += 1;
                stats.evicted_chunks += 1;
                evicted_any = true;
            }
        }
        if evicted_any {
            self.rebuild_index();
        }

        // 2. Load missing chunks in view window
        for dy in -self.view_radius..=self.view_radius {
            for dx in -self.view_radius..=self.view_radius {
                let target_coord =
                    ChunkCoord::new(center_coord.x.wrapping_add(dx), center_coord.y.wrapping_add(dy));

                if let Some(idx) = self.find_chunk_index(target_coord) {
                    self.slots[idx].last_accessed_tick = tick;
                    continue;
                }

                // Need to allocate slot and generate chunk
                let free_idx = self.find_free_or_lru_slot(center_coord);
                let mut chunk = Chunk::generate(self.world_seed, target_coord, tick);
                stats.reapplied_deltas += self.deltas.apply_to(&mut chunk);

                if self.slots[free_idx].active {
                    self.total_evicted += 1;
                    stats.evicted_chunks += 1;
                    self.slots[free_idx].active = false;
                    self.rebuild_index();
                }

                self.slots[free_idx].chunk = chunk;
                self.slots[free_idx].active = true;
                self.slots[free_idx].last_accessed_tick = tick;
                self.index_insert(target_coord, free_idx);

                self.total_generated += 1;
                stats.generated_chunks += 1;
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

    // ---- unified tile / biome access --------------------------------------

    /// Samples the full procedural profile of world tile `(tile_x, tile_y)`.
    ///
    /// Always reflects pristine generation (player edits are not folded in).
    #[inline]
    pub fn sample_world_biome(&self, tile_x: i64, tile_y: i64) -> BiomeSample {
        sample_world_biome(tile_x as i32, tile_y as i32, self.world_seed)
    }

    /// Samples the pristine procedural terrain tile at world coordinates.
    #[inline]
    pub fn sample_world_tile(&self, tile_x: i64, tile_y: i64) -> TileType {
        sample_world_tile(tile_x as i32, tile_y as i32, self.world_seed)
    }

    /// Queries the terrain tile at world integer coordinate `(tile_x, tile_y)`.
    ///
    /// Loaded chunk ➔ preserved player delta ➔ procedural sample, in that order,
    /// so the answer is identical whether or not the chunk is streamed in.
    #[inline]
    pub fn tile_at(&self, tile_x: i64, tile_y: i64) -> TileType {
        let (coord, lx, ly) = ChunkCoord::from_world_tile(tile_x, tile_y, S);
        if let Some(chunk) = self.chunk_at(coord) {
            return chunk.tiles[ly][lx];
        }
        if let Some(t) = self.deltas.get(coord, (ly * S + lx) as u16) {
            return t;
        }
        sample_world_tile(tile_x as i32, tile_y as i32, self.world_seed)
    }

    /// Queries the biome at world integer coordinate `(tile_x, tile_y)`.
    #[inline]
    pub fn biome_at(&self, tile_x: i64, tile_y: i64) -> BiomeKind {
        let (coord, lx, ly) = ChunkCoord::from_world_tile(tile_x, tile_y, S);
        if let Some(chunk) = self.chunk_at(coord) {
            return BiomeKind::from_u8(chunk.biomes[ly][lx]);
        }
        self.sample_world_biome(tile_x, tile_y).biome
    }

    /// Queries `(deposit kind, richness level 0..=31)` at world coordinates.
    #[inline]
    pub fn deposit_at(&self, tile_x: i64, tile_y: i64) -> (ResourceDeposit, u8) {
        let (coord, lx, ly) = ChunkCoord::from_world_tile(tile_x, tile_y, S);
        if let Some(chunk) = self.chunk_at(coord) {
            return unpack_resource(chunk.resources[ly][lx]);
        }
        let s = self.sample_world_biome(tile_x, tile_y);
        unpack_resource(pack_resource(s.deposit_kind, s.deposit_richness))
    }

    /// Sets tile at world coordinate, marks the chunk dirty (when loaded) and
    /// records the edit in the persistent delta slab so it survives eviction.
    ///
    /// Returns `false` only when the chunk is not loaded *and* the delta slab is full.
    pub fn set_tile(&mut self, tile_x: i64, tile_y: i64, tile: TileType) -> bool {
        let (coord, lx, ly) = ChunkCoord::from_world_tile(tile_x, tile_y, S);
        let recorded = self.deltas.record(coord, (ly * S + lx) as u16, tile);
        if let Some(chunk) = self.chunk_at_mut(coord) {
            chunk.set_tile(lx, ly, tile);
            return true;
        }
        recorded
    }

    /// Checks if a world position is walkable.
    #[inline]
    pub fn is_walkable_pos(&self, pos: FixedVec2) -> bool {
        let tile = self.tile_at(pos.x.to_int(), pos.y.to_int());
        tile.is_walkable()
    }

    /// Deterministically selects a spawn tile inside the navigation window
    /// `[0, window_w) x [0, window_h)` (each at most [`SPAWN_WINDOW_MAX`]) from which
    /// at least `min_area` tiles are reachable on foot, scanning candidates by
    /// Manhattan distance from the origin (ties broken by ascending `x`).
    ///
    /// Procedural terrain can wall the origin into a tiny pocket; this guarantees the
    /// player starts on open ground. Falls back to the most open candidate seen.
    /// Zero heap allocation: BFS scratch lives on the stack.
    pub fn find_open_spawn(&self, window_w: usize, window_h: usize, min_area: usize) -> (i64, i64) {
        let w = if window_w > SPAWN_WINDOW_MAX { SPAWN_WINDOW_MAX } else { window_w };
        let h = if window_h > SPAWN_WINDOW_MAX { SPAWN_WINDOW_MAX } else { window_h };
        if w == 0 || h == 0 {
            return (0, 0);
        }
        let mut visited = [false; SPAWN_WINDOW_MAX * SPAWN_WINDOW_MAX];
        let mut queue = [0u16; SPAWN_WINDOW_MAX * SPAWN_WINDOW_MAX];
        let mut best = (0i64, 0i64);
        let mut best_area = 0usize;

        for d in 0..(w + h - 1) {
            for x in 0..=d {
                let y = d - x;
                if x >= w || y >= h {
                    continue;
                }
                if !self.tile_at(x as i64, y as i64).is_walkable() {
                    continue;
                }
                let area = self.reachable_area(x, y, w, h, min_area, &mut visited, &mut queue);
                if area >= min_area {
                    return (x as i64, y as i64);
                }
                if area > best_area {
                    best_area = area;
                    best = (x as i64, y as i64);
                }
            }
        }
        best
    }

    /// Breadth-first count of walkable tiles reachable from `(sx, sy)` inside the
    /// `w x h` window, capped at `cap`.
    fn reachable_area(
        &self,
        sx: usize,
        sy: usize,
        w: usize,
        h: usize,
        cap: usize,
        visited: &mut [bool; SPAWN_WINDOW_MAX * SPAWN_WINDOW_MAX],
        queue: &mut [u16; SPAWN_WINDOW_MAX * SPAWN_WINDOW_MAX],
    ) -> usize {
        let mut i = 0usize;
        while i < w * h {
            visited[i] = false;
            i += 1;
        }
        visited[sy * w + sx] = true;
        queue[0] = (sy * w + sx) as u16;
        let mut head = 0usize;
        let mut tail = 1usize;
        let mut count = 1usize;

        while head < tail && count < cap {
            let idx = queue[head] as usize;
            head += 1;
            let (x, y) = (idx % w, idx / w);
            let neighbours = [
                (x.wrapping_sub(1), y),
                (x + 1, y),
                (x, y.wrapping_sub(1)),
                (x, y + 1),
            ];
            let mut n = 0usize;
            while n < 4 {
                let (nx, ny) = neighbours[n];
                n += 1;
                if nx >= w || ny >= h {
                    continue;
                }
                let nidx = ny * w + nx;
                if visited[nidx] || !self.tile_at(nx as i64, ny as i64).is_walkable() {
                    continue;
                }
                visited[nidx] = true;
                count += 1;
                queue[tail] = nidx as u16;
                tail += 1;
            }
        }
        count
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
            let dist = slot.chunk.header.coord.chebyshev_dist(center);
            if dist > max_dist || (dist == max_dist && slot.last_accessed_tick < oldest_tick) {
                max_dist = dist;
                oldest_tick = slot.last_accessed_tick;
                best_idx = i;
            }
        }

        best_idx
    }

    /// Computes deterministic hash across all loaded chunks and preserved deltas.
    pub fn compute_hash(&self) -> u64 {
        let mut h = self.world_seed;
        for slot in &self.slots {
            if slot.active {
                h ^= slot.chunk.compute_hash().wrapping_mul(0x9E3779B97F4A7C15);
            }
        }
        h ^ self.deltas.compute_hash().rotate_left(23)
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

        // Key packing is bijective, including negative coordinates.
        let c = ChunkCoord::new(-7, 123_456);
        assert_eq!(
            ChunkDelta {
                chunk_key: c.key(),
                ..ChunkDelta::default()
            }
            .coord(),
            c
        );
    }

    #[test]
    fn test_cache_line_layout() {
        use core::mem::{align_of, size_of};
        assert_eq!(size_of::<ChunkHeader>(), 64);
        assert_eq!(align_of::<ChunkHeader>(), 64);
        assert_eq!(size_of::<ChunkDelta>(), 16);
        assert_eq!(align_of::<Chunk<32>>(), 64);
        assert_eq!(size_of::<Chunk<32>>() % 64, 0);
        assert_eq!(size_of::<Chunk<32>>(), 64 + 3 * 32 * 32);
        assert_eq!(align_of::<ChunkSlot<32>>(), 64);
        assert_eq!(size_of::<ChunkSlot<32>>() % 64, 0);
    }

    #[test]
    fn test_chunk_determinism() {
        let seed = 42;
        let coord = ChunkCoord::new(5, -3);

        let chunk1 = Chunk::<32>::generate(seed, coord, 10);
        let chunk2 = Chunk::<32>::generate(seed, coord, 10);

        assert_eq!(chunk1, chunk2);
        assert_eq!(chunk1.compute_hash(), chunk2.compute_hash());
        assert_eq!(chunk1.header.state_hash, chunk1.compute_hash());

        let total: u32 = chunk1.header.biome_histogram.iter().map(|&c| c as u32).sum();
        assert_eq!(total, 32 * 32);
        assert_eq!(
            chunk1.header.water_tile_count as usize,
            chunk1.tiles.iter().flatten().filter(|t| **t == TileType::Water).count()
        );
    }

    #[test]
    fn test_resource_packing_roundtrip() {
        for kind in [
            ResourceDeposit::None,
            ResourceDeposit::IronOre,
            ResourceDeposit::CopperOre,
            ResourceDeposit::Coal,
            ResourceDeposit::Stone,
            ResourceDeposit::Uranium,
            ResourceDeposit::CrudeOil,
        ] {
            for richness in [0u16, 1, 63, 64, 640, 1600, 2047, 65535] {
                let (k, level) = unpack_resource(pack_resource(kind, richness));
                assert_eq!(k, kind);
                assert!(level <= 31);
                assert_eq!(level == 0, richness == 0);
            }
        }
    }

    #[test]
    fn test_delta_map_probe_and_overflow() {
        let mut map = ChunkDeltaMap::<8>::new();
        let c = ChunkCoord::new(3, -9);
        for i in 0..8u16 {
            assert!(map.record(c, i, TileType::Rock));
        }
        assert_eq!(map.len(), 8);
        // Overwrite of a tracked tile always succeeds even when full.
        assert!(map.record(c, 2, TileType::Water));
        assert_eq!(map.get(c, 2), Some(TileType::Water));
        // Brand new key is rejected when full.
        assert!(!map.record(c, 99, TileType::Forest));
        assert_eq!(map.rejected, 1);
        assert_eq!(map.get(c, 99), None);
        assert_eq!(map.count_for_chunk(c), 8);
        map.clear();
        assert!(map.is_empty());
        assert_eq!(map.get(c, 2), None);
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

        // Index table agrees with a linear scan for every loaded slot.
        for (i, slot) in mgr.slots.iter().enumerate() {
            if slot.active {
                assert_eq!(mgr.find_chunk_index(slot.chunk.header.coord), Some(i));
            }
        }
    }
}
