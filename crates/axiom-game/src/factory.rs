//! # Factory & Logistics System (Track 1 / T-040)
//!
//! Autonomous resource extraction and automated logistics base-building loop:
//!
//! ```text
//! [OreDeposit] -> MiningDrill -> Belt -> Belt -> Inserter -> Smelter -> Inserter -> Chest
//!                 (extract)     (transport)     (feed)      (refine)   (unload)   (store)
//! ```
//!
//! Design axioms:
//! - **Absolute determinism**: every transition is `next = f(state)` on integer ticks.
//!   No floating point, no RNG, no wall clock.
//! - **Cache-line resident**: every grid cell is one [`FactoryStructure`] that is exactly
//!   64 bytes and 64-byte aligned, so a tile occupies exactly one L1 cache line.
//! - **Zero heap in the hot loop**: [`FactoryGrid::step`] uses only fixed-size stack
//!   buffers; it never allocates.

use axiom_core::deterministic_hash;

use crate::defense::{BaseHeart, LaserTurret, Turret, Wall, TURRET_AMMO_CAP};

// ---------------------------------------------------------------------------
// Items
// ---------------------------------------------------------------------------

/// Types of resources and items in the game.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ItemType {
    #[default]
    None = 0,
    IronOre = 1,
    CopperOre = 2,
    Coal = 3,
    Stone = 4,
    IronPlate = 10,
    CopperPlate = 11,
    StoneBrick = 12,
}

impl ItemType {
    /// Returns the smelting product for a raw input, or `None` if the item cannot be smelted.
    #[inline(always)]
    pub const fn smelt_product(self) -> Option<ItemType> {
        match self {
            ItemType::IronOre => Some(ItemType::IronPlate),
            ItemType::CopperOre => Some(ItemType::CopperPlate),
            ItemType::Stone => Some(ItemType::StoneBrick),
            _ => None,
        }
    }

    #[inline(always)]
    pub const fn is_none(self) -> bool {
        matches!(self, ItemType::None)
    }

    /// Turret damage multiplier in percent for this item as ammo; `0` = not ammo.
    #[inline(always)]
    pub const fn ammo_damage_pct(self) -> u16 {
        match self {
            ItemType::IronOre => 100,
            ItemType::IronPlate => 150,
            ItemType::CopperPlate => 200,
            _ => 0,
        }
    }

    /// Whether a turret can load this item.
    #[inline(always)]
    pub const fn is_ammo(self) -> bool {
        self.ammo_damage_pct() > 0
    }
}

// ---------------------------------------------------------------------------
// Direction
// ---------------------------------------------------------------------------

/// Cardinal facing of a structure (output side for drills/belts/inserters).
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Direction {
    #[default]
    North = 0,
    East = 1,
    South = 2,
    West = 3,
}

impl Direction {
    /// The opposite facing (used by inserters to locate their pickup side).
    #[inline(always)]
    pub const fn opposite(self) -> Self {
        match self {
            Direction::North => Direction::South,
            Direction::East => Direction::West,
            Direction::South => Direction::North,
            Direction::West => Direction::East,
        }
    }

    /// Integer tile offset `(dx, dy)` for this direction. North is `-y`.
    #[inline(always)]
    pub const fn offset(self) -> (i32, i32) {
        match self {
            Direction::North => (0, -1),
            Direction::East => (1, 0),
            Direction::South => (0, 1),
            Direction::West => (-1, 0),
        }
    }
}

// ---------------------------------------------------------------------------
// Structure payloads
// ---------------------------------------------------------------------------

/// Work-cycle scale: a machine completes one action when `progress >= CYCLE`.
/// With `speed = 100` a machine acts every tick; `speed = 50` every 2 ticks.
pub const CYCLE: u16 = 100;

/// Maximum ore a smelter buffers in its input slot.
pub const SMELTER_INPUT_CAP: u8 = 8;
/// Maximum plates a smelter buffers in its output slot before it stalls.
pub const SMELTER_OUTPUT_CAP: u8 = 8;

/// A finite raw-resource patch in the ground.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct OreDeposit {
    pub item: ItemType,
    pub remaining: u32,
}

/// Extracts ore from the deposit it was built on and pushes it forward.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct MiningDrill {
    pub direction: Direction,
    pub ore: ItemType,
    pub speed: u16,
    pub progress: u16,
    /// Ore left in the patch beneath the drill.
    pub remaining: u32,
    /// Lifetime units extracted by this drill.
    pub mined: u32,
}

