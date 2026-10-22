//! Native Desktop Window Studio for Axiom Engine.
//!
//! Provides a 60 FPS interactive graphical window powered by `minifb` and `axiom-render`.
//! Complete interactive factory defense simulation with real-time mouse and keyboard controls.

use std::cell::RefCell;
use std::io::Write;
use std::time::Duration;

use minifb::{Key, KeyRepeat, MouseButton, MouseMode, Window, WindowOptions};

use axiom_core::ChunkCoord;
use axiom_game::{
    generate_ore_veins, Chest, Direction, FactoryGrid, FactoryStructure, ItemType,
    StructureKind, WaveDirector, MONSTER_TEAM, TURRET_AMMO_CAP,
};
use axiom_render::{
    get_belt_sprite, get_drill_sprite, get_heart_sprite, get_inserter_sprite,
    get_monster_sprite, get_smelter_sprite, get_turret_sprite, FrameBuffer,
    CHEST_SPRITE, DIR_EAST, ORE_DEPOSIT, WALL_SPRITE,
};

pub const WIN_WIDTH: usize = 704;
pub const WIN_HEIGHT: usize = 544;
pub const GRID_W: usize = 32;
pub const GRID_H: usize = 32;
pub const TILE_SIZE: i32 = 16;
pub const GRID_PIXEL_SIZE: i32 = GRID_W as i32 * TILE_SIZE; // 512 px
pub const GRID_OFFSET_X: i32 = 96; // 96 px left sidebar
pub const GRID_OFFSET_Y: i32 = (WIN_HEIGHT as i32 - GRID_PIXEL_SIZE) / 2; // 16 px
pub const RIGHT_SIDEBAR_X: i32 = GRID_OFFSET_X + GRID_PIXEL_SIZE; // 608 px

pub const TICK_RATE_HZ: u64 = 60;
pub const DT_FIXED: Duration = Duration::from_nanos(1_000_000_000 / TICK_RATE_HZ); // 16,666,666 ns
pub const MAX_ACCUMULATOR: Duration = Duration::from_millis(100); // 100ms clamp prevents spiral of death

pub const SAVE_MAGIC: u64 = 0x4D4F_4958_4156_4153; // "SAVAXIOM"
pub const SAVE_VERSION: u32 = 0x0001_0300; // v1.3.0

/// 64-byte aligned binary header for Axiom Desktop saves.
#[repr(C, align(64))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SaveFileHeader {
    pub magic: u64,
    pub version: u32,
    pub _pad0: u32,
    pub tick: u64,
    pub heart_hp: i32,
    pub heart_max_hp: i32,
    pub wave: u32,
    pub _pad1: u32,
    pub total_smelted: u64,
    pub checksum: u64,
    pub reserved: [u8; 8],
}

const _: () = assert!(core::mem::size_of::<SaveFileHeader>() == 64);
const _: () = assert!(core::mem::align_of::<SaveFileHeader>() == 64);

/// Computes a standard 64-bit FNV-1a checksum over a byte slice.
pub fn fnv1a_hash(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

/// Real-time profiler metrics and telemetry snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfilerMetrics {
    pub sim_time_us: u64,
    pub render_time_us: u64,
    pub fps: u32,
    pub ups: u32,
    pub active_structures: usize,
    pub active_monsters: usize,
    pub active_items: usize,
    pub state_hash: u64,
    pub history: [u16; 60],
    pub history_idx: usize,
}

impl Default for ProfilerMetrics {
    fn default() -> Self {
        Self {
            sim_time_us: 0,
            render_time_us: 0,
            fps: 0,
            ups: 0,
            active_structures: 0,
            active_monsters: 0,
            active_items: 0,
            state_hash: 0,
            history: [0; 60],
            history_idx: 0,
        }
    }
}

impl ProfilerMetrics {
    /// Records a new frame time sample in microseconds into the 60-frame ring buffer.
    pub fn record_frame(&mut self, frame_time_us: u64) {
        self.history[self.history_idx] = frame_time_us.min(u16::MAX as u64) as u16;
        self.history_idx = (self.history_idx + 1) % 60;
    }
}

/// Native interactive desktop engine holding grid, wave director, and persistence.
pub struct DesktopEngine {
    pub grid: FactoryGrid<GRID_W, GRID_H>,
    pub director: WaveDirector<64>,
    pub tick: u64,
    pub notification: RefCell<Option<(String, u32)>>,
    pub show_profiler: bool,
    pub profiler: ProfilerMetrics,
    pub cursor_x: usize,
    pub cursor_y: usize,
    pub selected_tool: SelectedTool,
    pub current_direction: Direction,
}

impl DesktopEngine {
    /// Creates a new engine initialized with starter base and wave defense.
    pub fn new(seed: u64) -> Self {
        let mut grid = FactoryGrid::<GRID_W, GRID_H>::new();
        setup_starter_base(&mut grid, seed);

        let mut director = WaveDirector::<64>::new(seed);
        director.bind_chunk(ChunkCoord::new(0, 0));

        Self {
            grid,
            director,
            tick: 0,
            notification: RefCell::new(None),
            show_profiler: false,
            profiler: ProfilerMetrics::default(),
            cursor_x: 16,
            cursor_y: 16,
            selected_tool: SelectedTool::Turret,
            current_direction: Direction::East,
        }
    }

    /// Advances simulation by one deterministic tick.
    pub fn step(&mut self) {
        self.director.tick(&mut self.grid);
        self.grid.step();
        self.tick += 1;

        let mut notif = self.notification.borrow_mut();
        if let Some((_, ref mut count)) = *notif {
            if *count > 0 {
                *count -= 1;
            } else {
                *notif = None;
            }
        }
    }

    /// Retrieves current HUD notification message if active.
    pub fn notification(&self) -> Option<(String, u32)> {
        self.notification.borrow().clone()
    }

    /// Sets HUD notification with a display lifetime in frames.
    pub fn set_notification(&self, msg: impl Into<String>, duration_ticks: u32) {
        *self.notification.borrow_mut() = Some((msg.into(), duration_ticks));
    }

    /// Saves the current engine state (grid, monsters, wave director, stats) to a binary file.
    pub fn save_to_file(&self, path: &str) -> std::io::Result<()> {
        let _ = std::fs::create_dir_all("saves");
        if let Some(parent) = std::path::Path::new(path).parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }

        let d_rep = self.director.report(&self.grid);
        let f_rep = self.grid.report();

        let grid_size = core::mem::size_of::<FactoryGrid<GRID_W, GRID_H>>();
        let director_size = core::mem::size_of::<WaveDirector<64>>();
        let mut payload = Vec::with_capacity(grid_size + director_size);

        let grid_bytes = unsafe {
            core::slice::from_raw_parts(&self.grid as *const _ as *const u8, grid_size)
        };
        let director_bytes = unsafe {
            core::slice::from_raw_parts(&self.director as *const _ as *const u8, director_size)
        };
        payload.extend_from_slice(grid_bytes);
        payload.extend_from_slice(director_bytes);

        let checksum = fnv1a_hash(&payload);

        let header = SaveFileHeader {
            magic: SAVE_MAGIC,
            version: SAVE_VERSION,
            _pad0: 0,
            tick: self.tick,
            heart_hp: d_rep.heart_hp as i32,
            heart_max_hp: d_rep.heart_max_hp as i32,
            wave: d_rep.wave_number as u32,
            _pad1: 0,
            total_smelted: f_rep.total_smelted,
            checksum,
            reserved: [0; 8],
        };

        let header_bytes = unsafe {
            core::slice::from_raw_parts(
                &header as *const SaveFileHeader as *const u8,
                core::mem::size_of::<SaveFileHeader>(),
            )
        };

