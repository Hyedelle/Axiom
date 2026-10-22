//! # Colony Workforce: NPC Labour & Logistics Assignment (Track 1 / T-042)
//!
//! Bridges the 100k background NPC ecology ([`axiom_core::npc_crowd::Npc`]) with the
//! factory systems ([`FactoryGrid`] and [`WorldFactory`]):
//!
//! ```text
//! NpcTable / NpcCrowdPool --hire--> ColonyWorkforce --assign--> factory tile
//!   (id, energy)                   (64B slab)                   Drill    <- Miner
//!                                                                Smelter  <- SmelterOperator
//!                                                                Inserter <- Technician
//!                                                                any -> any  Hauler (carries items)
//! ```
//!
//! Design axioms:
//! - **Absolute determinism**: stamina, buffs and hauling are pure integer transitions
//!   `next = f(state)`. No floating point, no RNG, no wall clock.
//! - **Cache-line resident**: every [`WorkerAssignment`] is exactly 64 bytes and 64-byte
//!   aligned; [`ColonyWorkforce`] is a flat slab of them.
//! - **Zero heap in the hot loop**: the crate is `no_std` without `alloc`; [`ColonyWorkforce::tick`]
//!   touches only the slab and the grid in place.

use axiom_core::npc_crowd::{Npc, NpcActivity};
use axiom_core::{deterministic_hash, ChunkCoord};

use crate::factory::{FactoryGrid, ItemType, StructureKind, CYCLE};
use crate::world_factory::WorldFactory;

// ---------------------------------------------------------------------------
// Tunables (all integers)
// ---------------------------------------------------------------------------

/// Default number of worker slots in a [`ColonyWorkforce`].
pub const DEFAULT_WORKFORCE_CAP: usize = 256;

/// Stamina scale, matches `Npc::energy` (0 ..= 1000).
pub const STAMINA_MAX: u16 = 1000;
/// Stamina drained per working tick.
pub const WORK_STAMINA_COST: u16 = 2;
/// Stamina regained per resting tick.
pub const REST_STAMINA_GAIN: u16 = 5;
/// A working worker whose stamina drops below this goes to rest.
pub const EXHAUSTED_THRESHOLD: u16 = 100;
/// A resting worker whose stamina reaches this resumes work (hysteresis).
pub const RESTED_THRESHOLD: u16 = 600;

/// Skill cap; each level adds [`SKILL_BUFF_PCT`] to the productivity buff.
pub const MAX_SKILL: u8 = 10;
/// Extra productivity percent per skill level.
pub const SKILL_BUFF_PCT: u16 = 5;
/// Productive ticks needed to gain one skill level.
pub const XP_PER_SKILL: u32 = 600;

/// Hauler action rate (progress per tick); `50` = one pick-up or drop every 2 ticks.
pub const HAUL_SPEED: u16 = 50;

// ---------------------------------------------------------------------------
// Role & state
// ---------------------------------------------------------------------------

/// Job a colonist performs at its post.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum WorkerRole {
    /// Boosts a [`crate::MiningDrill`].
    #[default]
    Miner = 1,
    /// Boosts a [`crate::Smelter`].
    SmelterOperator = 2,
    /// Carries items by hand from a source tile to a target tile.
    Hauler = 3,
    /// Boosts an [`crate::Inserter`] or a [`crate::Belt`].
    Technician = 4,
}

impl WorkerRole {
    /// Base productivity buff in percent (before skill).
    #[inline(always)]
    pub const fn base_buff_pct(self) -> u16 {
        match self {
            WorkerRole::Miner => 25,
            WorkerRole::SmelterOperator => 25,
            WorkerRole::Hauler => 0,
            WorkerRole::Technician => 20,
        }
    }

    /// Whether this role can be posted on a tile holding `kind`.
    #[inline]
    pub fn accepts(self, kind: &StructureKind) -> bool {
        matches!(
            (self, kind),
            (WorkerRole::Miner, StructureKind::Drill(_))
                | (WorkerRole::SmelterOperator, StructureKind::Smelter(_))
                | (WorkerRole::Technician, StructureKind::Inserter(_))
                | (WorkerRole::Technician, StructureKind::Belt(_))
                | (WorkerRole::Hauler, StructureKind::Belt(_))
                | (WorkerRole::Hauler, StructureKind::Smelter(_))
                | (WorkerRole::Hauler, StructureKind::Chest(_))
        )
    }

    /// Machine roles occupy their post exclusively; haulers may share a source.
    #[inline(always)]
    pub const fn is_exclusive(self) -> bool {
        !matches!(self, WorkerRole::Hauler)
    }
}

/// Lifecycle state of a hired colonist.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum WorkerState {
    /// Hired but not posted anywhere.
    #[default]
    Idle = 0,
    /// Posted and productive this tick.
    Working = 1,
    /// Posted but recovering stamina; no buff / no hauling.
    Resting = 2,
}

// ---------------------------------------------------------------------------
// 64-byte worker record
// ---------------------------------------------------------------------------

/// Padding so the record is exactly one 64-byte cache line (48 bytes of fields + 16).
const WORKER_RESERVED: usize = 16;

/// One hired colonist and its factory post. Exactly 64 bytes, 64-byte aligned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(C, align(64))]
pub struct WorkerAssignment {
    /// NPC identity from the crowd pool (`0` = free slot).
    pub npc_id: u32,
    /// Chunk of the post (`(0, 0)` for a standalone [`FactoryGrid`]).
    pub chunk: ChunkCoord,
    /// Post tile (machine tile, or hauler pick-up tile).
    pub tile_x: u16,
    pub tile_y: u16,
    /// Hauler drop-off tile (unused by machine roles).
    pub target_x: u16,
    pub target_y: u16,
    /// Machine speed captured at assignment; restored on unassign / rest.
    pub base_speed: u16,
    /// Physical stamina (0 ..= [`STAMINA_MAX`]).
    pub stamina: u16,
    pub role: WorkerRole,
    pub state: WorkerState,
    /// Skill level (0 ..= [`MAX_SKILL`]).
    pub skill: u8,
    /// Item in a hauler's hands.
    pub carrying: ItemType,
    /// Hauler action progress (0 ..= [`CYCLE`]).
    pub progress: u16,
    /// Machine speed applied on the last tick (telemetry).
    pub effective_speed: u16,
    /// Productive ticks since the last skill level-up.
    pub experience: u32,
    /// Lifetime items carried by hand.
    pub items_hauled: u32,
    /// Lifetime productive ticks.
    pub productive_ticks: u32,
    /// Tick at which the colonist was hired.
    pub hired_tick: u32,
    pub _reserved: [u8; WORKER_RESERVED],
}

