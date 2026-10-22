//! Seed-based Deterministic Procedural Content Generation (PCG).
//!
//! Provides integer-only PRNG (SplitMix64 / XorShift64), deterministic
//! tilemap generation, and the infinite-world biome / resource sampler
//! without floating-point math or OS randomness.

use crate::hash::deterministic_hash;

/// Discrete world terrain tile types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum TileType {
    /// Walkable open ground
    Plains = 0,
    /// Tree cover (resource node, slows movement)
    Forest = 1,
    /// Impassable mineral/mountain terrain
    Rock = 2,
    /// Impassable water bodies
    Water = 3,
}

impl TileType {
    #[inline(always)]
    pub fn is_walkable(self) -> bool {
        matches!(self, Self::Plains | Self::Forest)
    }

    #[inline(always)]
    pub fn is_obstacle(self) -> bool {
        matches!(self, Self::Rock | Self::Water)
    }

    #[inline(always)]
    pub const fn from_u8(v: u8) -> Self {
        match v & 3 {
            0 => Self::Plains,
            1 => Self::Forest,
            2 => Self::Rock,
            _ => Self::Water,
        }
    }
}

/// 100% Deterministic SplitMix64 pseudo-random number generator.
///
/// Guaranteed identical output sequence across all platforms for a given 64-bit seed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rng {
    state: u64,
}

impl Rng {
    /// Initializes RNG with a non-zero deterministic seed.
    pub const fn with_seed(seed: u64) -> Self {
        // Guarantee state is not completely zero
        Self {
            state: if seed == 0 { 0x9E3779B97F4A7C15 } else { seed },
        }
    }

    /// Generates next pseudo-random 64-bit unsigned integer using SplitMix64.
    #[inline]
    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E3779B97F4A7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
        z ^ (z >> 31)
    }

    /// Generates next 32-bit integer.
    #[inline(always)]
    pub fn next_u32(&mut self) -> u32 {
        self.next_u64() as u32
    }

    /// Returns a pseudo-random integer in `[min, max]` (inclusive).
    pub fn range_u32(&mut self, min: u32, max: u32) -> u32 {
        if min >= max {
            return min;
        }
        let range = max - min + 1;
        min + (self.next_u32() % range)
    }
}

// ---------------------------------------------------------------------------
// Infinite-world integer noise & biome sampler (Phase 3)
// ---------------------------------------------------------------------------

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

/// Maximum value produced by every noise sampler (`0..=NOISE_MAX`).
pub const NOISE_MAX: u32 = 0xFFFF;

/// Memoized lattice cell for one value-noise layer.
///
/// Consecutive tiles share the same lattice cell `scale` times in a row, so a
/// row-major chunk sweep hashes each lattice corner once instead of once per tile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(C)]
pub struct NoiseCell {
    pub gx: i32,
    pub gy: i32,
    pub v00: i32,
    pub v10: i32,
    pub v01: i32,
    pub v11: i32,
    pub valid: bool,
}

impl NoiseCell {
    pub const EMPTY: Self = Self {
        gx: 0,
        gy: 0,
        v00: 0,
        v10: 0,
        v01: 0,
        v11: 0,
        valid: false,
    };
}

/// Largest supported lattice shift (scale `2^14` tiles).
pub const NOISE_MAX_SHIFT: u32 = 14;

/// Re-hashes the four lattice corners of the cell containing lattice point `(gx0, gy0)`
/// unless the memo already holds them.
#[inline(always)]
pub fn noise_cell_refresh(cell: &mut NoiseCell, gx0: i32, gy0: i32, seed: u64) {
    if cell.valid && cell.gx == gx0 && cell.gy == gy0 {
        return;
    }
    let gx1 = gx0.wrapping_add(1);
    let gy1 = gy0.wrapping_add(1);
    cell.gx = gx0;
    cell.gy = gy0;
    cell.v00 = (hash2d(gx0, gy0, seed) & NOISE_MAX) as i32;
    cell.v10 = (hash2d(gx1, gy0, seed) & NOISE_MAX) as i32;
    cell.v01 = (hash2d(gx0, gy1, seed) & NOISE_MAX) as i32;
    cell.v11 = (hash2d(gx1, gy1, seed) & NOISE_MAX) as i32;
    cell.valid = true;
}