        let mut file = std::fs::File::create(path)?;
        file.write_all(header_bytes)?;
        file.write_all(&payload)?;
        file.flush()?;

        self.set_notification(format!("[QUICK SAVE: {}]", path), 120);
        Ok(())
    }

    /// Restores engine state from a binary save archive with full integrity validation.
    pub fn load_from_file(&mut self, path: &str) -> std::io::Result<()> {
        let bytes = std::fs::read(path)?;
        let header_size = core::mem::size_of::<SaveFileHeader>();
        let grid_size = core::mem::size_of::<FactoryGrid<GRID_W, GRID_H>>();
        let director_size = core::mem::size_of::<WaveDirector<64>>();
        let expected_size = header_size + grid_size + director_size;

        if bytes.len() < expected_size {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                format!(
                    "Corrupted save file: size {} bytes is less than expected {} bytes",
                    bytes.len(),
                    expected_size
                ),
            ));
        }

        let header = unsafe { core::ptr::read_unaligned(bytes.as_ptr() as *const SaveFileHeader) };
        if header.magic != SAVE_MAGIC {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "Invalid save magic: expected 0x{:X}, found 0x{:X}",
                    SAVE_MAGIC, header.magic
                ),
            ));
        }
        if header.version != SAVE_VERSION {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "Unsupported save version: expected 0x{:X}, found 0x{:X}",
                    SAVE_VERSION, header.version
                ),
            ));
        }

        let payload_bytes = &bytes[header_size..expected_size];
        let computed_checksum = fnv1a_hash(payload_bytes);
        if computed_checksum != header.checksum {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "Save checksum mismatch: expected 0x{:X}, computed 0x{:X}",
                    header.checksum, computed_checksum
                ),
            ));
        }

        self.grid = unsafe {
            core::ptr::read_unaligned(payload_bytes.as_ptr() as *const FactoryGrid<GRID_W, GRID_H>)
        };
        self.director = unsafe {
            core::ptr::read_unaligned(payload_bytes[grid_size..].as_ptr() as *const WaveDirector<64>)
        };
        self.tick = header.tick;

        self.set_notification(format!("[QUICK LOAD COMPLETED (Tick {})]", header.tick), 120);
        Ok(())
    }

    /// Computes deterministic state hash of the current engine (grid, director, tick).
    pub fn compute_state_hash(&self) -> u64 {
        self.grid.compute_hash()
            ^ self.director.compute_hash().rotate_left(17)
            ^ self.tick.rotate_left(37)
    }

    /// Updates profiler telemetry metrics with current simulation time, render time, FPS, and UPS.
    pub fn update_profiler_metrics(&mut self, sim_time_us: u64, render_time_us: u64, fps: u32, ups: u32) {
        let frame_time_us = sim_time_us.saturating_add(render_time_us);
        self.profiler.sim_time_us = sim_time_us;
        self.profiler.render_time_us = render_time_us;
        self.profiler.fps = if fps > 0 {
            fps
        } else if frame_time_us > 0 {
            (1_000_000 / frame_time_us).min(999) as u32
        } else {
            60
        };
        self.profiler.ups = if ups > 0 {
            ups
        } else {
            TICK_RATE_HZ as u32
        };
        self.profiler.record_frame(frame_time_us);

        let mut structures = 0;
        for row in &self.grid.cells {
            for cell in row {
                if !matches!(cell.kind, StructureKind::Empty) {
                    structures += 1;
                }
            }
        }
        self.profiler.active_structures = structures;

        let d_rep = self.director.report(&self.grid);
        self.profiler.active_monsters = d_rep.monsters_alive as usize;

        let f_rep = self.grid.report();
        self.profiler.active_items = (f_rep.items_on_belts
            + f_rep.items_held_by_inserters
            + f_rep.ore_in_smelters
            + f_rep.plates_in_smelters) as usize
            + f_rep.total_stored as usize;

        self.profiler.state_hash = self.compute_state_hash();
    }
}

/// User selectable build tools (keys 1~8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectedTool {
    Belt = 1,
    Drill = 2,
    Smelter = 3,
    Chest = 4,
    Wall = 5,
    Turret = 6,
    Inserter = 7,
    Clear = 8,
}

impl SelectedTool {
    pub const fn label(self) -> &'static str {
        match self {
            SelectedTool::Belt => "1:BELT",
            SelectedTool::Drill => "2:DRILL",
            SelectedTool::Smelter => "3:SMELT",
            SelectedTool::Chest => "4:CHEST",
            SelectedTool::Wall => "5:WALL",
            SelectedTool::Turret => "6:TURRET",
            SelectedTool::Inserter => "7:INSRT",
            SelectedTool::Clear => "8:CLEAR",
        }
    }
}

/// Rotates direction clockwise: East -> South -> West -> North -> East.
fn next_direction(dir: Direction) -> Direction {
    match dir {
        Direction::East => Direction::South,
        Direction::South => Direction::West,
        Direction::West => Direction::North,
        Direction::North => Direction::East,
    }
}

const fn direction_label(dir: Direction) -> &'static str {
    match dir {
        Direction::North => "NORTH",
        Direction::East => "EAST",
        Direction::South => "SOUTH",
        Direction::West => "WEST",
    }
}

/// Prepares the initial factory base with defenses, power, logistics, and ore deposits.
fn setup_starter_base(grid: &mut FactoryGrid<GRID_W, GRID_H>, seed: u64) {
    // 1. Procedural ore deposits across the map
    let veins = generate_ore_veins::<GRID_W>(seed, ChunkCoord::new(0, 0));
    for v in veins.active() {
        let r = v.radius as i32;
        for dy in -r..=r {
            for dx in -r..=r {
                let dist = (dx.abs() + dy.abs()) as u32;
                if dist > v.radius as u32 {
                    continue;
                }
                let x = v.cx as i32 + dx;
                let y = v.cy as i32 + dy;
                if x < 0 || y < 0 || x as usize >= GRID_W || y as usize >= GRID_H {
                    continue;
                }
                // Leave central 7x7 clear around center (16, 16)
                if (x - 16).abs() <= 3 && (y - 16).abs() <= 3 {
                    continue;
                }
                grid.place_deposit(x as usize, y as usize, v.item, v.amount_at(dist));
            }
        }
    }

    // 2. Base Heart at center (16, 16)
    grid.place_base_heart(16, 16);

    // 3. Perimeter defense walls
    for x in 13..=19 {
        grid.place_wall(x, 13);
        grid.place_wall(x, 19);
    }
    for y in 14..=18 {
        if y != 16 {
            // Opening at y=16 for conveyor logistics
            grid.place_wall(13, y);
        }
        grid.place_wall(19, y);
    }

    // 4. Defensive Turrets at the four corners
    grid.place_turret(14, 14);
    grid.place_turret(18, 14);
    grid.place_turret(14, 18);
    grid.place_turret(18, 18);

    // Pre-arm turrets with ammunition
    for &(tx, ty) in &[(14, 14), (18, 14), (14, 18), (18, 18)] {
        if let StructureKind::Turret(ref mut t) = grid.cells[ty][tx].kind {
            t.ammo = TURRET_AMMO_CAP;
            t.ammo_item = ItemType::IronPlate;
        }
    }

    // 5. Ammo storage chest adjacent to corner turret (14, 14)
    grid.place_chest(15, 14);
    if let StructureKind::Chest(ref mut c) = grid.cells[14][15].kind {
        c.item = ItemType::IronPlate;
        c.quantity = 100;
    }

    // 6. Automated Iron Production Line:
    // Ore Deposit -> Mining Drill -> Belt -> Smelter -> Inserter -> Belt -> Plate Storage Chest
    grid.place_deposit(8, 16, ItemType::IronOre, 5000);
    grid.place_drill(8, 16, Direction::East, 50);
    grid.place_belt(9, 16, Direction::East, 50);
    grid.place_belt(10, 16, Direction::East, 50);
    grid.place_inserter(11, 16, Direction::East, 50);
    grid.place_smelter(12, 16, 50);
    grid.place_inserter(13, 16, Direction::East, 50);
    grid.place_belt(14, 16, Direction::East, 50);
    grid.place_chest(15, 16);
}