impl Default for WorkerAssignment {
    #[inline(always)]
    fn default() -> Self {
        Self::empty()
    }
}

impl WorkerAssignment {
    pub const fn empty() -> Self {
        Self {
            npc_id: 0,
            chunk: ChunkCoord::new(0, 0),
            tile_x: 0,
            tile_y: 0,
            target_x: 0,
            target_y: 0,
            base_speed: 0,
            stamina: STAMINA_MAX,
            role: WorkerRole::Miner,
            state: WorkerState::Idle,
            skill: 0,
            carrying: ItemType::None,
            progress: 0,
            effective_speed: 0,
            experience: 0,
            items_hauled: 0,
            productive_ticks: 0,
            hired_tick: 0,
            _reserved: [0; WORKER_RESERVED],
        }
    }

    #[inline(always)]
    pub const fn is_hired(&self) -> bool {
        self.npc_id != 0
    }

    #[inline(always)]
    pub const fn is_posted(&self) -> bool {
        self.is_hired() && !matches!(self.state, WorkerState::Idle)
    }

    /// Total productivity buff in percent: role base + skill bonus.
    #[inline(always)]
    pub const fn buff_pct(&self) -> u16 {
        self.role.base_buff_pct() + (self.skill as u16) * SKILL_BUFF_PCT
    }

    /// `base_speed` scaled by the buff (integer math, clamped to `u16::MAX`).
    #[inline(always)]
    pub const fn buffed_speed(&self) -> u16 {
        let s = (self.base_speed as u32) * (100 + self.buff_pct() as u32) / 100;
        if s > u16::MAX as u32 {
            u16::MAX
        } else {
            s as u16
        }
    }

    /// Drops the post but keeps the colonist hired.
    #[inline]
    fn clear_post(&mut self) {
        self.state = WorkerState::Idle;
        self.base_speed = 0;
        self.effective_speed = 0;
        self.progress = 0;
    }
}

// ---------------------------------------------------------------------------
// Machine speed access
// ---------------------------------------------------------------------------

/// Mutable handle to the `speed` field of any buffable machine on a tile.
#[inline(always)]
fn machine_speed_mut(kind: &mut StructureKind) -> Option<&mut u16> {
    match kind {
        StructureKind::Drill(d) => Some(&mut d.speed),
        StructureKind::Smelter(s) => Some(&mut s.speed),
        StructureKind::Inserter(i) => Some(&mut i.speed),
        StructureKind::Belt(b) => Some(&mut b.speed),
        _ => None,
    }
}

#[inline(always)]
fn machine_speed(kind: &StructureKind) -> u16 {
    match kind {
        StructureKind::Drill(d) => d.speed,
        StructureKind::Smelter(s) => s.speed,
        StructureKind::Inserter(i) => i.speed,
        StructureKind::Belt(b) => b.speed,
        _ => 0,
    }
}

/// Writes `speed` to the machine at the worker's post (no-op if the post is gone).
#[inline]
fn write_post_speed<const W: usize, const H: usize>(
    w: &mut WorkerAssignment,
    grid: &mut FactoryGrid<W, H>,
    speed: u16,
) {
    let (x, y) = (w.tile_x as usize, w.tile_y as usize);
    if x < W && y < H {
        if let Some(s) = machine_speed_mut(&mut grid.cells[y][x].kind) {
            *s = speed;
            w.effective_speed = speed;
        }
    }
}

// ---------------------------------------------------------------------------
// Per-worker tick (free function so the slab loop borrows cleanly)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Default)]
struct TickOutcome {
    productive: bool,
    resting: bool,
    hauled: u32,
}

/// One hauler action: pick up at the post tile or drop at the target tile.
#[inline]
fn haul<const W: usize, const H: usize>(
    w: &mut WorkerAssignment,
    grid: &mut FactoryGrid<W, H>,
    tick: u32,
) -> u32 {
    w.progress = w.progress.saturating_add(HAUL_SPEED).min(CYCLE);
    if w.progress < CYCLE {
        return 0;
    }
    let (sx, sy) = (w.tile_x as usize, w.tile_y as usize);
    let (tx, ty) = (w.target_x as usize, w.target_y as usize);
    if w.carrying.is_none() {
        let item = grid.cells[sy][sx].try_take();
        if !item.is_none() {
            w.carrying = item;
            w.progress = 0;
            grid.cells[sy][sx].last_action_tick = tick;
        }
        0
    } else if tx < W && ty < H {
        let was_chest = matches!(grid.cells[ty][tx].kind, StructureKind::Chest(_));
        if grid.cells[ty][tx].try_accept(w.carrying) {
            w.carrying = ItemType::None;
            w.progress = 0;
            w.items_hauled = w.items_hauled.saturating_add(1);
            grid.cells[ty][tx].last_action_tick = tick;
            if was_chest {
                grid.total_stored += 1;
            }
            1
        } else {
            0
        }
    } else {
        0
    }
}