/// Vertical lerp of the cell's left and right edges at row fraction `fy` (`0..scale`).
#[inline(always)]
pub const fn noise_row_edges(cell: &NoiseCell, fy: i64, shift: u32) -> (i64, i64) {
    let left = cell.v00 as i64 + (((cell.v01 - cell.v00) as i64 * fy) >> shift);
    let right = cell.v10 as i64 + (((cell.v11 - cell.v10) as i64 * fy) >> shift);
    (left, right)
}

/// Horizontal lerp between row edges at column fraction `fx` (`0..scale`).
#[inline(always)]
pub const fn noise_lerp_x(left: i64, right: i64, fx: i64, shift: u32) -> u32 {
    (left + (((right - left) * fx) >> shift)) as u32
}

/// Bilinear integer value noise on a power-of-two lattice (`scale = 1 << shift`).
///
/// Arithmetic shifts give floor division and masks give euclidean remainders, so
/// the sampler is a handful of integer ops with no division and no float. The
/// row-edge-then-column formula is shared with [`fill_noise_row`] so that single
/// tile sampling and bulk chunk generation are bit-identical.
#[inline(always)]
pub fn sample_value_noise_cached(
    x: i32,
    y: i32,
    shift: u32,
    seed: u64,
    cell: &mut NoiseCell,
) -> u32 {
    let shift = if shift > NOISE_MAX_SHIFT { NOISE_MAX_SHIFT } else { shift };
    let mask = (1i32 << shift) - 1;
    noise_cell_refresh(cell, x >> shift, y >> shift, seed);
    let (left, right) = noise_row_edges(cell, (y & mask) as i64, shift);
    noise_lerp_x(left, right, (x & mask) as i64, shift)
}

/// Fills `row` with value noise for tiles `(x0 + i, y)`, `i < row.len()`, walking the
/// lattice in runs so each cell is hashed once and each tile costs one multiply.
pub fn fill_noise_row(row: &mut [u32], x0: i32, y: i32, shift: u32, seed: u64, cell: &mut NoiseCell) {
    let shift = if shift > NOISE_MAX_SHIFT { NOISE_MAX_SHIFT } else { shift };
    let mask = (1i32 << shift) - 1;
    let scale = (mask as usize) + 1;
    let gy0 = y >> shift;
    let fy = (y & mask) as i64;
    let len = row.len();

    let mut i = 0usize;
    let mut x = x0;
    while i < len {
        noise_cell_refresh(cell, x >> shift, gy0, seed);
        let (left, right) = noise_row_edges(cell, fy, shift);
        let mut fx = (x & mask) as usize;
        let run = scale - fx; // tiles left until the next lattice column
        let run_end = if i + run < len { i + run } else { len };
        while i < run_end {
            row[i] = noise_lerp_x(left, right, fx as i64, shift);
            fx += 1;
            i += 1;
        }
        x = x.wrapping_add(run as i32);
    }
}

/// Bilinear integer value noise on a power-of-two lattice (uncached form).
#[inline]
pub fn sample_value_noise(x: i32, y: i32, shift: u32, seed: u64) -> u32 {
    let mut cell = NoiseCell::EMPTY;
    sample_value_noise_cached(x, y, shift, seed, &mut cell)
}

/// The eight world biomes of the infinite open world.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u8)]
pub enum BiomeKind {
    /// Temperate plains: grassland, woods, ponds.
    #[default]
    TemperatePlains = 0,
    /// Boreal forest: conifer cover, rocky outcrops.
    BorealForest = 1,
    /// Arid desert: sand, sandstone mesas, rare oases.
    AridDesert = 2,
    /// Frozen tundra: snowfields, permafrost ice spires.
    FrozenTundra = 3,
    /// Volcanic crag: basalt, obsidian, lava / sulphur pools.
    VolcanicCrag = 4,
    /// Swamp marsh: mud flats, bog water, reed beds.
    SwampMarsh = 5,
    /// Ocean depths: deep open water.
    OceanDepths = 6,
    /// Mushroom glade: luminous fungi over fertile soil.
    MushroomGlade = 7,
}

impl BiomeKind {
    pub const COUNT: usize = 8;

    pub const ALL: [BiomeKind; 8] = [
        BiomeKind::TemperatePlains,
        BiomeKind::BorealForest,
        BiomeKind::AridDesert,
        BiomeKind::FrozenTundra,
        BiomeKind::VolcanicCrag,
        BiomeKind::SwampMarsh,
        BiomeKind::OceanDepths,
        BiomeKind::MushroomGlade,
    ];

