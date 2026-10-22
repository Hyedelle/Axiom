//! # Biome-Driven Dynamic Monster Spawning & Stronghold Territory Defense (Phase 3 / T-071)
//!
//! Built directly on the T-070 biome / resource layers (`BiomeKind`, chunk `biomes` plane,
//! `ChunkHeader::deposit_tile_count`):
//!
//! - [`MonsterArchetype`]: eight biome-native monster kinds with integer stat blocks
//!   (hp / attack / armor / move interval) and a land-or-water habitat flag.
//! - [`BIOME_SPAWN_TABLE`]: weighted per-biome spawn tables (4 entries each, pack sizes),
//!   plus [`BIOME_SPAWN_DENSITY`]; deposit-rich chunks attract extra "vein guardian" packs.
//! - [`MonsterSpawner`]: fixed-slab (zero heap) world monster pool that rolls spawns for every
//!   loaded chunk around the streaming centre, roams monsters inside their home chunk, and
//!   despawns packs whose home chunk leaves the view ring (chunk eviction).
//! - [`TerritoryDefense`]: a stronghold heart with a Chebyshev territory radius, timed assault
//!   waves spawned on the territory ring from the surrounding biomes, greedy integer march
//!   toward the heart, and fixed-slab defence posts that shoot the nearest intruder.
//!
//! Axioms: pure integer arithmetic (no floats); every roll is a [`hash2d`] of
//! `(coordinate, epoch, seed)` so the same seed and input sequence yield bit-identical monster
//! populations; zero heap allocation at runtime; 64-byte aligned records.

use core::hash::Hasher;

use crate::chunk::{ChunkCoord, ChunkManager};
use crate::hash::Fnv1aHasher;
use crate::pcg::{hash2d, BiomeKind, TileType};

// ---------------------------------------------------------------------------
// Tunables (all integers)
// ---------------------------------------------------------------------------

/// Default capacity of the world monster slab.
pub const DEFAULT_MAX_WORLD_MONSTERS: usize = 256;
/// Ticks between two ambient spawn rolls.
pub const DEFAULT_SPAWN_INTERVAL: u32 = 20;
/// Ambient monsters allowed per home chunk.
pub const DEFAULT_MAX_MONSTERS_PER_CHUNK: u8 = 6;
/// Monsters whose home chunk is farther (Chebyshev, in chunks) than this from the
/// streaming centre are despawned.
pub const DEFAULT_DESPAWN_RADIUS: i32 = 3;
/// Largest chunk ring radius scanned per spawn roll (stack scratch bound).
pub const SPAWN_RING_MAX_RADIUS: i32 = 7;
const SPAWN_RING_SPAN: usize = (SPAWN_RING_MAX_RADIUS as usize) * 2 + 1;
/// Candidate tiles tried per chunk before a roll gives up.
pub const SPAWN_CANDIDATE_TRIES: u32 = 8;
/// Chunks holding at least this many deposit tiles roll with [`GUARDIAN_DENSITY_BONUS`].
pub const GUARDIAN_DEPOSIT_THRESHOLD: u16 = 48;
/// Extra spawn chance (per 256) for deposit-rich "guardian" chunks.
pub const GUARDIAN_DENSITY_BONUS: u8 = 64;
/// Every this many chunks of Chebyshev distance from the origin raises the monster tier.
pub const TIER_CHUNK_STEP: i32 = 4;
pub const MAX_TIER: u8 = 15;
/// Pack members are placed within this Chebyshev radius of the pack anchor.
pub const PACK_PLACEMENT_RADIUS: i32 = 2;
/// Ticks between two heart strikes by one monster.
pub const MONSTER_ATTACK_COOLDOWN: u8 = 4;

/// Default number of defence posts around a stronghold.
pub const DEFAULT_MAX_DEFENSE_POSTS: usize = 16;
/// Default territory radius (Chebyshev tiles) around the heart.
pub const DEFAULT_TERRITORY_RADIUS: i32 = 12;
/// Assault waves spawn this many tiles outside the territory edge.
pub const WAVE_RING_MARGIN: i32 = 4;
pub const DEFAULT_WAVE_INTERVAL: u32 = 200;
pub const DEFAULT_FIRST_WAVE_TICK: u64 = 60;
pub const DEFAULT_BASE_WAVE_SIZE: u16 = 4;
pub const DEFAULT_WAVE_GROWTH: u16 = 2;
pub const DEFAULT_STRONGHOLD_HP: u32 = 2_000;
/// Ring positions tried per wave member before the member is skipped.
pub const WAVE_PLACEMENT_TRIES: u32 = 8;

const SPAWN_SALT: u64 = 0x5A11_0A7E_B10E_5EED;
const ROAM_SALT: u64 = 0x0A0A_B0A7_1CE5_7A11;
const WAVE_SALT: u64 = 0xBA5E_DEFE_17CE_9A1E;

// ---------------------------------------------------------------------------
// Archetypes & spawn tables
// ---------------------------------------------------------------------------

/// Biome-native monster archetypes (one per [`BiomeKind`], index aligned).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u8)]
pub enum MonsterArchetype {
    /// Temperate plains raider.
    #[default]
    Goblin = 0,
    /// Boreal forest pack hunter.
    TimberWolf = 1,
    /// Arid desert armoured arthropod.
    SandScorpion = 2,
    /// Frozen tundra spectre.
    FrostWraith = 3,
    /// Volcanic crag heavy brute.
    MagmaGolem = 4,
    /// Swamp marsh ambusher.
    BogLurker = 5,
    /// Ocean depths aquatic titan (water tiles only).
    DeepLeviathan = 6,
    /// Mushroom glade swarmer.
    SporeStalker = 7,
}

impl MonsterArchetype {
    pub const COUNT: usize = 8;

    pub const ALL: [MonsterArchetype; 8] = [
        MonsterArchetype::Goblin,
        MonsterArchetype::TimberWolf,
        MonsterArchetype::SandScorpion,
        MonsterArchetype::FrostWraith,
        MonsterArchetype::MagmaGolem,
        MonsterArchetype::BogLurker,
        MonsterArchetype::DeepLeviathan,
        MonsterArchetype::SporeStalker,
    ];