/// Advances one posted worker by one tick against the grid holding its post.
fn tick_worker<const W: usize, const H: usize>(
    w: &mut WorkerAssignment,
    grid: &mut FactoryGrid<W, H>,
    tick: u64,
) -> TickOutcome {
    let mut out = TickOutcome::default();
    if !w.is_posted() {
        return out;
    }

    // 1. Validate the post still holds a structure this role can serve.
    let (x, y) = (w.tile_x as usize, w.tile_y as usize);
    let post_ok = match grid.get(x, y) {
        Some(cell) => w.role.accepts(&cell.kind),
        None => false,
    };
    if !post_ok {
        w.clear_post();
        return out;
    }

    // 2. Stamina hysteresis.
    if w.state == WorkerState::Working && w.stamina < EXHAUSTED_THRESHOLD {
        w.state = WorkerState::Resting;
        w.progress = 0;
    } else if w.state == WorkerState::Resting && w.stamina >= RESTED_THRESHOLD {
        w.state = WorkerState::Working;
    }

    // 3. Act.
    match w.state {
        WorkerState::Resting => {
            w.stamina = w.stamina.saturating_add(REST_STAMINA_GAIN).min(STAMINA_MAX);
            if w.role != WorkerRole::Hauler {
                let base = w.base_speed;
                write_post_speed(w, grid, base);
            }
            out.resting = true;
        }
        WorkerState::Working => {
            w.stamina = w.stamina.saturating_sub(WORK_STAMINA_COST);
            if w.role == WorkerRole::Hauler {
                out.hauled = haul(w, grid, tick as u32);
            } else {
                let buffed = w.buffed_speed();
                write_post_speed(w, grid, buffed);
            }
            w.productive_ticks = w.productive_ticks.saturating_add(1);
            w.experience += 1;
            if w.experience >= XP_PER_SKILL {
                w.experience = 0;
                if w.skill < MAX_SKILL {
                    w.skill += 1;
                }
            }
            out.productive = true;
        }
        WorkerState::Idle => {}
    }
    out
}

// ---------------------------------------------------------------------------
// Workforce slab
// ---------------------------------------------------------------------------

/// Aggregate workforce counters, snapshotted via [`ColonyWorkforce::report`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct WorkforceReport {
    pub tick: u64,
    pub hired: u32,
    pub working: u32,
    pub resting: u32,
    pub idle: u32,
    pub items_carried: u32,
    pub total_productive_ticks: u64,
    pub total_rest_ticks: u64,
    pub total_hauled: u64,
}

/// Flat slab of [`WorkerAssignment`]s plus colony-wide counters.
#[derive(Debug, Clone, PartialEq, Eq)]
#[repr(C, align(64))]
pub struct ColonyWorkforce<const CAP: usize = DEFAULT_WORKFORCE_CAP> {
    pub workers: [WorkerAssignment; CAP],
    pub current_tick: u64,
    pub total_productive_ticks: u64,
    pub total_rest_ticks: u64,
    pub total_hauled: u64,
    pub hired_count: u32,
}

impl<const CAP: usize> Default for ColonyWorkforce<CAP> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const CAP: usize> ColonyWorkforce<CAP> {
    pub const fn new() -> Self {
        Self {
            workers: [WorkerAssignment::empty(); CAP],
            current_tick: 0,
            total_productive_ticks: 0,
            total_rest_ticks: 0,
            total_hauled: 0,
            hired_count: 0,
        }
    }

    // ---- queries ---------------------------------------------------------

    #[inline(always)]
    pub const fn capacity(&self) -> usize {
        CAP
    }

    #[inline(always)]
    pub const fn len(&self) -> usize {
        self.hired_count as usize
    }

    #[inline(always)]
    pub const fn is_empty(&self) -> bool {
        self.hired_count == 0
    }

    #[inline(always)]
    pub const fn is_full(&self) -> bool {
        self.hired_count as usize >= CAP
    }

    /// The hired worker in `slot`, if any.
    #[inline]
    pub fn get(&self, slot: usize) -> Option<&WorkerAssignment> {
        match self.workers.get(slot) {
            Some(w) if w.is_hired() => Some(w),
            _ => None,
        }
    }

    /// Slot of the worker hired from NPC `npc_id`.
    #[inline]
    pub fn find_by_npc(&self, npc_id: u32) -> Option<usize> {
        if npc_id == 0 {
            return None;
        }
        self.workers.iter().position(|w| w.npc_id == npc_id)
    }

    /// Slot of the exclusive worker posted on `(chunk, x, y)`, if any.
    #[inline]
    pub fn find_post(&self, chunk: ChunkCoord, x: u16, y: u16) -> Option<usize> {
        self.workers.iter().position(|w| {
            w.is_posted()
                && w.role.is_exclusive()
                && w.chunk == chunk
                && w.tile_x == x
                && w.tile_y == y
        })
    }

    // ---- hiring ----------------------------------------------------------

    /// Hires a colonist from the NPC ecology; stamina starts at the NPC's energy.
    #[inline]
    pub fn hire(&mut self, npc: &Npc) -> Option<usize> {
        self.hire_raw(npc.id, npc.energy)
    }

    /// Hires by raw id. Rejects id `0`, duplicates, and a full slab.
    pub fn hire_raw(&mut self, npc_id: u32, stamina: u16) -> Option<usize> {
        if npc_id == 0 || self.is_full() || self.find_by_npc(npc_id).is_some() {
            return None;
        }
        let slot = self.workers.iter().position(|w| !w.is_hired())?;
        let mut w = WorkerAssignment::empty();
        w.npc_id = npc_id;
        w.stamina = stamina.min(STAMINA_MAX);
        w.hired_tick = self.current_tick as u32;
        self.workers[slot] = w;
        self.hired_count += 1;
        Some(slot)
    }

    /// Dismisses the worker in `slot`, restoring its machine's base speed first.
    pub fn fire<const W: usize, const H: usize>(
        &mut self,
        slot: usize,
        grid: &mut FactoryGrid<W, H>,
    ) -> bool {
        if self.get(slot).is_none() {
            return false;
        }
        self.unassign(slot, grid);
        self.workers[slot] = WorkerAssignment::empty();
        self.hired_count -= 1;
        true
    }

    // ---- assignment ------------------------------------------------------