/// Builds the currently selected tool onto the grid.
fn place_structure(
    grid: &mut FactoryGrid<GRID_W, GRID_H>,
    x: usize,
    y: usize,
    tool: SelectedTool,
    dir: Direction,
) {
    if x >= GRID_W || y >= GRID_H {
        return;
    }
    // Protect base heart from replacement
    if matches!(grid.cells[y][x].kind, StructureKind::BaseHeart(_)) {
        return;
    }

    match tool {
        SelectedTool::Belt => {
            grid.place_belt(x, y, dir, 50);
        }
        SelectedTool::Drill => {
            if matches!(grid.cells[y][x].kind, StructureKind::Deposit(_)) {
                grid.place_drill(x, y, dir, 50);
            } else {
                // If ground is empty, seed iron ore patch
                grid.place_deposit(x, y, ItemType::IronOre, 5000);
                grid.place_drill(x, y, dir, 50);
            }
        }
        SelectedTool::Smelter => {
            grid.place_smelter(x, y, 50);
        }
        SelectedTool::Chest => {
            grid.place_chest(x, y);
            // Default pre-stocked ammo for easy base defense
            grid.cells[y][x].kind = StructureKind::Chest(Chest {
                item: ItemType::IronPlate,
                quantity: 50,
            });
        }
        SelectedTool::Wall => {
            grid.place_wall(x, y);
        }
        SelectedTool::Turret => {
            grid.place_turret(x, y);
            if let StructureKind::Turret(ref mut t) = grid.cells[y][x].kind {
                t.ammo = TURRET_AMMO_CAP;
                t.ammo_item = ItemType::IronPlate;
            }
        }
        SelectedTool::Inserter => {
            grid.place_inserter(x, y, dir, 50);
        }
        SelectedTool::Clear => {
            demolish_tile(grid, x, y);
        }
    }
}

/// Demolishes or clears a tile, restoring any buried ore deposit.
fn demolish_tile(grid: &mut FactoryGrid<GRID_W, GRID_H>, x: usize, y: usize) {
    if x >= GRID_W || y >= GRID_H {
        return;
    }
    if matches!(grid.cells[y][x].kind, StructureKind::BaseHeart(_)) {
        return;
    }
    let restored = match grid.cells[y][x].kind {
        StructureKind::Drill(d) if d.remaining > 0 => Some((d.ore, d.remaining)),
        _ => None,
    };
    match restored {
        Some((ore, amount)) => grid.place_deposit(x, y, ore, amount),
        None => grid.cells[y][x] = FactoryStructure::empty(),
    }
}

/// Color mapping for game items.
fn item_color(item: ItemType) -> u32 {
    match item {
        ItemType::None => 0x00000000,
        ItemType::IronOre => 0xFF94A3B8,
        ItemType::CopperOre => 0xFFEA580C,
        ItemType::Coal => 0xFF27272A,
        ItemType::Stone => 0xFFA8A29E,
        ItemType::IronPlate => 0xFFE2E8F0,
        ItemType::CopperPlate => 0xFFFB923C,
        ItemType::StoneBrick => 0xFFD6D3D1,
    }
}

/// Bresenham integer line renderer for tracer bullets.
fn draw_line(fb: &mut FrameBuffer, mut x0: i32, mut y0: i32, x1: i32, y1: i32, color: u32) {
    let dx = (x1 - x0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let dy = -(y1 - y0).abs();
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy;
    loop {
        fb.set_pixel(x0, y0, color);
        if x0 == x1 && y0 == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x0 += sx;
        }
        if e2 <= dx {
            err += dx;
            y0 += sy;
        }
    }
}

/// HP durability bar along the top edge of structures.
fn draw_hp_bar(fb: &mut FrameBuffer, sx: i32, sy: i32, ts: i32, hp: u16, max_hp: u16) {
    if max_hp == 0 || hp >= max_hp {
        return;
    }
    let w = (ts - 2).max(1);
    let fill = (w as i64 * hp as i64 / max_hp as i64) as i32;
    fb.draw_rect(sx + 1, sy + 1, w, 2, 0xFF450A0A);
    fb.draw_rect(sx + 1, sy + 1, fill, 2, 0xFF22C55E);
}

/// Software rasterizer for factory cells using 16x16 sprite atlas.
fn draw_cell(
    fb: &mut FrameBuffer,
    cell: &FactoryStructure,
    sx: i32,
    sy: i32,
    ts: i32,
    phase: i32,
) {
    match cell.kind {
        StructureKind::Empty => {}
        StructureKind::Deposit(d) => {
            if d.item == ItemType::CopperOre {
                fb.draw_sprite_16x16_tinted(sx, sy, &ORE_DEPOSIT, 0xFFF97316);
            } else {
                fb.draw_sprite_16x16(sx, sy, &ORE_DEPOSIT);
            }
            if d.remaining < 100 {
                fb.draw_rect(sx + 6, sy + 6, 4, 4, 0xFF0B1220);
            }
        }
        StructureKind::Belt(b) => {
            let sprite = get_belt_sprite(b.direction as u32, phase as u64);
            fb.draw_sprite_16x16(sx, sy, sprite);
            if !b.item.is_none() {
                let is = 6;
                let travel = 10;
                let off = (travel as i64 * b.progress.min(axiom_game::CYCLE) as i64
                    / axiom_game::CYCLE as i64) as i32;
                let mid = 5;
                let (ix, iy) = match b.direction {
                    Direction::North => (sx + mid, sy + travel - off),
                    Direction::East => (sx + off, sy + mid),
                    Direction::South => (sx + mid, sy + off),
                    Direction::West => (sx + travel - off, sy + mid),
                };
                fb.draw_rect(ix, iy, is, is, item_color(b.item));
                fb.draw_rect(ix + 1, iy + 1, is - 2, is - 2, 0xFFFFFFFF);
            }
        }
        StructureKind::Drill(d) => {
            let sprite = get_drill_sprite(d.direction as u32, phase as u64);
            fb.draw_sprite_16x16(sx, sy, sprite);
            if !d.ore.is_none() {
                fb.draw_rect(sx + 6, sy + 6, 4, 4, item_color(d.ore));
            }
            let bar = (14 * d.progress.min(axiom_game::CYCLE) as i64
                / axiom_game::CYCLE as i64) as i32;
            fb.draw_rect(sx + 1, sy + ts - 2, bar, 2, 0xFFFBBF24);
        }
        StructureKind::Inserter(i) => {
            let sprite = get_inserter_sprite(i.direction as u32, phase as u64);
            fb.draw_sprite_16x16(sx, sy, sprite);
            if !i.held.is_none() {
                fb.draw_rect(sx + 6, sy + 6, 4, 4, item_color(i.held));
            }
        }
        StructureKind::Smelter(s) => {
            let burning = s.progress > 0 || s.input_count > 0;
            let tick = if burning { phase as u64 } else { 0 };
            let sprite = get_smelter_sprite(tick);
            fb.draw_sprite_16x16(sx, sy, sprite);
            if burning {
                let puff = 3;
                let rise = (phase % 8) * ts / 8;
                fb.draw_rect(sx + ts / 2 - puff / 2, sy - rise - puff, puff, puff, 0xFF9CA3AF);
            }
            if s.output_count > 0 {
                let bar = (14 * s.output_count as i64
                    / axiom_game::SMELTER_OUTPUT_CAP as i64) as i32;
                fb.draw_rect(sx + ts - 3, sy + 1 + 14 - bar, 2, bar, item_color(s.output));
            }
        }
        StructureKind::Chest(c) => {
            fb.draw_sprite_16x16(sx, sy, &CHEST_SPRITE);
            if c.quantity > 0 {
                let fill = (10 * c.quantity.min(100) as i64 / 100) as i32;
                fb.draw_rect(
                    sx + 13,
                    sy + 13 - fill,
                    2,
                    fill,
                    item_color(c.item),
                );
            }
        }
        StructureKind::Wall(w) => {
            fb.draw_sprite_16x16(sx, sy, &WALL_SPRITE);
            draw_hp_bar(fb, sx, sy, ts, w.hp, w.max_hp);
        }
        StructureKind::Turret(t) => {
            let sprite = get_turret_sprite(DIR_EAST);
            fb.draw_sprite_16x16(sx, sy, sprite);
            let pips = (t.ammo as i32 * 5 / axiom_game::TURRET_AMMO_CAP as i32).min(5);
            for i in 0..pips {
                fb.draw_rect(sx + 2 + i * 2, sy + ts - 3, 2, 2, 0xFFFDE047);
            }
            if t.ammo == 0 {
                fb.draw_rect(sx + 2, sy + ts - 3, 2, 2, 0xFFEF4444);
            }
            draw_hp_bar(fb, sx, sy, ts, t.hp, t.max_hp);
        }
        StructureKind::BaseHeart(h) => {
            let sprite = get_heart_sprite(phase as u64);
            fb.draw_sprite_16x16(sx, sy, sprite);
            draw_hp_bar(fb, sx, sy, ts, h.hp, h.max_hp);
        }
    }
}