    #[inline(always)]
    pub const fn from_u8(v: u8) -> Self {
        match v & 7 {
            0 => Self::Goblin,
            1 => Self::TimberWolf,
            2 => Self::SandScorpion,
            3 => Self::FrostWraith,
            4 => Self::MagmaGolem,
            5 => Self::BogLurker,
            6 => Self::DeepLeviathan,
            _ => Self::SporeStalker,
        }
    }

    #[inline(always)]
    pub const fn as_u8(self) -> u8 {
        self as u8
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::Goblin => "goblin",
            Self::TimberWolf => "timber-wolf",
            Self::SandScorpion => "sand-scorpion",
            Self::FrostWraith => "frost-wraith",
            Self::MagmaGolem => "magma-golem",
            Self::BogLurker => "bog-lurker",
            Self::DeepLeviathan => "deep-leviathan",
            Self::SporeStalker => "spore-stalker",
        }
    }

    /// The biome this archetype is native to (index aligned with [`BiomeKind`]).
    #[inline(always)]
    pub const fn native_biome(self) -> BiomeKind {
        BiomeKind::from_u8(self as u8)
    }

    pub const fn base_hp(self) -> u16 {
        match self {
            Self::Goblin => 60,
            Self::TimberWolf => 50,
            Self::SandScorpion => 70,
            Self::FrostWraith => 55,
            Self::MagmaGolem => 180,
            Self::BogLurker => 90,
            Self::DeepLeviathan => 240,
            Self::SporeStalker => 45,
        }
    }

    pub const fn base_attack(self) -> u16 {
        match self {
            Self::Goblin => 10,
            Self::TimberWolf => 12,
            Self::SandScorpion => 14,
            Self::FrostWraith => 16,
            Self::MagmaGolem => 24,
            Self::BogLurker => 12,
            Self::DeepLeviathan => 30,
            Self::SporeStalker => 9,
        }
    }

    /// Flat damage reduction applied to every hit received.
    pub const fn armor(self) -> u16 {
        match self {
            Self::Goblin => 0,
            Self::TimberWolf => 0,
            Self::SandScorpion => 2,
            Self::FrostWraith => 1,
            Self::MagmaGolem => 6,
            Self::BogLurker => 3,
            Self::DeepLeviathan => 8,
            Self::SporeStalker => 0,
        }
    }

    /// Monsters advance one tile every this many ticks.
    pub const fn move_interval(self) -> u8 {
        match self {
            Self::Goblin => 2,
            Self::TimberWolf => 1,
            Self::SandScorpion => 2,
            Self::FrostWraith => 2,
            Self::MagmaGolem => 4,
            Self::BogLurker => 3,
            Self::DeepLeviathan => 3,
            Self::SporeStalker => 1,
        }
    }

    /// True for archetypes that live on water tiles instead of walkable ground.
    #[inline(always)]
    pub const fn is_aquatic(self) -> bool {
        matches!(self, Self::DeepLeviathan)
    }

    /// True when the archetype can stand on `tile`.
    #[inline(always)]
    pub const fn accepts_tile(self, tile: TileType) -> bool {
        if self.is_aquatic() {
            matches!(tile, TileType::Water)
        } else {
            matches!(tile, TileType::Plains | TileType::Forest)
        }
    }

    /// Hit points at `tier` (`+25%` of base per tier, saturating).
    #[inline]
    pub const fn hp_at_tier(self, tier: u8) -> u16 {
        let base = self.base_hp() as u32;
        let v = base + (base * tier as u32) / 4;
        if v > u16::MAX as u32 {
            u16::MAX
        } else {
            v as u16
        }
    }

    /// Attack power at `tier` (`+12.5%` of base per tier, saturating).
    #[inline]
    pub const fn attack_at_tier(self, tier: u8) -> u16 {
        let base = self.base_attack() as u32;
        let v = base + (base * tier as u32) / 8;
        if v > u16::MAX as u32 {
            u16::MAX
        } else {
            v as u16
        }
    }
}

/// One weighted row of a biome spawn table (4 bytes, POD).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(C)]
pub struct SpawnEntry {
    pub archetype: MonsterArchetype,
    /// Relative selection weight inside the biome row (`0` disables the entry).
    pub weight: u8,
    pub pack_min: u8,
    pub pack_max: u8,
}

impl SpawnEntry {
    pub const fn new(archetype: MonsterArchetype, weight: u8, pack_min: u8, pack_max: u8) -> Self {
        Self {
            archetype,
            weight,
            pack_min,
            pack_max,
        }
    }
}

/// Weighted monster spawn rows per biome (index = `BiomeKind as u8`).
pub const BIOME_SPAWN_TABLE: [[SpawnEntry; 4]; BiomeKind::COUNT] = [
    // TemperatePlains
    [
        SpawnEntry::new(MonsterArchetype::Goblin, 8, 2, 4),
        SpawnEntry::new(MonsterArchetype::TimberWolf, 3, 2, 3),
        SpawnEntry::new(MonsterArchetype::SporeStalker, 1, 1, 2),
        SpawnEntry::new(MonsterArchetype::Goblin, 4, 1, 2),
    ],
    // BorealForest
    [
        SpawnEntry::new(MonsterArchetype::TimberWolf, 8, 2, 4),
        SpawnEntry::new(MonsterArchetype::Goblin, 3, 1, 3),
        SpawnEntry::new(MonsterArchetype::FrostWraith, 2, 1, 1),
        SpawnEntry::new(MonsterArchetype::TimberWolf, 3, 1, 2),
    ],
    // AridDesert
    [
        SpawnEntry::new(MonsterArchetype::SandScorpion, 8, 1, 3),
        SpawnEntry::new(MonsterArchetype::Goblin, 2, 1, 2),
        SpawnEntry::new(MonsterArchetype::MagmaGolem, 1, 1, 1),
        SpawnEntry::new(MonsterArchetype::SandScorpion, 5, 1, 2),
    ],
    // FrozenTundra
    [
        SpawnEntry::new(MonsterArchetype::FrostWraith, 8, 1, 3),
        SpawnEntry::new(MonsterArchetype::TimberWolf, 4, 2, 3),
        SpawnEntry::new(MonsterArchetype::FrostWraith, 2, 1, 1),
        SpawnEntry::new(MonsterArchetype::Goblin, 2, 1, 1),
    ],
    // VolcanicCrag
    [
        SpawnEntry::new(MonsterArchetype::MagmaGolem, 6, 1, 2),
        SpawnEntry::new(MonsterArchetype::SandScorpion, 3, 1, 2),
        SpawnEntry::new(MonsterArchetype::FrostWraith, 1, 1, 1),
        SpawnEntry::new(MonsterArchetype::MagmaGolem, 6, 1, 1),
    ],
    // SwampMarsh
    [
        SpawnEntry::new(MonsterArchetype::BogLurker, 8, 1, 3),
        SpawnEntry::new(MonsterArchetype::SporeStalker, 4, 2, 3),
        SpawnEntry::new(MonsterArchetype::Goblin, 2, 1, 2),
        SpawnEntry::new(MonsterArchetype::BogLurker, 2, 1, 1),
    ],
    // OceanDepths (aquatic only)
    [
        SpawnEntry::new(MonsterArchetype::DeepLeviathan, 4, 1, 1),
        SpawnEntry::new(MonsterArchetype::DeepLeviathan, 2, 1, 2),
        SpawnEntry::new(MonsterArchetype::DeepLeviathan, 1, 1, 1),
        SpawnEntry::new(MonsterArchetype::DeepLeviathan, 1, 1, 1),
    ],
    // MushroomGlade
    [
        SpawnEntry::new(MonsterArchetype::SporeStalker, 8, 2, 5),
        SpawnEntry::new(MonsterArchetype::BogLurker, 3, 1, 2),
        SpawnEntry::new(MonsterArchetype::Goblin, 2, 1, 2),
        SpawnEntry::new(MonsterArchetype::SporeStalker, 3, 1, 3),
    ],
];