/// Single-lane conveyor: holds at most one item, advances it toward `direction`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Belt {
    pub direction: Direction,
    pub item: ItemType,
    pub speed: u16,
    pub progress: u16,
}

/// Arm that picks an item from the tile behind it and drops it on the tile in front.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Inserter {
    pub direction: Direction,
    pub held: ItemType,
    pub speed: u16,
    pub progress: u16,
    /// Lifetime items moved by this arm.
    pub moved: u32,
}

/// Furnace: converts raw ore to plates one unit at a time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Smelter {
    pub input: ItemType,
    pub output: ItemType,
    pub input_count: u8,
    pub output_count: u8,
    pub speed: u16,
    pub progress: u16,
    /// Lifetime plates produced.
    pub smelted: u32,
}

/// Passive storage; accepts any single item type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Chest {
    pub item: ItemType,
    pub quantity: u32,
}

/// Smart mechanical arm: only picks up items matching its designated `filter`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct FilterInserter {
    pub direction: Direction,
    pub held: ItemType,
    pub filter: ItemType,
    pub _pad: u8,
    pub speed: u16,
    pub progress: u16,
    pub moved: u32,
}

impl FilterInserter {
    pub const fn new(direction: Direction, filter: ItemType) -> Self {
        Self {
            direction,
            held: ItemType::None,
            filter,
            _pad: 0,
            speed: 50,
            progress: 0,
            moved: 0,
        }
    }
}

/// Tagged structure payload. All variants are plain `Copy` data (no pointers).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum StructureKind {
    #[default]
    Empty,
    Deposit(OreDeposit),
    Drill(MiningDrill),
    Belt(Belt),
    Inserter(Inserter),
    Smelter(Smelter),
    Chest(Chest),
    /// Defence barrier (T-043, see [`crate::defense`]).
    Wall(Wall),
    /// Ammo-fed automatic gun (T-043).
    Turret(Turret),
    /// Defence objective (T-043).
    BaseHeart(BaseHeart),
    /// 2x Speed Conveyor Belt (T-064).
    FastBelt(Belt),
    /// Item-filtering Smart Inserter (T-064).
    FilterInserter(FilterInserter),
    /// High-tech Energy Beam Projector (T-064).
    LaserTurret(LaserTurret),
}

// ---------------------------------------------------------------------------
// 64-byte cache-line cell
// ---------------------------------------------------------------------------

/// Padding so that the cell is exactly one 64-byte cache line.
/// `StructureKind` is 20 bytes (tag + largest 16-byte payload: `MiningDrill` / `Turret`);
/// 20 + 4 + 40 = 64.
const STRUCTURE_RESERVED: usize = 40;

/// One factory tile. Exactly 64 bytes, 64-byte aligned (one L1 cache line per tile).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(C, align(64))]
pub struct FactoryStructure {
    pub kind: StructureKind,
    /// Tick at which this structure last performed an action (debug / telemetry).
    pub last_action_tick: u32,
    pub _reserved: [u8; STRUCTURE_RESERVED],
}

impl Default for FactoryStructure {
    #[inline(always)]
    fn default() -> Self {
        Self::empty()
    }
}

impl FactoryStructure {
    pub const fn empty() -> Self {
        Self {
            kind: StructureKind::Empty,
            last_action_tick: 0,
            _reserved: [0; STRUCTURE_RESERVED],
        }
    }

    pub const fn with_kind(kind: StructureKind) -> Self {
        Self {
            kind,
            last_action_tick: 0,
            _reserved: [0; STRUCTURE_RESERVED],
        }
    }

    #[inline(always)]
    pub const fn is_empty(&self) -> bool {
        matches!(self.kind, StructureKind::Empty)
    }

    /// Attempts to push `item` into this structure (belt slot, smelter input, chest).
    /// Returns `true` on success; the cell is left untouched on failure.
    #[inline]
    pub(crate) fn try_accept(&mut self, item: ItemType) -> bool {
        match &mut self.kind {
            StructureKind::Belt(b) | StructureKind::FastBelt(b) => {
                if b.item.is_none() {
                    b.item = item;
                    b.progress = 0;
                    true
                } else {
                    false
                }
            }
            StructureKind::Smelter(s) => {
                if item.smelt_product().is_none() {
                    return false;
                }
                let compatible = s.input_count == 0 || s.input == item;
                if compatible && s.input_count < SMELTER_INPUT_CAP {
                    s.input = item;
                    s.input_count += 1;
                    true
                } else {
                    false
                }
            }
            StructureKind::Chest(c) if c.quantity == 0 || c.item == item => {
                c.item = item;
                c.quantity = c.quantity.saturating_add(1);
                true
            }
            StructureKind::Turret(t) => {
                if !item.is_ammo() {
                    return false;
                }
                let compatible = t.ammo == 0 || t.ammo_item == item;
                if compatible && t.ammo < TURRET_AMMO_CAP {
                    t.ammo_item = item;
                    t.ammo += 1;
                    true
                } else {
                    false
                }
            }
            _ => false,
        }
    }

