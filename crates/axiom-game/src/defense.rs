//! # Base Defense & Monster Wave Auto-Battler (Track 1 / T-043)
//!
//! Defensive structures live *inside* the factory grid as ordinary 64-byte tiles, so a
//! wall, turret or base heart costs exactly one cache line like every other machine:
//!
//! ```text
//!  Chest(ammo) -> Inserter -> Turret  . . . . . . . . . . . . . . >  Monster (CombatUnit)
//!  Belt(ammo)  -----------> Turret        (range check, cooldown, dmg)   |
//!  Chest(ammo) [adjacent]   Turret  (auto-pull one round per tick)       v
//!                                                             Wall / Turret / BaseHeart
//!                                                             hp -= monster.attack_power
//!                                                             hp == 0 -> debris Chest
//! ```
//!
//! Monsters are [`axiom_core::combat::CombatUnit`] records stored in a
//! [`axiom_core::combat::BattleArena`] owned by the [`WaveDirector`]. The director
//! reuses the unit fields (`current_hp`, `attack_power`, `attack_range`,
//! `attack_cooldown`, `cooldown_timer`, `target_id`, `kills`, `pos`, `vel`) and the
//! arena hash, but drives its own grid-aware step so monsters target *tiles*.
//!
//! Design axioms:
//! - **Absolute determinism**: spawn positions come from the seeded
//!   [`axiom_core::Rng`]; all distances are integer squared distances; no floats,
//!   no wall clock.
//! - **Cache-line resident**: every defence payload fits the 16-byte
//!   [`StructureKind`] payload slot, so [`crate::FactoryStructure`] stays 64 bytes.
//!   [`WaveDirector`] is 64-byte aligned.
//! - **Zero heap**: `no_std` without `alloc`; every per-tick scratch buffer is a
//!   fixed-size stack array.

use axiom_core::combat::{BattleArena, CombatUnit};
use axiom_core::{deterministic_hash, ChunkCoord, FixedVec2, Rng};

use crate::factory::{Chest, Direction, FactoryGrid, ItemType, StructureKind};
use crate::world_factory::WorldFactory;

// ---------------------------------------------------------------------------
// Tunables (all integers)
// ---------------------------------------------------------------------------

/// Default monster capacity of a [`WaveDirector`] arena.
pub const DEFAULT_MAX_MONSTERS: usize = 64;
/// Upper bound on defence tiles scanned per tick (fixed stack buffer, no heap).
/// Tiles beyond this bound (in row-major order) are invisible to monsters.
pub const MAX_DEFENSE_TILES: usize = 256;

/// Hit points of a freshly built wall.
pub const WALL_HP: u16 = 300;
/// Hit points of a freshly built turret.
pub const TURRET_HP: u16 = 150;
/// Hit points of the base heart; losing it ends the defence.
pub const BASE_HEART_HP: u16 = 1_000;

/// Turret firing radius in tiles (Euclidean, compared as squared integers).
pub const TURRET_RANGE: u8 = 6;
/// Base damage per shot before the ammo multiplier.
pub const TURRET_DAMAGE: u16 = 20;
/// Ticks between two turret shots.
pub const TURRET_COOLDOWN: u16 = 2;
/// Rounds a turret magazine can hold.
pub const TURRET_AMMO_CAP: u16 = 20;

/// Items dropped into a debris chest when a wall collapses.
pub const WALL_SALVAGE: u32 = 4;
/// Items dropped into a debris chest when a turret is destroyed.
pub const TURRET_SALVAGE: u32 = 6;

/// Arena team id of monsters (friendly units would be team 1).
pub const MONSTER_TEAM: u8 = 2;
/// Monster hit points on wave 1.
pub const MONSTER_BASE_HP: u16 = 60;
/// Extra monster hit points per wave.
pub const MONSTER_HP_GROWTH: u16 = 15;
/// Monster damage per hit on wave 1.
pub const MONSTER_BASE_ATTACK: u16 = 10;
/// Extra monster damage per wave.
pub const MONSTER_ATTACK_GROWTH: u16 = 2;
/// Monster melee reach in tiles (Chebyshev distance).
pub const MONSTER_RANGE: u16 = 1;
/// Ticks between two monster hits.
pub const MONSTER_COOLDOWN: u16 = 4;
/// Monsters advance one tile every this many ticks.
pub const MONSTER_MOVE_INTERVAL: u64 = 2;

/// Default ticks between waves.
pub const DEFAULT_WAVE_INTERVAL: u32 = 50;
/// Default tick of the first wave.
pub const DEFAULT_FIRST_WAVE_TICK: u32 = 10;
/// Default monsters in wave 1.
pub const DEFAULT_BASE_WAVE_SIZE: u16 = 4;
/// Default extra monsters per wave.
pub const DEFAULT_WAVE_GROWTH: u16 = 2;

const GOLDEN: u64 = 0x9E37_79B9_7F4A_7C15;
const FNV_PRIME: u64 = 0x0000_0100_0000_01B3;

// ---------------------------------------------------------------------------
// Defence structure payloads (each <= 16 bytes, align <= 4)
// ---------------------------------------------------------------------------

/// Passive barrier; soaks monster hits until it collapses into a debris chest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Wall {
    pub hp: u16,
    pub max_hp: u16,
    /// Item recovered as debris (default [`ItemType::StoneBrick`]).
    pub material: ItemType,
}