    #[inline(always)]
    pub const fn from_u8(v: u8) -> Self {
        match v & 7 {
            0 => Self::TemperatePlains,
            1 => Self::BorealForest,
            2 => Self::AridDesert,
            3 => Self::FrozenTundra,
            4 => Self::VolcanicCrag,
            5 => Self::SwampMarsh,
            6 => Self::OceanDepths,
            _ => Self::MushroomGlade,
        }
    }

    #[inline(always)]
    pub const fn as_u8(self) -> u8 {
        self as u8
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::TemperatePlains => "temperate-plains",
            Self::BorealForest => "boreal-forest",
            Self::AridDesert => "arid-desert",
            Self::FrozenTundra => "frozen-tundra",
            Self::VolcanicCrag => "volcanic-crag",
            Self::SwampMarsh => "swamp-marsh",
            Self::OceanDepths => "ocean-depths",
            Self::MushroomGlade => "mushroom-glade",
        }
    }

    /// True when the biome can host mineral / fossil deposits at all.
    #[inline(always)]
    pub const fn supports_deposits(self) -> bool {
        !matches!(self, Self::OceanDepths)
    }
}

/// Mineral / fossil deposit kinds seeded into the terrain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u8)]
pub enum ResourceDeposit {
    #[default]
    None = 0,
    IronOre = 1,
    CopperOre = 2,
    Coal = 3,
    Stone = 4,
    Uranium = 5,
    CrudeOil = 6,
}

impl ResourceDeposit {
    pub const COUNT: usize = 7;

    #[inline(always)]
    pub const fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::IronOre,
            2 => Self::CopperOre,
            3 => Self::Coal,
            4 => Self::Stone,
            5 => Self::Uranium,
            6 => Self::CrudeOil,
            _ => Self::None,
        }
    }

    #[inline(always)]
    pub const fn as_u8(self) -> u8 {
        self as u8
    }

    #[inline(always)]
    pub const fn is_none(self) -> bool {
        matches!(self, Self::None)
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::IronOre => "iron-ore",
            Self::CopperOre => "copper-ore",
            Self::Coal => "coal",
            Self::Stone => "stone",
            Self::Uranium => "uranium",
            Self::CrudeOil => "crude-oil",
        }
    }
}

/// Full procedural sample of one world tile (16 bytes, POD).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(C)]
pub struct BiomeSample {
    /// Composite elevation (`0..=NOISE_MAX`).
    pub elevation: u16,
    /// Composite moisture (`0..=NOISE_MAX`).
    pub moisture: u16,
    /// Composite temperature (`0..=NOISE_MAX`).
    pub temperature: u16,
    /// Deposit ore units at this tile (`0` when `deposit_kind == None`).
    pub deposit_richness: u16,
    pub biome: BiomeKind,
    pub tile: TileType,
    pub deposit_kind: ResourceDeposit,
    /// Manhattan distance to the owning vein centre (`VEIN_NO_DISTANCE` when none).
    pub vein_distance: u8,
    pub _reserved: [u8; 4],
}

impl Default for TileType {
    #[inline(always)]
    fn default() -> Self {
        Self::Plains
    }
}

// --- Noise layer table -------------------------------------------------------

/// Number of value-noise layers blended per tile.
pub const NOISE_LAYER_COUNT: usize = 8;

const LAYER_ELEV_LOW: usize = 0;
const LAYER_ELEV_MID: usize = 1;
const LAYER_ELEV_HIGH: usize = 2;
const LAYER_MOIST_LOW: usize = 3;
const LAYER_MOIST_HIGH: usize = 4;
const LAYER_TEMP_LOW: usize = 5;
const LAYER_TEMP_HIGH: usize = 6;
const LAYER_DETAIL: usize = 7;

/// Lattice shift per layer (`scale = 1 << shift` tiles).
pub const NOISE_LAYER_SHIFTS: [u32; NOISE_LAYER_COUNT] = [6, 5, 4, 6, 4, 7, 5, 2];
/// Lattice scale (tiles) per layer, derived from [`NOISE_LAYER_SHIFTS`].
pub const NOISE_LAYER_SCALES: [i32; NOISE_LAYER_COUNT] = [64, 32, 16, 64, 16, 128, 32, 4];