    /// Attempts to take one item out of this structure (belt slot, smelter output, chest).
    #[inline]
    pub(crate) fn try_take(&mut self) -> ItemType {
        match &mut self.kind {
            StructureKind::Belt(b) | StructureKind::FastBelt(b) => {
                let item = b.item;
                if !item.is_none() {
                    b.item = ItemType::None;
                    b.progress = 0;
                }
                item
            }
            StructureKind::Smelter(s) => {
                if s.output_count > 0 {
                    s.output_count -= 1;
                    let item = s.output;
                    if s.output_count == 0 {
                        s.output = ItemType::None;
                    }
                    item
                } else {
                    ItemType::None
                }
            }
            StructureKind::Chest(c) => {
                if c.quantity > 0 {
                    c.quantity -= 1;
                    let item = c.item;
                    if c.quantity == 0 {
                        c.item = ItemType::None;
                    }
                    item
                } else {
                    ItemType::None
                }
            }
            _ => ItemType::None,
        }
    }

    /// Peeks at the item available to take without removing it.
    #[inline]
    pub(crate) fn peek_item(&self) -> ItemType {
        match &self.kind {
            StructureKind::Belt(b) | StructureKind::FastBelt(b) => b.item,
            StructureKind::Smelter(s) => {
                if s.output_count > 0 {
                    s.output
                } else {
                    ItemType::None
                }
            }
            StructureKind::Chest(c) => {
                if c.quantity > 0 {
                    c.item
                } else {
                    ItemType::None
                }
            }
            _ => ItemType::None,
        }
    }
}

// ---------------------------------------------------------------------------
// Grid
// ---------------------------------------------------------------------------

pub const DEFAULT_FACTORY_W: usize = 32;
pub const DEFAULT_FACTORY_H: usize = 32;

/// Upper bound on belt hand-offs scheduled in one tick (fixed stack buffer, no heap).
pub const MAX_BELT_TRANSFERS: usize = 1024;

/// Aggregate production counters, snapshotted via [`FactoryGrid::report`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct FactoryReport {
    pub tick: u64,
    pub total_mined: u64,
    pub total_smelted: u64,
    pub total_stored: u64,
    pub items_on_belts: u32,
    pub items_held_by_inserters: u32,
    pub ore_in_smelters: u32,
    pub plates_in_smelters: u32,
}

/// 2D factory grid simulating deposits, drills, belts, inserters, smelters and chests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FactoryGrid<const W: usize = DEFAULT_FACTORY_W, const H: usize = DEFAULT_FACTORY_H> {
    pub cells: [[FactoryStructure; W]; H],
    pub current_tick: u64,
    pub total_mined: u64,
    pub total_smelted: u64,
    pub total_stored: u64,
}

impl<const W: usize, const H: usize> Default for FactoryGrid<W, H> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const W: usize, const H: usize> FactoryGrid<W, H> {
    pub const fn new() -> Self {
        Self {
            cells: [[FactoryStructure::empty(); W]; H],
            current_tick: 0,
            total_mined: 0,
            total_smelted: 0,
            total_stored: 0,
        }
    }

    // ---- placement -------------------------------------------------------

    #[inline(always)]
    pub(crate) fn set(&mut self, x: usize, y: usize, kind: StructureKind) {
        if x < W && y < H {
            self.cells[y][x] = FactoryStructure::with_kind(kind);
        }
    }

    /// Places a finite ore patch.
    pub fn place_deposit(&mut self, x: usize, y: usize, item: ItemType, amount: u32) {
        self.set(x, y, StructureKind::Deposit(OreDeposit { item, remaining: amount }));
    }

    /// Places a drill. If the tile currently holds a deposit, the drill absorbs its reserve;
    /// otherwise the drill has nothing to mine (`remaining = 0`).
    pub fn place_drill(&mut self, x: usize, y: usize, direction: Direction, speed: u16) {
        if x >= W || y >= H {
            return;
        }
        let (ore, remaining) = match self.cells[y][x].kind {
            StructureKind::Deposit(d) => (d.item, d.remaining),
            _ => (ItemType::None, 0),
        };
        self.set(
            x,
            y,
            StructureKind::Drill(MiningDrill {
                direction,
                ore,
                speed,
                progress: 0,
                remaining,
                mined: 0,
            }),
        );
    }