/// 3x5 Bitmap font glyph definition.
fn glyph_3x5(c: char) -> u16 {
    match c.to_ascii_uppercase() {
        '0' => 0x7557,
        '1' => 0x2C97,
        '2' => 0x71E7,
        '3' => 0x71CF,
        '4' => 0x5BC9,
        '5' => 0x79CF,
        '6' => 0x79EF,
        '7' => 0x7152,
        '8' => 0x7BEF,
        '9' => 0x7BCF,
        'A' => 0x2BE5,
        'B' => 0x6DAE,
        'C' => 0x3923,
        'D' => 0x6D6E,
        'E' => 0x79A7,
        'F' => 0x79A4,
        'G' => 0x396B,
        'H' => 0x5BE5,
        'I' => 0x7497,
        'J' => 0x126A,
        'K' => 0x5D35,
        'L' => 0x4927,
        'M' => 0x5FE5,
        'N' => 0x6D65,
        'O' => 0x2D62,
        'P' => 0x7BE4,
        'Q' => 0x2D73,
        'R' => 0x7BA5,
        'S' => 0x3896,
        'T' => 0x7492,
        'U' => 0x5D67,
        'V' => 0x5D52,
        'W' => 0x5D7D,
        'X' => 0x5AAB,
        'Y' => 0x5A92,
        'Z' => 0x72A7,
        ':' => 0x0410,
        '-' => 0x01C0,
        '/' => 0x12A4,
        '.' => 0x0002,
        '[' => 0x7247,
        ']' => 0x7127,
        '(' => 0x2922,
        ')' => 0x224A,
        '_' => 0x0007,
        '|' => 0x2492,
        _ => 0,
    }
}

/// Alpha blends a solid color over existing framebuffer pixels using pure integer math.
fn draw_rect_blend(fb: &mut FrameBuffer, x: i32, y: i32, w: i32, h: i32, color: u32) {
    let a = (color >> 24) & 0xFF;
    if a == 0 {
        return;
    }
    if a == 255 {
        fb.draw_rect(x, y, w, h, color);
        return;
    }
    let inv_a = 255 - a;
    let cr = (color >> 16) & 0xFF;
    let cg = (color >> 8) & 0xFF;
    let cb = color & 0xFF;

    let x_start = x.max(0) as usize;
    let x_end = ((x + w).min(fb.width as i32)).max(0) as usize;
    let y_start = y.max(0) as usize;
    let y_end = ((y + h).min(fb.height as i32)).max(0) as usize;

    for py in y_start..y_end {
        let row_offset = py * fb.width;
        for px in x_start..x_end {
            let bg = fb.pixels[row_offset + px];
            let br = (bg >> 16) & 0xFF;
            let bg_g = (bg >> 8) & 0xFF;
            let bb = bg & 0xFF;
            let r = (cr * a + br * inv_a) / 255;
            let g = (cg * a + bg_g * inv_a) / 255;
            let b = (cb * a + bb * inv_a) / 255;
            fb.pixels[row_offset + px] = 0xFF00_0000 | (r << 16) | (g << 8) | b;
        }
    }
}

/// Renders a single 3x5 ASCII character.
fn draw_char(fb: &mut FrameBuffer, c: char, x: i32, y: i32, color: u32) {
    let glyph = glyph_3x5(c);
    if glyph == 0 {
        return;
    }
    for row in 0..5 {
        let row_bits = (glyph >> (12 - row * 3)) & 0b111;
        for col in 0..3 {
            if (row_bits & (1 << (2 - col))) != 0 {
                fb.set_pixel(x + col, y + row, color);
            }
        }
    }
}

/// Renders a text string with 3x5 font (4 pixels pitch).
fn draw_text(fb: &mut FrameBuffer, text: &str, mut x: i32, y: i32, color: u32) {
    for c in text.chars() {
        draw_char(fb, c, x, y, color);
        x += 4;
    }
}