/// Seed salt per layer (mixed with the world seed).
const NOISE_LAYER_SALTS: [u64; NOISE_LAYER_COUNT] = [
    0x0000_0000_0000_0065,
    0x0000_0000_0000_1F8D,
    0x0000_0000_0003_7E21,
    0x0000_0000_0000_D431,
    0x0000_0000_0001_A2C7,
    0x0000_0000_0002_5B19,
    0x0000_0000_0004_C0DE,
    0x0000_0000_0007_A5E1,
];

// --- Biome classification thresholds (0..=NOISE_MAX) ---------------------------

pub const OCEAN_LEVEL: u32 = 20_000;
pub const CRAG_LEVEL: u32 = 49_000;
pub const COLD_MAX_TEMPERATURE: u32 = 27_000;
pub const HOT_MIN_TEMPERATURE: u32 = 40_000;
pub const DRY_MAX_MOISTURE: u32 = 27_000;
pub const WET_MIN_MOISTURE: u32 = 40_000;

// --- Resource vein lattice -----------------------------------------------------

/// Vein lattice shift (`VEIN_CELL_SIZE = 1 << VEIN_CELL_SHIFT`).
pub const VEIN_CELL_SHIFT: u32 = 4;
/// Vein lattice cell edge in tiles.
pub const VEIN_CELL_SIZE: i32 = 1 << VEIN_CELL_SHIFT;
/// Maximum vein radius (Manhattan) in tiles. Centres are clamped so that a vein
/// never reaches beyond the directly adjacent lattice cell.
pub const VEIN_MAX_RADIUS: i32 = 5;
/// Probability (out of 256) that a lattice cell hosts a vein.
pub const VEIN_DENSITY_PER_256: u32 = 112;
/// `BiomeSample::vein_distance` when the tile holds no deposit.
pub const VEIN_NO_DISTANCE: u8 = u8::MAX;
const VEIN_SALT: u64 = 0x5EED_0F0F_C0FF_EE11;
const VEIN_RICHNESS_MIN: u32 = 400;
const VEIN_RICHNESS_SPAN: u32 = 1_200;

/// Deposit kinds that may spawn in each biome (index = `BiomeKind as u8`).
pub const BIOME_DEPOSIT_TABLE: [[ResourceDeposit; 4]; BiomeKind::COUNT] = [
    // TemperatePlains
    [
        ResourceDeposit::IronOre,
        ResourceDeposit::CopperOre,
        ResourceDeposit::Coal,
        ResourceDeposit::Stone,
    ],
    // BorealForest
    [
        ResourceDeposit::IronOre,
        ResourceDeposit::Coal,
        ResourceDeposit::Stone,
        ResourceDeposit::IronOre,
    ],
    // AridDesert
    [
        ResourceDeposit::CopperOre,
        ResourceDeposit::CrudeOil,
        ResourceDeposit::Stone,
        ResourceDeposit::CrudeOil,
    ],
    // FrozenTundra
    [
        ResourceDeposit::IronOre,
        ResourceDeposit::Uranium,
        ResourceDeposit::Stone,
        ResourceDeposit::Uranium,
    ],
    // VolcanicCrag
    [
        ResourceDeposit::Uranium,
        ResourceDeposit::Stone,
        ResourceDeposit::CopperOre,
        ResourceDeposit::Uranium,
    ],
    // SwampMarsh
    [
        ResourceDeposit::CrudeOil,
        ResourceDeposit::Coal,
        ResourceDeposit::CrudeOil,
        ResourceDeposit::Coal,
    ],
    // OceanDepths (never sampled: supports_deposits() == false)
    [
        ResourceDeposit::None,
        ResourceDeposit::None,
        ResourceDeposit::None,
        ResourceDeposit::None,
    ],
    // MushroomGlade
    [
        ResourceDeposit::Coal,
        ResourceDeposit::CopperOre,
        ResourceDeposit::IronOre,
        ResourceDeposit::Coal,
    ],
];

/// Memoized vein lattice cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(C)]
pub struct VeinCell {
    pub cx: i32,
    pub cy: i32,
    pub center_x: i32,
    pub center_y: i32,
    pub radius: i32,
    pub base_richness: u32,
    pub kind_roll: u8,
    pub has_vein: bool,
    pub valid: bool,
}

impl VeinCell {
    pub const EMPTY: Self = Self {
        cx: 0,
        cy: 0,
        center_x: 0,
        center_y: 0,
        radius: 0,
        base_richness: 0,
        kind_roll: 0,
        has_vein: false,
        valid: false,
    };