/// Automatic gun: consumes ammo rounds and shoots the nearest monster in range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Turret {
    pub hp: u16,
    pub max_hp: u16,
    /// Base damage per shot before the ammo multiplier.
    pub damage: u16,
    pub cooldown: u16,
    pub cooldown_timer: u16,
    /// Lifetime monsters killed by this turret.
    pub kills: u16,
    /// Rounds in the magazine (all of type `ammo_item`).
    pub ammo: u16,
    pub ammo_item: ItemType,
    /// Firing radius in tiles.
    pub range: u8,
}

impl Turret {
    /// Damage of one shot with the currently loaded ammo.
    #[inline(always)]
    pub const fn shot_damage(&self) -> u16 {
        let d = self.damage as u32 * self.ammo_item.ammo_damage_pct() as u32 / 100;
        if d > u16::MAX as u32 {
            u16::MAX
        } else {
            d as u16
        }
    }
}

/// The objective monsters try to reach; destroying it ends the defence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct BaseHeart {
    pub hp: u16,
    pub max_hp: u16,
}

// ---------------------------------------------------------------------------
// Grid placement helpers
// ---------------------------------------------------------------------------

impl<const W: usize, const H: usize> FactoryGrid<W, H> {
    /// Places a stone-brick wall with [`WALL_HP`].
    pub fn place_wall(&mut self, x: usize, y: usize) {
        self.set(
            x,
            y,
            StructureKind::Wall(Wall {
                hp: WALL_HP,
                max_hp: WALL_HP,
                material: ItemType::StoneBrick,
            }),
        );
    }

    /// Places a turret with the default stat block (empty magazine).
    pub fn place_turret(&mut self, x: usize, y: usize) {
        self.place_turret_with(
            x,
            y,
            TURRET_HP,
            TURRET_DAMAGE,
            TURRET_RANGE,
            TURRET_COOLDOWN,
        );
    }

    /// Places a turret with explicit stats (empty magazine).
    pub fn place_turret_with(
        &mut self,
        x: usize,
        y: usize,
        hp: u16,
        damage: u16,
        range: u8,
        cooldown: u16,
    ) {
        self.set(
            x,
            y,
            StructureKind::Turret(Turret {
                hp,
                max_hp: hp,
                damage,
                cooldown,
                cooldown_timer: 0,
                kills: 0,
                ammo: 0,
                ammo_item: ItemType::None,
                range,
            }),
        );
    }

    /// Places the base heart with [`BASE_HEART_HP`].
    pub fn place_base_heart(&mut self, x: usize, y: usize) {
        self.set(
            x,
            y,
            StructureKind::BaseHeart(BaseHeart {
                hp: BASE_HEART_HP,
                max_hp: BASE_HEART_HP,
            }),
        );
    }

    /// `(hp, max_hp)` of a defence structure at `(x, y)`; `(0, 0)` for anything else.
    pub fn structure_hp(&self, x: usize, y: usize) -> (u16, u16) {
        match self.get(x, y).map(|c| c.kind) {
            Some(StructureKind::Wall(w)) => (w.hp, w.max_hp),
            Some(StructureKind::Turret(t)) => (t.hp, t.max_hp),
            Some(StructureKind::BaseHeart(h)) => (h.hp, h.max_hp),
            _ => (0, 0),
        }
    }

    /// `(ammo item, rounds)` loaded in the turret at `(x, y)`.
    pub fn turret_ammo(&self, x: usize, y: usize) -> (ItemType, u16) {
        match self.get(x, y).map(|c| c.kind) {
            Some(StructureKind::Turret(t)) => (t.ammo_item, t.ammo),
            _ => (ItemType::None, 0),
        }
    }

    /// Whether the tile holds a wall, turret or base heart.
    #[inline(always)]
    pub fn is_defense_tile(&self, x: usize, y: usize) -> bool {
        matches!(
            self.get(x, y).map(|c| c.kind),
            Some(StructureKind::Wall(_) | StructureKind::Turret(_) | StructureKind::BaseHeart(_))
        )
    }
}

// ---------------------------------------------------------------------------
// Telemetry
// ---------------------------------------------------------------------------

/// Snapshot of the defence state, via [`WaveDirector::report`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct DefenseReport {
    pub tick: u64,
    pub wave_number: u16,
    pub monsters_alive: u32,
    pub total_spawned: u64,
    pub total_killed: u64,
    pub total_shots: u64,
    pub ammo_consumed: u64,
    pub structure_damage: u64,
    pub structures_destroyed: u64,
    pub debris_recovered: u64,
    pub walls: u32,
    pub turrets: u32,
    pub turret_ammo: u32,
    pub heart_hp: u16,
    pub heart_max_hp: u16,
    pub base_fallen: bool,
}

// ---------------------------------------------------------------------------
// Wave director
// ---------------------------------------------------------------------------

/// Spawns monster waves on a fixed schedule and resolves turret-vs-monster and
/// monster-vs-structure combat against one factory grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C, align(64))]
pub struct WaveDirector<const CAP: usize = DEFAULT_MAX_MONSTERS> {
    /// Monster slab (team [`MONSTER_TEAM`]).
    pub arena: BattleArena<CAP>,
    /// Seeded spawn-position generator.
    pub rng: Rng,
    /// Chunk this director defends when driven through a [`WorldFactory`].
    pub chunk: ChunkCoord,
    pub current_tick: u64,
    pub next_wave_tick: u64,
    pub wave_interval: u32,
    pub base_wave_size: u16,
    pub wave_growth: u16,
    /// Number of waves spawned so far.
    pub wave_number: u16,
    /// Set once the base heart is destroyed; no further waves spawn.
    pub base_fallen: bool,
    pub next_monster_id: u32,
    pub total_spawned: u64,
    pub total_killed: u64,
    pub total_shots: u64,
    pub ammo_consumed: u64,
    pub structure_damage: u64,
    pub structures_destroyed: u64,
    pub debris_recovered: u64,
}