/// Ambient spawn chance per roll, out of 256 (index = `BiomeKind as u8`).
pub const BIOME_SPAWN_DENSITY: [u8; BiomeKind::COUNT] = [96, 112, 72, 64, 80, 128, 40, 144];

/// Selects a spawn row from `biome`'s table using the 16-bit roll `r`.
///
/// Weighted selection over the four rows; rows with weight `0` are skipped.
pub const fn pick_spawn_entry(biome: BiomeKind, r: u32) -> SpawnEntry {
    let rows = &BIOME_SPAWN_TABLE[biome as usize];
    let total = rows[0].weight as u32 + rows[1].weight as u32 + rows[2].weight as u32 + rows[3].weight as u32;
    if total == 0 {
        return rows[0];
    }
    let mut pick = r % total;
    let mut i = 0usize;
    while i < 4 {
        let w = rows[i].weight as u32;
        if pick < w {
            return rows[i];
        }
        pick -= w;
        i += 1;
    }
    rows[3]
}

/// True when `(entry.archetype)` is listed in `biome`'s spawn table.
pub const fn biome_allows(biome: BiomeKind, archetype: MonsterArchetype) -> bool {
    let rows = &BIOME_SPAWN_TABLE[biome as usize];
    let mut i = 0usize;
    while i < 4 {
        if rows[i].weight > 0 && rows[i].archetype as u8 == archetype as u8 {
            return true;
        }
        i += 1;
    }
    false
}

/// Monster tier for a chunk at Chebyshev chunk distance `chunk_dist` from the origin.
#[inline(always)]
pub const fn tier_for_chunk_distance(chunk_dist: i32) -> u8 {
    let d = if chunk_dist < 0 { -chunk_dist } else { chunk_dist };
    let t = d / TIER_CHUNK_STEP;
    if t > MAX_TIER as i32 {
        MAX_TIER
    } else {
        t as u8
    }
}

#[inline(always)]
const fn cheb(ax: i32, ay: i32, bx: i32, by: i32) -> i32 {
    let dx = (ax as i64 - bx as i64).abs();
    let dy = (ay as i64 - by as i64).abs();
    let m = if dx > dy { dx } else { dy };
    if m > i32::MAX as i64 {
        i32::MAX
    } else {
        m as i32
    }
}

// ---------------------------------------------------------------------------
// World monster record
// ---------------------------------------------------------------------------

/// Lifecycle state of a [`WorldMonster`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u8)]
pub enum MonsterState {
    /// Slot is free.
    #[default]
    Dead = 0,
    /// Ambient wanderer leashed to its home chunk.
    Roaming = 1,
    /// Assaulting a stronghold heart.
    Hunting = 2,
}

impl MonsterState {
    #[inline(always)]
    pub const fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::Roaming,
            2 => Self::Hunting,
            _ => Self::Dead,
        }
    }
}

/// One live monster in the open world (64 bytes, one cache line).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(C, align(64))]
pub struct WorldMonster {
    /// Unique id (`0` = free slot).
    pub id: u32,
    pub pack_id: u32,
    pub tile_x: i32,
    pub tile_y: i32,
    /// Home chunk; the monster despawns when it leaves the view ring.
    pub home_cx: i32,
    pub home_cy: i32,
    pub spawn_tick: u64,
    pub hp: u16,
    pub max_hp: u16,
    pub attack: u16,
    pub armor: u16,
    pub attack_timer: u8,
    pub move_interval: u8,
    pub archetype: MonsterArchetype,
    pub tier: u8,
    pub state: MonsterState,
    /// Biome of the spawn tile.
    pub biome: BiomeKind,
    pub _pad0: [u8; 2],
    pub kills: u16,
    pub _reserved: [u8; 14],
}

impl Default for WorldMonster {
    fn default() -> Self {
        Self::empty()
    }
}

impl WorldMonster {
    pub const fn empty() -> Self {
        Self {
            id: 0,
            pack_id: 0,
            tile_x: 0,
            tile_y: 0,
            home_cx: 0,
            home_cy: 0,
            spawn_tick: 0,
            hp: 0,
            max_hp: 0,
            attack: 0,
            armor: 0,
            attack_timer: 0,
            move_interval: 1,
            archetype: MonsterArchetype::Goblin,
            tier: 0,
            state: MonsterState::Dead,
            biome: BiomeKind::TemperatePlains,
            _pad0: [0; 2],
            kills: 0,
            _reserved: [0; 14],
        }
    }

    #[inline(always)]
    pub const fn is_alive(&self) -> bool {
        self.id != 0 && self.hp > 0 && !matches!(self.state, MonsterState::Dead)
    }

    #[inline(always)]
    pub const fn home(&self) -> ChunkCoord {
        ChunkCoord::new(self.home_cx, self.home_cy)
    }