    pub fn place_belt(&mut self, x: usize, y: usize, direction: Direction, speed: u16) {
        self.set(
            x,
            y,
            StructureKind::Belt(Belt {
                direction,
                item: ItemType::None,
                speed,
                progress: 0,
            }),
        );
    }

    /// Places a 2x-speed fast conveyor belt (speed: 100, moves 1 item per tick).
    pub fn place_fast_belt(&mut self, x: usize, y: usize, direction: Direction) {
        self.set(
            x,
            y,
            StructureKind::FastBelt(Belt {
                direction,
                item: ItemType::None,
                speed: 100,
                progress: 0,
            }),
        );
    }

    pub fn place_inserter(&mut self, x: usize, y: usize, direction: Direction, speed: u16) {
        self.set(
            x,
            y,
            StructureKind::Inserter(Inserter {
                direction,
                held: ItemType::None,
                speed,
                progress: 0,
                moved: 0,
            }),
        );
    }

    /// Places an item-selective filter inserter.
    pub fn place_filter_inserter(
        &mut self,
        x: usize,
        y: usize,
        direction: Direction,
        filter: ItemType,
    ) {
        self.set(
            x,
            y,
            StructureKind::FilterInserter(FilterInserter::new(direction, filter)),
        );
    }

    pub fn place_smelter(&mut self, x: usize, y: usize, speed: u16) {
        self.set(
            x,
            y,
            StructureKind::Smelter(Smelter {
                input: ItemType::None,
                output: ItemType::None,
                input_count: 0,
                output_count: 0,
                speed,
                progress: 0,
                smelted: 0,
            }),
        );
    }

    pub fn place_chest(&mut self, x: usize, y: usize) {
        self.set(
            x,
            y,
            StructureKind::Chest(Chest {
                item: ItemType::None,
                quantity: 0,
            }),
        );
    }

    // ---- queries ---------------------------------------------------------

    /// Neighbour coordinate in `dir`, or `None` at the grid edge.
    #[inline(always)]
    pub fn neighbor(x: usize, y: usize, dir: Direction) -> Option<(usize, usize)> {
        let (dx, dy) = dir.offset();
        let nx = x as i64 + dx as i64;
        let ny = y as i64 + dy as i64;
        if nx >= 0 && ny >= 0 && (nx as usize) < W && (ny as usize) < H {
            Some((nx as usize, ny as usize))
        } else {
            None
        }
    }

    #[inline(always)]
    pub fn get(&self, x: usize, y: usize) -> Option<&FactoryStructure> {
        if x < W && y < H {
            Some(&self.cells[y][x])
        } else {
            None
        }
    }

    /// Item type and quantity stored in a chest at `(x, y)`.
    pub fn chest_contents(&self, x: usize, y: usize) -> (ItemType, u32) {
        match self.get(x, y).map(|c| c.kind) {
            Some(StructureKind::Chest(c)) => (c.item, c.quantity),
            _ => (ItemType::None, 0),
        }
    }

    /// Snapshot of aggregate counters plus in-flight inventory (used for conservation checks).
    pub fn report(&self) -> FactoryReport {
        let mut r = FactoryReport {
            tick: self.current_tick,
            total_mined: self.total_mined,
            total_smelted: self.total_smelted,
            total_stored: self.total_stored,
            ..FactoryReport::default()
        };
        for row in &self.cells {
            for cell in row {
                match cell.kind {
                    StructureKind::Belt(b) | StructureKind::FastBelt(b)
                        if !b.item.is_none() =>
                    {
                        r.items_on_belts += 1
                    }
                    StructureKind::Inserter(i) if !i.held.is_none() => {
                        r.items_held_by_inserters += 1
                    }
                    StructureKind::FilterInserter(fi) if !fi.held.is_none() => {
                        r.items_held_by_inserters += 1
                    }
                    StructureKind::Smelter(s) => {
                        r.ore_in_smelters += s.input_count as u32;
                        r.plates_in_smelters += s.output_count as u32;
                    }
                    _ => {}
                }
            }
        }
        r
    }