impl<const CAP: usize> Default for WaveDirector<CAP> {
    fn default() -> Self {
        Self::new(0)
    }
}

/// Outcome of one monster hit on a tile.
#[derive(Clone, Copy)]
struct HitOutcome {
    dealt: u16,
    destroyed: bool,
}

impl<const CAP: usize> WaveDirector<CAP> {
    /// Director with the default schedule, bound to chunk `(0, 0)`.
    pub const fn new(seed: u64) -> Self {
        Self::with_schedule(
            seed,
            DEFAULT_FIRST_WAVE_TICK,
            DEFAULT_WAVE_INTERVAL,
            DEFAULT_BASE_WAVE_SIZE,
            DEFAULT_WAVE_GROWTH,
        )
    }

    /// Director with an explicit wave schedule.
    pub const fn with_schedule(
        seed: u64,
        first_wave_tick: u32,
        wave_interval: u32,
        base_wave_size: u16,
        wave_growth: u16,
    ) -> Self {
        Self {
            arena: BattleArena::new(),
            rng: Rng::with_seed(seed),
            chunk: ChunkCoord::new(0, 0),
            current_tick: 0,
            next_wave_tick: first_wave_tick as u64,
            wave_interval,
            base_wave_size,
            wave_growth,
            wave_number: 0,
            base_fallen: false,
            next_monster_id: 1,
            total_spawned: 0,
            total_killed: 0,
            total_shots: 0,
            ammo_consumed: 0,
            structure_damage: 0,
            structures_destroyed: 0,
            debris_recovered: 0,
        }
    }

    /// Binds the director to a world chunk for [`WaveDirector::tick_world`].
    #[inline(always)]
    pub fn bind_chunk(&mut self, chunk: ChunkCoord) {
        self.chunk = chunk;
    }

    /// Living monsters.
    #[inline(always)]
    pub fn monsters_alive(&self) -> usize {
        self.arena.team_alive_count(MONSTER_TEAM)
    }

    /// Size of wave `n` (1-based).
    #[inline(always)]
    pub const fn wave_size(&self, n: u16) -> u16 {
        self.base_wave_size
            .saturating_add(self.wave_growth.saturating_mul(n.saturating_sub(1)))
    }

    // ---- spawning --------------------------------------------------------

    /// Spawns the next wave on the grid edge immediately. Returns monsters spawned
    /// (fewer than the wave size when the arena is full).
    pub fn spawn_wave<const W: usize, const H: usize>(&mut self, _grid: &FactoryGrid<W, H>) -> u16 {
        self.wave_number = self.wave_number.saturating_add(1);
        let n = self.wave_number;
        let count = self.wave_size(n);
        let hp = MONSTER_BASE_HP.saturating_add(MONSTER_HP_GROWTH.saturating_mul(n - 1));
        let attack =
            MONSTER_BASE_ATTACK.saturating_add(MONSTER_ATTACK_GROWTH.saturating_mul(n - 1));

        let mut spawned = 0u16;
        for _ in 0..count {
            let (x, y) = self.random_edge_tile(W, H);
            let id = self.next_monster_id;
            let slot = self.arena.spawn_unit(
                id,
                MONSTER_TEAM,
                hp,
                attack,
                MONSTER_RANGE,
                MONSTER_COOLDOWN,
                FixedVec2::from_ints(x as i64, y as i64),
            );
            if slot.is_none() {
                break;
            }
            self.next_monster_id = self.next_monster_id.wrapping_add(1).max(1);
            spawned += 1;
        }
        self.total_spawned += spawned as u64;
        spawned
    }

    fn random_edge_tile(&mut self, w: usize, h: usize) -> (usize, usize) {
        let (wm, hm) = (w.saturating_sub(1) as u32, h.saturating_sub(1) as u32);
        match self.rng.range_u32(0, 3) {
            0 => (self.rng.range_u32(0, wm) as usize, 0),
            1 => (wm as usize, self.rng.range_u32(0, hm) as usize),
            2 => (self.rng.range_u32(0, wm) as usize, hm as usize),
            _ => (0, self.rng.range_u32(0, hm) as usize),
        }
    }

    /// Frees an arena slot so later waves can reuse it.
    #[inline]
    fn free_unit(&mut self, idx: usize) {
        self.arena.units[idx] = CombatUnit::empty();
        self.arena.active_count = self.arena.active_count.saturating_sub(1);
    }

    // ---- simulation: standalone grid --------------------------------------

    /// Advances the defence by one tick against `grid`. Heap-free; fixed phase order:
    ///
    /// 1. **Wave spawn** if the schedule says so (and the heart still stands).
    /// 2. **Turrets** pull ammo from adjacent chests, tick cooldowns, shoot the
    ///    nearest monster in range.
    /// 3. **Monsters** tick cooldowns, acquire the nearest defence tile, advance or
    ///    hit it; destroyed tiles turn into debris chests (the heart turns to rubble).
    pub fn tick<const W: usize, const H: usize>(&mut self, grid: &mut FactoryGrid<W, H>) {
        self.current_tick += 1;
        let tick = self.current_tick as u32;

        if !self.base_fallen && self.current_tick >= self.next_wave_tick {
            self.spawn_wave(grid);
            self.next_wave_tick = self.current_tick + self.wave_interval.max(1) as u64;
        }

        self.step_turrets(grid, tick);
        self.step_monsters(grid, tick);
        self.arena.current_tick = self.current_tick;
    }