    #[inline(always)]
    fn refresh(&mut self, cx: i32, cy: i32, seed: u64) {
        if self.valid && self.cx == cx && self.cy == cy {
            return;
        }
        let h = hash2d(cx, cy, seed ^ VEIN_SALT);
        self.cx = cx;
        self.cy = cy;
        self.valid = true;
        self.has_vein = (h & 0xFF) < VEIN_DENSITY_PER_256;
        self.radius = 2 + ((h >> 8) & 3) as i32; // 2..=5
        // Clamp the centre so the vein stays inside this cell + its direct neighbours.
        let margin = self.radius;
        let span = (VEIN_CELL_SIZE - 2 * margin).max(1);
        let ox = margin + (((h >> 10) & 0xF) as i32 % span);
        let oy = margin + (((h >> 14) & 0xF) as i32 % span);
        self.center_x = cx.wrapping_mul(VEIN_CELL_SIZE).wrapping_add(ox);
        self.center_y = cy.wrapping_mul(VEIN_CELL_SIZE).wrapping_add(oy);
        self.kind_roll = ((h >> 18) & 3) as u8;
        self.base_richness = VEIN_RICHNESS_MIN + ((h >> 20) % VEIN_RICHNESS_SPAN);
    }
}

/// Per-generator scratch state that memoizes lattice corners between adjacent tiles.
///
/// Purely an accelerator: results are bit-identical with or without reuse.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BiomeNoiseContext {
    pub cells: [NoiseCell; NOISE_LAYER_COUNT],
    pub veins: [VeinCell; 4],
}

impl BiomeNoiseContext {
    #[inline(always)]
    pub const fn new() -> Self {
        Self {
            cells: [NoiseCell::EMPTY; NOISE_LAYER_COUNT],
            veins: [VeinCell::EMPTY; 4],
        }
    }

    #[inline(always)]
    fn layer(&mut self, idx: usize, wx: i32, wy: i32, seed: u64) -> u32 {
        sample_value_noise_cached(
            wx,
            wy,
            NOISE_LAYER_SHIFTS[idx],
            seed.wrapping_add(NOISE_LAYER_SALTS[idx]),
            &mut self.cells[idx],
        )
    }
}

/// Whittaker-style biome lookup from the three climate fields.
#[inline(always)]
pub const fn classify_biome(elevation: u32, moisture: u32, temperature: u32) -> BiomeKind {
    if elevation < OCEAN_LEVEL {
        return BiomeKind::OceanDepths;
    }
    if elevation > CRAG_LEVEL {
        return if temperature < COLD_MAX_TEMPERATURE {
            BiomeKind::FrozenTundra
        } else {
            BiomeKind::VolcanicCrag
        };
    }
    let cold = temperature < COLD_MAX_TEMPERATURE;
    let hot = temperature > HOT_MIN_TEMPERATURE;
    let dry = moisture < DRY_MAX_MOISTURE;
    let wet = moisture > WET_MIN_MOISTURE;

    if cold {
        if wet {
            BiomeKind::BorealForest
        } else {
            BiomeKind::FrozenTundra
        }
    } else if hot {
        if dry {
            BiomeKind::AridDesert
        } else if wet {
            BiomeKind::SwampMarsh
        } else {
            BiomeKind::TemperatePlains
        }
    } else if wet {
        BiomeKind::MushroomGlade
    } else if dry && elevation > (OCEAN_LEVEL + CRAG_LEVEL) / 2 {
        BiomeKind::BorealForest
    } else {
        BiomeKind::TemperatePlains
    }
}