    /// Deterministic FNV-1a hash of the full grid state (non-empty tiles + counters).
    pub fn compute_hash(&self) -> u64 {
        let mut h = self.current_tick.wrapping_mul(0x9E37_79B9_7F4A_7C15)
            ^ self.total_mined.rotate_left(17)
            ^ self.total_smelted.rotate_left(34)
            ^ self.total_stored.rotate_left(51);
        for row in &self.cells {
            for cell in row {
                if !cell.is_empty() {
                    h ^= deterministic_hash(&cell.kind);
                    h = h.rotate_left(5).wrapping_mul(0x0000_0100_0000_01B3);
                }
            }
        }
        h
    }

    // ---- simulation ------------------------------------------------------

    /// Advances the factory by exactly one tick. Heap-free; fixed phase order:
    ///
    /// 1. **Belts** advance and hand items forward (two-phase: schedule, then apply).
    /// 2. **Inserters** pick from behind / drop in front.
    /// 3. **Drills** extract ore and push it forward.
    /// 4. **Smelters** convert buffered ore into plates.
    pub fn step(&mut self) {
        self.current_tick += 1;
        let tick = self.current_tick as u32;

        self.step_belts(tick);
        self.step_inserters(tick);
        self.step_drills(tick);
        self.step_smelters(tick);
    }

    /// Runs `n` ticks.
    pub fn run(&mut self, n: u32) {
        for _ in 0..n {
            self.step();
        }
    }

    fn step_belts(&mut self, tick: u32) {
        // (sx, sy, tx, ty) -- fixed stack buffer; no heap.
        let mut transfers = [(0u16, 0u16, 0u16, 0u16); MAX_BELT_TRANSFERS];
        let mut count = 0usize;

        for y in 0..H {
            for x in 0..W {
                if let StructureKind::Belt(b) | StructureKind::FastBelt(b) =
                    &mut self.cells[y][x].kind
                {
                    if b.item.is_none() {
                        continue;
                    }
                    b.progress = b.progress.saturating_add(b.speed).min(CYCLE);
                    if b.progress >= CYCLE && count < MAX_BELT_TRANSFERS {
                        if let Some((tx, ty)) = Self::neighbor(x, y, b.direction) {
                            transfers[count] = (x as u16, y as u16, tx as u16, ty as u16);
                            count += 1;
                        }
                    }
                }
            }
        }

        for &(sx, sy, tx, ty) in transfers[..count].iter() {
            let (sx, sy, tx, ty) = (sx as usize, sy as usize, tx as usize, ty as usize);
            let item = match self.cells[sy][sx].kind {
                StructureKind::Belt(b) | StructureKind::FastBelt(b) => b.item,
                _ => continue,
            };
            if item.is_none() {
                continue;
            }
            // Belts may only feed belts, chests and turret magazines directly
            // (inserters feed machines).
            let target_ok = matches!(
                self.cells[ty][tx].kind,
                StructureKind::Belt(_)
                    | StructureKind::FastBelt(_)
                    | StructureKind::Chest(_)
                    | StructureKind::Turret(_)
            );
            if !target_ok {
                continue;
            }
            let was_chest = matches!(self.cells[ty][tx].kind, StructureKind::Chest(_));
            if self.cells[ty][tx].try_accept(item) {
                self.cells[sy][sx].try_take();
                self.cells[sy][sx].last_action_tick = tick;
                self.cells[ty][tx].last_action_tick = tick;
                if was_chest {
                    self.total_stored += 1;
                }
            }
        }
    }