    /// Posts a machine-role worker on `(x, y)` of a standalone grid (chunk `(0, 0)`).
    /// Haulers must use [`ColonyWorkforce::assign_hauler`].
    #[inline]
    pub fn assign<const W: usize, const H: usize>(
        &mut self,
        slot: usize,
        role: WorkerRole,
        x: usize,
        y: usize,
        grid: &mut FactoryGrid<W, H>,
    ) -> bool {
        self.assign_in(slot, role, ChunkCoord::new(0, 0), x, y, grid)
    }

    /// Posts a machine-role worker on local tile `(x, y)` of the grid at `chunk`.
    ///
    /// Fails if the slot is empty, the tile is out of range, the structure does not
    /// match the role, or another exclusive worker already holds that post. A worker
    /// that is already posted is first unassigned (base speed restored).
    pub fn assign_in<const W: usize, const H: usize>(
        &mut self,
        slot: usize,
        role: WorkerRole,
        chunk: ChunkCoord,
        x: usize,
        y: usize,
        grid: &mut FactoryGrid<W, H>,
    ) -> bool {
        if role == WorkerRole::Hauler || self.get(slot).is_none() {
            return false;
        }
        if x >= W || y >= H || x > u16::MAX as usize || y > u16::MAX as usize {
            return false;
        }
        let cell = &grid.cells[y][x];
        if !role.accepts(&cell.kind) {
            return false;
        }
        if let Some(other) = self.find_post(chunk, x as u16, y as u16) {
            if other != slot {
                return false;
            }
        }
        let base = machine_speed(&cell.kind);

        // Release any previous post against this grid (only valid if same chunk).
        if self.workers[slot].is_posted() && self.workers[slot].chunk == chunk {
            self.unassign(slot, grid);
        }

        let w = &mut self.workers[slot];
        w.role = role;
        w.chunk = chunk;
        w.tile_x = x as u16;
        w.tile_y = y as u16;
        w.target_x = x as u16;
        w.target_y = y as u16;
        w.base_speed = base;
        w.effective_speed = base;
        w.carrying = ItemType::None;
        w.progress = 0;
        w.state = if w.stamina >= EXHAUSTED_THRESHOLD {
            WorkerState::Working
        } else {
            WorkerState::Resting
        };
        true
    }

    /// Posts a hauler carrying items from `(sx, sy)` to `(tx, ty)` on a standalone grid.
    #[inline]
    pub fn assign_hauler<const W: usize, const H: usize>(
        &mut self,
        slot: usize,
        sx: usize,
        sy: usize,
        tx: usize,
        ty: usize,
        grid: &mut FactoryGrid<W, H>,
    ) -> bool {
        self.assign_hauler_in(slot, ChunkCoord::new(0, 0), sx, sy, tx, ty, grid)
    }

    /// Posts a hauler on the grid at `chunk`. Source must be takeable (belt / smelter
    /// output / chest); the target must be a non-empty tile within the same grid.
    #[allow(clippy::too_many_arguments)]
    pub fn assign_hauler_in<const W: usize, const H: usize>(
        &mut self,
        slot: usize,
        chunk: ChunkCoord,
        sx: usize,
        sy: usize,
        tx: usize,
        ty: usize,
        grid: &mut FactoryGrid<W, H>,
    ) -> bool {
        if self.get(slot).is_none() {
            return false;
        }
        if sx >= W || sy >= H || tx >= W || ty >= H {
            return false;
        }
        if sx > u16::MAX as usize || sy > u16::MAX as usize {
            return false;
        }
        if tx > u16::MAX as usize || ty > u16::MAX as usize {
            return false;
        }
        if !WorkerRole::Hauler.accepts(&grid.cells[sy][sx].kind) || grid.cells[ty][tx].is_empty() {
            return false;
        }
        if (sx, sy) == (tx, ty) {
            return false;
        }
        if self.workers[slot].is_posted() && self.workers[slot].chunk == chunk {
            self.unassign(slot, grid);
        }

        let w = &mut self.workers[slot];
        w.role = WorkerRole::Hauler;
        w.chunk = chunk;
        w.tile_x = sx as u16;
        w.tile_y = sy as u16;
        w.target_x = tx as u16;
        w.target_y = ty as u16;
        w.base_speed = HAUL_SPEED;
        w.effective_speed = HAUL_SPEED;
        w.carrying = ItemType::None;
        w.progress = 0;
        w.state = if w.stamina >= EXHAUSTED_THRESHOLD {
            WorkerState::Working
        } else {
            WorkerState::Resting
        };
        true
    }

    /// Removes the worker's post and restores the machine's base speed on `grid`.
    /// A hauler drops whatever it carries back onto its source tile when possible.
    pub fn unassign<const W: usize, const H: usize>(
        &mut self,
        slot: usize,
        grid: &mut FactoryGrid<W, H>,
    ) -> bool {
        if slot >= CAP || !self.workers[slot].is_posted() {
            return false;
        }
        let w = &mut self.workers[slot];
        let (x, y) = (w.tile_x as usize, w.tile_y as usize);
        if w.role == WorkerRole::Hauler {
            if !w.carrying.is_none() && x < W && y < H && grid.cells[y][x].try_accept(w.carrying) {
                w.carrying = ItemType::None;
            }
        } else if x < W && y < H && w.role.accepts(&grid.cells[y][x].kind) {
            let base = w.base_speed;
            write_post_speed(w, grid, base);
        }
        w.clear_post();
        true
    }

    // ---- simulation: standalone grid --------------------------------------

    /// Advances every posted worker by one tick against `grid`. Heap-free.
    ///
    /// Call before [`FactoryGrid::step`] so buffs apply to this tick's production
    /// (or use [`ColonyWorkforce::step_with`]).
    pub fn tick<const W: usize, const H: usize>(&mut self, grid: &mut FactoryGrid<W, H>) {
        self.current_tick += 1;
        let tick = self.current_tick;
        let (mut productive, mut resting, mut hauled) = (0u64, 0u64, 0u64);
        for w in self.workers.iter_mut() {
            if !w.is_posted() {
                continue;
            }
            let o = tick_worker(w, grid, tick);
            productive += o.productive as u64;
            resting += o.resting as u64;
            hauled += o.hauled as u64;
        }
        self.total_productive_ticks += productive;
        self.total_rest_ticks += resting;
        self.total_hauled += hauled;
    }