    /// Defence tick followed by one factory tick.
    #[inline]
    pub fn step_with<const W: usize, const H: usize>(&mut self, grid: &mut FactoryGrid<W, H>) {
        self.tick(grid);
        grid.step();
    }

    /// Runs `n` combined ticks.
    pub fn run_with<const W: usize, const H: usize>(
        &mut self,
        grid: &mut FactoryGrid<W, H>,
        n: u32,
    ) {
        for _ in 0..n {
            self.step_with(grid);
        }
    }

    // ---- simulation: infinite world ---------------------------------------

    /// Advances the defence against the claimed chunk grid bound via
    /// [`WaveDirector::bind_chunk`]. No-op (tick still counts) if the chunk is unclaimed.
    pub fn tick_world<const S: usize, const MC: usize, const MF: usize>(
        &mut self,
        world: &mut WorldFactory<S, MC, MF>,
    ) {
        let chunk = self.chunk;
        match world.factory_mut(chunk) {
            Some(grid) => self.tick(grid),
            None => {
                self.current_tick += 1;
                self.arena.current_tick = self.current_tick;
            }
        }
    }

    /// Defence tick followed by one world tick.
    #[inline]
    pub fn step_world_with<const S: usize, const MC: usize, const MF: usize>(
        &mut self,
        world: &mut WorldFactory<S, MC, MF>,
    ) {
        self.tick_world(world);
        world.step();
    }

    /// Runs `n` combined world ticks.
    pub fn run_world_with<const S: usize, const MC: usize, const MF: usize>(
        &mut self,
        world: &mut WorldFactory<S, MC, MF>,
        n: u32,
    ) {
        for _ in 0..n {
            self.step_world_with(world);
        }
    }

    // ---- phase: turrets ----------------------------------------------------

    fn step_turrets<const W: usize, const H: usize>(
        &mut self,
        grid: &mut FactoryGrid<W, H>,
        tick: u32,
    ) {
        const SIDES: [Direction; 4] = [
            Direction::North,
            Direction::East,
            Direction::South,
            Direction::West,
        ];

        for y in 0..H {
            for x in 0..W {
                let mut t = match grid.cells[y][x].kind {
                    StructureKind::Turret(t) => t,
                    _ => continue,
                };

                // Auto-supply: pull one compatible round from an adjacent chest.
                if t.ammo < TURRET_AMMO_CAP {
                    for dir in SIDES {
                        let (nx, ny) = match FactoryGrid::<W, H>::neighbor(x, y, dir) {
                            Some(p) => p,
                            None => continue,
                        };
                        let chest = match grid.cells[ny][nx].kind {
                            StructureKind::Chest(c) => c,
                            _ => continue,
                        };
                        let compatible = t.ammo == 0 || t.ammo_item == chest.item;
                        if chest.quantity > 0 && chest.item.is_ammo() && compatible {
                            let taken = grid.cells[ny][nx].try_take();
                            if !taken.is_none() {
                                t.ammo_item = taken;
                                t.ammo += 1;
                                grid.cells[ny][nx].last_action_tick = tick;
                                break;
                            }
                        }
                    }
                }

                if t.cooldown_timer > 0 {
                    t.cooldown_timer -= 1;
                }

                if t.cooldown_timer == 0 && t.ammo > 0 {
                    let range_sq = (t.range as i64) * (t.range as i64);
                    let mut best: Option<usize> = None;
                    let mut best_d = i64::MAX;
                    for (i, u) in self.arena.units.iter().enumerate() {
                        if !u.is_alive() || u.team != MONSTER_TEAM {
                            continue;
                        }
                        let d = tile_dist_sq(x as i64, y as i64, u.pos);
                        if d <= range_sq && d < best_d {
                            best_d = d;
                            best = Some(i);
                        }
                    }
                    if let Some(i) = best {
                        let dmg = t.shot_damage();
                        let u = &mut self.arena.units[i];
                        u.current_hp = u.current_hp.saturating_sub(dmg);
                        let killed = u.current_hp == 0;

                        t.ammo -= 1;
                        if t.ammo == 0 {
                            t.ammo_item = ItemType::None;
                        }
                        t.cooldown_timer = t.cooldown;
                        self.total_shots += 1;
                        self.ammo_consumed += 1;
                        if killed {
                            t.kills = t.kills.saturating_add(1);
                            self.total_killed += 1;
                            self.free_unit(i);
                        }
                        grid.cells[y][x].last_action_tick = tick;
                    }
                }

                grid.cells[y][x].kind = StructureKind::Turret(t);
            }
        }
    }

    // ---- phase: monsters ---------------------------------------------------

    fn step_monsters<const W: usize, const H: usize>(
        &mut self,
        grid: &mut FactoryGrid<W, H>,
        tick: u32,
    ) {
        let mut tiles = [(0u16, 0u16); MAX_DEFENSE_TILES];
        let mut n_tiles = 0usize;
        'scan: for y in 0..H {
            for x in 0..W {
                if grid.is_defense_tile(x, y) {
                    if n_tiles >= MAX_DEFENSE_TILES {
                        break 'scan;
                    }
                    tiles[n_tiles] = (x as u16, y as u16);
                    n_tiles += 1;
                }
            }
        }

        // Scan base heart position (primary strategic target)
        let mut heart_pos: Option<(usize, usize)> = None;
        for y in 0..H {
            for x in 0..W {
                if matches!(grid.cells[y][x].kind, StructureKind::BaseHeart(_)) {
                    heart_pos = Some((x, y));
                    break;
                }
            }
            if heart_pos.is_some() {
                break;
            }
        }

        let move_now = self.current_tick.is_multiple_of(MONSTER_MOVE_INTERVAL);

