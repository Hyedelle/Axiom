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
use axiom_render::FrameBuffer;

pub const WIN_WIDTH: usize = 640;
pub const WIN_HEIGHT: usize = 480;
pub const GRID_W: usize = 32;
pub const GRID_H: usize = 32;
pub const TILE_SIZE: i32 = 14;
pub const GRID_PIXEL_SIZE: i32 = GRID_W as i32 * TILE_SIZE; // 448 px
pub const GRID_OFFSET_X: i32 = (WIN_WIDTH as i32 - GRID_PIXEL_SIZE) / 2; // 96 px
pub const GRID_OFFSET_Y: i32 = (WIN_HEIGHT as i32 - GRID_PIXEL_SIZE) / 2; // 16 px

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

/// Native interactive desktop engine holding grid, wave director, and persistence.
pub struct DesktopEngine {
    pub grid: FactoryGrid<GRID_W, GRID_H>,
    pub director: WaveDirector<64>,
    pub tick: u64,
    pub notification: RefCell<Option<(String, u32)>>,
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

/// Machine direction notch on tile edges.
fn draw_facing(fb: &mut FrameBuffer, sx: i32, sy: i32, ts: i32, dir: Direction, color: u32) {
    let t = (ts / 6).max(2);
    match dir {
        Direction::North => fb.draw_rect(sx + 1, sy, ts - 2, t, color),
        Direction::East => fb.draw_rect(sx + ts - t, sy + 1, t, ts - 2, color),
        Direction::South => fb.draw_rect(sx + 1, sy + ts - t, ts - 2, t, color),
        Direction::West => fb.draw_rect(sx, sy + 1, t, ts - 2, color),
    }
}

/// Software rasterizer for factory cells.
fn draw_cell(
    fb: &mut FrameBuffer,
    cell: &FactoryStructure,
    sx: i32,
    sy: i32,
    ts: i32,
    phase: i32,
) {
    let inner = (ts - 2).max(1);
    let pip = (ts / 3).max(2);
    let pip_off = (ts - pip) / 2;

    match cell.kind {
        StructureKind::Empty => {}
        StructureKind::Deposit(d) => {
            fb.draw_rect(sx + 1, sy + 1, inner, inner, item_color(d.item));
            if d.remaining < 100 {
                fb.draw_rect(sx + pip_off, sy + pip_off, pip, pip, 0xFF0B1220);
            }
        }
        StructureKind::Belt(b) => {
            fb.draw_rect(sx + 1, sy + 1, inner, inner, 0xFF334155);
            draw_facing(fb, sx, sy, ts, b.direction, 0xFF64748B);
            if !b.item.is_none() {
                let is = (ts * 2 / 5).max(3);
                let travel = (ts - is).max(0);
                let off = (travel as i64 * b.progress.min(axiom_game::CYCLE) as i64
                    / axiom_game::CYCLE as i64) as i32;
                let mid = (ts - is) / 2;
                let (ix, iy) = match b.direction {
                    Direction::North => (sx + mid, sy + travel - off),
                    Direction::East => (sx + off, sy + mid),
                    Direction::South => (sx + mid, sy + off),
                    Direction::West => (sx + travel - off, sy + mid),
                };
                fb.draw_rect(ix, iy, is, is, item_color(b.item));
            }
        }
        StructureKind::Drill(d) => {
            fb.draw_rect(sx + 1, sy + 1, inner, inner, 0xFFB45309);
            let core = if d.remaining == 0 { 0xFF7F1D1D } else { 0xFF78350F };
            fb.draw_rect(sx + 2, sy + 2, (ts - 4).max(1), (ts - 4).max(1), core);
            if !d.ore.is_none() {
                fb.draw_rect(sx + pip_off, sy + pip_off, pip, pip, item_color(d.ore));
            }
            draw_facing(fb, sx, sy, ts, d.direction, 0xFFFCD34D);
            let bar = (inner as i64 * d.progress.min(axiom_game::CYCLE) as i64
                / axiom_game::CYCLE as i64) as i32;
            fb.draw_rect(sx + 1, sy + ts - 3, bar, 2, 0xFFFBBF24);
        }
        StructureKind::Inserter(i) => {
            fb.draw_rect(sx + 2, sy + 2, (ts - 4).max(1), (ts - 4).max(1), 0xFF6D28D9);
            let t = (ts / 5).max(2);
            let c = ts / 2;
            match i.direction {
                Direction::North => fb.draw_rect(sx + c - t / 2, sy, t, c, 0xFFC4B5FD),
                Direction::East => fb.draw_rect(sx + c, sy + c - t / 2, ts - c, t, 0xFFC4B5FD),
                Direction::South => fb.draw_rect(sx + c - t / 2, sy + c, t, ts - c, 0xFFC4B5FD),
                Direction::West => fb.draw_rect(sx, sy + c - t / 2, c, t, 0xFFC4B5FD),
            }
            if !i.held.is_none() {
                fb.draw_rect(sx + pip_off, sy + pip_off, pip, pip, item_color(i.held));
            }
        }
        StructureKind::Smelter(s) => {
            fb.draw_rect(sx + 1, sy + 1, inner, inner, 0xFF3F3F46);
            let mouth = (ts / 2).max(2);
            let mx = sx + (ts - mouth) / 2;
            let my = sy + ts - mouth - 1;
            fb.draw_rect(mx, my, mouth, mouth, 0xFF7F1D1D);
            let burning = s.progress > 0 || s.input_count > 0;
            if burning {
                let fire = if (phase / 4) % 2 == 0 { 0xFFF97316 } else { 0xFFFACC15 };
                let f = (mouth - 2).max(1);
                fb.draw_rect(mx + 1, my + 1, f, f, fire);
                let puff = (ts / 6).max(2);
                let rise = (phase % 8) * ts / 8;
                fb.draw_rect(sx + ts / 2 - puff / 2, sy - rise - puff, puff, puff, 0xFF9CA3AF);
            }
            if s.output_count > 0 {
                let bar = (inner as i64 * s.output_count as i64
                    / axiom_game::SMELTER_OUTPUT_CAP as i64) as i32;
                fb.draw_rect(sx + ts - 3, sy + 1 + inner - bar, 2, bar, item_color(s.output));
            }
        }
        StructureKind::Chest(c) => {
            fb.draw_rect(sx + 1, sy + 1, inner, inner, 0xFF92400E);
            fb.draw_rect(sx + 1, sy + 1, inner, (ts / 4).max(1), 0xFFD97706);
            if c.quantity > 0 {
                let h = (ts - 4).max(1);
                let fill = (h as i64 * c.quantity.min(100) as i64 / 100) as i32;
                fb.draw_rect(
                    sx + 2,
                    sy + 2 + h - fill,
                    (ts - 4).max(1),
                    fill,
                    item_color(c.item),
                );
            }
        }
        StructureKind::Wall(w) => {
            fb.draw_rect(sx, sy, ts, ts, 0xFF9CA3AF);
            fb.draw_rect(sx, sy + ts / 2, ts, 1, 0xFF6B7280);
            fb.draw_rect(sx + ts / 2, sy, 1, ts / 2, 0xFF6B7280);
            fb.draw_rect(sx + ts / 4, sy + ts / 2, 1, ts - ts / 2, 0xFF6B7280);
            draw_hp_bar(fb, sx, sy, ts, w.hp, w.max_hp);
        }
        StructureKind::Turret(t) => {
            fb.draw_rect(sx + 1, sy + 1, inner, inner, 0xFF14532D);
            fb.draw_rect(sx + 2, sy + 2, (ts - 4).max(1), (ts - 4).max(1), 0xFF16A34A);
            fb.draw_rect(sx + pip_off, sy + pip_off, pip, pip, 0xFF052E16);
            let pips = (t.ammo as i32 * 5 / axiom_game::TURRET_AMMO_CAP as i32).min(5);
            let pw = (ts / 7).max(1);
            for i in 0..pips {
                fb.draw_rect(sx + 2 + i * (pw + 1), sy + ts - pw - 2, pw, pw, 0xFFFDE047);
            }
            if t.ammo == 0 {
                fb.draw_rect(sx + 2, sy + ts - pw - 2, pw, pw, 0xFFEF4444);
            }
            draw_hp_bar(fb, sx, sy, ts, t.hp, t.max_hp);
        }
        StructureKind::BaseHeart(h) => {
            fb.draw_rect(sx, sy, ts, ts, 0xFF881337);
            fb.draw_rect(sx + 1, sy + 1, inner, inner, 0xFFBE123C);
            let pulse = pip + (phase / 8) % 2;
            let po = (ts - pulse) / 2;
            fb.draw_rect(sx + po, sy + po, pulse, pulse, 0xFFFFE4E6);
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
        _ => 0,
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

    // 60 FPS fixed target rate (~16.6ms)
    #[allow(deprecated)]
    window.limit_update_rate(Some(Duration::from_micros(16600)));
    window.set_target_fps(60);

    let seed = 0xABCD_9999_7777;
    let mut engine = DesktopEngine::new(seed);

    let mut framebuffer = FrameBuffer::new(WIN_WIDTH, WIN_HEIGHT);

    let mut cursor_x: usize = 16;
    let mut cursor_y: usize = 16;
    let mut selected_tool = SelectedTool::Turret;
    let mut current_direction = Direction::East;

    println!("🚀 [Axiom Desktop Studio] Launched in 640x480 native window at 60 FPS.");
    println!("   Controls: WASD (Move cursor), 1~8 (Select tool), R (Rotate), LMB (Build), RMB (Clear), F5 (QuickSave), F9 (QuickLoad), ESC (Exit)");

    while window.is_open() && !window.is_key_down(Key::Escape) {
        // --- 1. Keyboard Input Handling ---
        // WASD Cursor Navigation
        if window.is_key_pressed(Key::W, KeyRepeat::Yes) || window.is_key_pressed(Key::Up, KeyRepeat::Yes) {
            cursor_y = cursor_y.saturating_sub(1);
        }
        if window.is_key_pressed(Key::S, KeyRepeat::Yes) || window.is_key_pressed(Key::Down, KeyRepeat::Yes) {
            cursor_y = (cursor_y + 1).min(GRID_H - 1);
        }
        if window.is_key_pressed(Key::A, KeyRepeat::Yes) || window.is_key_pressed(Key::Left, KeyRepeat::Yes) {
            cursor_x = cursor_x.saturating_sub(1);
        }
        if window.is_key_pressed(Key::D, KeyRepeat::Yes) || window.is_key_pressed(Key::Right, KeyRepeat::Yes) {
            cursor_x = (cursor_x + 1).min(GRID_W - 1);
        }

        // 1~8 Tool Selection
        if window.is_key_pressed(Key::Key1, KeyRepeat::No) || window.is_key_pressed(Key::NumPad1, KeyRepeat::No) {
            selected_tool = SelectedTool::Belt;
        }
        if window.is_key_pressed(Key::Key2, KeyRepeat::No) || window.is_key_pressed(Key::NumPad2, KeyRepeat::No) {
            selected_tool = SelectedTool::Drill;
        }
        if window.is_key_pressed(Key::Key3, KeyRepeat::No) || window.is_key_pressed(Key::NumPad3, KeyRepeat::No) {
            selected_tool = SelectedTool::Smelter;
        }
        if window.is_key_pressed(Key::Key4, KeyRepeat::No) || window.is_key_pressed(Key::NumPad4, KeyRepeat::No) {
            selected_tool = SelectedTool::Chest;
        }
        if window.is_key_pressed(Key::Key5, KeyRepeat::No) || window.is_key_pressed(Key::NumPad5, KeyRepeat::No) {
            selected_tool = SelectedTool::Wall;
        }
        if window.is_key_pressed(Key::Key6, KeyRepeat::No) || window.is_key_pressed(Key::NumPad6, KeyRepeat::No) {
            selected_tool = SelectedTool::Turret;
        }
        if window.is_key_pressed(Key::Key7, KeyRepeat::No) || window.is_key_pressed(Key::NumPad7, KeyRepeat::No) {
            selected_tool = SelectedTool::Inserter;
        }
        if window.is_key_pressed(Key::Key8, KeyRepeat::No) || window.is_key_pressed(Key::NumPad8, KeyRepeat::No) {
            selected_tool = SelectedTool::Clear;
        }

        // R: Rotate Direction
        if window.is_key_pressed(Key::R, KeyRepeat::No) {
            current_direction = next_direction(current_direction);
        }

        // Space / Enter: Place structure at cursor
        if window.is_key_pressed(Key::Space, KeyRepeat::No) || window.is_key_pressed(Key::Enter, KeyRepeat::No) {
            place_structure(&mut engine.grid, cursor_x, cursor_y, selected_tool, current_direction);
        }

        // Delete / Backspace: Clear structure at cursor
        if window.is_key_pressed(Key::Delete, KeyRepeat::No) || window.is_key_pressed(Key::Backspace, KeyRepeat::No) {
            demolish_tile(&mut engine.grid, cursor_x, cursor_y);
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
                cursor_x = gx as usize;
                cursor_y = gy as usize;

                if window.get_mouse_down(MouseButton::Left) {
                    place_structure(&mut engine.grid, cursor_x, cursor_y, selected_tool, current_direction);
                }
                if window.get_mouse_down(MouseButton::Right) {
                    demolish_tile(&mut engine.grid, cursor_x, cursor_y);
                }
            }
        }

        // --- 3. Simulation Step ---
        engine.step();

        // --- 4. Render Scene into FrameBuffer ---
        framebuffer.clear(0xFF0A0F1D);
        let phase = (engine.tick & 0x7FFF_FFFF) as i32;
        let now = engine.director.current_tick as u32;

        // 4.1 Floor & Structures
        for y in 0..GRID_H {
            let sy = GRID_OFFSET_Y + y as i32 * TILE_SIZE;
            for x in 0..GRID_W {
                let sx = GRID_OFFSET_X + x as i32 * TILE_SIZE;
                let floor = if (x + y) % 2 == 0 { 0xFF141F30 } else { 0xFF0F1726 };
                framebuffer.draw_rect(sx, sy, TILE_SIZE, TILE_SIZE, floor);
                draw_cell(&mut framebuffer, &engine.grid.cells[y][x], sx, sy, TILE_SIZE, phase);
            }
        }

        // 4.2 Monsters from WaveDirector Arena
        let ms = (TILE_SIZE * 3 / 5).max(2);
        let mo = (TILE_SIZE - ms) / 2;
        for u in engine.director.arena.units.iter() {
            if !u.is_alive() || u.team != MONSTER_TEAM {
                continue;
            }
            let sx = GRID_OFFSET_X + u.pos.x.to_int() as i32 * TILE_SIZE;
            let sy = GRID_OFFSET_Y + u.pos.y.to_int() as i32 * TILE_SIZE;
            framebuffer.draw_rect(sx + mo, sy + mo, ms, ms, 0xFF7F1D1D);
            framebuffer.draw_rect(sx + mo + 1, sy + mo + 1, (ms - 2).max(1), (ms - 2).max(1), 0xFFDC2626);
            draw_hp_bar(&mut framebuffer, sx, sy, TILE_SIZE, u.current_hp, u.max_hp);
        }

        // 4.3 Turret Tracer Rounds
        if now > 0 {
            for y in 0..GRID_H {
                for x in 0..GRID_W {
                    let cell = &engine.grid.cells[y][x];
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
                    framebuffer.draw_rect(cx - flash / 2, cy - flash / 2, flash, flash, 0xFFFDE047);

                    let range_sq = t.range as i64 * t.range as i64;
                    let mut best: Option<(i32, i32)> = None;
                    let mut best_d = i64::MAX;
                    for u in engine.director.arena.units.iter() {
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
                        draw_line(&mut framebuffer, cx, cy, tx, ty, 0xFFFDE047);
                    }
                }
            }
        }

        // 4.4 Build Cursor Box
        let cx = GRID_OFFSET_X + cursor_x as i32 * TILE_SIZE;
        let cy = GRID_OFFSET_Y + cursor_y as i32 * TILE_SIZE;
        framebuffer.draw_rect(cx, cy, TILE_SIZE, 1, 0xFF38BDF8);
        framebuffer.draw_rect(cx, cy + TILE_SIZE - 1, TILE_SIZE, 1, 0xFF38BDF8);
        framebuffer.draw_rect(cx, cy, 1, TILE_SIZE, 0xFF38BDF8);
        framebuffer.draw_rect(cx + TILE_SIZE - 1, cy, 1, TILE_SIZE, 0xFF38BDF8);

        // 4.5 Left Sidebar HUD
        framebuffer.draw_rect(0, 0, 96, WIN_HEIGHT as i32, 0xFF0B1322);
        framebuffer.draw_rect(95, 0, 1, WIN_HEIGHT as i32, 0xFF1E293B);

        draw_text(&mut framebuffer, "AXIOM 1.3", 10, 12, 0xFF38BDF8);
        draw_text(&mut framebuffer, "STUDIO", 10, 22, 0xFF94A3B8);
        framebuffer.draw_rect(10, 32, 76, 1, 0xFF1E293B);

        draw_text(&mut framebuffer, "TOOLS:", 10, 40, 0xFFFBBF24);
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
            let is_sel = *t == selected_tool;
            if is_sel {
                framebuffer.draw_rect(8, ty - 2, 80, 14, 0xFF1E3A8A);
                framebuffer.draw_rect(8, ty - 2, 80, 1, 0xFF60A5FA);
                framebuffer.draw_rect(8, ty + 11, 80, 1, 0xFF60A5FA);
            }
            let color = if is_sel { 0xFFFFFFFF } else { 0xFF94A3B8 };
            draw_text(&mut framebuffer, t.label(), 12, ty + 2, color);
        }

        let dir_y = 210;
        framebuffer.draw_rect(10, dir_y, 76, 1, 0xFF1E293B);
        draw_text(&mut framebuffer, "DIR [R]:", 10, dir_y + 8, 0xFFFBBF24);
        draw_text(&mut framebuffer, direction_label(current_direction), 10, dir_y + 20, 0xFF34D399);

        // 4.6 Right Sidebar Telemetry & Controls
        framebuffer.draw_rect(544, 0, 96, WIN_HEIGHT as i32, 0xFF0B1322);
        framebuffer.draw_rect(544, 0, 1, WIN_HEIGHT as i32, 0xFF1E293B);

        let d_rep = engine.director.report(&engine.grid);
        let f_rep = engine.grid.report();

        draw_text(&mut framebuffer, "DEFENSE", 552, 12, 0xFFF43F5E);
        draw_text(&mut framebuffer, &format!("WAVE:{}", d_rep.wave_number), 552, 24, 0xFFE2E8F0);
        draw_text(&mut framebuffer, &format!("MOBS:{}", d_rep.monsters_alive), 552, 34, 0xFFF87171);
        draw_text(&mut framebuffer, &format!("KILL:{}", d_rep.total_killed), 552, 44, 0xFF34D399);
        draw_text(&mut framebuffer, &format!("AMMO:{}", d_rep.turret_ammo), 552, 54, 0xFFFBBF24);

        framebuffer.draw_rect(552, 68, 76, 1, 0xFF1E293B);
        draw_text(&mut framebuffer, "FACTORY", 552, 76, 0xFF60A5FA);
        draw_text(&mut framebuffer, &format!("MINE:{}", f_rep.total_mined), 552, 88, 0xFFCBD5E1);
        draw_text(&mut framebuffer, &format!("SMLT:{}", f_rep.total_smelted), 552, 98, 0xFFCBD5E1);
        draw_text(&mut framebuffer, &format!("STOR:{}", f_rep.total_stored), 552, 108, 0xFFCBD5E1);
        draw_text(&mut framebuffer, &format!("BELT:{}", f_rep.items_on_belts), 552, 118, 0xFFCBD5E1);

        framebuffer.draw_rect(552, 134, 76, 1, 0xFF1E293B);
        draw_text(&mut framebuffer, "CONTROLS", 552, 142, 0xFFFBBF24);
        draw_text(&mut framebuffer, "WASD:MOVE", 552, 154, 0xFF94A3B8);
        draw_text(&mut framebuffer, "LMB:BUILD", 552, 164, 0xFF94A3B8);
        draw_text(&mut framebuffer, "RMB:CLEAR", 552, 174, 0xFF94A3B8);
        draw_text(&mut framebuffer, "1-8:TOOL", 552, 184, 0xFF94A3B8);
        draw_text(&mut framebuffer, "R:ROTATE", 552, 194, 0xFF94A3B8);
        draw_text(&mut framebuffer, "F5:QSAVE", 552, 204, 0xFF38BDF8);
        draw_text(&mut framebuffer, "F9:QLOAD", 552, 214, 0xFF38BDF8);
        draw_text(&mut framebuffer, "ESC:EXIT", 552, 224, 0xFF94A3B8);

        // 4.7 Top Header Bar (Heart HP & Status)
        framebuffer.draw_rect(96, 0, 448, 16, 0xFF0F172A);
        draw_text(&mut framebuffer, "HEART:", 104, 5, 0xFFF43F5E);
        let hp_bar_w = 120;
        framebuffer.draw_rect(142, 4, hp_bar_w, 8, 0xFF450A0A);
        let hp_fill = if d_rep.heart_max_hp > 0 {
            (hp_bar_w * d_rep.heart_hp as i32) / d_rep.heart_max_hp as i32
        } else {
            0
        };
        framebuffer.draw_rect(142, 4, hp_fill, 8, 0xFFF43F5E);
        draw_text(
            &mut framebuffer,
            &format!("{}/{}", d_rep.heart_hp, d_rep.heart_max_hp),
            270,
            5,
            0xFFFFFFFF,
        );

        if d_rep.base_fallen {
            draw_text(&mut framebuffer, "[BASE DESTROYED]", 370, 5, 0xFFEF4444);
        } else {
            draw_text(
                &mut framebuffer,
                &format!("NEXT WAVE: TICK {}", engine.director.next_wave_tick),
                350,
                5,
                0xFFFBBF24,
            );
        }

        // 4.8 Notification Toast / Alert Banner
        if let Some((ref msg, _)) = engine.notification() {
            let box_w = (msg.len() as i32 * 4 + 16).max(180);
            let box_h = 16;
            let box_x = GRID_OFFSET_X + (GRID_PIXEL_SIZE - box_w) / 2;
            let box_y = 22;

            framebuffer.draw_rect(box_x, box_y, box_w, box_h, 0xFF042F2E);
            framebuffer.draw_rect(box_x, box_y, box_w, 1, 0xFF14B8A6);
            framebuffer.draw_rect(box_x, box_y + box_h - 1, box_w, 1, 0xFF14B8A6);
            framebuffer.draw_rect(box_x, box_y, 1, box_h, 0xFF14B8A6);
            framebuffer.draw_rect(box_x + box_w - 1, box_y, 1, box_h, 0xFF14B8A6);

            let text_x = box_x + (box_w - (msg.len() as i32 * 4)) / 2;
            let text_y = box_y + 5;
            draw_text(&mut framebuffer, msg, text_x, text_y, 0xFF5EEAD4);
        }

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
}