/// Resolves the concrete terrain tile for a biome from local detail noise.
#[inline(always)]
pub const fn biome_tile(biome: BiomeKind, elevation: u32, moisture: u32, detail: u32) -> TileType {
    match biome {
        BiomeKind::OceanDepths => TileType::Water,
        BiomeKind::TemperatePlains => {
            if elevation < OCEAN_LEVEL + 1_500 {
                TileType::Water // shoreline shallows
            } else if moisture > 36_000 && detail > 50_000 {
                TileType::Water // pond
            } else if detail > 38_000 {
                TileType::Forest
            } else {
                TileType::Plains
            }
        }
        BiomeKind::BorealForest => {
            if detail > 52_000 {
                TileType::Rock
            } else if detail > 16_000 {
                TileType::Forest
            } else {
                TileType::Plains
            }
        }
        BiomeKind::AridDesert => {
            if detail > 56_000 {
                TileType::Rock // sandstone mesa
            } else if moisture > 24_000 && detail < 6_000 {
                TileType::Water // oasis
            } else {
                TileType::Plains // sand
            }
        }
        BiomeKind::FrozenTundra => {
            if detail > 52_000 {
                TileType::Rock // ice spire / permafrost outcrop
            } else {
                TileType::Plains // snowfield
            }
        }
        BiomeKind::VolcanicCrag => {
            if detail > 58_000 {
                TileType::Water // lava / sulphur pool (impassable)
            } else if detail < 22_000 {
                TileType::Plains // ash flats
            } else {
                TileType::Rock // basalt / obsidian
            }
        }
        BiomeKind::SwampMarsh => {
            if detail > 44_000 {
                TileType::Water // bog
            } else if detail < 20_000 {
                TileType::Forest // reed bed
            } else {
                TileType::Plains // mud flat
            }
        }
        BiomeKind::MushroomGlade => {
            if detail > 30_000 {
                TileType::Forest // giant fungi
            } else {
                TileType::Plains // fertile soil
            }
        }
    }
}

/// Seed used by noise layer `idx` for world `seed`.
#[inline(always)]
pub const fn layer_seed(seed: u64, idx: usize) -> u64 {
    seed.wrapping_add(NOISE_LAYER_SALTS[idx])
}

/// Samples the full biome / terrain / deposit profile of world tile `(wx, wy)`.
///
/// Integer-only, pure, and bit-identical across platforms for a given seed.
/// `ctx` is a reusable memo that accelerates row-major sweeps.
#[inline]
pub fn sample_world_biome_cached(
    wx: i32,
    wy: i32,
    seed: u64,
    ctx: &mut BiomeNoiseContext,
) -> BiomeSample {
    let mut fields = [0u32; NOISE_LAYER_COUNT];
    let mut i = 0;
    while i < NOISE_LAYER_COUNT {
        fields[i] = ctx.layer(i, wx, wy, seed);
        i += 1;
    }
    compose_biome_sample(wx, wy, seed, &fields, &mut ctx.veins)
}

/// Composes the biome sample of tile `(wx, wy)` from its already sampled noise
/// `fields` (one per layer, see [`NOISE_LAYER_SHIFTS`]). Shared by the single-tile
/// sampler and bulk chunk generation.
#[inline(always)]
pub fn compose_biome_sample(
    wx: i32,
    wy: i32,
    seed: u64,
    fields: &[u32; NOISE_LAYER_COUNT],
    veins: &mut [VeinCell; 4],
) -> BiomeSample {
    let elevation =
        (fields[LAYER_ELEV_LOW] * 4 + fields[LAYER_ELEV_MID] * 2 + fields[LAYER_ELEV_HIGH]) / 7;
    let moisture = (fields[LAYER_MOIST_LOW] * 3 + fields[LAYER_MOIST_HIGH]) / 4;
    let temperature = (fields[LAYER_TEMP_LOW] * 3 + fields[LAYER_TEMP_HIGH]) / 4;
    let detail = fields[LAYER_DETAIL];

    let biome = classify_biome(elevation, moisture, temperature);
    let tile = biome_tile(biome, elevation, moisture, detail);

    let mut sample = BiomeSample {
        elevation: elevation as u16,
        moisture: moisture as u16,
        temperature: temperature as u16,
        deposit_richness: 0,
        biome,
        tile,
        deposit_kind: ResourceDeposit::None,
        vein_distance: VEIN_NO_DISTANCE,
        _reserved: [0; 4],
    };

    if tile == TileType::Water || !biome.supports_deposits() {
        return sample;
    }

    // Resource veins: check this lattice cell plus the neighbours a vein could reach.
    let cx = wx >> VEIN_CELL_SHIFT;
    let cy = wy >> VEIN_CELL_SHIFT;
    let lx = wx & (VEIN_CELL_SIZE - 1);
    let ly = wy & (VEIN_CELL_SIZE - 1);
    let ndx = if lx < VEIN_MAX_RADIUS {
        -1
    } else if lx >= VEIN_CELL_SIZE - VEIN_MAX_RADIUS {
        1
    } else {
        0
    };
    let ndy = if ly < VEIN_MAX_RADIUS {
        -1
    } else if ly >= VEIN_CELL_SIZE - VEIN_MAX_RADIUS {
        1
    } else {
        0
    };

    let candidates: [(i32, i32); 4] = [(0, 0), (ndx, 0), (0, ndy), (ndx, ndy)];
    let mut best_dist = i32::MAX;
    let mut best_richness: u32 = 0;
    let mut best_roll: u8 = 0;

    let mut slot = 0usize;
    while slot < 4 {
        let (dx, dy) = candidates[slot];
        slot += 1;
        // Skip candidates that collapse onto an earlier one.
        if slot != 1 && dx == 0 && dy == 0 {
            continue;
        }
        if slot == 4 && (dx == 0 || dy == 0) {
            continue;
        }
        let cell = &mut veins[slot - 1];
        cell.refresh(cx.wrapping_add(dx), cy.wrapping_add(dy), seed);
        if !cell.has_vein {
            continue;
        }
        let ddx = wx.wrapping_sub(cell.center_x);
        let ddy = wy.wrapping_sub(cell.center_y);
        let dist = (if ddx < 0 { -ddx } else { ddx }) + (if ddy < 0 { -ddy } else { ddy });
        if dist > cell.radius {
            continue;
        }
        let richness =
            (cell.base_richness * (cell.radius + 1 - dist) as u32) / (cell.radius + 1) as u32;
        if dist < best_dist || (dist == best_dist && richness > best_richness) {
            best_dist = dist;
            best_richness = richness;
            best_roll = cell.kind_roll;
        }
    }

    if best_dist != i32::MAX {
        let kind = BIOME_DEPOSIT_TABLE[biome as usize][best_roll as usize];
        if !kind.is_none() {
            sample.deposit_kind = kind;
            sample.deposit_richness = best_richness.min(u16::MAX as u32) as u16;
            sample.vein_distance = best_dist as u8;
        }
    }

    sample
}