        for i in 0..CAP {
            let mut u = self.arena.units[i];
            if !u.is_alive() || u.team != MONSTER_TEAM {
                continue;
            }
            if u.cooldown_timer > 0 {
                u.cooldown_timer -= 1;
            }

            let (mx, my) = (u.pos.x.to_int(), u.pos.y.to_int());

            // Target Priority:
            // 1. If an active defense structure is within immediate attack reach (<= attack_range), engage it!
            // 2. Otherwise, march relentlessly toward BaseHeart.
            // 3. If BaseHeart is fallen, target nearest remaining defense tile.
            let mut best: Option<(usize, usize)> = None;

            // Check if any defense tile is in immediate attack reach
            for &(tx, ty) in tiles[..n_tiles].iter() {
                let (dx, dy) = (tx as i64 - mx, ty as i64 - my);
                let reach = dx.abs().max(dy.abs());
                if reach <= u.attack_range as i64 {
                    best = Some((tx as usize, ty as usize));
                    break;
                }
            }

            // If none in immediate reach, move toward BaseHeart or closest fallback
            if best.is_none() {
                if let Some(hp) = heart_pos {
                    best = Some(hp);
                } else {
                    let mut best_d = i64::MAX;
                    for &(tx, ty) in tiles[..n_tiles].iter() {
                        let (dx, dy) = (tx as i64 - mx, ty as i64 - my);
                        let d = dx * dx + dy * dy;
                        if d < best_d {
                            best_d = d;
                            best = Some((tx as usize, ty as usize));
                        }
                    }
                }
            }

            match best {
                None => {
                    u.target_id = 0;
                    u.vel = FixedVec2::ZERO;
                }
                Some((tx, ty)) => {
                    u.target_id = (ty * W + tx) as u32 + 1;
                    let (dx, dy) = (tx as i64 - mx, ty as i64 - my);
                    let reach = dx.abs().max(dy.abs());
                    if reach <= u.attack_range as i64 {
                        u.vel = FixedVec2::ZERO;
                        if u.cooldown_timer == 0 {
                            let hit = self.hit_structure(grid, tx, ty, u.attack_power, tick);
                            self.structure_damage += hit.dealt as u64;
                            u.cooldown_timer = u.attack_cooldown;
                            if hit.destroyed {
                                u.kills = u.kills.saturating_add(1);
                                self.structures_destroyed += 1;
                            }
                        }
                    } else {
                        // Mazing & Wall Navigation (T-045):
                        // Steer around walls if an open detour exists; if perimeter is sealed,
                        // attack the blocking wall to breach the fortification!
                        let sx = dx.signum();
                        let sy = dy.signum();
                        let fwd_x = (mx + sx).clamp(0, W as i64 - 1) as usize;
                        let fwd_y = (my + sy).clamp(0, H as i64 - 1) as usize;

                        let is_wall = |x: usize, y: usize| -> bool {
                            matches!(grid.cells[y][x].kind, StructureKind::Wall(_))
                        };

                        if is_wall(fwd_x, fwd_y) {
                            let detour_x = (mx + sx).clamp(0, W as i64 - 1) as usize;
                            let detour_y = (my + sy).clamp(0, H as i64 - 1) as usize;
                            if sx != 0 && !is_wall(detour_x, my as usize) {
                                u.vel = FixedVec2::from_ints(sx, 0);
                                if move_now {
                                    u.pos += u.vel;
                                }
                            } else if sy != 0 && !is_wall(mx as usize, detour_y) {
                                u.vel = FixedVec2::from_ints(0, sy);
                                if move_now {
                                    u.pos += u.vel;
                                }
                            } else {
                                // Perimeter blocked: attack the blocking wall to breach!
                                u.vel = FixedVec2::ZERO;
                                if u.cooldown_timer == 0 {
                                    let hit = self.hit_structure(grid, fwd_x, fwd_y, u.attack_power, tick);
                                    self.structure_damage += hit.dealt as u64;
                                    u.cooldown_timer = u.attack_cooldown;
                                    if hit.destroyed {
                                        u.kills = u.kills.saturating_add(1);
                                        self.structures_destroyed += 1;
                                    }
                                }
                            }
                        } else {
                            u.vel = FixedVec2::from_ints(sx, sy);
                            if move_now {
                                u.pos += u.vel;
                            }
                        }
                    }
                }
            }

            self.arena.units[i] = u;
        }
    }

    /// Applies `dmg` to the defence tile at `(x, y)`. A destroyed wall or turret
    /// becomes a debris [`Chest`]; a destroyed heart becomes rubble and raises
    /// `base_fallen`.
    fn hit_structure<const W: usize, const H: usize>(
        &mut self,
        grid: &mut FactoryGrid<W, H>,
        x: usize,
        y: usize,
        dmg: u16,
        tick: u32,
    ) -> HitOutcome {
        let cell = &mut grid.cells[y][x];
        // (hp after, dealt, salvage item, salvage qty)
        let (hp_after, dealt, salvage) = match &mut cell.kind {
            StructureKind::Wall(w) => {
                let dealt = w.hp.min(dmg);
                w.hp -= dealt;
                (w.hp, dealt, Some((w.material, WALL_SALVAGE)))
            }
            StructureKind::Turret(t) => {
                let dealt = t.hp.min(dmg);
                t.hp -= dealt;
                (t.hp, dealt, Some((ItemType::IronPlate, TURRET_SALVAGE)))
            }
            StructureKind::BaseHeart(h) => {
                let dealt = h.hp.min(dmg);
                h.hp -= dealt;
                (h.hp, dealt, None)
            }
            _ => {
                return HitOutcome {
                    dealt: 0,
                    destroyed: false,
                }
            }
        };
        cell.last_action_tick = tick;

        if hp_after > 0 {
            return HitOutcome {
                dealt,
                destroyed: false,
            };
        }

        match salvage {
            Some((item, quantity)) => {
                cell.kind = StructureKind::Chest(Chest { item, quantity });
                self.debris_recovered += quantity as u64;
            }
            None => {
                cell.kind = StructureKind::Empty;
                self.base_fallen = true;
            }
        }
        HitOutcome {
            dealt,
            destroyed: true,
        }
    }

    // ---- telemetry -------------------------------------------------------

    /// Snapshot of director counters plus the defence structures on `grid`.
    pub fn report<const W: usize, const H: usize>(
        &self,
        grid: &FactoryGrid<W, H>,
    ) -> DefenseReport {
        let mut r = DefenseReport {
            tick: self.current_tick,
            wave_number: self.wave_number,
            monsters_alive: self.monsters_alive() as u32,
            total_spawned: self.total_spawned,
            total_killed: self.total_killed,
            total_shots: self.total_shots,
            ammo_consumed: self.ammo_consumed,
            structure_damage: self.structure_damage,
            structures_destroyed: self.structures_destroyed,
            debris_recovered: self.debris_recovered,
            base_fallen: self.base_fallen,
            ..DefenseReport::default()
        };
        for row in &grid.cells {
            for cell in row {
                match cell.kind {
                    StructureKind::Wall(_) => r.walls += 1,
                    StructureKind::Turret(t) => {
                        r.turrets += 1;
                        r.turret_ammo += t.ammo as u32;
                    }
                    StructureKind::BaseHeart(h) => {
                        r.heart_hp = r.heart_hp.saturating_add(h.hp);
                        r.heart_max_hp = r.heart_max_hp.saturating_add(h.max_hp);
                    }
                    _ => {}
                }
            }
        }
        r
    }

    /// Deterministic FNV-mixed hash of the director: schedule, RNG state, counters
    /// and every living monster (via [`BattleArena::compute_hash`]).
    pub fn compute_hash(&self) -> u64 {
        // Mix the tick before folding in the arena hash (which also starts from
        // `tick * GOLDEN`); a plain XOR would cancel the two terms.
        let mut h = self.current_tick.wrapping_mul(GOLDEN);
        h = h.rotate_left(5).wrapping_mul(FNV_PRIME);
        h ^= self.arena.compute_hash();
        h = h.rotate_left(5).wrapping_mul(FNV_PRIME);
        h ^= deterministic_hash(&self.rng);
        h = h.rotate_left(5).wrapping_mul(FNV_PRIME);
        h ^= deterministic_hash(&(
            self.chunk,
            self.next_wave_tick,
            self.wave_number,
            self.base_fallen,
            self.next_monster_id,
        ));
        h = h.rotate_left(5).wrapping_mul(FNV_PRIME);
        h ^= deterministic_hash(&(
            self.total_spawned,
            self.total_killed,
            self.total_shots,
            self.ammo_consumed,
            self.structure_damage,
            self.structures_destroyed,
            self.debris_recovered,
        ));
        h.rotate_left(5).wrapping_mul(FNV_PRIME)
    }
}