    fn step_inserters(&mut self, tick: u32) {
        for y in 0..H {
            for x in 0..W {
                match self.cells[y][x].kind {
                    StructureKind::Inserter(mut ins) => {
                        ins.progress = ins.progress.saturating_add(ins.speed).min(CYCLE);
                        if ins.progress >= CYCLE {
                            if ins.held.is_none() {
                                // Pick up from the tile behind.
                                if let Some((bx, by)) =
                                    Self::neighbor(x, y, ins.direction.opposite())
                                {
                                    let item = self.cells[by][bx].try_take();
                                    if !item.is_none() {
                                        ins.held = item;
                                        ins.progress = 0;
                                        self.cells[by][bx].last_action_tick = tick;
                                        self.cells[y][x].last_action_tick = tick;
                                    }
                                }
                            } else if let Some((fx, fy)) = Self::neighbor(x, y, ins.direction) {
                                // Drop onto the tile in front.
                                let was_chest =
                                    matches!(self.cells[fy][fx].kind, StructureKind::Chest(_));
                                if self.cells[fy][fx].try_accept(ins.held) {
                                    ins.held = ItemType::None;
                                    ins.progress = 0;
                                    ins.moved = ins.moved.saturating_add(1);
                                    self.cells[fy][fx].last_action_tick = tick;
                                    self.cells[y][x].last_action_tick = tick;
                                    if was_chest {
                                        self.total_stored += 1;
                                    }
                                }
                            }
                        }
                        self.cells[y][x].kind = StructureKind::Inserter(ins);
                    }
                    StructureKind::FilterInserter(mut fi) => {
                        fi.progress = fi.progress.saturating_add(fi.speed).min(CYCLE);
                        if fi.progress >= CYCLE {
                            if fi.held.is_none() {
                                // Pick up only matching filter from behind
                                if let Some((bx, by)) =
                                    Self::neighbor(x, y, fi.direction.opposite())
                                {
                                    let cand = self.cells[by][bx].peek_item();
                                    if !cand.is_none()
                                        && (fi.filter == ItemType::None || fi.filter == cand)
                                    {
                                        let item = self.cells[by][bx].try_take();
                                        if !item.is_none() {
                                            fi.held = item;
                                            fi.progress = 0;
                                            self.cells[by][bx].last_action_tick = tick;
                                            self.cells[y][x].last_action_tick = tick;
                                        }
                                    }
                                }
                            } else if let Some((fx, fy)) = Self::neighbor(x, y, fi.direction) {
                                // Drop onto the tile in front
                                let was_chest =
                                    matches!(self.cells[fy][fx].kind, StructureKind::Chest(_));
                                if self.cells[fy][fx].try_accept(fi.held) {
                                    fi.held = ItemType::None;
                                    fi.progress = 0;
                                    fi.moved = fi.moved.saturating_add(1);
                                    self.cells[fy][fx].last_action_tick = tick;
                                    self.cells[y][x].last_action_tick = tick;
                                    if was_chest {
                                        self.total_stored += 1;
                                    }
                                }
                            }
                        }
                        self.cells[y][x].kind = StructureKind::FilterInserter(fi);
                    }
                    _ => {}
                }
            }
        }
    }

    fn step_drills(&mut self, tick: u32) {
        for y in 0..H {
            for x in 0..W {
                let mut drill = match self.cells[y][x].kind {
                    StructureKind::Drill(d) => d,
                    _ => continue,
                };
                if drill.remaining == 0 || drill.ore.is_none() {
                    continue;
                }
                drill.progress = drill.progress.saturating_add(drill.speed).min(CYCLE);
                if drill.progress >= CYCLE {
                    if let Some((tx, ty)) = Self::neighbor(x, y, drill.direction) {
                        let was_chest = matches!(self.cells[ty][tx].kind, StructureKind::Chest(_));
                        if self.cells[ty][tx].try_accept(drill.ore) {
                            drill.remaining -= 1;
                            drill.mined = drill.mined.saturating_add(1);
                            drill.progress = 0;
                            self.total_mined += 1;
                            self.cells[ty][tx].last_action_tick = tick;
                            self.cells[y][x].last_action_tick = tick;
                            if was_chest {
                                self.total_stored += 1;
                            }
                        }
                    }
                }
                self.cells[y][x].kind = StructureKind::Drill(drill);
            }
        }
    }