/// Samples the biome profile of a single world tile (uncached convenience form).
#[inline]
pub fn sample_world_biome(wx: i32, wy: i32, seed: u64) -> BiomeSample {
    let mut ctx = BiomeNoiseContext::new();
    sample_world_biome_cached(wx, wy, seed, &mut ctx)
}

/// Determines the terrain tile at world coordinate `(wx, wy)` with zero floating point.
#[inline]
pub fn sample_world_tile(wx: i32, wy: i32, seed: u64) -> TileType {
    sample_world_biome(wx, wy, seed).tile
}

// ---------------------------------------------------------------------------
// Fixed-size legacy tilemap (cellular automata)
// ---------------------------------------------------------------------------

pub const DEFAULT_MAP_W: usize = 64;
pub const DEFAULT_MAP_H: usize = 64;

/// Fixed-size deterministic tilemap grid generated from a seed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TileMap<const W: usize = DEFAULT_MAP_W, const H: usize = DEFAULT_MAP_H> {
    pub tiles: [[TileType; W]; H],
    pub seed: u64,
}

impl<const W: usize, const H: usize> TileMap<W, H> {
    /// Creates an empty tilemap filled with plains.
    pub const fn empty(seed: u64) -> Self {
        Self {
            tiles: [[TileType::Plains; W]; H],
            seed,
        }
    }

    /// Generates a coherent world terrain using deterministic cellular automata.
    ///
    /// Forms natural lakes, rock formations, and forest patches.
    pub fn generate(seed: u64) -> Self {
        let mut map = Self::empty(seed);
        let mut rng = Rng::with_seed(seed);

        // 1. Initial probabilistic noise assignment
        for y in 0..H {
            for x in 0..W {
                // Border tiles are boundary rocks
                if x == 0 || y == 0 || x == W - 1 || y == H - 1 {
                    map.tiles[y][x] = TileType::Rock;
                    continue;
                }

                let roll = rng.range_u32(0, 99);
                map.tiles[y][x] = if roll < 15 {
                    TileType::Water
                } else if roll < 30 {
                    TileType::Rock
                } else if roll < 55 {
                    TileType::Forest
                } else {
                    TileType::Plains
                };
            }
        }

        // 2. Cellular automata smoothing pass (2 iterations)
        for _ in 0..2 {
            let prev = map.tiles;
            for y in 1..(H - 1) {
                for x in 1..(W - 1) {
                    let mut water_neighbors = 0;
                    let mut rock_neighbors = 0;
                    let mut forest_neighbors = 0;

                    for dy in [-1isize, 0, 1] {
                        for dx in [-1isize, 0, 1] {
                            if dx == 0 && dy == 0 {
                                continue;
                            }
                            let nx = (x as isize + dx) as usize;
                            let ny = (y as isize + dy) as usize;
                            match prev[ny][nx] {
                                TileType::Water => water_neighbors += 1,
                                TileType::Rock => rock_neighbors += 1,
                                TileType::Forest => forest_neighbors += 1,
                                TileType::Plains => {}
                            }
                        }
                    }

                    // Clustering rules
                    if water_neighbors >= 4 {
                        map.tiles[y][x] = TileType::Water;
                    } else if rock_neighbors >= 4 {
                        map.tiles[y][x] = TileType::Rock;
                    } else if forest_neighbors >= 4 {
                        map.tiles[y][x] = TileType::Forest;
                    }
                }
            }
        }

        map
    }