    /// Workforce tick followed by one factory tick.
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

    /// Advances every posted worker against the claimed chunk grid holding its post.
    /// Workers whose chunk is no longer claimed fall back to `Idle` (still hired).
    pub fn tick_world<const S: usize, const MC: usize, const MF: usize>(
        &mut self,
        world: &mut WorldFactory<S, MC, MF>,
    ) {
        self.current_tick += 1;
        let tick = self.current_tick;
        let (mut productive, mut resting, mut hauled) = (0u64, 0u64, 0u64);
        for w in self.workers.iter_mut() {
            if !w.is_posted() {
                continue;
            }
            match world.factory_mut(w.chunk) {
                Some(grid) => {
                    let o = tick_worker(w, grid, tick);
                    productive += o.productive as u64;
                    resting += o.resting as u64;
                    hauled += o.hauled as u64;
                }
                None => w.clear_post(),
            }
        }
        self.total_productive_ticks += productive;
        self.total_rest_ticks += resting;
        self.total_hauled += hauled;
    }

    /// Workforce tick followed by one world tick.
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

    // ---- NPC ecology bridge ----------------------------------------------

    /// Writes the worker's post and stamina back into its crowd-pool NPC.
    /// Returns `false` if `npc` is not the colonist in `slot`.
    pub fn sync_npc(&self, slot: usize, npc: &mut Npc) -> bool {
        let w = match self.get(slot) {
            Some(w) if w.npc_id == npc.id => w,
            _ => return false,
        };
        npc.energy = w.stamina;
        npc.activity = match w.state {
            WorkerState::Working => NpcActivity::Work,
            WorkerState::Resting => NpcActivity::Rest,
            WorkerState::Idle => NpcActivity::Idle,
        };
        if w.is_posted() {
            npc.work = (w.tile_x as i32, w.tile_y as i32);
        }
        true
    }

    // ---- telemetry -------------------------------------------------------

    pub fn report(&self) -> WorkforceReport {
        let mut r = WorkforceReport {
            tick: self.current_tick,
            hired: self.hired_count,
            total_productive_ticks: self.total_productive_ticks,
            total_rest_ticks: self.total_rest_ticks,
            total_hauled: self.total_hauled,
            ..WorkforceReport::default()
        };
        for w in self.workers.iter().filter(|w| w.is_hired()) {
            match w.state {
                WorkerState::Working => r.working += 1,
                WorkerState::Resting => r.resting += 1,
                WorkerState::Idle => r.idle += 1,
            }
            if !w.carrying.is_none() {
                r.items_carried += 1;
            }
        }
        r
    }