impl DesktopEngine {
    /// Renders the entire factory scene, arena units, tracer bullets, cursor, sidebars, and overlays.
    pub fn render(&self, fb: &mut FrameBuffer) {
        fb.clear(0xFF0A0F1D);
        let phase = (self.tick & 0x7FFF_FFFF) as i32;
        let now = self.director.current_tick as u32;

        // 1. Floor & Structures
        for y in 0..GRID_H {
            let sy = GRID_OFFSET_Y + y as i32 * TILE_SIZE;
            for x in 0..GRID_W {
                let sx = GRID_OFFSET_X + x as i32 * TILE_SIZE;
                let floor = if (x + y) % 2 == 0 { 0xFF141F30 } else { 0xFF0F1726 };
                fb.draw_rect(sx, sy, TILE_SIZE, TILE_SIZE, floor);
                draw_cell(fb, &self.grid.cells[y][x], sx, sy, TILE_SIZE, phase);
            }
        }

        // 2. Monsters from WaveDirector Arena
        let monster_sprite = get_monster_sprite(phase as u64);
        for u in self.director.arena.units.iter() {
            if !u.is_alive() || u.team != MONSTER_TEAM {
                continue;
            }
            let sx = GRID_OFFSET_X + u.pos.x.to_int() as i32 * TILE_SIZE;
            let sy = GRID_OFFSET_Y + u.pos.y.to_int() as i32 * TILE_SIZE;
            fb.draw_sprite_16x16(sx, sy, monster_sprite);
            draw_hp_bar(fb, sx, sy, TILE_SIZE, u.current_hp, u.max_hp);
        }

        // 3. Turret Tracer Rounds
        if now > 0 {
            for y in 0..GRID_H {
                for x in 0..GRID_W {
                    let cell = &self.grid.cells[y][x];
                    let t = match cell.kind {
                        StructureKind::Turret(t) => t,
                        _ => continue,
                    };
                    let fired = cell.last_action_tick == now && t.cooldown > 0 && t.cooldown_timer == t.cooldown;
                    if !fired {
                        continue;
                    }
                    let cx = GRID_OFFSET_X + x as i32 * TILE_SIZE + TILE_SIZE / 2;
                    let cy = GRID_OFFSET_Y + y as i32 * TILE_SIZE + TILE_SIZE / 2;
                    let flash = (TILE_SIZE / 2).max(2);
                    fb.draw_rect(cx - flash / 2, cy - flash / 2, flash, flash, 0xFFFDE047);

                    let range_sq = t.range as i64 * t.range as i64;
                    let mut best: Option<(i32, i32)> = None;
                    let mut best_d = i64::MAX;
                    for u in self.director.arena.units.iter() {
                        if !u.is_alive() || u.team != MONSTER_TEAM {
                            continue;
                        }
                        let (ux, uy) = (u.pos.x.to_int(), u.pos.y.to_int());
                        let (dx, dy) = (ux - x as i64, uy - y as i64);
                        let d = dx * dx + dy * dy;
                        if d <= range_sq && d < best_d {
                            best_d = d;
                            best = Some((ux as i32, uy as i32));
                        }
                    }
                    if let Some((ux, uy)) = best {
                        let tx = GRID_OFFSET_X + ux * TILE_SIZE + TILE_SIZE / 2;
                        let ty = GRID_OFFSET_Y + uy * TILE_SIZE + TILE_SIZE / 2;
                        draw_line(fb, cx, cy, tx, ty, 0xFFFDE047);
                    }
                }
            }
        }

        // 4. Build Cursor Box
        let cx = GRID_OFFSET_X + self.cursor_x as i32 * TILE_SIZE;
        let cy = GRID_OFFSET_Y + self.cursor_y as i32 * TILE_SIZE;
        fb.draw_rect(cx, cy, TILE_SIZE, 1, 0xFF38BDF8);
        fb.draw_rect(cx, cy + TILE_SIZE - 1, TILE_SIZE, 1, 0xFF38BDF8);
        fb.draw_rect(cx, cy, 1, TILE_SIZE, 0xFF38BDF8);
        fb.draw_rect(cx + TILE_SIZE - 1, cy, 1, TILE_SIZE, 0xFF38BDF8);

        // 5. Left Sidebar HUD
        fb.draw_rect(0, 0, 96, WIN_HEIGHT as i32, 0xFF0B1322);
        fb.draw_rect(95, 0, 1, WIN_HEIGHT as i32, 0xFF1E293B);

        draw_text(fb, "AXIOM 1.3", 10, 12, 0xFF38BDF8);
        draw_text(fb, "STUDIO", 10, 22, 0xFF94A3B8);
        fb.draw_rect(10, 32, 76, 1, 0xFF1E293B);

        draw_text(fb, "TOOLS:", 10, 40, 0xFFFBBF24);
        let tools = [
            SelectedTool::Belt,
            SelectedTool::Drill,
            SelectedTool::Smelter,
            SelectedTool::Chest,
            SelectedTool::Wall,
            SelectedTool::Turret,
            SelectedTool::Inserter,
            SelectedTool::Clear,
        ];
        for (i, t) in tools.iter().enumerate() {
            let ty = 54 + i as i32 * 18;
            let is_sel = *t == self.selected_tool;
            if is_sel {
                fb.draw_rect(8, ty - 2, 80, 14, 0xFF1E3A8A);
                fb.draw_rect(8, ty - 2, 80, 1, 0xFF60A5FA);
                fb.draw_rect(8, ty + 11, 80, 1, 0xFF60A5FA);
            }
            let color = if is_sel { 0xFFFFFFFF } else { 0xFF94A3B8 };
            draw_text(fb, t.label(), 12, ty + 2, color);
        }

        let dir_y = 210;
        fb.draw_rect(10, dir_y, 76, 1, 0xFF1E293B);
        draw_text(fb, "DIR [R]:", 10, dir_y + 8, 0xFFFBBF24);
        draw_text(fb, direction_label(self.current_direction), 10, dir_y + 20, 0xFF34D399);

        // 6. Right Sidebar Telemetry & Controls
        fb.draw_rect(RIGHT_SIDEBAR_X, 0, 96, WIN_HEIGHT as i32, 0xFF0B1322);
        fb.draw_rect(RIGHT_SIDEBAR_X, 0, 1, WIN_HEIGHT as i32, 0xFF1E293B);

        let d_rep = self.director.report(&self.grid);
        let f_rep = self.grid.report();
        let rx = RIGHT_SIDEBAR_X + 8;

        draw_text(fb, "DEFENSE", rx, 12, 0xFFF43F5E);
        draw_text(fb, &format!("WAVE:{}", d_rep.wave_number), rx, 24, 0xFFE2E8F0);
        draw_text(fb, &format!("MOBS:{}", d_rep.monsters_alive), rx, 34, 0xFFF87171);
        draw_text(fb, &format!("KILL:{}", d_rep.total_killed), rx, 44, 0xFF34D399);
        draw_text(fb, &format!("AMMO:{}", d_rep.turret_ammo), rx, 54, 0xFFFBBF24);

        fb.draw_rect(rx, 68, 76, 1, 0xFF1E293B);
        draw_text(fb, "FACTORY", rx, 76, 0xFF60A5FA);
        draw_text(fb, &format!("MINE:{}", f_rep.total_mined), rx, 88, 0xFFCBD5E1);
        draw_text(fb, &format!("SMLT:{}", f_rep.total_smelted), rx, 98, 0xFFCBD5E1);
        draw_text(fb, &format!("STOR:{}", f_rep.total_stored), rx, 108, 0xFFCBD5E1);
        draw_text(fb, &format!("BELT:{}", f_rep.items_on_belts), rx, 118, 0xFFCBD5E1);

        fb.draw_rect(rx, 134, 76, 1, 0xFF1E293B);
        draw_text(fb, "CONTROLS", rx, 142, 0xFFFBBF24);
        draw_text(fb, "WASD:MOVE", rx, 154, 0xFF94A3B8);
        draw_text(fb, "LMB:BUILD", rx, 164, 0xFF94A3B8);
        draw_text(fb, "RMB:CLEAR", rx, 174, 0xFF94A3B8);
        draw_text(fb, "1-8:TOOL", rx, 184, 0xFF94A3B8);
        draw_text(fb, "R:ROTATE", rx, 194, 0xFF94A3B8);
        draw_text(fb, "F3:PROF", rx, 204, 0xFF38BDF8);
        draw_text(fb, "F5:QSAVE", rx, 214, 0xFF38BDF8);
        draw_text(fb, "F9:QLOAD", rx, 224, 0xFF38BDF8);
        draw_text(fb, "ESC:EXIT", rx, 234, 0xFF94A3B8);

        // 7. Top Header Bar (Heart HP & Status)
        fb.draw_rect(GRID_OFFSET_X, 0, GRID_PIXEL_SIZE, 16, 0xFF0F172A);
        draw_text(fb, "HEART:", 104, 5, 0xFFF43F5E);
        let hp_bar_w = 120;
        fb.draw_rect(142, 4, hp_bar_w, 8, 0xFF450A0A);
        let hp_fill = if d_rep.heart_max_hp > 0 {
            (hp_bar_w * d_rep.heart_hp as i32) / d_rep.heart_max_hp as i32
        } else {
            0
        };
        fb.draw_rect(142, 4, hp_fill, 8, 0xFFF43F5E);
        draw_text(
            fb,
            &format!("{}/{}", d_rep.heart_hp, d_rep.heart_max_hp),
            270,
            5,
            0xFFFFFFFF,
        );

        if d_rep.base_fallen {
            draw_text(fb, "[BASE DESTROYED]", 370, 5, 0xFFEF4444);
        } else {
            draw_text(
                fb,
                &format!("NEXT WAVE: TICK {}", self.director.next_wave_tick),
                350,
                5,
                0xFFFBBF24,
            );
        }

        // 8. Notification Toast / Alert Banner
        if let Some((ref msg, _)) = self.notification() {
            let box_w = (msg.len() as i32 * 4 + 16).max(180);
            let box_h = 16;
            let box_x = GRID_OFFSET_X + (GRID_PIXEL_SIZE - box_w) / 2;
            let box_y = 22;

            fb.draw_rect(box_x, box_y, box_w, box_h, 0xFF042F2E);
            fb.draw_rect(box_x, box_y, box_w, 1, 0xFF14B8A6);
            fb.draw_rect(box_x, box_y + box_h - 1, box_w, 1, 0xFF14B8A6);
            fb.draw_rect(box_x, box_y, 1, box_h, 0xFF14B8A6);
            fb.draw_rect(box_x + box_w - 1, box_y, 1, box_h, 0xFF14B8A6);

            let text_x = box_x + (box_w - (msg.len() as i32 * 4)) / 2;
            let text_y = box_y + 5;
            draw_text(fb, msg, text_x, text_y, 0xFF5EEAD4);
        }

        // 9. Profiler Overlay (F3)
        if self.show_profiler {
            self.render_profiler_overlay(fb);
        }
    }

