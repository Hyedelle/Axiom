//! # World Factory: Infinite Chunk Exploration x Factory Base Expansion (Track 1 / T-041)
//!
//! Bridges the open-world terrain streamer ([`ChunkManager`]) with the factory
//! simulation ([`FactoryGrid`]) so that a base can grow across chunk borders:
//!
//! ```text
//!  chunk (0,0)                       | chunk (1,0)
//!  [OreVein] Drill> Belt> Belt> Belt |> Belt> Belt> Inserter> Smelter Inserter> Chest
//!                                    ^
//!                        CrossChunkLogistics hand-off
//! ```
//!
//! - **Procedural ore**: every chunk owns a seed derived from `(world_seed, coord)`.
//!   [`generate_ore_veins`] expands it into 1..=4 diamond-shaped veins; claiming the
//!   chunk materialises them as [`OreDeposit`] tiles (never on water).
//! - **Cross-chunk belts**: after every chunk grid ticks, [`CrossChunkLogistics`]
//!   moves ready items on outward-facing edge belts into the neighbouring chunk grid.
//! - **Axioms**: integer-only ticks, no RNG except the seeded SplitMix64 [`Rng`],
//!   no wall clock, 64-byte aligned slots, zero heap in [`WorldFactory::step`].

use axiom_core::{deterministic_hash, Chunk, ChunkCoord, ChunkManager, ChunkStreamStats, Rng, TileType};
use axiom_core::{DEFAULT_CHUNK_SIZE, DEFAULT_MAX_CHUNKS};

use crate::factory::{Direction, FactoryGrid, FactoryReport, ItemType, StructureKind, CYCLE};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum simultaneously claimed factory chunks (3x3 base footprint by default).
pub const DEFAULT_MAX_FACTORY_CHUNKS: usize = 9;

/// Hard cap on ore veins spawned in a single chunk.
pub const MAX_VEINS_PER_CHUNK: usize = 4;
/// Minimum veins a chunk rolls (every chunk has at least one deposit cluster).
pub const MIN_VEINS_PER_CHUNK: u32 = 1;
/// Largest vein radius in tiles (Manhattan distance from the vein centre).
pub const MAX_VEIN_RADIUS: u32 = 3;
/// Richness range of the vein centre tile, in ore units.
pub const VEIN_RICHNESS_MIN: u32 = 200;
pub const VEIN_RICHNESS_MAX: u32 = 800;

/// Upper bound on edge hand-offs scheduled per chunk per tick (fixed stack buffer).
/// A chunk has `4 * S` edge tiles; `S <= 64` is covered.
pub const MAX_CROSS_CHUNK_TRANSFERS: usize = 256;

/// Salt mixed into the per-chunk ore seed so ore layout is decorrelated from terrain.
const ORE_SEED_SALT: u64 = 0x4F52_455F_5645_494E; // "ORE_VEIN"

// ---------------------------------------------------------------------------
// Procedural ore veins
// ---------------------------------------------------------------------------

/// One procedural ore cluster inside a chunk. 8 bytes, plain `Copy` data.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct OreVein {
    pub item: ItemType,
    /// Centre tile (chunk-local).
    pub cx: u8,
    pub cy: u8,
    /// Manhattan radius; tiles with `dist <= radius` receive ore.
    pub radius: u8,
    /// Ore units on the centre tile.
    pub richness: u16,
    /// Ore units lost per tile of distance from the centre.
    pub falloff: u16,
}

impl OreVein {
    /// Ore units on the tile at Manhattan distance `dist`, or 0 outside the vein.
    #[inline(always)]
    pub const fn amount_at(&self, dist: u32) -> u32 {
        if dist > self.radius as u32 {
            0
        } else {
            let amt = self.richness as u32 - self.falloff as u32 * dist;
            if amt == 0 {
                1
            } else {
                amt
            }
        }
    }
}

/// The fixed set of veins rolled for one chunk. Exactly one cache line.
#[repr(C, align(64))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct OreVeinSet {
    pub veins: [OreVein; MAX_VEINS_PER_CHUNK],
    pub count: u8,
    /// Seed actually used (for telemetry / replay).
    pub chunk_seed: u64,
}

impl OreVeinSet {
    #[inline(always)]
    pub fn active(&self) -> &[OreVein] {
        &self.veins[..self.count as usize]
    }
}

/// Derives the ore seed of a chunk from the world seed and its coordinate.
#[inline]
pub fn chunk_ore_seed(world_seed: u64, coord: ChunkCoord) -> u64 {
    deterministic_hash(&(world_seed, coord.x, coord.y, ORE_SEED_SALT))
}