    /// Applies `raw` damage minus armor (minimum 1). Returns `true` when this hit killed.
    pub fn take_damage(&mut self, raw: u16) -> bool {
        let dmg = if raw > self.armor { raw - self.armor } else { 1 };
        self.hp = self.hp.saturating_sub(dmg);
        if self.hp == 0 {
            self.state = MonsterState::Dead;
            true
        } else {
            false
        }
    }

    fn hash_into(&self, h: &mut Fnv1aHasher) {
        h.write(&self.id.to_le_bytes());
        h.write(&self.pack_id.to_le_bytes());
        h.write(&self.tile_x.to_le_bytes());
        h.write(&self.tile_y.to_le_bytes());
        h.write(&self.home_cx.to_le_bytes());
        h.write(&self.home_cy.to_le_bytes());
        h.write(&self.spawn_tick.to_le_bytes());
        h.write(&self.hp.to_le_bytes());
        h.write(&self.max_hp.to_le_bytes());
        h.write(&self.attack.to_le_bytes());
        h.write(&self.armor.to_le_bytes());
        h.write(&[
            self.attack_timer,
            self.move_interval,
            self.archetype as u8,
            self.tier,
            self.state as u8,
            self.biome as u8,
        ]);
        h.write(&self.kills.to_le_bytes());
    }
}

/// Metrics returned by one [`MonsterSpawner::update`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SpawnStats {
    pub rolled_chunks: u32,
    pub spawned: u32,
    pub packs: u32,
    pub despawned: u32,
    pub moved: u32,
    pub alive: u32,
}

/// Placement offsets around a pack anchor (Chebyshev radius 2, anchor first).
const PACK_OFFSETS: [(i32, i32); 25] = [
    (0, 0),
    (1, 0),
    (0, 1),
    (-1, 0),
    (0, -1),
    (1, 1),
    (-1, 1),
    (-1, -1),
    (1, -1),
    (2, 0),
    (0, 2),
    (-2, 0),
    (0, -2),
    (2, 1),
    (1, 2),
    (-1, 2),
    (-2, 1),
    (-2, -1),
    (-1, -2),
    (1, -2),
    (2, -1),
    (2, 2),
    (-2, 2),
    (-2, -2),
    (2, -2),
];

// ---------------------------------------------------------------------------
// Monster spawner
// ---------------------------------------------------------------------------

/// Fixed-slab biome monster population driven by the loaded chunk window.
#[derive(Debug, Clone, PartialEq, Eq)]
#[repr(C, align(64))]
pub struct MonsterSpawner<const CAP: usize = DEFAULT_MAX_WORLD_MONSTERS> {
    pub monsters: [WorldMonster; CAP],
    pub alive_count: usize,
    pub next_id: u32,
    pub next_pack_id: u32,
    pub world_seed: u64,
    pub spawn_interval: u32,
    pub despawn_radius: i32,
    pub max_per_chunk: u8,
    /// Chebyshev chunk radius around the centre that receives ambient rolls.
    pub spawn_radius: i32,
    /// Ambient rolls stop once this many monsters are alive, reserving the rest of the
    /// slab for assault waves (default: 3/4 of `CAP`).
    pub ambient_cap: usize,
    /// No ambient spawn within `safe_radius` tiles of `(safe_x, safe_y)` when `safe_radius > 0`.
    pub safe_x: i32,
    pub safe_y: i32,
    pub safe_radius: i32,
    pub total_spawned: u64,
    pub total_despawned: u64,
    pub total_killed: u64,
}

impl<const CAP: usize> Default for MonsterSpawner<CAP> {
    fn default() -> Self {
        Self::new(0)
    }
}

impl<const CAP: usize> MonsterSpawner<CAP> {
    pub const fn new(world_seed: u64) -> Self {
        Self {
            monsters: [WorldMonster::empty(); CAP],
            alive_count: 0,
            next_id: 1,
            next_pack_id: 1,
            world_seed,
            spawn_interval: DEFAULT_SPAWN_INTERVAL,
            despawn_radius: DEFAULT_DESPAWN_RADIUS,
            max_per_chunk: DEFAULT_MAX_MONSTERS_PER_CHUNK,
            spawn_radius: DEFAULT_DESPAWN_RADIUS,
            ambient_cap: CAP - CAP / 4,
            safe_x: 0,
            safe_y: 0,
            safe_radius: 0,
            total_spawned: 0,
            total_despawned: 0,
            total_killed: 0,
        }
    }

    /// Disables ambient spawning within `radius` tiles (Chebyshev) of `(x, y)`.
    pub fn set_safe_zone(&mut self, x: i32, y: i32, radius: i32) {
        self.safe_x = x;
        self.safe_y = y;
        self.safe_radius = radius;
    }

    #[inline(always)]
    pub const fn capacity(&self) -> usize {
        CAP
    }

    #[inline(always)]
    pub const fn alive(&self) -> usize {
        self.alive_count
    }