    /// Deterministic FNV-1a hash of every hired worker plus counters.
    pub fn compute_hash(&self) -> u64 {
        let mut h = self.current_tick.wrapping_mul(0x9E37_79B9_7F4A_7C15)
            ^ self.total_productive_ticks.rotate_left(11)
            ^ self.total_rest_ticks.rotate_left(29)
            ^ self.total_hauled.rotate_left(47)
            ^ (self.hired_count as u64).rotate_left(59);
        for w in self.workers.iter().filter(|w| w.is_hired()) {
            h ^= deterministic_hash(w);
            h = h.rotate_left(5).wrapping_mul(0x0000_0100_0000_01B3);
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
    use crate::factory::Direction;
    use axiom_core::npc_crowd::NpcTable;
    use core::mem::{align_of, size_of};

    type Grid = FactoryGrid<16, 8>;
    type Force = ColonyWorkforce<8>;

    /// Canonical T-040 line on row 2, with slow (40) drill and smelter so a +25 %
    /// buff crosses a cycle boundary (40 -> 3 ticks/action, 50 -> 2 ticks/action).
    fn build_line(grid: &mut Grid, reserve: u32) {
        let y = 2;
        grid.place_deposit(0, y, ItemType::IronOre, reserve);
        grid.place_drill(0, y, Direction::East, 40);
        grid.place_belt(1, y, Direction::East, 100);
        grid.place_belt(2, y, Direction::East, 100);
        grid.place_belt(3, y, Direction::East, 100);
        grid.place_inserter(4, y, Direction::East, 100);
        grid.place_smelter(5, y, 40);
        grid.place_inserter(6, y, Direction::East, 100);
        grid.place_chest(7, y);
    }

    fn drill_speed(grid: &Grid, x: usize, y: usize) -> u16 {
        match grid.cells[y][x].kind {
            StructureKind::Drill(d) => d.speed,
            _ => panic!("no drill at ({x},{y})"),
        }
    }

    fn smelter_speed(grid: &Grid, x: usize, y: usize) -> u16 {
        match grid.cells[y][x].kind {
            StructureKind::Smelter(s) => s.speed,
            _ => panic!("no smelter at ({x},{y})"),
        }
    }

    fn combined_hash(force: &Force, grid: &Grid) -> u64 {
        grid.compute_hash() ^ force.compute_hash().rotate_left(29)
    }

    #[test]
    fn test_worker_assignment_is_one_cache_line() {
        assert_eq!(size_of::<WorkerAssignment>(), 64, "must be exactly 64 bytes");
        assert_eq!(align_of::<WorkerAssignment>(), 64, "must be 64-byte aligned");
        assert_eq!(align_of::<ColonyWorkforce<4>>(), 64);
        assert_eq!(size_of::<ColonyWorkforce<4>>() % 64, 0);
        assert_eq!(size_of::<[WorkerAssignment; 256]>(), 256 * 64);
    }

    #[test]
    fn test_hire_fire_slab_reuse_and_npc_bridge() {
        let mut table = NpcTable::<16>::new();
        let a = table.spawn(101, (1, 1), (5, 5), 0, 256).unwrap();
        let b = table.spawn(102, (2, 2), (6, 6), 1, 256).unwrap();

        let mut force = Force::new();
        let mut grid = Grid::new();
        assert!(force.is_empty());

        let sa = force.hire(&table.npcs[a]).unwrap();
        let sb = force.hire(&table.npcs[b]).unwrap();
        assert_eq!((sa, sb), (0, 1));
        assert_eq!(force.len(), 2);
        assert_eq!(force.get(sa).unwrap().stamina, 1000);
        assert_eq!(force.find_by_npc(102), Some(1));

        // Duplicates and id 0 are rejected.
        assert_eq!(force.hire(&table.npcs[a]), None);
        assert_eq!(force.hire_raw(0, 500), None);

        // Firing frees the slot; the next hire reuses it.
        assert!(force.fire(sa, &mut grid));
        assert!(!force.fire(sa, &mut grid));
        assert_eq!(force.len(), 1);
        assert_eq!(force.hire_raw(103, 1500), Some(0));
        assert_eq!(force.get(0).unwrap().stamina, STAMINA_MAX, "stamina clamps");

        // Fill to capacity.
        for id in 200..206 {
            assert!(force.hire_raw(id, 1000).is_some());
        }
        assert!(force.is_full());
        assert_eq!(force.hire_raw(999, 1000), None);
    }

    #[test]
    fn test_assign_validates_role_structure_and_exclusivity() {
        let mut grid = Grid::new();
        build_line(&mut grid, 100);
        let mut force = Force::new();
        let m = force.hire_raw(1, 1000).unwrap();
        let m2 = force.hire_raw(2, 1000).unwrap();
        let s = force.hire_raw(3, 1000).unwrap();
        let t = force.hire_raw(4, 1000).unwrap();

        assert!(!force.assign(m, WorkerRole::Miner, 1, 2, &mut grid), "miner on belt");
        assert!(!force.assign(m, WorkerRole::Miner, 15, 7, &mut grid), "miner on empty");
        assert!(!force.assign(m, WorkerRole::Miner, 99, 99, &mut grid), "out of range");
        assert!(!force.assign(m, WorkerRole::Hauler, 7, 2, &mut grid), "hauler via assign");
        assert!(!force.assign(7, WorkerRole::Miner, 0, 2, &mut grid), "unhired slot");

        assert!(force.assign(m, WorkerRole::Miner, 0, 2, &mut grid));
        assert!(!force.assign(m2, WorkerRole::Miner, 0, 2, &mut grid), "post is exclusive");
        assert!(force.assign(m, WorkerRole::Miner, 0, 2, &mut grid), "re-posting self is ok");
        assert!(!force.assign(s, WorkerRole::SmelterOperator, 0, 2, &mut grid));
        assert!(force.assign(s, WorkerRole::SmelterOperator, 5, 2, &mut grid));
        assert!(!force.assign(t, WorkerRole::Technician, 5, 2, &mut grid));
        assert!(force.assign(t, WorkerRole::Technician, 4, 2, &mut grid));

        let w = force.get(m).unwrap();
        assert_eq!(w.state, WorkerState::Working);
        assert_eq!(w.base_speed, 40);
        assert_eq!(w.buff_pct(), 25);
        assert_eq!(w.buffed_speed(), 50);
        assert_eq!(force.report().working, 3);
        assert_eq!(force.report().idle, 1);
    }

    #[test]
    fn test_workers_buff_production_over_100_ticks() {
        let mut plain = Grid::new();
        build_line(&mut plain, 1_000);
        plain.run(100);

        let mut grid = Grid::new();
        build_line(&mut grid, 1_000);
        let mut force = Force::new();
        let m = force.hire_raw(1, 1000).unwrap();
        let s = force.hire_raw(2, 1000).unwrap();
        assert!(force.assign(m, WorkerRole::Miner, 0, 2, &mut grid));
        assert!(force.assign(s, WorkerRole::SmelterOperator, 5, 2, &mut grid));
        force.run_with(&mut grid, 100);

        assert_eq!(drill_speed(&grid, 0, 2), 50, "drill runs at base 40 * 1.25");
        assert_eq!(smelter_speed(&grid, 5, 2), 50);
        assert!(
            grid.total_mined > plain.total_mined,
            "buffed {} vs plain {}",
            grid.total_mined,
            plain.total_mined
        );
        assert!(grid.total_smelted > plain.total_smelted);

        let r = force.report();
        assert_eq!(r.total_productive_ticks, 200);
        assert_eq!(r.total_rest_ticks, 0);
        assert_eq!(force.get(m).unwrap().stamina, 1000 - 100 * WORK_STAMINA_COST);
        assert_eq!(force.get(m).unwrap().productive_ticks, 100);
    }

    #[test]
    fn test_stamina_exhaustion_rest_and_resume() {
        let mut grid = Grid::new();
        build_line(&mut grid, 10_000);
        let mut force = Force::new();
        let m = force.hire_raw(1, EXHAUSTED_THRESHOLD + 4).unwrap();
        assert!(force.assign(m, WorkerRole::Miner, 0, 2, &mut grid));

        // 104 -> 102 -> 100 -> 98 over 3 working ticks; the 4th tick flips to rest.
        force.run_with(&mut grid, 3);
        assert_eq!(force.get(m).unwrap().state, WorkerState::Working);
        assert_eq!(drill_speed(&grid, 0, 2), 50);
        force.step_with(&mut grid);
        assert_eq!(force.get(m).unwrap().state, WorkerState::Resting);
        assert_eq!(drill_speed(&grid, 0, 2), 40, "rest removes the buff");
        assert_eq!(force.report().resting, 1);

        // Rest until RESTED_THRESHOLD, then resume with buff.
        let mut ticks_rested = 1u32;
        while force.get(m).unwrap().state == WorkerState::Resting {
            force.step_with(&mut grid);
            ticks_rested += 1;
            assert!(ticks_rested < 200, "worker never resumed");
        }
        assert!(force.get(m).unwrap().stamina >= RESTED_THRESHOLD);
        assert_eq!(force.get(m).unwrap().state, WorkerState::Working);
        assert_eq!(drill_speed(&grid, 0, 2), 50);
        assert!(force.total_rest_ticks > 0);
        assert!(force.total_productive_ticks > 3);
    }

    #[test]
    fn test_unassign_and_fire_restore_base_speed() {
        let mut grid = Grid::new();
        build_line(&mut grid, 100);
        let mut force = Force::new();
        let m = force.hire_raw(1, 1000).unwrap();
        let s = force.hire_raw(2, 1000).unwrap();
        assert!(force.assign(m, WorkerRole::Miner, 0, 2, &mut grid));
        assert!(force.assign(s, WorkerRole::SmelterOperator, 5, 2, &mut grid));
        force.run_with(&mut grid, 5);
        assert_eq!(drill_speed(&grid, 0, 2), 50);
        assert_eq!(smelter_speed(&grid, 5, 2), 50);

        assert!(force.unassign(m, &mut grid));
        assert!(!force.unassign(m, &mut grid));
        assert_eq!(drill_speed(&grid, 0, 2), 40);
        assert_eq!(force.get(m).unwrap().state, WorkerState::Idle);
        assert_eq!(force.len(), 2, "unassigned worker stays hired");

        assert!(force.fire(s, &mut grid));
        assert_eq!(smelter_speed(&grid, 5, 2), 40);
        assert_eq!(force.len(), 1);

        // Base speed stays restored after further ticks.
        force.run_with(&mut grid, 5);
        assert_eq!(drill_speed(&grid, 0, 2), 40);
        assert_eq!(smelter_speed(&grid, 5, 2), 40);
    }

    #[test]
    fn test_hauler_carries_items_with_conservation() {
        let mut grid = Grid::new();
        grid.place_chest(0, 0);
        for _ in 0..20 {
            assert!(grid.cells[0][0].try_accept(ItemType::IronOre));
        }
        grid.place_smelter(5, 5, 100);
        grid.place_chest(9, 7);

        let mut force = Force::new();
        let h = force.hire_raw(1, 1000).unwrap();
        let h2 = force.hire_raw(2, 1000).unwrap();
        assert!(!force.assign_hauler(h, 0, 0, 0, 0, &mut grid), "source == target");
        assert!(!force.assign_hauler(h, 1, 1, 5, 5, &mut grid), "empty source");
        assert!(!force.assign_hauler(h, 0, 0, 3, 3, &mut grid), "empty target");
        assert!(force.assign_hauler(h, 0, 0, 5, 5, &mut grid));
        assert!(force.assign_hauler(h2, 5, 5, 9, 7, &mut grid), "haulers may share tiles");
        assert_eq!(force.get(h).unwrap().role, WorkerRole::Hauler);

        force.run_with(&mut grid, 100);

        let (_, left) = grid.chest_contents(0, 0);
        let (out_item, stored) = grid.chest_contents(9, 7);
        let sm = match grid.cells[5][5].kind {
            StructureKind::Smelter(s) => s,
            _ => unreachable!(),
        };
        let carried = force.report().items_carried;
        let total = left + sm.input_count as u32 + sm.output_count as u32 + stored + carried;
        assert_eq!(total, 20, "every ore is accounted for");
        assert!(stored > 0, "plates must reach the output chest");
        assert_eq!(out_item, ItemType::IronPlate);
        assert_eq!(left, 0, "20 items at 1 per 2 ticks are long gone after 100 ticks");
        assert!(force.get(h).unwrap().items_hauled >= 20 - carried);
        assert_eq!(
            force.total_hauled,
            force.get(h).unwrap().items_hauled as u64 + force.get(h2).unwrap().items_hauled as u64
        );
        assert_eq!(grid.total_stored, stored as u64);
    }

    #[test]
    fn test_technician_buffs_inserter_and_belt() {
        let mut grid = Grid::new();
        build_line(&mut grid, 100);
        grid.place_inserter(4, 2, Direction::East, 40);
        grid.place_belt(1, 2, Direction::East, 40);
        let mut force = Force::new();
        let t = force.hire_raw(1, 1000).unwrap();
        let t2 = force.hire_raw(2, 1000).unwrap();
        assert!(force.assign(t, WorkerRole::Technician, 4, 2, &mut grid));
        assert!(force.assign(t2, WorkerRole::Technician, 1, 2, &mut grid));
        force.run_with(&mut grid, 3);
        match grid.cells[2][4].kind {
            StructureKind::Inserter(i) => assert_eq!(i.speed, 48),
            _ => unreachable!(),
        }
        match grid.cells[2][1].kind {
            StructureKind::Belt(b) => assert_eq!(b.speed, 48),
            _ => unreachable!(),
        }
    }

    #[test]
    fn test_replaced_structure_drops_post_to_idle() {
        let mut grid = Grid::new();
        build_line(&mut grid, 100);
        let mut force = Force::new();
        let m = force.hire_raw(1, 1000).unwrap();
        assert!(force.assign(m, WorkerRole::Miner, 0, 2, &mut grid));
        force.run_with(&mut grid, 2);
        grid.place_belt(0, 2, Direction::East, 100);
        force.step_with(&mut grid);
        let w = force.get(m).unwrap();
        assert_eq!(w.state, WorkerState::Idle);
        assert!(w.is_hired());
        assert_eq!(force.report().idle, 1);
    }

    #[test]
    fn test_skill_levels_up_and_raises_buff() {
        let mut grid = Grid::new();
        build_line(&mut grid, 100_000);
        let mut force = Force::new();
        let m = force.hire_raw(1, 1000).unwrap();
        assert!(force.assign(m, WorkerRole::Miner, 0, 2, &mut grid));
        let mut w = *force.get(m).unwrap();
        w.skill = MAX_SKILL;
        assert_eq!(w.buff_pct(), 25 + 50);
        assert_eq!(w.buffed_speed(), 40 * 175 / 100);
        w.base_speed = u16::MAX;
        assert_eq!(w.buffed_speed(), u16::MAX, "clamps instead of overflowing");

        // Worker rests periodically, so run long enough for XP_PER_SKILL productive ticks.
        let mut ticks = 0u32;
        while force.get(m).unwrap().skill == 0 {
            force.step_with(&mut grid);
            ticks += 1;
            assert!(ticks < 5_000, "skill never levelled");
        }
        assert_eq!(force.get(m).unwrap().productive_ticks, XP_PER_SKILL);
        assert_eq!(force.get(m).unwrap().experience, 0);
    }

    #[test]
    fn test_sync_npc_writes_back_post_and_energy() {
        let mut table = NpcTable::<4>::new();
        let a = table.spawn(77, (1, 1), (0, 0), 0, 256).unwrap();
        let mut grid = Grid::new();
        build_line(&mut grid, 100);
        let mut force = Force::new();
        let m = force.hire(&table.npcs[a]).unwrap();
        assert!(force.assign(m, WorkerRole::SmelterOperator, 5, 2, &mut grid));
        force.run_with(&mut grid, 10);

        assert!(force.sync_npc(m, &mut table.npcs[a]));
        assert_eq!(table.npcs[a].work, (5, 2));
        assert_eq!(table.npcs[a].energy, 1000 - 10 * WORK_STAMINA_COST);
        assert_eq!(table.npcs[a].activity, NpcActivity::Work);

        let mut stranger = Npc::empty();
        stranger.id = 78;
        assert!(!force.sync_npc(m, &mut stranger));
        assert!(!force.sync_npc(3, &mut table.npcs[a]));
    }

    #[test]
    fn test_world_factory_post_idles_when_chunk_released() {
        type World = WorldFactory<16, 9, 4>;
        let coord = ChunkCoord::new(1, -1);
        let mut world = World::new(0xA51_0000_0042, 1);
        world.explore(coord);
        assert!(world.claim_chunk(coord).is_some());
        {
            let g = world.factory_mut(coord).unwrap();
            g.place_deposit(2, 2, ItemType::CopperOre, 1_000);
            g.place_drill(2, 2, Direction::East, 40);
            g.place_chest(3, 2);
        }

        let mut force = Force::new();
        let m = force.hire_raw(1, 1000).unwrap();
        let g = world.factory_mut(coord).unwrap();
        assert!(force.assign_in(m, WorkerRole::Miner, coord, 2, 2, g));

        force.run_world_with(&mut world, 60);
        let stored = world.factory(coord).unwrap().chest_contents(3, 2).1;
        assert_eq!(stored, 30, "buffed drill (50) stores 1 ore / 2 ticks");
        assert_eq!(force.get(m).unwrap().state, WorkerState::Working);
        assert_eq!(force.get(m).unwrap().chunk, coord);

        // Player walks far away: terrain streams out but the claimed base keeps producing.
        world.explore(ChunkCoord::new(40, 40));
        force.run_world_with(&mut world, 20);
        assert_eq!(world.factory(coord).unwrap().chest_contents(3, 2).1, 40);

        // Releasing the chunk strands the post: worker goes Idle but stays hired.
        assert!(world.release_chunk(coord));
        force.step_world_with(&mut world);
        assert_eq!(force.get(m).unwrap().state, WorkerState::Idle);
        assert_eq!(force.len(), 1);
    }

    #[test]
    fn test_100_tick_workforce_is_bit_identical() {
        fn setup(skill: u8) -> (Force, Grid) {
            let mut grid = Grid::new();
            build_line(&mut grid, 1_000);
            grid.place_chest(0, 0);
            for _ in 0..30 {
                grid.cells[0][0].try_accept(ItemType::CopperOre);
            }
            grid.place_smelter(5, 5, 60);
            let mut force = Force::new();
            let m = force.hire_raw(11, 1000).unwrap();
            let s = force.hire_raw(12, 130).unwrap(); // will rest and resume
            let t = force.hire_raw(13, 1000).unwrap();
            let h = force.hire_raw(14, 900).unwrap();
            assert!(force.assign(m, WorkerRole::Miner, 0, 2, &mut grid));
            assert!(force.assign(s, WorkerRole::SmelterOperator, 5, 2, &mut grid));
            assert!(force.assign(t, WorkerRole::Technician, 4, 2, &mut grid));
            assert!(force.assign_hauler(h, 0, 0, 5, 5, &mut grid));
            force.workers[m].skill = skill;
            (force, grid)
        }

        let (mut fa, mut ga) = setup(0);
        let (mut fb, mut gb) = setup(0);
        assert_eq!(combined_hash(&fa, &ga), combined_hash(&fb, &gb));

        for t in 1..=100 {
            fa.step_with(&mut ga);
            fb.step_with(&mut gb);
            assert_eq!(combined_hash(&fa, &ga), combined_hash(&fb, &gb), "diverged at tick {t}");
            assert_eq!(fa, fb, "workforce diverged at tick {t}");
            assert_eq!(ga, gb, "grid diverged at tick {t}");
        }
        let master = combined_hash(&fa, &ga);

        // Fresh replay reproduces the identical master hash.
        let (mut fc, mut gc) = setup(0);
        fc.run_with(&mut gc, 100);
        assert_eq!(combined_hash(&fc, &gc), master);

        // The run exercised every subsystem.
        let r = fa.report();
        assert!(r.total_rest_ticks > 0, "smelter operator must have rested");
        assert!(r.total_hauled > 0, "hauler must have carried ore");
        assert!(ga.total_smelted > 0);

        // A different skill level changes production -> different hash.
        let (mut fd, mut gd) = setup(MAX_SKILL);
        fd.run_with(&mut gd, 100);
        assert_ne!(combined_hash(&fd, &gd), master);
    }
}