/// Rolls the ore veins of chunk `coord` for a world of seed `world_seed`.
///
/// Pure: depends only on `(world_seed, coord, S)`; streaming order is irrelevant.
pub fn generate_ore_veins<const S: usize>(world_seed: u64, coord: ChunkCoord) -> OreVeinSet {
    let chunk_seed = chunk_ore_seed(world_seed, coord);
    let mut rng = Rng::with_seed(chunk_seed);
    let mut set = OreVeinSet {
        chunk_seed,
        ..OreVeinSet::default()
    };

    let count = rng.range_u32(MIN_VEINS_PER_CHUNK, MAX_VEINS_PER_CHUNK as u32) as usize;
    let max_radius = MAX_VEIN_RADIUS.min((S as u32).saturating_sub(1) / 2);

    for i in 0..count {
        // Weighted roll: iron 40%, copper 30%, coal 20%, stone 10%.
        let item = match rng.range_u32(0, 9) {
            0..=3 => ItemType::IronOre,
            4..=6 => ItemType::CopperOre,
            7..=8 => ItemType::Coal,
            _ => ItemType::Stone,
        };
        let radius = rng.range_u32(1, max_radius.max(1));
        let hi = (S as u32).saturating_sub(1).saturating_sub(radius);
        let cx = rng.range_u32(radius, hi) as u8;
        let cy = rng.range_u32(radius, hi) as u8;
        let richness = rng.range_u32(VEIN_RICHNESS_MIN, VEIN_RICHNESS_MAX);
        // falloff * radius < richness, so every vein tile holds >= 1 ore.
        let falloff = richness / (radius + 1);

        set.veins[i] = OreVein {
            item,
            cx,
            cy,
            radius: radius as u8,
            richness: richness as u16,
            falloff: falloff as u16,
        };
    }
    set.count = count as u8;
    set
}

/// Visits every land tile of every vein as `(lx, ly, item, amount)`, in row-major
/// order per vein. Water tiles are skipped. Overlapping veins visit a tile twice;
/// callers decide what wins (the spawner keeps the first).
pub fn for_each_vein_tile<const S: usize>(
    veins: &OreVeinSet,
    chunk: &Chunk<S>,
    mut f: impl FnMut(usize, usize, ItemType, u32),
) {
    for v in veins.active() {
        let r = v.radius as i32;
        let (cx, cy) = (v.cx as i32, v.cy as i32);
        for dy in -r..=r {
            for dx in -r..=r {
                let dist = (dx.abs() + dy.abs()) as u32;
                if dist > v.radius as u32 {
                    continue;
                }
                let (lx, ly) = (cx + dx, cy + dy);
                if lx < 0 || ly < 0 || lx as usize >= S || ly as usize >= S {
                    continue;
                }
                let (lx, ly) = (lx as usize, ly as usize);
                if chunk.tiles[ly][lx] == TileType::Water {
                    continue;
                }
                f(lx, ly, v.item, v.amount_at(dist));
            }
        }
    }
}

/// Aggregate ore content of one chunk (what an exploring player "sees").
#[repr(C, align(64))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct OreSurvey {
    pub coord: ChunkCoord,
    pub veins: u32,
    pub deposit_tiles: u32,
    pub iron_ore: u32,
    pub copper_ore: u32,
    pub coal: u32,
    pub stone: u32,
}

impl OreSurvey {
    #[inline(always)]
    pub const fn total_ore(&self) -> u64 {
        self.iron_ore as u64 + self.copper_ore as u64 + self.coal as u64 + self.stone as u64
    }
}

/// Surveys chunk terrain + seed without materialising any factory grid.
pub fn survey_chunk<const S: usize>(world_seed: u64, chunk: &Chunk<S>) -> OreSurvey {
    let veins = generate_ore_veins::<S>(world_seed, chunk.coord);
    let mut s = OreSurvey {
        coord: chunk.coord,
        veins: veins.count as u32,
        ..OreSurvey::default()
    };
    // Mirror the spawner's first-wins rule so the survey matches what gets placed.
    let mut taken = [[false; S]; S];
    for_each_vein_tile(&veins, chunk, |lx, ly, item, amount| {
        if taken[ly][lx] {
            return;
        }
        taken[ly][lx] = true;
        s.deposit_tiles += 1;
        match item {
            ItemType::IronOre => s.iron_ore += amount,
            ItemType::CopperOre => s.copper_ore += amount,
            ItemType::Coal => s.coal += amount,
            ItemType::Stone => s.stone += amount,
            _ => {}
        }
    });
    s
}

// ---------------------------------------------------------------------------
// Factory chunk slot
// ---------------------------------------------------------------------------

/// One claimed chunk's factory grid plus bookkeeping. 64-byte aligned slab entry.
#[repr(C, align(64))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FactoryChunkSlot<const S: usize = DEFAULT_CHUNK_SIZE> {
    pub grid: FactoryGrid<S, S>,
    pub coord: ChunkCoord,
    pub active: bool,
    pub claimed_tick: u64,
    pub deposit_tiles: u32,
}