    /// Number of live monsters homed in `coord`.
    pub fn count_in_chunk(&self, coord: ChunkCoord) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < CAP {
            let m = &self.monsters[i];
            if m.is_alive() && m.home_cx == coord.x && m.home_cy == coord.y {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// Number of live monsters of `archetype`.
    pub fn count_archetype(&self, archetype: MonsterArchetype) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < CAP {
            let m = &self.monsters[i];
            if m.is_alive() && m.archetype == archetype {
                n += 1;
            }
            i += 1;
        }
        n
    }

    #[inline]
    fn in_safe_zone(&self, x: i32, y: i32) -> bool {
        self.safe_radius > 0 && cheb(x, y, self.safe_x, self.safe_y) <= self.safe_radius
    }

    /// Places one monster. Returns the slot index or `None` when the slab is full.
    #[allow(clippy::too_many_arguments)]
    pub fn spawn_monster(
        &mut self,
        archetype: MonsterArchetype,
        tier: u8,
        tile_x: i32,
        tile_y: i32,
        home: ChunkCoord,
        biome: BiomeKind,
        pack_id: u32,
        state: MonsterState,
        tick: u64,
    ) -> Option<usize> {
        if self.alive_count >= CAP || matches!(state, MonsterState::Dead) {
            return None;
        }
        let mut i = 0usize;
        while i < CAP {
            if self.monsters[i].id == 0 {
                let id = self.next_id;
                self.next_id = self.next_id.wrapping_add(1);
                if self.next_id == 0 {
                    self.next_id = 1;
                }
                self.monsters[i] = WorldMonster {
                    id,
                    pack_id,
                    tile_x,
                    tile_y,
                    home_cx: home.x,
                    home_cy: home.y,
                    spawn_tick: tick,
                    hp: archetype.hp_at_tier(tier),
                    max_hp: archetype.hp_at_tier(tier),
                    attack: archetype.attack_at_tier(tier),
                    armor: archetype.armor(),
                    attack_timer: 0,
                    move_interval: archetype.move_interval(),
                    archetype,
                    tier,
                    state,
                    biome,
                    _pad0: [0; 2],
                    kills: 0,
                    _reserved: [0; 14],
                };
                self.alive_count += 1;
                self.total_spawned += 1;
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// Frees slot `idx`. Returns `true` when a live monster was removed.
    pub fn remove_at(&mut self, idx: usize) -> bool {
        if idx >= CAP || self.monsters[idx].id == 0 {
            return false;
        }
        self.monsters[idx] = WorldMonster::empty();
        self.alive_count = self.alive_count.saturating_sub(1);
        true
    }

    /// Releases every slot whose monster is dead (`hp == 0`). Returns released count.
    pub fn reap_dead(&mut self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < CAP {
            if self.monsters[i].id != 0 && !self.monsters[i].is_alive() {
                self.remove_at(i);
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// Despawns monsters whose home chunk is farther than `despawn_radius` (Chebyshev
    /// chunks) from `center`. Mirrors chunk eviction so no monster outlives its chunk.
    pub fn despawn_out_of_range(&mut self, center: ChunkCoord) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < CAP {
            let m = &self.monsters[i];
            if m.id != 0 && cheb(m.home_cx, m.home_cy, center.x, center.y) > self.despawn_radius {
                self.remove_at(i);
                self.total_despawned += 1;
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// Rolls ambient spawns for every loaded chunk within `spawn_radius` of the manager's
    /// centre. Runs only on ticks that are multiples of `spawn_interval`.
    pub fn spawn_from_chunks<const S: usize, const MC: usize, const DC: usize>(
        &mut self,
        mgr: &ChunkManager<S, MC, DC>,
        tick: u64,
        stats: &mut SpawnStats,
    ) {
        let interval = if self.spawn_interval == 0 { 1 } else { self.spawn_interval as u64 };
        if tick % interval != 0 {
            return;
        }
        let epoch = tick / interval;
        let roll_seed = self.world_seed ^ SPAWN_SALT ^ epoch.wrapping_mul(0x9E37_79B9_7F4A_7C15);

        let mut radius = if self.spawn_radius < mgr.view_radius { self.spawn_radius } else { mgr.view_radius };
        if radius > SPAWN_RING_MAX_RADIUS {
            radius = SPAWN_RING_MAX_RADIUS;
        }
        if radius < 0 {
            return;
        }
        let span = (radius as usize) * 2 + 1;
        let center = mgr.center_chunk;

        // Per-chunk population inside the ring (single pass over the slab).
        let mut counts = [0u8; SPAWN_RING_SPAN * SPAWN_RING_SPAN];
        let mut i = 0usize;
        while i < CAP {
            let m = &self.monsters[i];
            if m.is_alive() {
                let dx = m.home_cx as i64 - center.x as i64;
                let dy = m.home_cy as i64 - center.y as i64;
                if dx >= -(radius as i64) && dx <= radius as i64 && dy >= -(radius as i64) && dy <= radius as i64 {
                    let idx = ((dy + radius as i64) as usize) * span + (dx + radius as i64) as usize;
                    counts[idx] = counts[idx].saturating_add(1);
                }
            }
            i += 1;
        }

        let mut dy = -radius;
        while dy <= radius {
            let mut dx = -radius;
            while dx <= radius {
                let ring_idx = ((dy + radius) as usize) * span + (dx + radius) as usize;
                dx += 1;
                let coord = ChunkCoord::new(center.x.wrapping_add(dx - 1), center.y.wrapping_add(dy));
                let Some(chunk) = mgr.chunk_at(coord) else {
                    continue;
                };
                stats.rolled_chunks += 1;
                let present = counts[ring_idx];
                if present >= self.max_per_chunk || self.alive_count >= self.ambient_cap {
                    continue;
                }

                let h = hash2d(coord.x, coord.y, roll_seed);
                let mut density = BIOME_SPAWN_DENSITY[chunk.header.primary_biome as usize] as u32;
                if chunk.header.deposit_tile_count >= GUARDIAN_DEPOSIT_THRESHOLD {
                    density += GUARDIAN_DENSITY_BONUS as u32;
                }
                if (h & 0xFF) >= density {
                    continue;
                }

                // Candidate anchor tiles inside the chunk.
                let tier = tier_for_chunk_distance(cheb(coord.x, coord.y, 0, 0));
                let mut attempt = 0u32;
                let mut spawned_here = false;
                while attempt < SPAWN_CANDIDATE_TRIES && !spawned_here {
                    let hh = hash2d(attempt as i32, coord.x ^ coord.y.rotate_left(16), roll_seed ^ h as u64);
                    attempt += 1;
                    let lx = (hh as usize) % S;
                    let ly = ((hh >> 8) as usize) % S;
                    let biome = BiomeKind::from_u8(chunk.biomes[ly][lx]);
                    let entry = pick_spawn_entry(biome, hh >> 16);
                    if entry.weight == 0 || !entry.archetype.accepts_tile(chunk.tiles[ly][lx]) {
                        continue;
                    }
                    let ax = coord.x.wrapping_mul(S as i32).wrapping_add(lx as i32);
                    let ay = coord.y.wrapping_mul(S as i32).wrapping_add(ly as i32);
                    if self.in_safe_zone(ax, ay) {
                        continue;
                    }

                    let slab_room = (self.ambient_cap - self.alive_count) as u32;
                    let mut room = (self.max_per_chunk - present) as u32;
                    if room > slab_room {
                        room = slab_room;
                    }
                    let span_pack = (entry.pack_max.max(entry.pack_min) - entry.pack_min) as u32 + 1;
                    let mut pack = entry.pack_min as u32 + ((hh >> 24) % span_pack);
                    if pack > room {
                        pack = room;
                    }
                    if pack == 0 {
                        break;
                    }

                    let pack_id = self.next_pack_id;
                    self.next_pack_id = self.next_pack_id.wrapping_add(1).max(1);
                    let mut placed = 0u32;
                    let mut k = 0usize;
                    while k < PACK_OFFSETS.len() && placed < pack {
                        let (ox, oy) = PACK_OFFSETS[k];
                        k += 1;
                        let px = ax.wrapping_add(ox);
                        let py = ay.wrapping_add(oy);
                        if !entry.archetype.accepts_tile(mgr.tile_at(px as i64, py as i64)) {
                            continue;
                        }
                        if self.in_safe_zone(px, py) {
                            continue;
                        }
                        let slot = self.spawn_monster(
                            entry.archetype,
                            tier,
                            px,
                            py,
                            coord,
                            biome,
                            pack_id,
                            MonsterState::Roaming,
                            tick,
                        );
                        if slot.is_none() {
                            break;
                        }
                        placed += 1;
                    }
                    if placed > 0 {
                        counts[ring_idx] = counts[ring_idx].saturating_add(placed as u8);
                        stats.spawned += placed;
                        stats.packs += 1;
                        spawned_here = true;
                    }
                }
            }
            dy += 1;
        }
    }

    /// Moves roaming monsters one step on their move interval, leashed to their home chunk.
    pub fn step_roam<const S: usize, const MC: usize, const DC: usize>(
        &mut self,
        mgr: &ChunkManager<S, MC, DC>,
        tick: u64,
        stats: &mut SpawnStats,
    ) {
        let roam_seed = self.world_seed ^ ROAM_SALT;
        let mut i = 0usize;
        while i < CAP {
            let m = self.monsters[i];
            i += 1;
            if !m.is_alive() || !matches!(m.state, MonsterState::Roaming) {
                continue;
            }
            let interval = if m.move_interval == 0 { 1 } else { m.move_interval as u64 };
            if (tick.wrapping_add(m.id as u64)) % interval != 0 {
                continue;
            }
            let r = hash2d(m.id as i32, tick as i32, roam_seed ^ (tick >> 32));
            // 1/4 of rolls idle so packs drift instead of jittering every interval.
            if (r >> 2) & 3 == 0 {
                continue;
            }
            let (ox, oy) = match r & 3 {
                0 => (1, 0),
                1 => (-1, 0),
                2 => (0, 1),
                _ => (0, -1),
            };
            let nx = m.tile_x.wrapping_add(ox);
            let ny = m.tile_y.wrapping_add(oy);
            let (home, _, _) = ChunkCoord::from_world_tile(nx as i64, ny as i64, S);
            if home.x != m.home_cx || home.y != m.home_cy {
                continue;
            }
            if !m.archetype.accepts_tile(mgr.tile_at(nx as i64, ny as i64)) || self.in_safe_zone(nx, ny) {
                continue;
            }
            let slot = &mut self.monsters[i - 1];
            slot.tile_x = nx;
            slot.tile_y = ny;
            stats.moved += 1;
        }
    }

    /// One full ambient update: eviction despawn ➔ spawn rolls ➔ roaming.
    pub fn update<const S: usize, const MC: usize, const DC: usize>(
        &mut self,
        mgr: &ChunkManager<S, MC, DC>,
        tick: u64,
    ) -> SpawnStats {
        let mut stats = SpawnStats::default();
        stats.despawned = self.despawn_out_of_range(mgr.center_chunk);
        self.spawn_from_chunks(mgr, tick, &mut stats);
        self.step_roam(mgr, tick, &mut stats);
        stats.alive = self.alive_count as u32;
        stats
    }

    /// Deterministic FNV-1a hash of the whole population.
    pub fn compute_hash(&self) -> u64 {
        let mut h = Fnv1aHasher::new();
        h.write(&self.world_seed.to_le_bytes());
        h.write(&(self.alive_count as u64).to_le_bytes());
        h.write(&self.next_id.to_le_bytes());
        h.write(&self.next_pack_id.to_le_bytes());
        let mut i = 0usize;
        while i < CAP {
            let m = &self.monsters[i];
            if m.id != 0 {
                h.write(&(i as u32).to_le_bytes());
                m.hash_into(&mut h);
            }
            i += 1;
        }
        h.finish()
    }
}

// ---------------------------------------------------------------------------
// Stronghold territory defence
// ---------------------------------------------------------------------------

/// Static defence emplacement guarding a stronghold (16 bytes, POD).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(C)]
pub struct DefensePost {
    pub x: i32,
    pub y: i32,
    pub damage: u16,
    /// Firing radius in tiles (compared as squared integers).
    pub range: u8,
    pub cooldown: u8,
    pub timer: u8,
    pub active: bool,
    pub _pad: [u8; 2],
}

impl DefensePost {
    pub const fn new(x: i32, y: i32, damage: u16, range: u8, cooldown: u8) -> Self {
        Self {
            x,
            y,
            damage,
            range,
            cooldown,
            timer: 0,
            active: true,
            _pad: [0; 2],
        }
    }
}

/// Per-tick report of a [`TerritoryDefense::step`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DefenseTickReport {
    pub wave_spawned: u16,
    /// Hunting monsters inside the territory after this tick.
    pub intruders: u16,
    pub post_shots: u16,
    pub kills: u16,
    pub heart_damage: u32,
}

/// Stronghold heart + territory ring + defence posts + assault wave schedule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C, align(64))]
pub struct TerritoryDefense<const POSTS: usize = DEFAULT_MAX_DEFENSE_POSTS> {
    pub heart_x: i32,
    pub heart_y: i32,
    pub territory_radius: i32,
    pub hp: u32,
    pub max_hp: u32,
    pub wave_index: u16,
    pub base_wave_size: u16,
    pub wave_growth: u16,
    pub wave_interval: u32,
    pub next_wave_tick: u64,
    pub seed: u64,
    pub kills: u32,
    pub total_waves: u32,
    pub damage_taken: u64,
    pub breached: bool,
    pub post_count: u8,
    pub _pad: [u8; 6],
    pub posts: [DefensePost; POSTS],
}

impl<const POSTS: usize> Default for TerritoryDefense<POSTS> {
    fn default() -> Self {
        Self::new(0, 0, 0, DEFAULT_TERRITORY_RADIUS, DEFAULT_STRONGHOLD_HP)
    }
}

impl<const POSTS: usize> TerritoryDefense<POSTS> {
    pub const fn new(seed: u64, heart_x: i32, heart_y: i32, territory_radius: i32, hp: u32) -> Self {
        Self {
            heart_x,
            heart_y,
            territory_radius: if territory_radius < 1 { 1 } else { territory_radius },
            hp,
            max_hp: hp,
            wave_index: 0,
            base_wave_size: DEFAULT_BASE_WAVE_SIZE,
            wave_growth: DEFAULT_WAVE_GROWTH,
            wave_interval: DEFAULT_WAVE_INTERVAL,
            next_wave_tick: DEFAULT_FIRST_WAVE_TICK,
            seed: seed ^ WAVE_SALT,
            kills: 0,
            total_waves: 0,
            damage_taken: 0,
            breached: false,
            post_count: 0,
            _pad: [0; 6],
            posts: [DefensePost {
                x: 0,
                y: 0,
                damage: 0,
                range: 0,
                cooldown: 0,
                timer: 0,
                active: false,
                _pad: [0; 2],
            }; POSTS],
        }
    }

    /// Overrides the wave schedule.
    pub fn set_schedule(&mut self, first_wave_tick: u64, interval: u32, base_size: u16, growth: u16) {
        self.next_wave_tick = first_wave_tick;
        self.wave_interval = if interval == 0 { 1 } else { interval };
        self.base_wave_size = base_size;
        self.wave_growth = growth;
    }

    /// Adds a defence post. Returns `false` when the post slab is full.
    pub fn add_post(&mut self, x: i32, y: i32, damage: u16, range: u8, cooldown: u8) -> bool {
        let n = self.post_count as usize;
        if n >= POSTS {
            return false;
        }
        self.posts[n] = DefensePost::new(x, y, damage, range, cooldown);
        self.post_count += 1;
        true
    }

    #[inline(always)]
    pub const fn wave_size(&self, n: u16) -> u16 {
        self.base_wave_size.saturating_add(self.wave_growth.saturating_mul(n))
    }

    #[inline(always)]
    pub const fn in_territory(&self, x: i32, y: i32) -> bool {
        cheb(x, y, self.heart_x, self.heart_y) <= self.territory_radius
    }

    /// Spawns one assault wave on the territory ring from the surrounding biomes.
    /// Returns the number of monsters placed.
    pub fn spawn_wave<const CAP: usize, const S: usize, const MC: usize, const DC: usize>(
        &mut self,
        spawner: &mut MonsterSpawner<CAP>,
        mgr: &ChunkManager<S, MC, DC>,
        tick: u64,
    ) -> u16 {
        let n = self.wave_index;
        let size = self.wave_size(n);
        let ring = self.territory_radius + WAVE_RING_MARGIN;
        let perimeter = (ring as u32) * 8;
        let tier_raw = n / 3;
        let tier = if tier_raw > MAX_TIER as u16 { MAX_TIER } else { tier_raw as u8 };
        let pack_id = spawner.next_pack_id;
        spawner.next_pack_id = spawner.next_pack_id.wrapping_add(1).max(1);

        let mut placed = 0u16;
        let mut member = 0u32;
        while member < size as u32 {
            member += 1;
            let mut attempt = 0u32;
            while attempt < WAVE_PLACEMENT_TRIES {
                let r = hash2d(member as i32, attempt as i32, self.seed ^ (n as u64).rotate_left(40));
                attempt += 1;
                let t = r % perimeter;
                let side = t / (ring as u32 * 2);
                let off = (t % (ring as u32 * 2)) as i32 - ring;
                let (dx, dy) = match side {
                    0 => (off, -ring),
                    1 => (ring, off),
                    2 => (off, ring),
                    _ => (-ring, off),
                };
                let px = self.heart_x.wrapping_add(dx);
                let py = self.heart_y.wrapping_add(dy);
                let biome = mgr.biome_at(px as i64, py as i64);
                let entry = pick_spawn_entry(biome, r >> 16);
                if entry.weight == 0 || !entry.archetype.accepts_tile(mgr.tile_at(px as i64, py as i64)) {
                    continue;
                }
                let (home, _, _) = ChunkCoord::from_world_tile(px as i64, py as i64, S);
                if spawner
                    .spawn_monster(
                        entry.archetype,
                        tier,
                        px,
                        py,
                        home,
                        biome,
                        pack_id,
                        MonsterState::Hunting,
                        tick,
                    )
                    .is_some()
                {
                    placed += 1;
                }
                break;
            }
        }

        self.wave_index = self.wave_index.wrapping_add(1);
        self.total_waves += 1;
        placed
    }

    /// Advances the defence by one tick: schedule ➔ aggro ➔ march & strike ➔ posts.
    pub fn step<const CAP: usize, const S: usize, const MC: usize, const DC: usize>(
        &mut self,
        spawner: &mut MonsterSpawner<CAP>,
        mgr: &ChunkManager<S, MC, DC>,
        tick: u64,
    ) -> DefenseTickReport {
        let mut report = DefenseTickReport::default();

        // 1. Wave schedule.
        if !self.breached && tick >= self.next_wave_tick {
            report.wave_spawned = self.spawn_wave(spawner, mgr, tick);
            self.next_wave_tick = tick + self.wave_interval as u64;
        }

        // 2. Aggro + march + strike.
        let mut i = 0usize;
        while i < CAP {
            let m = spawner.monsters[i];
            i += 1;
            if !m.is_alive() {
                continue;
            }
            let slot = &mut spawner.monsters[i - 1];
            if matches!(m.state, MonsterState::Roaming) {
                if self.in_territory(m.tile_x, m.tile_y) {
                    slot.state = MonsterState::Hunting;
                } else {
                    continue;
                }
            }

            let dist = cheb(m.tile_x, m.tile_y, self.heart_x, self.heart_y);
            if dist <= 1 {
                if slot.attack_timer == 0 {
                    if !self.breached {
                        let dmg = m.attack as u32;
                        let applied = if dmg > self.hp { self.hp } else { dmg };
                        self.hp -= applied;
                        self.damage_taken += applied as u64;
                        report.heart_damage += applied;
                        if self.hp == 0 {
                            self.breached = true;
                        }
                    }
                    slot.attack_timer = MONSTER_ATTACK_COOLDOWN;
                } else {
                    slot.attack_timer -= 1;
                }
            } else {
                let interval = if m.move_interval == 0 { 1 } else { m.move_interval as u64 };
                if (tick.wrapping_add(m.id as u64)) % interval == 0 {
                    let ddx = self.heart_x as i64 - m.tile_x as i64;
                    let ddy = self.heart_y as i64 - m.tile_y as i64;
                    let sx = if ddx > 0 { 1 } else if ddx < 0 { -1 } else { 0 };
                    let sy = if ddy > 0 { 1 } else if ddy < 0 { -1 } else { 0 };
                    let primary_x = ddx.abs() >= ddy.abs();
                    let candidates: [(i32, i32); 4] = if primary_x {
                        [(sx, 0), (0, sy), (0, 1), (0, -1)]
                    } else {
                        [(0, sy), (sx, 0), (1, 0), (-1, 0)]
                    };
                    let mut c = 0usize;
                    while c < 4 {
                        let (ox, oy) = candidates[c];
                        c += 1;
                        if ox == 0 && oy == 0 {
                            continue;
                        }
                        let nx = m.tile_x.wrapping_add(ox);
                        let ny = m.tile_y.wrapping_add(oy);
                        if m.archetype.accepts_tile(mgr.tile_at(nx as i64, ny as i64)) {
                            slot.tile_x = nx;
                            slot.tile_y = ny;
                            break;
                        }
                    }
                }
            }
            if self.in_territory(slot.tile_x, slot.tile_y) {
                report.intruders = report.intruders.saturating_add(1);
            }
        }

        // 3. Defence posts: nearest live monster in range, lowest id on ties.
        let mut p = 0usize;
        while p < self.post_count as usize && p < POSTS {
            let post = self.posts[p];
            p += 1;
            if !post.active {
                continue;
            }
            if post.timer > 0 {
                self.posts[p - 1].timer -= 1;
                continue;
            }
            let range_sq = (post.range as i64) * (post.range as i64);
            let mut best: Option<(usize, i64, u32)> = None;
            let mut j = 0usize;
            while j < CAP {
                let m = &spawner.monsters[j];
                j += 1;
                if !m.is_alive() {
                    continue;
                }
                let dx = m.tile_x as i64 - post.x as i64;
                let dy = m.tile_y as i64 - post.y as i64;
                let d2 = dx * dx + dy * dy;
                if d2 > range_sq {
                    continue;
                }
                let better = match best {
                    None => true,
                    Some((_, bd, bid)) => d2 < bd || (d2 == bd && m.id < bid),
                };
                if better {
                    best = Some((j - 1, d2, m.id));
                }
            }
            if let Some((idx, _, _)) = best {
                let killed = spawner.monsters[idx].take_damage(post.damage);
                report.post_shots += 1;
                self.posts[p - 1].timer = post.cooldown;
                if killed {
                    report.kills += 1;
                    self.kills += 1;
                    spawner.total_killed += 1;
                }
            }
        }

        // 4. Release killed monsters.
        if report.kills > 0 {
            spawner.reap_dead();
        }
        report
    }

    /// Deterministic FNV-1a hash of the stronghold state.
    pub fn compute_hash(&self) -> u64 {
        let mut h = Fnv1aHasher::new();
        h.write(&self.heart_x.to_le_bytes());
        h.write(&self.heart_y.to_le_bytes());
        h.write(&self.territory_radius.to_le_bytes());
        h.write(&self.hp.to_le_bytes());
        h.write(&self.wave_index.to_le_bytes());
        h.write(&self.next_wave_tick.to_le_bytes());
        h.write(&self.kills.to_le_bytes());
        h.write(&self.damage_taken.to_le_bytes());
        h.write(&[self.breached as u8, self.post_count]);
        let mut p = 0usize;
        while p < self.post_count as usize && p < POSTS {
            let post = &self.posts[p];
            h.write(&post.x.to_le_bytes());
            h.write(&post.y.to_le_bytes());
            h.write(&post.damage.to_le_bytes());
            h.write(&[post.range, post.cooldown, post.timer, post.active as u8]);
            p += 1;
        }
        h.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_monster_record_layout() {
        assert_eq!(core::mem::size_of::<WorldMonster>(), 64);
        assert_eq!(core::mem::align_of::<WorldMonster>(), 64);
        assert_eq!(core::mem::size_of::<SpawnEntry>(), 4);
        assert_eq!(core::mem::size_of::<DefensePost>(), 16);
        assert_eq!(core::mem::align_of::<TerritoryDefense<16>>(), 64);
        assert_eq!(core::mem::align_of::<MonsterSpawner<8>>(), 64);
    }

    #[test]
    fn test_spawn_table_integrity() {
        for biome in BiomeKind::ALL {
            let rows = BIOME_SPAWN_TABLE[biome as usize];
            let total: u32 = rows.iter().map(|r| r.weight as u32).sum();
            assert!(total > 0, "{} has no weighted rows", biome.name());
            for r in rows {
                assert!(r.pack_min >= 1 && r.pack_max >= r.pack_min);
                assert_eq!(r.archetype.is_aquatic(), matches!(biome, BiomeKind::OceanDepths));
            }
            // Every roll lands on a weighted row of that biome.
            for roll in 0..256u32 {
                let e = pick_spawn_entry(biome, roll * 0x9E37);
                assert!(e.weight > 0);
                assert!(biome_allows(biome, e.archetype));
            }
        }
        for a in MonsterArchetype::ALL {
            assert_eq!(a.native_biome() as u8, a as u8);
            assert!(biome_allows(a.native_biome(), a));
            assert!(a.hp_at_tier(4) > a.hp_at_tier(0));
            assert!(a.attack_at_tier(8) > a.attack_at_tier(0));
        }
        assert_eq!(tier_for_chunk_distance(0), 0);
        assert_eq!(tier_for_chunk_distance(-9), 2);
        assert_eq!(tier_for_chunk_distance(i32::MAX), MAX_TIER);
    }

    #[test]
    fn test_take_damage_armor_floor() {
        let mut m = WorldMonster::empty();
        m.id = 1;
        m.state = MonsterState::Roaming;
        m.hp = 10;
        m.armor = 50;
        assert!(!m.take_damage(5));
        assert_eq!(m.hp, 9);
        assert!(m.take_damage(100));
        assert_eq!(m.hp, 0);
        assert!(!m.is_alive());
    }
}