    /// Renders the real-time F3 profiler & debug HUD overlay.
    pub fn render_profiler_overlay(&self, fb: &mut FrameBuffer) {
        let x = 108;
        let y = 12;
        let w = 496;
        let h = 100;

        // 1. Semi-transparent dark background (0xDD0F172A)
        draw_rect_blend(fb, x, y, w, h, 0xDD0F172A);

        // 2. Crisp border (0xFF38BDF8)
        let border = 0xFF38BDF8;
        fb.draw_rect(x, y, w, 1, border);
        fb.draw_rect(x, y + h - 1, w, 1, border);
        fb.draw_rect(x, y, 1, h, border);
        fb.draw_rect(x + w - 1, y, 1, h, border);

        // 3. Header title
        draw_text(fb, "[AXIOM PROFILER (F3)]", x + 8, y + 6, 0xFF38BDF8);

        // 4. Telemetry metrics
        let line1 = format!(
            "SIM: {} us | RENDER: {} us | FPS: {} | UPS: {}",
            self.profiler.sim_time_us, self.profiler.render_time_us, self.profiler.fps, self.profiler.ups
        );
        draw_text(fb, &line1, x + 8, y + 17, 0xFFE2E8F0);

        let line2 = format!(
            "STRUCTS: {} | MONSTERS: {} | ITEMS: {}",
            self.profiler.active_structures,
            self.profiler.active_monsters,
            self.profiler.active_items
        );
        draw_text(fb, &line2, x + 8, y + 27, 0xFF94A3B8);

        let line3 = format!("HASH: 0x{:016X}", self.profiler.state_hash);
        draw_text(fb, &line3, x + 8, y + 37, 0xFF34D399);

        // 5. 60-frame Sparkline / Bar Graph
        let base_y = y + 94; // bottom of bars
        let max_bar_h = 42;  // max height of bars
        let ref_target_h = 20; // height for 16.6ms frame
        let ref_y = base_y - ref_target_h;

        // 16.6ms target reference line (0xFF334155)
        fb.draw_rect(x + 8, ref_y, 480, 1, 0xFF334155);
        draw_text(fb, "16.6MS", x + w - 36, ref_y - 6, 0xFF475569);

        for i in 0..60 {
            let idx = (self.profiler.history_idx + i) % 60;
            let frame_us = self.profiler.history[idx] as u64;
            let bar_x = x + 8 + (i as i32 * 8);

            if frame_us > 0 {
                let bar_h = (((frame_us * ref_target_h as u64) / 16666).max(1)).min(max_bar_h as u64) as i32;
                let bar_y = base_y - bar_h;
                let color = if frame_us < 16_000 {
                    0xFF22C55E // Green (< 16.6ms)
                } else if frame_us <= 18_000 {
                    0xFFFBBF24 // Yellow (~16.6ms)
                } else {
                    0xFFEF4444 // Red (> 16.6ms)
                };
                fb.draw_rect(bar_x, bar_y, 6, bar_h, color);
            } else {
                // Empty history slot placeholder
                fb.draw_rect(bar_x, base_y - 1, 6, 1, 0xFF1E293B);
            }
        }
    }
}