    /// Retrieves tile at (x, y) coordinates with boundary check.
    #[inline(always)]
    pub fn tile_at(&self, x: usize, y: usize) -> Option<TileType> {
        if x < W && y < H {
            Some(self.tiles[y][x])
        } else {
            None
        }
    }

    /// Returns true if the tile at (x, y) is walkable.
    #[inline(always)]
    pub fn is_walkable(&self, x: usize, y: usize) -> bool {
        self.tile_at(x, y).is_some_and(|t| t.is_walkable())
    }

    /// Computes deterministic hash across the full generated tilemap.
    #[inline]
    pub fn compute_hash(&self) -> u64 {
        deterministic_hash(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rng_determinism() {
        let mut rng1 = Rng::with_seed(123456789);
        let mut rng2 = Rng::with_seed(123456789);

        for _ in 0..100 {
            assert_eq!(rng1.next_u64(), rng2.next_u64());
        }
    }

    #[test]
    fn test_tilemap_procedural_generation_determinism() {
        let seed = 987654321;
        let map_a = TileMap::<32, 32>::generate(seed);
        let map_b = TileMap::<32, 32>::generate(seed);

        // Bit-for-bit identical assertion
        assert_eq!(map_a, map_b, "Same seed must generate identical tilemap");
        assert_eq!(map_a.compute_hash(), map_b.compute_hash());

        // Boundary rocks verification
        for x in 0..32 {
            assert_eq!(map_a.tile_at(x, 0), Some(TileType::Rock));
            assert_eq!(map_a.tile_at(x, 31), Some(TileType::Rock));
        }

        // Different seed generates different map
        let map_diff = TileMap::<32, 32>::generate(11111);
        assert_ne!(map_a.compute_hash(), map_diff.compute_hash());
    }

    #[test]
    fn test_biome_sample_layout_and_cached_equivalence() {
        assert_eq!(core::mem::size_of::<BiomeSample>(), 16);
        assert_eq!(core::mem::size_of::<BiomeKind>(), 1);
        assert_eq!(core::mem::size_of::<ResourceDeposit>(), 1);

        // Row-major sweep with a shared memo must equal fresh per-tile sampling.
        let seed = 0xA11CE;
        let mut ctx = BiomeNoiseContext::new();
        for wy in -40..40 {
            for wx in -40..40 {
                let cached = sample_world_biome_cached(wx, wy, seed, &mut ctx);
                let fresh = sample_world_biome(wx, wy, seed);
                assert_eq!(cached, fresh, "memo must never change the result at ({wx},{wy})");
                assert_eq!(fresh.tile, sample_world_tile(wx, wy, seed));
                assert_eq!(fresh.deposit_kind.is_none(), fresh.deposit_richness == 0);
                assert_eq!(fresh.deposit_kind.is_none(), fresh.vein_distance == VEIN_NO_DISTANCE);
            }
        }
    }

    #[test]
    fn test_value_noise_range_and_lattice_continuity() {
        let seed = 77;
        for shift in [0u32, 2, 4, 6] {
            let scale = 1i32 << shift;
            for y in -70..70 {
                for x in -70..70 {
                    let v = sample_value_noise(x, y, shift, seed);
                    assert!(v <= NOISE_MAX);
                    // At lattice points the sampler must return the raw corner hash.
                    if x.rem_euclid(scale) == 0 && y.rem_euclid(scale) == 0 {
                        let expected =
                            hash2d(x.div_euclid(scale), y.div_euclid(scale), seed) & NOISE_MAX;
                        assert_eq!(v, expected);
                    }
                }
            }
        }
        for (i, &shift) in NOISE_LAYER_SHIFTS.iter().enumerate() {
            assert_eq!(1i32 << shift, NOISE_LAYER_SCALES[i]);
        }
    }
}