impl<const S: usize> Default for FactoryChunkSlot<S> {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl<const S: usize> FactoryChunkSlot<S> {
    pub const fn new() -> Self {
        Self {
            grid: FactoryGrid::new(),
            coord: ChunkCoord::new(0, 0),
            active: false,
            claimed_tick: 0,
            deposit_tiles: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// Cross-chunk logistics
// ---------------------------------------------------------------------------

/// Moves items across chunk borders. Exactly one cache line of counters.
#[repr(C, align(64))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct CrossChunkLogistics {
    /// Hand-offs completed during the last tick.
    pub transfers_last_tick: u32,
    /// Ready edge belts that found no (or a full) receiver during the last tick.
    pub blocked_last_tick: u32,
    pub total_transfers: u64,
    /// Items delivered straight into a chest in the neighbouring chunk.
    pub total_stored: u64,
}

/// Which edge of a chunk a tile sits on, together with the outward direction.
#[inline(always)]
fn edge_tile<const S: usize>(edge: Direction, i: usize) -> (usize, usize) {
    match edge {
        Direction::North => (i, 0),
        Direction::South => (i, S - 1),
        Direction::West => (0, i),
        Direction::East => (S - 1, i),
    }
}

/// Local coordinate of the receiving tile in the neighbouring chunk.
#[inline(always)]
fn wrap_across<const S: usize>(x: usize, y: usize, dir: Direction) -> (usize, usize) {
    match dir {
        Direction::North => (x, S - 1),
        Direction::South => (x, 0),
        Direction::West => (S - 1, y),
        Direction::East => (0, y),
    }
}

/// Splits two distinct slots out of the slab for simultaneous mutation.
#[inline]
fn pair_mut<T>(slots: &mut [T], a: usize, b: usize) -> (&mut T, &mut T) {
    debug_assert!(a != b);
    if a < b {
        let (l, r) = slots.split_at_mut(b);
        (&mut l[a], &mut r[0])
    } else {
        let (l, r) = slots.split_at_mut(a);
        (&mut r[0], &mut l[b])
    }
}

impl CrossChunkLogistics {
    pub const fn new() -> Self {
        Self {
            transfers_last_tick: 0,
            blocked_last_tick: 0,
            total_transfers: 0,
            total_stored: 0,
        }
    }

    /// Hands ready items on outward-facing edge belts to the adjacent chunk grid.
    ///
    /// Runs after every grid has stepped. Heap-free: candidates per source chunk are
    /// collected in a fixed stack buffer, then applied in deterministic slot/row order.
    pub fn step<const S: usize, const N: usize>(
        &mut self,
        slots: &mut [FactoryChunkSlot<S>; N],
        tick: u32,
    ) {
        self.transfers_last_tick = 0;
        self.blocked_last_tick = 0;

        const EDGES: [Direction; 4] =
            [Direction::North, Direction::East, Direction::South, Direction::West];

        for src_idx in 0..N {
            if !slots[src_idx].active {
                continue;
            }
            let src_coord = slots[src_idx].coord;

            // (sx, sy, dst_slot, tx, ty) -- fixed stack buffer, no heap.
            let mut pending = [(0u16, 0u16, 0u16, 0u16, 0u16); MAX_CROSS_CHUNK_TRANSFERS];
            let mut count = 0usize;

            for edge in EDGES {
                let (dx, dy) = edge.offset();
                let dst_coord = ChunkCoord::new(src_coord.x + dx, src_coord.y + dy);
                let dst_idx = slots
                    .iter()
                    .position(|s| s.active && s.coord == dst_coord);

                for i in 0..S {
                    let (x, y) = edge_tile::<S>(edge, i);
                    let belt = match slots[src_idx].grid.cells[y][x].kind {
                        StructureKind::Belt(b) => b,
                        _ => continue,
                    };
                    if belt.item.is_none() || belt.direction != edge || belt.progress < CYCLE {
                        continue;
                    }
                    let dst_idx = match dst_idx {
                        Some(d) => d,
                        None => {
                            self.blocked_last_tick += 1;
                            continue;
                        }
                    };
                    if count >= MAX_CROSS_CHUNK_TRANSFERS {
                        self.blocked_last_tick += 1;
                        continue;
                    }
                    let (tx, ty) = wrap_across::<S>(x, y, edge);
                    pending[count] = (x as u16, y as u16, dst_idx as u16, tx as u16, ty as u16);
                    count += 1;
                }
            }

            for &(sx, sy, dst_idx, tx, ty) in pending[..count].iter() {
                let (sx, sy, tx, ty) = (sx as usize, sy as usize, tx as usize, ty as usize);
                let (src, dst) = pair_mut(slots, src_idx, dst_idx as usize);

                let item = match src.grid.cells[sy][sx].kind {
                    StructureKind::Belt(b) => b.item,
                    _ => continue,
                };
                if item.is_none() {
                    continue;
                }
                // Same rule as in-chunk belts: belts feed only belts and chests.
                let target = &mut dst.grid.cells[ty][tx];
                let was_chest = matches!(target.kind, StructureKind::Chest(_));
                let target_ok = was_chest || matches!(target.kind, StructureKind::Belt(_));
                if target_ok && target.try_accept(item) {
                    target.last_action_tick = tick;
                    src.grid.cells[sy][sx].try_take();
                    src.grid.cells[sy][sx].last_action_tick = tick;
                    self.transfers_last_tick += 1;
                    self.total_transfers += 1;
                    if was_chest {
                        dst.grid.total_stored += 1;
                        self.total_stored += 1;
                    }
                } else {
                    self.blocked_last_tick += 1;
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// World factory
// ---------------------------------------------------------------------------

/// Aggregate counters across all claimed chunks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct WorldFactoryReport {
    pub tick: u64,
    pub loaded_terrain_chunks: u32,
    pub active_factory_chunks: u32,
    pub deposit_tiles: u32,
    pub total_mined: u64,
    pub total_smelted: u64,
    pub total_stored: u64,
    pub items_on_belts: u32,
    pub items_held_by_inserters: u32,
    pub ore_in_smelters: u32,
    pub plates_in_smelters: u32,
    pub cross_chunk_transfers: u64,
}

/// Infinite-world factory: terrain streamer + slab of per-chunk factory grids.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorldFactory<
    const S: usize = DEFAULT_CHUNK_SIZE,
    const MAX_CHUNKS: usize = DEFAULT_MAX_CHUNKS,
    const MAX_FACTORY_CHUNKS: usize = DEFAULT_MAX_FACTORY_CHUNKS,
> {
    pub terrain: ChunkManager<S, MAX_CHUNKS>,
    pub factories: [FactoryChunkSlot<S>; MAX_FACTORY_CHUNKS],
    pub logistics: CrossChunkLogistics,
    pub world_seed: u64,
    pub current_tick: u64,
    pub total_deposit_tiles: u64,
}

pub type DefaultWorldFactory =
    WorldFactory<DEFAULT_CHUNK_SIZE, DEFAULT_MAX_CHUNKS, DEFAULT_MAX_FACTORY_CHUNKS>;

impl<const S: usize, const MC: usize, const MF: usize> WorldFactory<S, MC, MF> {
    pub fn new(world_seed: u64, view_radius: i32) -> Self {
        Self {
            terrain: ChunkManager::new(world_seed, view_radius),
            factories: core::array::from_fn(|_| FactoryChunkSlot::new()),
            logistics: CrossChunkLogistics::new(),
            world_seed,
            current_tick: 0,
            total_deposit_tiles: 0,
        }
    }

    // ---- exploration -----------------------------------------------------

    /// Streams terrain around `center` (chunk coordinate).
    #[inline]
    pub fn explore(&mut self, center: ChunkCoord) -> ChunkStreamStats {
        self.terrain.update_center(center, self.current_tick)
    }

    /// Streams terrain around a world tile position.
    #[inline]
    pub fn explore_tile(&mut self, tile_x: i64, tile_y: i64) -> ChunkStreamStats {
        let (coord, _, _) = ChunkCoord::from_world_tile(tile_x, tile_y, S);
        self.explore(coord)
    }

    /// Ore survey of `coord`, from the loaded chunk if present, else from the seed.
    pub fn survey(&self, coord: ChunkCoord) -> OreSurvey {
        match self.terrain.chunk_at(coord) {
            Some(chunk) => survey_chunk(self.world_seed, chunk),
            None => {
                let chunk = Chunk::<S>::generate(self.world_seed, coord, self.current_tick);
                survey_chunk(self.world_seed, &chunk)
            }
        }
    }

    // ---- factory slots ---------------------------------------------------

    #[inline]
    pub fn find_factory(&self, coord: ChunkCoord) -> Option<usize> {
        self.factories
            .iter()
            .position(|s| s.active && s.coord == coord)
    }

    #[inline]
    pub fn factory(&self, coord: ChunkCoord) -> Option<&FactoryGrid<S, S>> {
        self.find_factory(coord).map(|i| &self.factories[i].grid)
    }

    #[inline]
    pub fn factory_mut(&mut self, coord: ChunkCoord) -> Option<&mut FactoryGrid<S, S>> {
        match self.find_factory(coord) {
            Some(i) => Some(&mut self.factories[i].grid),
            None => None,
        }
    }

    #[inline]
    pub fn active_factory_count(&self) -> usize {
        self.factories.iter().filter(|s| s.active).count()
    }

    /// Claims chunk `coord` as factory territory and spawns its procedural ore.
    ///
    /// Returns the slot index, or `None` when all slots are taken. Re-claiming a
    /// chunk is a no-op that returns the existing slot.
    pub fn claim_chunk(&mut self, coord: ChunkCoord) -> Option<usize> {
        if let Some(i) = self.find_factory(coord) {
            return Some(i);
        }
        let idx = self.factories.iter().position(|s| !s.active)?;

        let veins = generate_ore_veins::<S>(self.world_seed, coord);
        let slot = &mut self.factories[idx];
        slot.grid = FactoryGrid::new();
        slot.coord = coord;
        slot.active = true;
        slot.claimed_tick = self.current_tick;
        slot.deposit_tiles = 0;

        let mut spawned = 0u32;
        let mut spawn = |chunk: &Chunk<S>| {
            for_each_vein_tile(&veins, chunk, |lx, ly, item, amount| {
                if slot.grid.cells[ly][lx].is_empty() {
                    slot.grid.place_deposit(lx, ly, item, amount);
                    spawned += 1;
                }
            });
        };
        match self.terrain.chunk_at(coord) {
            Some(chunk) => spawn(chunk),
            None => spawn(&Chunk::<S>::generate(self.world_seed, coord, self.current_tick)),
        }
        slot.deposit_tiles = spawned;
        self.total_deposit_tiles += spawned as u64;
        Some(idx)
    }

    /// Releases a claimed chunk, discarding its structures.
    pub fn release_chunk(&mut self, coord: ChunkCoord) -> bool {
        match self.find_factory(coord) {
            Some(i) => {
                self.factories[i] = FactoryChunkSlot::new();
                true
            }
            None => false,
        }
    }

    // ---- world-coordinate placement -------------------------------------

    /// Resolves a world tile to `(slot index, lx, ly)` if its chunk is claimed.
    #[inline]
    pub fn locate(&self, tile_x: i64, tile_y: i64) -> Option<(usize, usize, usize)> {
        let (coord, lx, ly) = ChunkCoord::from_world_tile(tile_x, tile_y, S);
        self.find_factory(coord).map(|i| (i, lx, ly))
    }

    #[inline]
    fn with_grid_at(
        &mut self,
        tile_x: i64,
        tile_y: i64,
        f: impl FnOnce(&mut FactoryGrid<S, S>, usize, usize),
    ) -> bool {
        match self.locate(tile_x, tile_y) {
            Some((i, lx, ly)) => {
                f(&mut self.factories[i].grid, lx, ly);
                true
            }
            None => false,
        }
    }

    pub fn place_deposit_at(&mut self, tx: i64, ty: i64, item: ItemType, amount: u32) -> bool {
        self.with_grid_at(tx, ty, |g, x, y| g.place_deposit(x, y, item, amount))
    }

    pub fn place_drill_at(&mut self, tx: i64, ty: i64, dir: Direction, speed: u16) -> bool {
        self.with_grid_at(tx, ty, |g, x, y| g.place_drill(x, y, dir, speed))
    }

    pub fn place_belt_at(&mut self, tx: i64, ty: i64, dir: Direction, speed: u16) -> bool {
        self.with_grid_at(tx, ty, |g, x, y| g.place_belt(x, y, dir, speed))
    }

    pub fn place_inserter_at(&mut self, tx: i64, ty: i64, dir: Direction, speed: u16) -> bool {
        self.with_grid_at(tx, ty, |g, x, y| g.place_inserter(x, y, dir, speed))
    }

    pub fn place_smelter_at(&mut self, tx: i64, ty: i64, speed: u16) -> bool {
        self.with_grid_at(tx, ty, |g, x, y| g.place_smelter(x, y, speed))
    }

    pub fn place_chest_at(&mut self, tx: i64, ty: i64) -> bool {
        self.with_grid_at(tx, ty, |g, x, y| g.place_chest(x, y))
    }

    pub fn place_wall_at(&mut self, tx: i64, ty: i64) -> bool {
        self.with_grid_at(tx, ty, |g, x, y| g.place_wall(x, y))
    }

    pub fn place_turret_at(&mut self, tx: i64, ty: i64) -> bool {
        self.with_grid_at(tx, ty, |g, x, y| g.place_turret(x, y))
    }

    pub fn place_base_heart_at(&mut self, tx: i64, ty: i64) -> bool {
        self.with_grid_at(tx, ty, |g, x, y| g.place_base_heart(x, y))
    }

    /// Structure kind at a world tile (`Empty` if the chunk is unclaimed).
    pub fn structure_at(&self, tx: i64, ty: i64) -> StructureKind {
        match self.locate(tx, ty) {
            Some((i, lx, ly)) => self.factories[i].grid.cells[ly][lx].kind,
            None => StructureKind::Empty,
        }
    }

    /// Chest contents at a world tile.
    pub fn chest_contents_at(&self, tx: i64, ty: i64) -> (ItemType, u32) {
        match self.structure_at(tx, ty) {
            StructureKind::Chest(c) => (c.item, c.quantity),
            _ => (ItemType::None, 0),
        }
    }

    // ---- simulation ------------------------------------------------------

    /// Advances the whole world by one tick. Heap-free.
    ///
    /// 1. Every claimed chunk grid runs its own 4-phase [`FactoryGrid::step`].
    /// 2. [`CrossChunkLogistics`] hands ready edge-belt items to neighbouring chunks.
    pub fn step(&mut self) {
        self.current_tick += 1;
        let tick = self.current_tick as u32;
        for slot in self.factories.iter_mut() {
            if slot.active {
                slot.grid.step();
            }
        }
        self.logistics.step(&mut self.factories, tick);
    }

    pub fn run(&mut self, n: u32) {
        for _ in 0..n {
            self.step();
        }
    }

    // ---- telemetry -------------------------------------------------------

    pub fn report(&self) -> WorldFactoryReport {
        let mut r = WorldFactoryReport {
            tick: self.current_tick,
            loaded_terrain_chunks: self.terrain.loaded_count() as u32,
            cross_chunk_transfers: self.logistics.total_transfers,
            ..WorldFactoryReport::default()
        };
        for slot in self.factories.iter().filter(|s| s.active) {
            let g: FactoryReport = slot.grid.report();
            r.active_factory_chunks += 1;
            r.deposit_tiles += slot.deposit_tiles;
            r.total_mined += g.total_mined;
            r.total_smelted += g.total_smelted;
            r.total_stored += g.total_stored;
            r.items_on_belts += g.items_on_belts;
            r.items_held_by_inserters += g.items_held_by_inserters;
            r.ore_in_smelters += g.ore_in_smelters;
            r.plates_in_smelters += g.plates_in_smelters;
        }
        r
    }

    /// Deterministic hash of terrain, every claimed grid, logistics and the tick.
    /// Slot order independent (XOR fold) so eviction history does not leak in.
    pub fn compute_hash(&self) -> u64 {
        let mut h = self.terrain.compute_hash()
            ^ self.current_tick.wrapping_mul(0x9E37_79B9_7F4A_7C15)
            ^ self.logistics.total_transfers.rotate_left(23)
            ^ self.logistics.total_stored.rotate_left(41);
        for slot in self.factories.iter().filter(|s| s.active) {
            let coord_h = deterministic_hash(&slot.coord);
            h ^= slot
                .grid
                .compute_hash()
                .wrapping_add(coord_h)
                .wrapping_mul(0x0000_0100_0000_01B3)
                .rotate_left(13);
        }
        h
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::{align_of, size_of};

    /// Small world for fast, stack-light tests: 16x16 chunks, 4 factory slots.
    type TestWorld = WorldFactory<16, 16, 4>;

    const SEED: u64 = 0xA110_0041;

    /// Cross-chunk iron line on world row y = 4. Chunk (0,0) = x 0..16, chunk (1,0) = x 16..32.
    ///
    /// ```text
    /// x: 10     11..15        | 16..18     19        20       21        22
    ///    Drill> Belt x5 (edge)|> Belt x3 > Inserter> Smelter  Inserter> Chest
    /// ```
    fn build_cross_chunk_line(w: &mut TestWorld, reserve: u32) {
        w.explore(ChunkCoord::new(0, 0));
        assert!(w.claim_chunk(ChunkCoord::new(0, 0)).is_some());
        assert!(w.claim_chunk(ChunkCoord::new(1, 0)).is_some());

        let y = 4;
        assert!(w.place_deposit_at(10, y, ItemType::IronOre, reserve));
        assert!(w.place_drill_at(10, y, Direction::East, 50));
        for x in 11..=18 {
            assert!(w.place_belt_at(x, y, Direction::East, 100));
        }
        assert!(w.place_inserter_at(19, y, Direction::East, 100));
        assert!(w.place_smelter_at(20, y, 50));
        assert!(w.place_inserter_at(21, y, Direction::East, 100));
        assert!(w.place_chest_at(22, y));
    }

    #[test]
    fn test_layout_is_cache_line_aligned() {
        assert_eq!(size_of::<OreVein>(), 8);
        assert_eq!(size_of::<OreVeinSet>(), 64);
        assert_eq!(align_of::<OreVeinSet>(), 64);
        assert_eq!(size_of::<OreSurvey>(), 64);
        assert_eq!(align_of::<OreSurvey>(), 64);
        assert_eq!(size_of::<CrossChunkLogistics>(), 64);
        assert_eq!(align_of::<CrossChunkLogistics>(), 64);
        assert_eq!(align_of::<FactoryChunkSlot<16>>(), 64);
        assert_eq!(align_of::<FactoryChunkSlot<DEFAULT_CHUNK_SIZE>>(), 64);
        assert_eq!(size_of::<FactoryChunkSlot<16>>() % 64, 0);
        assert_eq!(align_of::<DefaultWorldFactory>(), 64);
        assert_eq!(size_of::<DefaultWorldFactory>() % 64, 0);
    }

    #[test]
    fn test_ore_veins_are_seeded_per_chunk_and_never_on_water() {
        let a = generate_ore_veins::<16>(SEED, ChunkCoord::new(3, -7));
        let b = generate_ore_veins::<16>(SEED, ChunkCoord::new(3, -7));
        assert_eq!(a, b, "same seed + coord must roll identical veins");
        assert!(a.count as usize >= MIN_VEINS_PER_CHUNK as usize);
        assert!(a.count as usize <= MAX_VEINS_PER_CHUNK);

        let c = generate_ore_veins::<16>(SEED, ChunkCoord::new(4, -7));
        let d = generate_ore_veins::<16>(SEED ^ 1, ChunkCoord::new(3, -7));
        assert_ne!(a, c, "neighbouring chunk must differ");
        assert_ne!(a, d, "different world seed must differ");

        for v in a.active() {
            assert!(!v.item.is_none());
            assert!(v.amount_at(v.radius as u32) >= 1, "edge of vein must hold ore");
            assert_eq!(v.amount_at(v.radius as u32 + 1), 0);
            assert!((v.cx as usize) < 16 && (v.cy as usize) < 16);
        }

        // Scan a wide area: every spawned deposit sits on land, surveys agree with spawns.
        let mut world = TestWorld::new(SEED, 1);
        let mut total_tiles = 0u32;
        for cy in -2..=2 {
            for cx in -2..=2 {
                let coord = ChunkCoord::new(cx, cy);
                world.explore(coord);
                let survey = world.survey(coord);
                assert!(survey.veins >= 1);
                assert!(survey.total_ore() > 0, "chunk {coord:?} has no ore");

                let idx = world.claim_chunk(coord).expect("free slot");
                let slot = &world.factories[idx];
                assert_eq!(slot.deposit_tiles, survey.deposit_tiles, "{coord:?}");
                total_tiles += slot.deposit_tiles;

                let chunk = world.terrain.chunk_at(coord).expect("loaded");
                let mut ore_sum = 0u64;
                for ly in 0..16 {
                    for lx in 0..16 {
                        if let StructureKind::Deposit(d) = slot.grid.cells[ly][lx].kind {
                            assert_ne!(chunk.tiles[ly][lx], TileType::Water, "{coord:?} ({lx},{ly})");
                            assert!(d.remaining >= 1);
                            ore_sum += d.remaining as u64;
                        }
                    }
                }
                assert_eq!(ore_sum, survey.total_ore(), "{coord:?}");
                assert!(world.release_chunk(coord));
            }
        }
        assert!(total_tiles >= 25, "25 chunks must spawn at least one tile each");
        assert_eq!(world.active_factory_count(), 0);
    }

    #[test]
    fn test_claim_is_idempotent_and_bounded() {
        let mut w = TestWorld::new(SEED, 1);
        let first = w.claim_chunk(ChunkCoord::new(0, 0)).unwrap();
        assert_eq!(w.claim_chunk(ChunkCoord::new(0, 0)), Some(first));
        assert!(w.claim_chunk(ChunkCoord::new(1, 0)).is_some());
        assert!(w.claim_chunk(ChunkCoord::new(2, 0)).is_some());
        assert!(w.claim_chunk(ChunkCoord::new(3, 0)).is_some());
        assert_eq!(w.claim_chunk(ChunkCoord::new(4, 0)), None, "4 slots only");
        assert_eq!(w.active_factory_count(), 4);
        assert!(!w.place_belt_at(64, 0, Direction::East, 100), "unclaimed chunk rejects placement");
        assert!(w.release_chunk(ChunkCoord::new(2, 0)));
        assert!(w.claim_chunk(ChunkCoord::new(4, 0)).is_some());
    }

    #[test]
    fn test_cross_chunk_belt_hand_off() {
        let mut w = TestWorld::new(SEED, 1);
        w.claim_chunk(ChunkCoord::new(0, 0)).unwrap();
        w.claim_chunk(ChunkCoord::new(1, 0)).unwrap();

        // Drill on the east edge of chunk (0,0), belt + chest on the west edge of chunk (1,0).
        w.place_deposit_at(14, 2, ItemType::CopperOre, 10);
        w.place_drill_at(14, 2, Direction::East, 100);
        w.place_belt_at(15, 2, Direction::East, 100); // local (15,2) -> faces off-grid
        w.place_belt_at(16, 2, Direction::East, 100); // local (0,2) in chunk (1,0)
        w.place_chest_at(17, 2);

        w.step(); // tick 1: drill pushes ore onto edge belt
        assert!(matches!(w.structure_at(15, 2), StructureKind::Belt(b) if b.item == ItemType::CopperOre));
        assert_eq!(w.logistics.total_transfers, 0);

        w.step(); // tick 2: edge belt ready -> hand-off across the border
        assert_eq!(w.logistics.transfers_last_tick, 1);
        assert!(matches!(w.structure_at(16, 2), StructureKind::Belt(b) if b.item == ItemType::CopperOre));

        w.run(30);
        assert_eq!(w.chest_contents_at(17, 2), (ItemType::CopperOre, 10));
        assert_eq!(w.logistics.total_transfers, 10);
        let r = w.report();
        assert_eq!(r.total_mined, 10);
        assert_eq!(r.total_stored, 10);
        assert_eq!(r.items_on_belts, 0);

        // Removing the receiver chunk blocks the border instead of losing items.
        let mut blocked = TestWorld::new(SEED, 1);
        blocked.claim_chunk(ChunkCoord::new(0, 0)).unwrap();
        blocked.place_deposit_at(14, 2, ItemType::CopperOre, 10);
        blocked.place_drill_at(14, 2, Direction::East, 100);
        blocked.place_belt_at(15, 2, Direction::East, 100);
        blocked.run(20);
        assert_eq!(blocked.logistics.total_transfers, 0);
        assert!(blocked.logistics.blocked_last_tick >= 1);
        assert_eq!(blocked.report().total_mined, 1, "drill stalls behind the blocked edge belt");
        assert_eq!(blocked.report().items_on_belts, 1);
    }

    #[test]
    fn test_multi_chunk_mine_belt_smelt_store_100_ticks() {
        let mut w = TestWorld::new(SEED, 1);
        build_cross_chunk_line(&mut w, 1_000);
        w.run(100);

        let (item, qty) = w.chest_contents_at(22, 4);
        assert_eq!(item, ItemType::IronPlate, "chest must contain iron plates");
        assert!(qty >= 30, "100 ticks should yield a steady plate flow, got {qty}");

        let r = w.report();
        assert_eq!(r.tick, 100);
        assert_eq!(r.active_factory_chunks, 2);
        assert_eq!(qty as u64, r.total_stored);
        assert!(r.cross_chunk_transfers >= 30, "ore must have crossed the border: {r:?}");
        assert!(r.cross_chunk_transfers <= r.total_mined);

        // Conservation across both chunks.
        let held_ore = match w.structure_at(19, 4) {
            StructureKind::Inserter(i) if i.held == ItemType::IronOre => 1u64,
            _ => 0,
        };
        let held_plate = match w.structure_at(21, 4) {
            StructureKind::Inserter(i) if i.held == ItemType::IronPlate => 1u64,
            _ => 0,
        };
        assert_eq!(
            r.total_mined,
            r.items_on_belts as u64 + held_ore + r.ore_in_smelters as u64 + r.total_smelted,
            "ore must be conserved across chunks: {r:?}"
        );
        assert_eq!(
            r.total_smelted,
            r.plates_in_smelters as u64 + held_plate + r.total_stored,
            "plates must be conserved across chunks: {r:?}"
        );

        // Mining happened in chunk (0,0), smelting/storing in chunk (1,0).
        let g0 = w.factory(ChunkCoord::new(0, 0)).unwrap().report();
        let g1 = w.factory(ChunkCoord::new(1, 0)).unwrap().report();
        assert_eq!(g0.total_mined, r.total_mined);
        assert_eq!(g0.total_smelted, 0);
        assert_eq!(g1.total_mined, 0);
        assert_eq!(g1.total_smelted, r.total_smelted);
        assert_eq!(g1.total_stored, r.total_stored);
    }

    #[test]
    fn test_100_tick_multi_chunk_is_bit_identical() {
        let mut a = TestWorld::new(SEED, 1);
        let mut b = TestWorld::new(SEED, 1);
        build_cross_chunk_line(&mut a, 1_000);
        build_cross_chunk_line(&mut b, 1_000);
        assert_eq!(a.compute_hash(), b.compute_hash());

        for t in 1..=100 {
            a.step();
            b.step();
            assert_eq!(a.compute_hash(), b.compute_hash(), "diverged at tick {t}");
            assert_eq!(a, b, "state diverged at tick {t}");
        }

        // Fresh replay reproduces the identical master hash.
        let mut c = TestWorld::new(SEED, 1);
        build_cross_chunk_line(&mut c, 1_000);
        c.run(100);
        assert_eq!(c.compute_hash(), a.compute_hash());

        // Different world seed -> different ore layout -> different hash.
        let mut d = TestWorld::new(SEED ^ 0xFF, 1);
        build_cross_chunk_line(&mut d, 1_000);
        d.run(100);
        assert_ne!(d.compute_hash(), a.compute_hash());
    }

    #[test]
    fn test_factories_survive_terrain_streaming_away() {
        let mut w = TestWorld::new(SEED, 1);
        build_cross_chunk_line(&mut w, 1_000);
        w.run(50);
        let mid = w.report();
        assert!(mid.total_stored > 0);

        // Player wanders far away: terrain chunks (0,0)/(1,0) get evicted...
        let stats = w.explore(ChunkCoord::new(40, 40));
        assert!(stats.evicted_chunks > 0);
        assert!(w.terrain.chunk_at(ChunkCoord::new(0, 0)).is_none());

        // ...but the base keeps producing, and surveys still come from the seed.
        w.run(50);
        let end = w.report();
        assert!(end.total_stored > mid.total_stored);
        assert_eq!(end.active_factory_chunks, 2);
        assert_eq!(w.survey(ChunkCoord::new(7, 7)), w.survey(ChunkCoord::new(7, 7)));
        w.explore(ChunkCoord::new(7, 7));
        let loaded = w.survey(ChunkCoord::new(7, 7));
        w.explore(ChunkCoord::new(40, 40));
        assert_eq!(loaded, w.survey(ChunkCoord::new(7, 7)), "survey must not depend on streaming");
    }

    #[test]
    fn test_procedural_deposit_is_mineable_at_default_chunk_size() {
        // Default 32x32 chunk geometry with a 2-slot slab (stack-light).
        let mut w = WorldFactory::<32, 8, 2>::new(SEED, 1);
        let coord = ChunkCoord::new(-3, 5);
        let idx = w.claim_chunk(coord).unwrap();
        let veins = generate_ore_veins::<32>(SEED, coord);
        let v = veins.veins[0];
        let (lx, ly) = (v.cx as usize, v.cy as usize);

        let deposit = match w.factories[idx].grid.cells[ly][lx].kind {
            StructureKind::Deposit(d) => d,
            other => panic!("vein centre must be a deposit, got {other:?}"),
        };
        assert_eq!(deposit.item, v.item);
        assert_eq!(deposit.remaining, v.richness as u32);

        // Drill the vein centre straight into a chest next to it.
        let (wx, wy) = (coord.x as i64 * 32 + lx as i64, coord.y as i64 * 32 + ly as i64);
        w.place_drill_at(wx, wy, Direction::East, 100);
        w.place_chest_at(wx + 1, wy);
        w.run(100);
        assert_eq!(w.chest_contents_at(wx + 1, wy), (v.item, 100));
        assert_eq!(w.report().total_mined, 100);
    }
}