    fn step_smelters(&mut self, tick: u32) {
        for y in 0..H {
            for x in 0..W {
                if let StructureKind::Smelter(s) = &mut self.cells[y][x].kind {
                    if s.input_count == 0 {
                        continue;
                    }
                    let product = match s.input.smelt_product() {
                        Some(p) => p,
                        None => continue,
                    };
                    let output_ok = s.output_count == 0 || s.output == product;
                    if !output_ok || s.output_count >= SMELTER_OUTPUT_CAP {
                        continue; // stalled: output slot full or incompatible
                    }
                    s.progress = s.progress.saturating_add(s.speed).min(CYCLE);
                    if s.progress >= CYCLE {
                        s.input_count -= 1;
                        if s.input_count == 0 {
                            s.input = ItemType::None;
                        }
                        s.output = product;
                        s.output_count += 1;
                        s.smelted = s.smelted.saturating_add(1);
                        s.progress = 0;
                        self.total_smelted += 1;
                        self.cells[y][x].last_action_tick = tick;
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::{align_of, size_of};

    /// Builds the canonical T-040 production line on row 2:
    ///
    /// ```text
    /// x: 0      1     2     3     4         5        6         7
    ///    Drill> Belt> Belt> Belt> Inserter> Smelter  Inserter> Chest
    /// ```
    fn build_line(grid: &mut FactoryGrid<16, 8>, reserve: u32) {
        let y = 2;
        grid.place_deposit(0, y, ItemType::IronOre, reserve);
        grid.place_drill(0, y, Direction::East, 50); // 1 ore / 2 ticks
        grid.place_belt(1, y, Direction::East, 100);
        grid.place_belt(2, y, Direction::East, 100);
        grid.place_belt(3, y, Direction::East, 100);
        grid.place_inserter(4, y, Direction::East, 100);
        grid.place_smelter(5, y, 50); // 1 plate / 2 ticks
        grid.place_inserter(6, y, Direction::East, 100);
        grid.place_chest(7, y);
    }

    #[test]
    fn test_factory_structure_is_one_cache_line() {
        assert_eq!(size_of::<FactoryStructure>(), 64, "must be exactly 64 bytes");
        assert_eq!(align_of::<FactoryStructure>(), 64, "must be 64-byte aligned");
        assert!(size_of::<StructureKind>() <= 64 - 4 - STRUCTURE_RESERVED);
    }

    #[test]
    fn test_direction_helpers() {
        assert_eq!(Direction::North.opposite(), Direction::South);
        assert_eq!(Direction::East.opposite(), Direction::West);
        assert_eq!(FactoryGrid::<4, 4>::neighbor(0, 0, Direction::North), None);
        assert_eq!(FactoryGrid::<4, 4>::neighbor(0, 0, Direction::West), None);
        assert_eq!(FactoryGrid::<4, 4>::neighbor(3, 3, Direction::East), None);
        assert_eq!(FactoryGrid::<4, 4>::neighbor(1, 1, Direction::South), Some((1, 2)));
    }

    #[test]
    fn test_mine_belt_insert_smelt_store_100_ticks() {
        let mut grid = FactoryGrid::<16, 8>::new();
        build_line(&mut grid, 1_000);
        grid.run(100);

        let (item, qty) = grid.chest_contents(7, 2);
        assert_eq!(item, ItemType::IronPlate, "chest must contain iron plates");
        assert!(qty >= 30, "100 ticks should yield a steady plate flow, got {qty}");
        assert_eq!(qty as u64, grid.total_stored);

        // Conservation: every mined ore is either in flight, in a furnace, or became a plate.
        let r = grid.report();
        let ore_in_flight = r.items_on_belts as u64 + r.ore_in_smelters as u64;
        // Inserter 4 holds ore, inserter 6 holds plates; split by inspecting each.
        let held_ore = match grid.cells[2][4].kind {
            StructureKind::Inserter(i) if i.held == ItemType::IronOre => 1u64,
            _ => 0,
        };
        let held_plate = match grid.cells[2][6].kind {
            StructureKind::Inserter(i) if i.held == ItemType::IronPlate => 1u64,
            _ => 0,
        };
        assert_eq!(
            r.total_mined,
            ore_in_flight + held_ore + r.total_smelted,
            "ore must be conserved: {r:?}"
        );
        assert_eq!(
            r.total_smelted,
            r.plates_in_smelters as u64 + held_plate + r.total_stored,
            "plates must be conserved: {r:?}"
        );
        assert!(r.total_mined >= r.total_smelted && r.total_smelted >= r.total_stored);
    }

    #[test]
    fn test_100_tick_loop_is_bit_identical_across_runs() {
        let mut a = FactoryGrid::<16, 8>::new();
        let mut b = FactoryGrid::<16, 8>::new();
        build_line(&mut a, 1_000);
        build_line(&mut b, 1_000);
        assert_eq!(a.compute_hash(), b.compute_hash());

        for t in 1..=100 {
            a.step();
            b.step();
            assert_eq!(a.compute_hash(), b.compute_hash(), "diverged at tick {t}");
            assert_eq!(a, b);
        }

        // Replaying the same 100 ticks from a clone reproduces the identical hash.
        let mut c = FactoryGrid::<16, 8>::new();
        build_line(&mut c, 1_000);
        c.run(100);
        assert_eq!(c.compute_hash(), a.compute_hash());

        // A different layout must not collide.
        let mut d = FactoryGrid::<16, 8>::new();
        build_line(&mut d, 999);
        d.run(100);
        assert_ne!(d.compute_hash(), a.compute_hash());
    }

    #[test]
    fn test_drill_stops_when_deposit_is_depleted() {
        let mut grid = FactoryGrid::<16, 8>::new();
        build_line(&mut grid, 5);
        grid.run(100);
        assert_eq!(grid.total_mined, 5);
        assert_eq!(grid.total_smelted, 5);
        assert_eq!(grid.chest_contents(7, 2), (ItemType::IronPlate, 5));
        match grid.cells[2][0].kind {
            StructureKind::Drill(d) => assert_eq!(d.remaining, 0),
            other => panic!("expected drill, got {other:?}"),
        }
    }

    #[test]
    fn test_belt_backpressure_never_loses_items() {
        // Drill -> belt -> belt -> (grid edge). Items must pile up, never vanish.
        let mut grid = FactoryGrid::<4, 2>::new();
        grid.place_deposit(0, 0, ItemType::CopperOre, 100);
        grid.place_drill(0, 0, Direction::East, 100);
        grid.place_belt(1, 0, Direction::East, 100);
        grid.place_belt(2, 0, Direction::East, 100);
        grid.place_belt(3, 0, Direction::East, 100); // faces off-grid: blocked
        grid.run(50);
        let r = grid.report();
        assert_eq!(r.total_mined, 3, "only three belt slots exist, drill must stall");
        assert_eq!(r.items_on_belts, 3);
    }

    #[test]
    fn test_smelter_rejects_non_smeltable_and_stalls_when_output_full() {
        let mut cell = FactoryStructure::with_kind(StructureKind::Smelter(Smelter {
            speed: 100,
            ..Smelter::default()
        }));
        assert!(!cell.try_accept(ItemType::IronPlate), "plates are not smeltable");
        for _ in 0..SMELTER_INPUT_CAP {
            assert!(cell.try_accept(ItemType::IronOre));
        }
        assert!(!cell.try_accept(ItemType::IronOre), "input cap reached");
        assert!(!cell.try_accept(ItemType::CopperOre), "mixed ore rejected");

        // Smelter with no unloader: output fills to cap and then stalls.
        let mut grid = FactoryGrid::<2, 1>::new();
        grid.place_deposit(0, 0, ItemType::IronOre, 100);
        grid.place_drill(0, 0, Direction::East, 100);
        grid.place_smelter(1, 0, 100);
        grid.run(100);
        match grid.cells[0][1].kind {
            StructureKind::Smelter(s) => {
                assert_eq!(s.output_count, SMELTER_OUTPUT_CAP);
                assert_eq!(s.input_count, SMELTER_INPUT_CAP);
                assert_eq!(s.output, ItemType::IronPlate);
            }
            other => panic!("expected smelter, got {other:?}"),
        }
        assert_eq!(
            grid.total_smelted,
            SMELTER_OUTPUT_CAP as u64,
            "smelter must stall once its output buffer is full"
        );
    }

    #[test]
    fn test_fast_belt_double_throughput() {
        // Normal belt (speed 50) takes 2 ticks per tile.
        // Fast belt (speed 100) takes 1 tick per tile.
        let mut grid = FactoryGrid::<3, 1>::new();
        grid.place_fast_belt(0, 0, Direction::East);
        grid.place_fast_belt(1, 0, Direction::East);
        grid.place_chest(2, 0);

        assert!(grid.cells[0][0].try_accept(ItemType::IronOre));
        assert_eq!(grid.chest_contents(2, 0).1, 0);

        // Tick 1: moves from (0,0) to (1,0)
        grid.step();
        assert_eq!(grid.chest_contents(2, 0).1, 0);

        // Tick 2: moves from (1,0) to (2,0) chest
        grid.step();
        assert_eq!(grid.chest_contents(2, 0), (ItemType::IronOre, 1));
    }

    #[test]
    fn test_filter_inserter_selectivity() {
        let mut grid = FactoryGrid::<3, 1>::new();
        // Chest 0 with CopperOre, FilterInserter at (1,0) configured only for IronOre, Chest at (2,0)
        grid.place_chest(0, 0);
        grid.cells[0][0].try_accept(ItemType::CopperOre);
        grid.place_filter_inserter(1, 0, Direction::East, ItemType::IronOre);
        grid.place_chest(2, 0);

        // Filter inserter must NOT pick up CopperOre!
        grid.run(10);
        assert_eq!(grid.chest_contents(2, 0).1, 0, "filter inserter must ignore copper");
        assert_eq!(grid.chest_contents(0, 0).1, 1, "copper must remain in source chest");

        // Empty chest 0 and replace with IronOre
        assert_eq!(grid.cells[0][0].try_take(), ItemType::CopperOre);
        assert!(grid.cells[0][0].try_accept(ItemType::IronOre));
        grid.run(4);
        assert_eq!(grid.chest_contents(2, 0), (ItemType::IronOre, 1), "iron ore must be transferred");
    }
}