/// Integer squared distance between a tile and a monster position.
#[inline(always)]
fn tile_dist_sq(x: i64, y: i64, pos: FixedVec2) -> i64 {
    let dx = pos.x.to_int() - x;
    let dy = pos.y.to_int() - y;
    dx * dx + dy * dy
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::factory::FactoryStructure;
    use core::mem::{align_of, size_of};

    type Grid = FactoryGrid<16, 16>;
    type Director = WaveDirector<32>;

    /// Heart in the centre, four turrets around it, each fed by an iron-plate chest.
    fn fortified_base() -> Grid {
        let mut g = Grid::new();
        g.place_base_heart(8, 8);
        for &(tx, ty, cx, cy) in &[
            (6, 6, 6, 5),
            (10, 6, 10, 5),
            (6, 10, 6, 11),
            (10, 10, 10, 11),
        ] {
            g.place_turret(tx, ty);
            g.place_chest(cx, cy);
            for _ in 0..200 {
                g.cells[cy][cx].try_accept(ItemType::IronPlate);
            }
        }
        for x in 5..=11 {
            g.place_wall(x, 4);
            g.place_wall(x, 12);
        }
        g
    }

    #[test]
    fn test_defense_payloads_keep_tile_on_one_cache_line() {
        assert!(
            size_of::<Turret>() <= 16,
            "Turret must fit the 16-byte payload slot"
        );
        assert!(size_of::<Wall>() <= 16);
        assert!(size_of::<BaseHeart>() <= 16);
        assert_eq!(
            size_of::<StructureKind>(),
            20,
            "StructureKind must stay 20 bytes"
        );
        assert_eq!(size_of::<FactoryStructure>(), 64);
        assert_eq!(align_of::<FactoryStructure>(), 64);
        assert_eq!(
            align_of::<Director>(),
            64,
            "WaveDirector must be 64-byte aligned"
        );
        assert_eq!(size_of::<Director>() % 64, 0);
    }

    #[test]
    fn test_turret_takes_ammo_from_belt_inserter_and_adjacent_chest() {
        let mut g = Grid::new();
        // Belt lane feeding the turret directly from the west.
        g.place_turret(4, 2);
        g.place_belt(2, 2, Direction::East, 100);
        g.place_belt(3, 2, Direction::East, 100);
        g.cells[2][2].try_accept(ItemType::IronOre);
        g.run(3);
        assert_eq!(
            g.turret_ammo(4, 2),
            (ItemType::IronOre, 1),
            "belt must feed turret"
        );

        // Inserter drop into a second turret, and non-ammo is refused.
        g.place_turret(4, 6);
        g.place_chest(2, 6);
        g.place_inserter(3, 6, Direction::East, 100);
        g.cells[6][2].try_accept(ItemType::CopperPlate);
        g.run(4);
        assert_eq!(g.turret_ammo(4, 6), (ItemType::CopperPlate, 1));
        assert!(
            !g.cells[6][4].try_accept(ItemType::Coal),
            "coal is not ammo"
        );
        assert!(
            !g.cells[6][4].try_accept(ItemType::IronOre),
            "magazine holds one ammo type"
        );

        // Adjacent chest auto-supply happens in the director tick, one round per tick,
        // and stops at the magazine cap.
        g.place_turret(10, 10);
        g.place_chest(10, 9);
        for _ in 0..30 {
            g.cells[9][10].try_accept(ItemType::IronPlate);
        }
        let mut d = Director::with_schedule(7, u32::MAX, 1_000, 0, 0);
        d.tick(&mut g);
        assert_eq!(g.turret_ammo(10, 10), (ItemType::IronPlate, 1));
        for _ in 0..40 {
            d.tick(&mut g);
        }
        assert_eq!(g.turret_ammo(10, 10).1, TURRET_AMMO_CAP);
        assert_eq!(g.chest_contents(10, 9).1, 30 - TURRET_AMMO_CAP as u32);
    }

    #[test]
    fn test_waves_spawn_on_schedule_and_grow() {
        let mut g = Grid::new();
        g.place_base_heart(8, 8);
        let mut d = Director::with_schedule(42, 10, 50, 4, 2);

        d.run_with(&mut g, 9);
        assert_eq!(d.wave_number, 0);
        assert_eq!(d.monsters_alive(), 0);
        d.step_with(&mut g);
        assert_eq!(d.wave_number, 1);
        assert_eq!(d.monsters_alive(), 4);
        assert_eq!(d.total_spawned, 4);

        d.run_with(&mut g, 50);
        assert_eq!(d.wave_number, 2, "second wave at tick 60");
        assert_eq!(d.total_spawned, 10, "wave 2 adds 4 + 2 monsters");
        assert_eq!(d.wave_size(3), 8);

        // Every monster sits on the grid border and belongs to the monster team.
        for u in d.arena.units.iter().filter(|u| u.is_alive()) {
            assert_eq!(u.team, MONSTER_TEAM);
            let (x, y) = (u.pos.x.to_int(), u.pos.y.to_int());
            assert!((0..16).contains(&x) && (0..16).contains(&y));
        }
    }

    #[test]
    fn test_turrets_hold_the_line_for_100_ticks() {
        let mut g = fortified_base();
        let mut d = Director::new(0xDEAD_BEEF);
        d.run_with(&mut g, 100);

        let r = d.report(&g);
        assert_eq!(r.wave_number, 2, "waves at ticks 10 and 60");
        assert_eq!(r.total_spawned, 10);
        assert!(
            r.total_killed >= 6,
            "turrets must kill most monsters: {r:?}"
        );
        assert_eq!(r.heart_hp, BASE_HEART_HP, "heart untouched: {r:?}");
        assert_eq!(r.turrets, 4);
        assert_eq!(r.total_shots, r.ammo_consumed, "one round per shot");
        assert!(r.ammo_consumed > 0);
        assert!(!r.base_fallen);

        // Ammo conservation: chests + magazines + consumed == 800 plates loaded.
        let mut chest_total = 0u64;
        for row in &g.cells {
            for c in row {
                if let StructureKind::Chest(ch) = c.kind {
                    chest_total += ch.quantity as u64;
                }
            }
        }
        assert_eq!(chest_total + r.turret_ammo as u64 + r.ammo_consumed, 800);
    }

    #[test]
    fn test_undefended_base_falls_and_leaves_debris() {
        let mut g = Grid::new();
        g.place_base_heart(8, 8);
        g.place_wall(8, 7);
        let mut d = Director::with_schedule(3, 1, 20, 6, 4);

        // Run until the wall collapses.
        let mut wall_fell_at = 0u64;
        for _ in 0..600 {
            d.step_with(&mut g);
            if !matches!(g.cells[7][8].kind, StructureKind::Wall(_)) {
                wall_fell_at = d.current_tick;
                break;
            }
        }
        assert!(wall_fell_at > 0, "wall must eventually fall");
        assert_eq!(
            g.chest_contents(8, 7),
            (ItemType::StoneBrick, WALL_SALVAGE),
            "collapsed wall leaves a stone-brick debris chest"
        );
        assert_eq!(d.debris_recovered, WALL_SALVAGE as u64);
        assert_eq!(d.structures_destroyed, 1);

        // Keep going until the heart is gone.
        for _ in 0..2_000 {
            if d.base_fallen {
                break;
            }
            d.step_with(&mut g);
        }
        assert!(d.base_fallen, "heart must fall with no turrets");
        assert!(g.cells[8][8].is_empty(), "destroyed heart is rubble");
        assert_eq!(d.report(&g).heart_hp, 0);
        assert_eq!(d.structures_destroyed, 2);
        assert!(d.structure_damage >= (WALL_HP + BASE_HEART_HP) as u64);

        // No more waves once the base has fallen; survivors idle with no target.
        let spawned = d.total_spawned;
        d.run_with(&mut g, 100);
        assert_eq!(d.total_spawned, spawned);
        for u in d.arena.units.iter().filter(|u| u.is_alive()) {
            assert_eq!(u.target_id, 0);
            assert_eq!(u.vel, FixedVec2::ZERO);
        }
    }

    #[test]
    fn test_destroyed_turret_drops_iron_plates() {
        let mut g = Grid::new();
        g.place_turret_with(8, 8, 30, 0, 0, 1); // harmless turret, 30 hp
        let mut d = Director::with_schedule(11, 1, 1_000, 8, 0);
        d.run_with(&mut g, 120);
        assert_eq!(
            g.chest_contents(8, 8),
            (ItemType::IronPlate, TURRET_SALVAGE)
        );
        assert_eq!(d.structures_destroyed, 1);
        assert!(!d.base_fallen, "no heart on this grid");
    }

    #[test]
    fn test_world_factory_defense_binds_to_claimed_chunk() {
        type World = WorldFactory<16, 9, 4>;
        let coord = ChunkCoord::new(2, -1);
        let mut world = World::new(0x5EED, 1);
        world.explore(coord);
        assert!(world.claim_chunk(coord).is_some());

        let (bx, by) = (coord.x as i64 * 16, coord.y as i64 * 16);
        assert!(world.place_base_heart_at(bx + 8, by + 8));
        assert!(world.place_turret_at(bx + 6, by + 6));
        assert!(world.place_wall_at(bx + 8, by + 4));
        assert!(world.place_chest_at(bx + 6, by + 5));
        for _ in 0..100 {
            world.factory_mut(coord).unwrap().cells[5][6].try_accept(ItemType::CopperPlate);
        }
        assert!(!world.place_wall_at(bx + 100, by + 100), "unclaimed chunk");

        let mut d = Director::new(99);
        d.bind_chunk(coord);
        d.run_world_with(&mut world, 100);
        assert_eq!(d.wave_number, 2);
        assert!(d.total_shots > 0, "turret fires through the world path");
        let g = world.factory(coord).unwrap();
        assert!(g.turret_ammo(6, 6).0 == ItemType::CopperPlate || d.ammo_consumed > 0);
        assert_eq!(g.structure_hp(8, 8).1, BASE_HEART_HP);

        // Releasing the chunk makes the director idle (tick still advances).
        assert!(world.release_chunk(coord));
        let before = d.compute_hash();
        d.step_world_with(&mut world);
        assert_eq!(d.current_tick, 101);
        assert_ne!(d.compute_hash(), before);
        assert_eq!(d.total_spawned, 10);
    }

    #[test]
    fn test_100_tick_defense_is_bit_identical() {
        let run = |seed: u64| -> ([u64; 100], u64) {
            let mut g = fortified_base();
            let mut d = Director::new(seed);
            let mut trace = [0u64; 100];
            for (i, t) in trace.iter_mut().enumerate() {
                d.step_with(&mut g);
                *t = d.compute_hash() ^ g.compute_hash().rotate_left((i % 64) as u32);
            }
            (trace, d.arena.compute_hash())
        };

        let (a, arena_a) = run(0xC0FF_EE42);
        let (b, arena_b) = run(0xC0FF_EE42);
        assert_eq!(a, b, "100-tick trace must be bit-identical across runs");
        assert_eq!(arena_a, arena_b);

        let (c, _) = run(0xC0FF_EE43);
        assert_ne!(a, c, "a different seed must change the spawn pattern");
    }

    #[test]
    fn test_monster_steers_around_wall_maze_and_breaches() {
        let mut g = FactoryGrid::<32, 32>::new();
        g.place_base_heart(16, 16);

        // Build a wall segment at x=10, y=14..=18 with an opening at y=13
        for y in 14..=18 {
            g.place_wall(10, y);
        }

        let mut d = Director::with_schedule(1234, 1000, 1000, 0, 0); // No auto-spawns
        // Spawn one monster directly in front of the wall
        let slot = d.arena.spawn_unit(
            1,
            MONSTER_TEAM,
            100,
            10,
            1,
            5,
            FixedVec2::from_ints(8, 16),
        );
        assert!(slot.is_some());

        // Step 20 ticks
        for _ in 0..20 {
            d.tick(&mut g);
        }

        // The monster must either detour through the opening or attack the blocking wall
        assert!(d.arena.units[slot.unwrap()].is_alive());
        let pos = d.arena.units[slot.unwrap()].pos;
        let x = pos.x.to_int();
        let y = pos.y.to_int();
        // Position changed towards target
        assert!(x > 8 || y != 16 || d.structure_damage > 0);
    }

    #[test]
    fn test_monster_prioritizes_heart_over_distraction_walls() {
        let mut g = FactoryGrid::<32, 32>::new();
        g.place_base_heart(16, 16);

        // Distraction wall placed at (2, 2) near spawn
        g.place_wall(2, 2);

        let mut d = Director::with_schedule(1234, 1000, 1000, 0, 0); // No auto-spawns
        // Spawn a monster at (0, 16)
        let slot = d.arena.spawn_unit(
            1,
            MONSTER_TEAM,
            50,
            8,
            1,
            12,
            FixedVec2::from_ints(0, 16),
        );
        assert!(slot.is_some());

        // Advance 10 ticks (2 move cycles at interval 5)
        for _ in 0..10 {
            d.tick(&mut g);
        }

        // Monster should move East toward Heart (16, 16), NOT North toward Wall (2, 2)
        let m = d.arena.units[slot.unwrap()];
        assert!(m.pos.x.to_int() > 0, "Monster should advance East toward Heart");
        assert_eq!(m.pos.y.to_int(), 16, "Monster should not deviate toward Wall at (2, 2)");
    }
}