/// Main desktop window runner.
pub fn run_desktop_window() -> Result<(), Box<dyn std::error::Error>> {
    let mut window = Window::new(
        "Axiom Native Engine v1.3.0 - Desktop Studio",
        WIN_WIDTH,
        WIN_HEIGHT,
        WindowOptions {
            resize: false,
            scale: minifb::Scale::X1,
            ..WindowOptions::default()
        },
    )?;

    // Smooth target refresh rate (144 FPS) decoupled from simulation tick rate
    window.set_target_fps(144);

    let seed = 0xABCD_9999_7777;
    let mut engine = DesktopEngine::new(seed);

    let mut framebuffer = FrameBuffer::new(WIN_WIDTH, WIN_HEIGHT);

    let mut frame_counter = 0u32;
    let mut tick_counter = 0u32;
    let mut current_fps = 60u32;
    let mut current_ups = 60u32;
    let mut fps_timer = std::time::Instant::now();

    let mut previous_time = std::time::Instant::now();
    let mut accumulator = Duration::ZERO;

    println!("🚀 [Axiom Desktop Studio] Launched in 704x544 native window at {} Hz fixed simulation.", TICK_RATE_HZ);
    println!("   Controls: WASD (Move), 1~8 (Tool), R (Rotate), LMB (Build), RMB (Clear), F3 (Profiler), F5 (Save), F9 (Load), ESC (Exit)");

    while window.is_open() && !window.is_key_down(Key::Escape) {
        let current_time = std::time::Instant::now();
        let mut delta_time = current_time.duration_since(previous_time);
        previous_time = current_time;
        if delta_time > MAX_ACCUMULATOR {
            delta_time = MAX_ACCUMULATOR;
        }
        accumulator += delta_time;

        // --- 1. Keyboard Input Handling ---
        // WASD Cursor Navigation
        if window.is_key_pressed(Key::W, KeyRepeat::Yes) || window.is_key_pressed(Key::Up, KeyRepeat::Yes) {
            engine.cursor_y = engine.cursor_y.saturating_sub(1);
        }
        if window.is_key_pressed(Key::S, KeyRepeat::Yes) || window.is_key_pressed(Key::Down, KeyRepeat::Yes) {
            engine.cursor_y = (engine.cursor_y + 1).min(GRID_H - 1);
        }
        if window.is_key_pressed(Key::A, KeyRepeat::Yes) || window.is_key_pressed(Key::Left, KeyRepeat::Yes) {
            engine.cursor_x = engine.cursor_x.saturating_sub(1);
        }
        if window.is_key_pressed(Key::D, KeyRepeat::Yes) || window.is_key_pressed(Key::Right, KeyRepeat::Yes) {
            engine.cursor_x = (engine.cursor_x + 1).min(GRID_W - 1);
        }

        // 1~8 Tool Selection
        if window.is_key_pressed(Key::Key1, KeyRepeat::No) || window.is_key_pressed(Key::NumPad1, KeyRepeat::No) {
            engine.selected_tool = SelectedTool::Belt;
        }
        if window.is_key_pressed(Key::Key2, KeyRepeat::No) || window.is_key_pressed(Key::NumPad2, KeyRepeat::No) {
            engine.selected_tool = SelectedTool::Drill;
        }
        if window.is_key_pressed(Key::Key3, KeyRepeat::No) || window.is_key_pressed(Key::NumPad3, KeyRepeat::No) {
            engine.selected_tool = SelectedTool::Smelter;
        }
        if window.is_key_pressed(Key::Key4, KeyRepeat::No) || window.is_key_pressed(Key::NumPad4, KeyRepeat::No) {
            engine.selected_tool = SelectedTool::Chest;
        }
        if window.is_key_pressed(Key::Key5, KeyRepeat::No) || window.is_key_pressed(Key::NumPad5, KeyRepeat::No) {
            engine.selected_tool = SelectedTool::Wall;
        }
        if window.is_key_pressed(Key::Key6, KeyRepeat::No) || window.is_key_pressed(Key::NumPad6, KeyRepeat::No) {
            engine.selected_tool = SelectedTool::Turret;
        }
        if window.is_key_pressed(Key::Key7, KeyRepeat::No) || window.is_key_pressed(Key::NumPad7, KeyRepeat::No) {
            engine.selected_tool = SelectedTool::Inserter;
        }
        if window.is_key_pressed(Key::Key8, KeyRepeat::No) || window.is_key_pressed(Key::NumPad8, KeyRepeat::No) {
            engine.selected_tool = SelectedTool::Clear;
        }

        // R: Rotate Direction
        if window.is_key_pressed(Key::R, KeyRepeat::No) {
            engine.current_direction = next_direction(engine.current_direction);
        }

        // Space / Enter: Place structure at cursor
        if window.is_key_pressed(Key::Space, KeyRepeat::No) || window.is_key_pressed(Key::Enter, KeyRepeat::No) {
            place_structure(&mut engine.grid, engine.cursor_x, engine.cursor_y, engine.selected_tool, engine.current_direction);
        }

        // Delete / Backspace: Clear structure at cursor
        if window.is_key_pressed(Key::Delete, KeyRepeat::No) || window.is_key_pressed(Key::Backspace, KeyRepeat::No) {
            demolish_tile(&mut engine.grid, engine.cursor_x, engine.cursor_y);
        }

        // F3: Toggle Profiler Overlay
        if window.is_key_pressed(Key::F3, KeyRepeat::No) {
            engine.show_profiler = !engine.show_profiler;
        }

        // F5: Quicksave
        if window.is_key_pressed(Key::F5, KeyRepeat::No) {
            if let Err(e) = engine.save_to_file("saves/quick.axiom") {
                engine.set_notification(format!("[SAVE FAILED: {}]", e), 120);
            }
        }

        // F9: Quickload
        if window.is_key_pressed(Key::F9, KeyRepeat::No) {
            if let Err(e) = engine.load_from_file("saves/quick.axiom") {
                engine.set_notification(format!("[LOAD FAILED: {}]", e), 120);
            }
        }

        // --- 2. Mouse Input Handling ---
        if let Some((mx, my)) = window.get_mouse_pos(MouseMode::Discard) {
            let mx = mx as i32;
            let my = my as i32;
            let gx = (mx - GRID_OFFSET_X) / TILE_SIZE;
            let gy = (my - GRID_OFFSET_Y) / TILE_SIZE;
            if gx >= 0 && gx < GRID_W as i32 && gy >= 0 && gy < GRID_H as i32 {
                engine.cursor_x = gx as usize;
                engine.cursor_y = gy as usize;

                if window.get_mouse_down(MouseButton::Left) {
                    place_structure(&mut engine.grid, engine.cursor_x, engine.cursor_y, engine.selected_tool, engine.current_direction);
                }
                if window.get_mouse_down(MouseButton::Right) {
                    demolish_tile(&mut engine.grid, engine.cursor_x, engine.cursor_y);
                }
            }
        }

        // --- 3. Simulation Step (Fixed Timestep Accumulator Loop) ---
        let sim_start = std::time::Instant::now();
        let mut ticks_this_frame = 0u32;
        while accumulator >= DT_FIXED {
            engine.step();
            accumulator -= DT_FIXED;
            ticks_this_frame += 1;
        }
        let sim_time_us = sim_start.elapsed().as_micros() as u64;

        // --- 4. Render Scene into FrameBuffer ---
        let render_start = std::time::Instant::now();
        engine.render(&mut framebuffer);
        let render_time_us = render_start.elapsed().as_micros() as u64;

        // FPS and UPS tracking
        frame_counter += 1;
        tick_counter += ticks_this_frame;
        if fps_timer.elapsed() >= std::time::Duration::from_secs(1) {
            current_fps = frame_counter;
            current_ups = tick_counter;
            frame_counter = 0;
            tick_counter = 0;
            fps_timer = std::time::Instant::now();
        }

        engine.update_profiler_metrics(sim_time_us, render_time_us, current_fps, current_ups);

        // --- 5. Push Frame to Desktop Window ---
        window.update_with_buffer(framebuffer.as_slice(), WIN_WIDTH, WIN_HEIGHT)?;
    }

    println!("🛑 [Axiom Desktop Studio] Clean shutdown completed.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_desktop_save_load_roundtrip() {
        let seed = 0xABCD_9999_7777;
        let mut engine = DesktopEngine::new(seed);

        // Place custom structures (turret, smelter, drill)
        engine.grid.place_turret(5, 5);
        if let StructureKind::Turret(ref mut t) = engine.grid.cells[5][5].kind {
            t.ammo = 42;
            t.ammo_item = ItemType::IronPlate;
        }

        engine.grid.place_smelter(6, 5, 20);
        if let StructureKind::Smelter(ref mut s) = engine.grid.cells[5][6].kind {
            s.input_count = 5;
            s.output_count = 2;
        }

        engine.grid.place_drill(7, 5, Direction::North, 30);

        // Advance simulation several ticks
        for _ in 0..10 {
            engine.step();
        }

        let save_path = "tmp/test_save.axiom";
        let saved_cells = engine.grid.cells;
        let saved_tick = engine.tick;
        let saved_wave = engine.director.wave_number;
        let saved_director_tick = engine.director.current_tick;
        let saved_mined = engine.grid.total_mined;
        let saved_smelted = engine.grid.total_smelted;

        engine.save_to_file(save_path).expect("Failed to save desktop state");

        // Mutate engine state significantly
        engine.grid.place_wall(5, 5);
        engine.grid.cells[5][6] = FactoryStructure::empty();
        engine.grid.cells[5][7] = FactoryStructure::empty();
        engine.tick += 500;
        engine.director.wave_number += 10;
        engine.grid.total_smelted += 999;

        assert_ne!(engine.grid.cells, saved_cells);
        assert_ne!(engine.tick, saved_tick);

        // Load back from file
        engine.load_from_file(save_path).expect("Failed to load desktop state");

        // Verify exact match of grid state and properties
        assert_eq!(engine.grid.cells, saved_cells);
        assert_eq!(engine.tick, saved_tick);
        assert_eq!(engine.director.wave_number, saved_wave);
        assert_eq!(engine.director.current_tick, saved_director_tick);
        assert_eq!(engine.grid.total_mined, saved_mined);
        assert_eq!(engine.grid.total_smelted, saved_smelted);

        // Clean up temporary test file
        let _ = std::fs::remove_file(save_path);
    }

    #[test]
    fn test_desktop_save_corruption_checks() {
        let seed = 0xABCD_9999_7777;
        let engine = DesktopEngine::new(seed);
        let save_path = "tmp/test_corrupt.axiom";

        engine.save_to_file(save_path).expect("Failed to save desktop state");

        // 1. Truncated file test
        let mut bytes = std::fs::read(save_path).unwrap();
        let short_bytes = &bytes[..40];
        let short_path = "tmp/test_short.axiom";
        std::fs::write(short_path, short_bytes).unwrap();

        let mut load_engine = DesktopEngine::new(seed);
        assert!(load_engine.load_from_file(short_path).is_err());
        let _ = std::fs::remove_file(short_path);

        // 2. Corrupted magic test
        let orig_magic = bytes[0];
        bytes[0] ^= 0xFF;
        let corrupt_magic_path = "tmp/test_bad_magic.axiom";
        std::fs::write(corrupt_magic_path, &bytes).unwrap();
        assert!(load_engine.load_from_file(corrupt_magic_path).is_err());
        let _ = std::fs::remove_file(corrupt_magic_path);
        bytes[0] = orig_magic;

        // 3. Checksum tampering test
        let last_idx = bytes.len() - 1;
        bytes[last_idx] ^= 0xFF;
        let tampered_path = "tmp/test_tampered.axiom";
        std::fs::write(tampered_path, &bytes).unwrap();
        assert!(load_engine.load_from_file(tampered_path).is_err());
        let _ = std::fs::remove_file(tampered_path);

        let _ = std::fs::remove_file(save_path);
    }

    #[test]
    fn test_desktop_profiler_metrics_collection() {
        let seed = 0xABCD_9999_7777;
        let mut engine = DesktopEngine::new(seed);

        // 1. Verify profiler toggling (default false)
        assert!(!engine.show_profiler);
        engine.show_profiler = !engine.show_profiler;
        assert!(engine.show_profiler);
        engine.show_profiler = !engine.show_profiler;
        assert!(!engine.show_profiler);

        // 2. Metric calculation & recording
        engine.update_profiler_metrics(1250, 2400, 60, 60);
        assert_eq!(engine.profiler.sim_time_us, 1250);
        assert_eq!(engine.profiler.render_time_us, 2400);
        assert_eq!(engine.profiler.fps, 60);
        assert_eq!(engine.profiler.ups, 60);
        assert!(engine.profiler.active_structures > 0);
        assert_ne!(engine.profiler.state_hash, 0);

        // 3. History buffer wrapping
        // Add 70 frames (total 71 frames recorded into 60-slot ring buffer)
        for i in 1..=70 {
            engine.update_profiler_metrics(500 + i as u64, 1000 + i as u64, 60, 60);
        }
        // Initial 1 + 70 = 71 samples. 71 % 60 = 11.
        assert_eq!(engine.profiler.history_idx, 11);
        for (i, &sample) in engine.profiler.history.iter().enumerate() {
            assert!(sample > 0, "Slot {} must hold a non-zero frame time", i);
        }

        // 4. Non-zero state hash generation & determinism
        let hash_a = engine.compute_state_hash();
        assert_ne!(hash_a, 0);
        assert_eq!(engine.profiler.state_hash, hash_a);

        // Advance simulation and verify hash changes deterministically
        engine.step();
        engine.update_profiler_metrics(400, 800, 60, 60);
        let hash_b = engine.compute_state_hash();
        assert_ne!(hash_b, 0);
        assert_ne!(hash_a, hash_b, "Advancing engine tick must yield distinct state hash");

        // 5. Verify profiler overlay renders cleanly onto FrameBuffer
        let mut fb = FrameBuffer::new(WIN_WIDTH, WIN_HEIGHT);
        engine.show_profiler = true;
        engine.render(&mut fb);
        // Verify overlay pixels in profiler region (x=108..604, y=12..112)
        let sample_pixel = fb.pixels[20 * WIN_WIDTH + 150];
        assert_ne!(sample_pixel, 0, "Overlay must draw pixels into framebuffer");
    }

    #[test]
    fn test_timestep_accumulator_logic() {
        let mut engine = DesktopEngine::new(0xABCD_1234);
        let mut accumulator = Duration::ZERO;
        let mut total_ticks = 0u32;

        // 1. Simulate 1,000ms with various fluctuating delta times (5ms, 16.6ms, 33ms, etc.)
        // Construct fluctuating delta times totaling exactly 1,000,000,000 ns (1 second)
        let pattern_ms = [5, 16, 33, 8, 24, 12, 17, 31, 14, 20];
        let mut remaining_ns = 1_000_000_000u64; // 1,000ms
        let mut idx = 0;
        let mut deltas = Vec::new();

        while remaining_ns > 0 {
            let ms = pattern_ms[idx % pattern_ms.len()];
            let ns = (ms as u64) * 1_000_000;
            if ns <= remaining_ns {
                deltas.push(Duration::from_nanos(ns));
                remaining_ns -= ns;
            } else {
                deltas.push(Duration::from_nanos(remaining_ns));
                remaining_ns = 0;
            }
            idx += 1;
        }

        for dt in deltas {
            let mut delta = dt;
            if delta > MAX_ACCUMULATOR {
                delta = MAX_ACCUMULATOR;
            }
            accumulator += delta;
            while accumulator >= DT_FIXED {
                engine.step();
                accumulator -= DT_FIXED;
                total_ticks += 1;
            }
        }

        // Exactly 60 ticks must execute for 1,000ms of simulated time
        assert_eq!(total_ticks, 60, "Fluctuating deltas totaling 1,000ms must execute exactly 60 ticks");
        assert_eq!(engine.tick, 60);

        // 2. Test clamping on 120ms lag spike
        let mut clamp_engine = DesktopEngine::new(0x5678);
        let mut clamp_acc = Duration::ZERO;
        let mut clamp_ticks = 0u32;

        // 120ms spike exceeds MAX_ACCUMULATOR (100ms)
        let spike = Duration::from_millis(120);
        let mut dt = spike;
        if dt > MAX_ACCUMULATOR {
            dt = MAX_ACCUMULATOR;
        }
        clamp_acc += dt;
        while clamp_acc >= DT_FIXED {
            clamp_engine.step();
            clamp_acc -= DT_FIXED;
            clamp_ticks += 1;
        }
        // Clamped to 100ms: 100ms / 16.666ms = 6 ticks (unclamped 120ms would be 7 ticks)
        assert_eq!(clamp_ticks, 6, "120ms spike clamped to 100ms must yield 6 ticks");

        // Complete 1,000ms effective time (100ms + 900ms = 1,000ms)
        let mut rem_effective = 900_000_000u64; // 900ms in ns
        let mut p_idx = 0;
        while rem_effective > 0 {
            let ms = pattern_ms[p_idx % pattern_ms.len()];
            let ns = (ms as u64) * 1_000_000;
            let step_ns = ns.min(rem_effective);
            rem_effective -= step_ns;

            let mut step_dt = Duration::from_nanos(step_ns);
            if step_dt > MAX_ACCUMULATOR {
                step_dt = MAX_ACCUMULATOR;
            }
            clamp_acc += step_dt;
            while clamp_acc >= DT_FIXED {
                clamp_engine.step();
                clamp_acc -= DT_FIXED;
                clamp_ticks += 1;
            }
            p_idx += 1;
        }

        assert_eq!(clamp_ticks, 60, "1,000ms effective simulation time with clamped spike must yield exactly 60 ticks");
        assert_eq!(clamp_engine.tick, 60);
    }
}
