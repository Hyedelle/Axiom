//! Infinite open-world camera viewport & streaming renderer for the Desktop Studio.
//!
//! Bridges the Phase 3 core (`ChunkManager` streaming, 8-biome PCG, resource veins,
//! `MonsterSpawner`, `TerritoryDefense`) onto the 704x544 `minifb` framebuffer.
//!
//! * [`WorldCamera`]: pure-integer camera. `(world_x, world_y)` is the world tile shown at the
//!   centre of the viewport; panning is done in whole tiles (keyboard) or by accumulating
//!   mouse-drag pixels into whole tiles (middle / right drag).
//! * [`OpenWorldView`]: owns the streaming world and re-centres the chunk manager whenever the
//!   camera crosses a chunk boundary, so chunks are loaded / evicted as the player explores.
//! * Rasteriser: visits only the `32x32` tile window visible in the viewport, reads tile, biome
//!   and deposit layers straight out of the loaded chunk slab (one O(1) slot lookup per tile) and
//!   paints an 8-biome palette, ore nuggets, the stronghold, defence posts and monsters.
//!
//! No floating point anywhere: every coordinate transform is `i64`/`i32` arithmetic.

use axiom_core::{
    BiomeKind, ChunkCoord, ChunkManager, ChunkStreamStats, DefenseTickReport, MonsterArchetype,
    MonsterSpawner, MonsterState, ResourceDeposit, SpawnStats, TerritoryDefense, TileType,
    DEFAULT_CHUNK_SIZE, DEFAULT_MAX_CHUNKS, DEFAULT_SPAWN_MIN_OPEN_AREA, DEFAULT_STRONGHOLD_HP,
    DEFAULT_TERRITORY_RADIUS,
};
use axiom_render::{get_heart_sprite, get_monster_sprite_by_kind, get_turret_sprite, FrameBuffer, DIR_EAST};

use crate::desktop::{
    draw_hp_bar, draw_rect_blend, draw_text, GRID_OFFSET_X, GRID_OFFSET_Y, GRID_PIXEL_SIZE, TILE_SIZE,
};

/// Viewport rectangle (pixels) inside the 704x544 window: the 512x512 central panel.
pub const VIEW_X: i32 = GRID_OFFSET_X;
pub const VIEW_Y: i32 = GRID_OFFSET_Y;
pub const VIEW_W: i32 = GRID_PIXEL_SIZE;
pub const VIEW_H: i32 = GRID_PIXEL_SIZE;
/// Tiles visible along each axis (512 px / 16 px).
pub const VIEW_TILES_W: i64 = (VIEW_W / TILE_SIZE) as i64;
pub const VIEW_TILES_H: i64 = (VIEW_H / TILE_SIZE) as i64;
/// Pixel offset of the camera tile's top-left corner inside the viewport.
pub const VIEW_CENTER_PX_X: i32 = (VIEW_TILES_W as i32 / 2) * TILE_SIZE;
pub const VIEW_CENTER_PX_Y: i32 = (VIEW_TILES_H as i32 / 2) * TILE_SIZE;

/// Default camera pan speed in tiles per key repeat / tick.
pub const CAMERA_SPEED: i64 = 1;
/// Multiplier applied while Shift is held.
pub const CAMERA_SPRINT_MULT: i64 = 4;
/// Camera coordinates are clamped so chunk coordinates always fit `i32` with margin.
pub const CAMERA_LIMIT: i64 = 1 << 30;

/// Chebyshev chunk radius streamed around the camera (5x5 = 25 chunks of 64 slots).
pub const WORLD_VIEW_RADIUS: i32 = 2;
pub const WORLD_MONSTER_CAP: usize = 256;
pub const WORLD_DEFENSE_POSTS: usize = 16;
/// Defence post layout around the stronghold heart (dx, dy, damage, range, cooldown).
pub const WORLD_DEFENSE_LAYOUT: [(i32, i32, u16, u8, u8); 4] = [
    (6, 0, 30, 5, 2),
    (-6, 0, 30, 5, 2),
    (0, 6, 30, 5, 2),
    (0, -6, 30, 5, 2),
];

pub type WorldChunkManager = ChunkManager<DEFAULT_CHUNK_SIZE, DEFAULT_MAX_CHUNKS>;

/// Pure-integer world camera (one cache line).
#[repr(C, align(64))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorldCamera {
    /// World tile coordinate shown at the viewport centre.
    pub world_x: i64,
    pub world_y: i64,
    /// Tiles moved per pan step.
    pub speed: i64,
    pub sprint_mult: i64,
    /// Mouse-drag anchor (window pixels) while a drag is active.
    pub drag_anchor_x: i32,
    pub drag_anchor_y: i32,
    pub dragging: bool,
    pub _pad: [u8; 7],
    /// Total tiles panned (telemetry).
    pub total_pan_tiles: u64,
}

const _: () = assert!(core::mem::size_of::<WorldCamera>() == 64);

impl Default for WorldCamera {
    fn default() -> Self {
        Self::new(0, 0)
    }
}

impl WorldCamera {
    pub const fn new(world_x: i64, world_y: i64) -> Self {
        Self {
            world_x,
            world_y,
            speed: CAMERA_SPEED,
            sprint_mult: CAMERA_SPRINT_MULT,
            drag_anchor_x: 0,
            drag_anchor_y: 0,
            dragging: false,
            _pad: [0; 7],
            total_pan_tiles: 0,
        }
    }

    #[inline(always)]
    const fn clamp_axis(v: i64) -> i64 {
        if v > CAMERA_LIMIT {
            CAMERA_LIMIT
        } else if v < -CAMERA_LIMIT {
            -CAMERA_LIMIT
        } else {
            v
        }
    }

    /// Moves the camera by `(dx, dy)` pan steps (each step = `speed` tiles, x `sprint_mult` when sprinting).
    pub fn pan(&mut self, dx: i64, dy: i64, sprint: bool) {
        let mult = if sprint { self.sprint_mult } else { 1 };
        let step = self.speed * mult;
        self.pan_tiles(dx * step, dy * step);
    }

    /// Moves the camera by an exact number of tiles.
    pub fn pan_tiles(&mut self, dx: i64, dy: i64) {
        let nx = Self::clamp_axis(self.world_x + dx);
        let ny = Self::clamp_axis(self.world_y + dy);
        self.total_pan_tiles += (nx - self.world_x).unsigned_abs() + (ny - self.world_y).unsigned_abs();
        self.world_x = nx;
        self.world_y = ny;
    }

    /// Teleports the camera.
    pub fn set_position(&mut self, world_x: i64, world_y: i64) {
        self.world_x = Self::clamp_axis(world_x);
        self.world_y = Self::clamp_axis(world_y);
    }

    /// Chunk coordinate the camera centre is in.
    pub fn chunk_coord(&self) -> ChunkCoord {
        ChunkCoord::from_world_tile(self.world_x, self.world_y, DEFAULT_CHUNK_SIZE).0
    }

    /// Inclusive first / exclusive last visible world tile on each axis.
    pub const fn visible_tile_range(&self) -> (i64, i64, i64, i64) {
        let x0 = self.world_x - VIEW_TILES_W / 2;
        let y0 = self.world_y - VIEW_TILES_H / 2;
        (x0, y0, x0 + VIEW_TILES_W, y0 + VIEW_TILES_H)
    }

    /// World tile ➔ window pixel of the tile's top-left corner.
    pub const fn world_to_screen(&self, wx: i64, wy: i64) -> (i32, i32) {
        let sx = VIEW_X + VIEW_CENTER_PX_X + ((wx - self.world_x) as i32) * TILE_SIZE;
        let sy = VIEW_Y + VIEW_CENTER_PX_Y + ((wy - self.world_y) as i32) * TILE_SIZE;
        (sx, sy)
    }

    /// Window pixel ➔ world tile (floor division so negative offsets round down).
    pub const fn screen_to_world(&self, px: i32, py: i32) -> (i64, i64) {
        let rel_x = (px - VIEW_X - VIEW_CENTER_PX_X) as i64;
        let rel_y = (py - VIEW_Y - VIEW_CENTER_PX_Y) as i64;
        (
            self.world_x + rel_x.div_euclid(TILE_SIZE as i64),
            self.world_y + rel_y.div_euclid(TILE_SIZE as i64),
        )
    }

    /// Whether a window pixel lies inside the viewport panel.
    pub const fn contains_pixel(px: i32, py: i32) -> bool {
        px >= VIEW_X && px < VIEW_X + VIEW_W && py >= VIEW_Y && py < VIEW_Y + VIEW_H
    }

    /// Starts a mouse drag pan at window pixel `(px, py)`.
    pub fn begin_drag(&mut self, px: i32, py: i32) {
        self.dragging = true;
        self.drag_anchor_x = px;
        self.drag_anchor_y = py;
    }

    /// Continues a drag: every full tile of pixel travel pans the camera one tile the
    /// opposite way (grab-and-pull). Returns the tiles panned on each axis.
    pub fn drag_to(&mut self, px: i32, py: i32) -> (i64, i64) {
        if !self.dragging {
            return (0, 0);
        }
        let dx_px = px - self.drag_anchor_x;
        let dy_px = py - self.drag_anchor_y;
        let tx = (dx_px / TILE_SIZE) as i64;
        let ty = (dy_px / TILE_SIZE) as i64;
        if tx != 0 || ty != 0 {
            self.drag_anchor_x += (tx as i32) * TILE_SIZE;
            self.drag_anchor_y += (ty as i32) * TILE_SIZE;
            self.pan_tiles(-tx, -ty);
        }
        (-tx, -ty)
    }

    pub fn end_drag(&mut self) {
        self.dragging = false;
    }
}

/// Sprite kind (0 goblin / 1 golem / 2 runner) and tint used to draw each archetype.
pub const fn archetype_sprite_style(archetype: MonsterArchetype) -> (u8, u32) {
    match archetype {
        MonsterArchetype::Goblin => (0, 0xFFFFFFFF),
        MonsterArchetype::TimberWolf => (2, 0xFFB8C4D6),
        MonsterArchetype::SandScorpion => (2, 0xFFF5C76A),
        MonsterArchetype::FrostWraith => (0, 0xFF9FE8FF),
        MonsterArchetype::MagmaGolem => (1, 0xFFFF7A3C),
        MonsterArchetype::BogLurker => (1, 0xFF7CB342),
        MonsterArchetype::DeepLeviathan => (1, 0xFF3F8CFF),
        MonsterArchetype::SporeStalker => (0, 0xFFE07BFF),
    }
}

/// Terrain palette: `(biome, tile, checker)` ➔ ARGB. Two shades per biome/tile for a subtle
/// checkerboard so camera motion is readable even on flat ground.
pub const fn terrain_color(biome: BiomeKind, tile: TileType, checker: bool) -> u32 {
    let pair: (u32, u32) = match tile {
        TileType::Water => match biome {
            BiomeKind::OceanDepths => (0xFF0B3D91, 0xFF0A3684),
            BiomeKind::SwampMarsh => (0xFF2F5D4E, 0xFF2A5446),
            BiomeKind::FrozenTundra => (0xFF6FA8DC, 0xFF66A0D4),
            BiomeKind::VolcanicCrag => (0xFFE0501C, 0xFFD4480F), // lava / sulphur pools
            _ => (0xFF1E6FD9, 0xFF1A66CC),
        },
        TileType::Rock => match biome {
            BiomeKind::VolcanicCrag => (0xFF3B2B2B, 0xFF332424),
            BiomeKind::FrozenTundra => (0xFF9FB3C8, 0xFF93A8BE),
            BiomeKind::AridDesert => (0xFFB58C5A, 0xFFAA8252),
            BiomeKind::MushroomGlade => (0xFF5E4B6E, 0xFF554364),
            _ => (0xFF6F7A86, 0xFF66717C),
        },
        TileType::Forest => match biome {
            BiomeKind::TemperatePlains => (0xFF2E7D32, 0xFF29702D),
            BiomeKind::BorealForest => (0xFF1B5E20, 0xFF17521C),
            BiomeKind::AridDesert => (0xFF8D9B3A, 0xFF829033), // scrub / oasis palms
            BiomeKind::FrozenTundra => (0xFF5C7F74, 0xFF53766B),
            BiomeKind::VolcanicCrag => (0xFF4E3B2A, 0xFF463425),
            BiomeKind::SwampMarsh => (0xFF4F6B2A, 0xFF476125), // reeds
            BiomeKind::OceanDepths => (0xFF2E6B5E, 0xFF286054), // kelp
            BiomeKind::MushroomGlade => (0xFF9C4DCC, 0xFF8E44BA), // glowing caps
        },
        TileType::Plains => match biome {
            BiomeKind::TemperatePlains => (0xFF66BB6A, 0xFF5FAF63),
            BiomeKind::BorealForest => (0xFF4F8A52, 0xFF487F4B),
            BiomeKind::AridDesert => (0xFFE3C784, 0xFFD9BC79),
            BiomeKind::FrozenTundra => (0xFFE8F1F8, 0xFFDCE7F0),
            BiomeKind::VolcanicCrag => (0xFF5A4A44, 0xFF51423D),
            BiomeKind::SwampMarsh => (0xFF6B5B3E, 0xFF615236),
            BiomeKind::OceanDepths => (0xFFC2B280, 0xFFB8A878), // sand bars
            BiomeKind::MushroomGlade => (0xFF6E4A8C, 0xFF63427F),
        },
    };
    if checker {
        pair.0
    } else {
        pair.1
    }
}

/// Ore nugget colour per deposit kind.
pub const fn deposit_color(kind: ResourceDeposit) -> u32 {
    match kind {
        ResourceDeposit::None => 0,
        ResourceDeposit::IronOre => 0xFFCFD8DC,
        ResourceDeposit::CopperOre => 0xFFE08A3C,
        ResourceDeposit::Coal => 0xFF1F1F1F,
        ResourceDeposit::Stone => 0xFF9E9E9E,
        ResourceDeposit::Uranium => 0xFF7CFF3A,
        ResourceDeposit::CrudeOil => 0xFF2B1A3F,
    }
}

/// One rasterised world cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorldCell {
    pub tile: TileType,
    pub biome: BiomeKind,
    pub deposit: ResourceDeposit,
    pub richness: u8,
    pub loaded: bool,
}

/// Reads tile / biome / deposit for a world coordinate with a single chunk-slot lookup.
/// Falls back to the manager's delta + procedural path for chunks outside the view ring.
pub fn sample_world_cell(mgr: &WorldChunkManager, wx: i64, wy: i64) -> WorldCell {
    let (coord, lx, ly) = ChunkCoord::from_world_tile(wx, wy, DEFAULT_CHUNK_SIZE);
    if let Some(chunk) = mgr.chunk_at(coord) {
        let (deposit, richness) = axiom_core::unpack_resource(chunk.resources[ly][lx]);
        return WorldCell {
            tile: chunk.tiles[ly][lx],
            biome: BiomeKind::from_u8(chunk.biomes[ly][lx]),
            deposit,
            richness,
            loaded: true,
        };
    }
    let (deposit, richness) = mgr.deposit_at(wx, wy);
    WorldCell {
        tile: mgr.tile_at(wx, wy),
        biome: mgr.biome_at(wx, wy),
        deposit,
        richness,
        loaded: false,
    }
}

/// Streaming open world + camera + ambient monsters + stronghold, owned by the desktop engine.
pub struct OpenWorldView {
    pub chunk_mgr: Box<WorldChunkManager>,
    pub spawner: Box<MonsterSpawner<WORLD_MONSTER_CAP>>,
    pub defense: TerritoryDefense<WORLD_DEFENSE_POSTS>,
    pub camera: WorldCamera,
    pub tick: u64,
    pub seed: u64,
    pub stream_stats: ChunkStreamStats,
    pub spawn_stats: SpawnStats,
    pub defense_report: DefenseTickReport,
    /// Number of times the chunk window was re-centred.
    pub stream_updates: u64,
    pub show_chunk_grid: bool,
}

impl OpenWorldView {
    /// Builds the world around the origin, anchors the stronghold on open ground and
    /// parks the camera on the heart.
    pub fn new(seed: u64) -> Self {
        let mut chunk_mgr = Box::new(WorldChunkManager::new(seed, WORLD_VIEW_RADIUS));
        let stream_stats = chunk_mgr.update_center(ChunkCoord::new(0, 0), 0);
        let (hx, hy) = chunk_mgr.find_open_spawn(
            DEFAULT_CHUNK_SIZE,
            DEFAULT_CHUNK_SIZE,
            DEFAULT_SPAWN_MIN_OPEN_AREA,
        );

        let mut spawner = Box::new(MonsterSpawner::<WORLD_MONSTER_CAP>::new(seed));
        spawner.set_safe_zone(hx as i32, hy as i32, DEFAULT_TERRITORY_RADIUS + 8);

        let mut defense = TerritoryDefense::<WORLD_DEFENSE_POSTS>::new(
            seed,
            hx as i32,
            hy as i32,
            DEFAULT_TERRITORY_RADIUS,
            DEFAULT_STRONGHOLD_HP,
        );
        for &(dx, dy, dmg, range, cd) in WORLD_DEFENSE_LAYOUT.iter() {
            defense.add_post(hx as i32 + dx, hy as i32 + dy, dmg, range, cd);
        }

        let mut view = Self {
            chunk_mgr,
            spawner,
            defense,
            camera: WorldCamera::new(hx, hy),
            tick: 0,
            seed,
            stream_stats,
            spawn_stats: SpawnStats::default(),
            defense_report: DefenseTickReport::default(),
            stream_updates: 1,
            show_chunk_grid: true,
        };
        view.sync_streaming();
        view
    }

    /// Re-centres the chunk window on the camera's chunk if it changed. Returns `true` when
    /// a streaming update ran.
    pub fn sync_streaming(&mut self) -> bool {
        let cam_chunk = self.camera.chunk_coord();
        if cam_chunk == self.chunk_mgr.center_chunk {
            return false;
        }
        self.stream_stats = self.chunk_mgr.update_center(cam_chunk, self.tick);
        self.stream_updates += 1;
        true
    }

    /// Pans the camera and streams chunks immediately so the next frame never shows a hole.
    pub fn pan_camera(&mut self, dx: i64, dy: i64, sprint: bool) -> bool {
        self.camera.pan(dx, dy, sprint);
        self.sync_streaming()
    }

    /// One deterministic world tick: streaming ➔ ambient monsters ➔ stronghold defence.
    pub fn step(&mut self) {
        self.tick += 1;
        self.sync_streaming();
        self.spawn_stats = self.spawner.update(&self.chunk_mgr, self.tick);
        self.defense_report = self.defense.step(&mut self.spawner, &self.chunk_mgr, self.tick);
    }

    pub fn loaded_chunks(&self) -> usize {
        self.chunk_mgr.loaded_count()
    }

    pub const fn max_chunks(&self) -> usize {
        DEFAULT_MAX_CHUNKS
    }

    pub fn monsters_alive(&self) -> usize {
        self.spawner.alive()
    }

    /// Biome under the camera centre.
    pub fn camera_biome(&self) -> BiomeKind {
        self.chunk_mgr.biome_at(self.camera.world_x, self.camera.world_y)
    }

    /// Deterministic hash of the whole open-world state.
    pub fn compute_hash(&self) -> u64 {
        self.chunk_mgr.compute_hash()
            ^ self.spawner.compute_hash().rotate_left(13)
            ^ self.defense.compute_hash().rotate_left(29)
            ^ (self.camera.world_x as u64).rotate_left(41)
            ^ (self.camera.world_y as u64).rotate_left(53)
            ^ self.tick.rotate_left(7)
    }

    // ---- rendering --------------------------------------------------------

    /// Rasterises terrain, deposits, stronghold, posts, monsters and the camera reticle.
    pub fn render(&self, fb: &mut FrameBuffer, phase: u64) {
        let cam = &self.camera;
        let (x0, y0, x1, y1) = cam.visible_tile_range();

        // 1. Terrain + deposits (32x32 visible tiles, single slot lookup each)
        let mut wy = y0;
        while wy < y1 {
            let mut wx = x0;
            while wx < x1 {
                let cell = sample_world_cell(&self.chunk_mgr, wx, wy);
                let (sx, sy) = cam.world_to_screen(wx, wy);
                let checker = ((wx + wy) & 1) == 0;
                let mut color = terrain_color(cell.biome, cell.tile, checker);
                if !cell.loaded {
                    // Unstreamed fallback (only reachable at the slab limit): dim it.
                    color = (color >> 1) & 0x7F7F7F7F | 0xFF00_0000;
                }
                fb.draw_rect(sx, sy, TILE_SIZE, TILE_SIZE, color);

                if !cell.deposit.is_none() {
                    draw_deposit_nuggets(fb, sx, sy, cell.deposit, cell.richness, wx, wy);
                }
                wx += 1;
            }
            wy += 1;
        }

        // 2. Chunk boundary grid
        if self.show_chunk_grid {
            let s = DEFAULT_CHUNK_SIZE as i64;
            let mut gx = x0.div_euclid(s) * s;
            while gx < x1 {
                if gx >= x0 {
                    let (sx, _) = cam.world_to_screen(gx, y0);
                    draw_rect_blend(fb, sx, VIEW_Y, 1, VIEW_H, 0x60FFFFFF);
                }
                gx += s;
            }
            let mut gy = y0.div_euclid(s) * s;
            while gy < y1 {
                if gy >= y0 {
                    let (_, sy) = cam.world_to_screen(x0, gy);
                    draw_rect_blend(fb, VIEW_X, sy, VIEW_W, 1, 0x60FFFFFF);
                }
                gy += s;
            }
        }

        // 3. Stronghold territory ring + heart + posts
        let d = &self.defense;
        let r = d.territory_radius as i64;
        let (hx, hy) = (d.heart_x as i64, d.heart_y as i64);
        let (tl_x, tl_y) = cam.world_to_screen(hx - r, hy - r);
        let (br_x, br_y) = cam.world_to_screen(hx + r + 1, hy + r + 1);
        let ring_color = if d.breached { 0xA0EF4444 } else { 0x90FBBF24 };
        draw_clipped_outline(fb, tl_x, tl_y, br_x - tl_x, br_y - tl_y, ring_color);

        for post in d.posts.iter().take(d.post_count as usize) {
            if !post.active {
                continue;
            }
            if let Some((sx, sy)) = self.visible_screen_pos(post.x as i64, post.y as i64) {
                fb.draw_sprite_16x16(sx, sy, get_turret_sprite(DIR_EAST));
            }
        }

        if let Some((sx, sy)) = self.visible_screen_pos(hx, hy) {
            if d.breached {
                fb.draw_sprite_16x16_tinted(sx, sy, get_heart_sprite(phase), 0xFF7F7F7F);
            } else {
                fb.draw_sprite_16x16(sx, sy, get_heart_sprite(phase));
            }
            let hp = d.hp.min(u16::MAX as u32) as u16;
            let max_hp = d.max_hp.min(u16::MAX as u32) as u16;
            draw_hp_bar(fb, sx, sy, TILE_SIZE, hp, max_hp);
        }

        // 4. Monsters
        for m in self.spawner.monsters.iter() {
            if !m.is_alive() {
                continue;
            }
            let Some((sx, sy)) = self.visible_screen_pos(m.tile_x as i64, m.tile_y as i64) else {
                continue;
            };
            let (kind, tint) = archetype_sprite_style(m.archetype);
            let sprite = get_monster_sprite_by_kind(kind, phase);
            if tint == 0xFFFFFFFF {
                fb.draw_sprite_16x16(sx, sy, sprite);
            } else {
                fb.draw_sprite_16x16_tinted(sx, sy, sprite, tint);
            }
            if m.hp < m.max_hp {
                draw_hp_bar(fb, sx, sy, TILE_SIZE, m.hp, m.max_hp);
            }
            if matches!(m.state, MonsterState::Hunting) {
                fb.draw_rect(sx + TILE_SIZE - 4, sy, 3, 3, 0xFFEF4444);
            }
            if m.tier > 0 {
                let bars = (m.tier as i32).min(5);
                fb.draw_rect(sx, sy + TILE_SIZE - 2, bars * 3, 2, 0xFFFBBF24);
            }
        }

        // 5. Camera reticle on the centre tile
        let (cx, cy) = cam.world_to_screen(cam.world_x, cam.world_y);
        fb.draw_rect(cx, cy, TILE_SIZE, 1, 0xFF38BDF8);
        fb.draw_rect(cx, cy + TILE_SIZE - 1, TILE_SIZE, 1, 0xFF38BDF8);
        fb.draw_rect(cx, cy, 1, TILE_SIZE, 0xFF38BDF8);
        fb.draw_rect(cx + TILE_SIZE - 1, cy, 1, TILE_SIZE, 0xFF38BDF8);
    }

    /// Screen position of a world tile if any part of it lies inside the viewport.
    pub fn visible_screen_pos(&self, wx: i64, wy: i64) -> Option<(i32, i32)> {
        let (x0, y0, x1, y1) = self.camera.visible_tile_range();
        if wx < x0 || wx >= x1 || wy < y0 || wy >= y1 {
            return None;
        }
        Some(self.camera.world_to_screen(wx, wy))
    }

    /// Right-sidebar world telemetry block starting at `(rx, y)`.
    pub fn render_sidebar(&self, fb: &mut FrameBuffer, rx: i32, y: i32) {
        let cam_chunk = self.camera.chunk_coord();
        draw_text(fb, "WORLD", rx, y, 0xFF34D399);
        draw_text(fb, &format!("X:{}", self.camera.world_x), rx, y + 12, 0xFFE2E8F0);
        draw_text(fb, &format!("Y:{}", self.camera.world_y), rx, y + 22, 0xFFE2E8F0);
        draw_text(fb, &format!("CX:{}", cam_chunk.x), rx, y + 32, 0xFF94A3B8);
        draw_text(fb, &format!("CY:{}", cam_chunk.y), rx, y + 42, 0xFF94A3B8);
        draw_text(fb, biome_short_name(self.camera_biome()), rx, y + 52, 0xFFFBBF24);

        fb.draw_rect(rx, y + 66, 76, 1, 0xFF1E293B);
        draw_text(fb, "STREAM", rx, y + 74, 0xFF60A5FA);
        draw_text(
            fb,
            &format!("CHNK:{}/{}", self.loaded_chunks(), self.max_chunks()),
            rx,
            y + 86,
            0xFFCBD5E1,
        );
        draw_text(fb, &format!("GEN:{}", self.chunk_mgr.total_generated), rx, y + 96, 0xFFCBD5E1);
        draw_text(fb, &format!("EVCT:{}", self.chunk_mgr.total_evicted), rx, y + 106, 0xFFCBD5E1);
        draw_text(fb, &format!("MOBS:{}", self.monsters_alive()), rx, y + 116, 0xFFF87171);
        draw_text(fb, &format!("WAVE:{}", self.defense.wave_index), rx, y + 126, 0xFFF43F5E);
        draw_text(fb, &format!("KILL:{}", self.defense.kills), rx, y + 136, 0xFF34D399);
    }

    /// Top header text for world mode.
    pub fn render_header(&self, fb: &mut FrameBuffer) {
        let d = &self.defense;
        draw_text(fb, "HEART:", 104, 5, 0xFFF43F5E);
        let hp_bar_w = 120;
        fb.draw_rect(142, 4, hp_bar_w, 8, 0xFF450A0A);
        let hp_fill = if d.max_hp > 0 {
            ((hp_bar_w as i64 * d.hp as i64) / d.max_hp as i64) as i32
        } else {
            0
        };
        fb.draw_rect(142, 4, hp_fill, 8, 0xFFF43F5E);
        draw_text(fb, &format!("{}/{}", d.hp, d.max_hp), 270, 5, 0xFFFFFFFF);
        if d.breached {
            draw_text(fb, "[STRONGHOLD BREACHED]", 370, 5, 0xFFEF4444);
        } else {
            draw_text(
                fb,
                &format!("NEXT WAVE: TICK {}", d.next_wave_tick),
                350,
                5,
                0xFFFBBF24,
            );
        }
    }
}

/// Draws 1-3 ore nuggets inside a tile; count scales with richness and the pattern is a pure
/// hash of the tile coordinate so it never flickers.
fn draw_deposit_nuggets(
    fb: &mut FrameBuffer,
    sx: i32,
    sy: i32,
    kind: ResourceDeposit,
    richness: u8,
    wx: i64,
    wy: i64,
) {
    let color = deposit_color(kind);
    let count = 1 + (richness as i32 / 11).min(2); // richness 0..=31 ➔ 1..=3 nuggets
    let h = axiom_core::hash2d(wx as i32, wy as i32, 0x4E55_4747_4554); // "NUGGET"
    for i in 0..count {
        let bits = h >> (i * 8);
        let ox = 2 + ((bits & 0x7) as i32); // 2..=9
        let oy = 2 + (((bits >> 3) & 0x7) as i32);
        fb.draw_rect(sx + ox, sy + oy, 4, 4, color);
        fb.draw_rect(sx + ox + 1, sy + oy + 1, 2, 2, color | 0xFF40_4040);
    }
}

/// 1-px alpha-blended rectangle outline clipped to the viewport panel.
fn draw_clipped_outline(fb: &mut FrameBuffer, x: i32, y: i32, w: i32, h: i32, color: u32) {
    let x_end = x + w;
    let y_end = y + h;
    let vx1 = VIEW_X + VIEW_W;
    let vy1 = VIEW_Y + VIEW_H;
    let cx0 = x.max(VIEW_X);
    let cx1 = x_end.min(vx1);
    let cy0 = y.max(VIEW_Y);
    let cy1 = y_end.min(vy1);
    if cx1 <= cx0 || cy1 <= cy0 {
        return;
    }
    if y >= VIEW_Y && y < vy1 {
        draw_rect_blend(fb, cx0, y, cx1 - cx0, 1, color);
    }
    if y_end - 1 >= VIEW_Y && y_end - 1 < vy1 {
        draw_rect_blend(fb, cx0, y_end - 1, cx1 - cx0, 1, color);
    }
    if x >= VIEW_X && x < vx1 {
        draw_rect_blend(fb, x, cy0, 1, cy1 - cy0, color);
    }
    if x_end - 1 >= VIEW_X && x_end - 1 < vx1 {
        draw_rect_blend(fb, x_end - 1, cy0, 1, cy1 - cy0, color);
    }
}

/// Sidebar-width biome label (max 9 glyphs of the 3x5 font).
pub const fn biome_short_name(biome: BiomeKind) -> &'static str {
    match biome {
        BiomeKind::TemperatePlains => "PLAINS",
        BiomeKind::BorealForest => "BOREAL",
        BiomeKind::AridDesert => "DESERT",
        BiomeKind::FrozenTundra => "TUNDRA",
        BiomeKind::VolcanicCrag => "VOLCANIC",
        BiomeKind::SwampMarsh => "SWAMP",
        BiomeKind::OceanDepths => "OCEAN",
        BiomeKind::MushroomGlade => "MUSHROOM",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::desktop::{WIN_HEIGHT, WIN_WIDTH};

    fn fnv(fb: &FrameBuffer) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for &p in fb.as_slice() {
            h ^= p as u64;
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
        h
    }

    #[test]
    fn test_world_camera_integer_transforms() {
        let mut cam = WorldCamera::new(100, -50);
        // Centre tile lands on the viewport centre pixel.
        assert_eq!(cam.world_to_screen(100, -50), (VIEW_X + VIEW_CENTER_PX_X, VIEW_Y + VIEW_CENTER_PX_Y));
        // Round-trip for every pixel inside the viewport.
        for py in (VIEW_Y..VIEW_Y + VIEW_H).step_by(7) {
            for px in (VIEW_X..VIEW_X + VIEW_W).step_by(5) {
                let (wx, wy) = cam.screen_to_world(px, py);
                let (sx, sy) = cam.world_to_screen(wx, wy);
                assert!(px >= sx && px < sx + TILE_SIZE, "px {} sx {}", px, sx);
                assert!(py >= sy && py < sy + TILE_SIZE, "py {} sy {}", py, sy);
            }
        }
        let (x0, y0, x1, y1) = cam.visible_tile_range();
        assert_eq!((x1 - x0, y1 - y0), (VIEW_TILES_W, VIEW_TILES_H));
        assert!(x0 <= 100 && 100 < x1 && y0 <= -50 && -50 < y1);

        // Keyboard pan: sprint multiplies the step.
        cam.pan(1, -1, false);
        assert_eq!((cam.world_x, cam.world_y), (101, -51));
        cam.pan(-1, 0, true);
        assert_eq!((cam.world_x, cam.world_y), (101 - CAMERA_SPRINT_MULT, -51));

        // Drag pan: 16 px of travel = 1 tile the opposite way; sub-tile travel accumulates.
        cam.set_position(0, 0);
        cam.begin_drag(300, 300);
        assert_eq!(cam.drag_to(310, 300), (0, 0));
        assert_eq!((cam.world_x, cam.world_y), (0, 0));
        assert_eq!(cam.drag_to(316, 268), (-1, 2));
        assert_eq!((cam.world_x, cam.world_y), (-1, 2));
        assert_eq!(cam.drag_to(316, 268), (0, 0));
        cam.end_drag();
        assert_eq!(cam.drag_to(400, 400), (0, 0));

        // Clamp keeps coordinates inside the i32-chunk-safe envelope.
        cam.set_position(i64::MAX, i64::MIN);
        assert_eq!((cam.world_x, cam.world_y), (CAMERA_LIMIT, -CAMERA_LIMIT));
        assert_eq!(cam.chunk_coord(), ChunkCoord::new((CAMERA_LIMIT / 32) as i32, (-CAMERA_LIMIT / 32) as i32));
    }

    #[test]
    fn test_desktop_camera_pan_streaming() {
        let seed = 0xABCD_9999_7777;
        let mut world = OpenWorldView::new(seed);
        let mut fb = FrameBuffer::new(WIN_WIDTH, WIN_HEIGHT);

        // Initial window: full 5x5 ring loaded around the stronghold.
        assert_eq!(world.loaded_chunks(), 25);
        assert_eq!(world.chunk_mgr.center_chunk, world.camera.chunk_coord());
        assert_eq!(world.defense.post_count as usize, WORLD_DEFENSE_LAYOUT.len());
        assert!(world.chunk_mgr.tile_at(world.camera.world_x, world.camera.world_y).is_walkable());

        let start = (world.camera.world_x, world.camera.world_y);
        let base_generated = world.chunk_mgr.total_generated;
        let base_evicted = world.chunk_mgr.total_evicted;
        let mut stream_events = 0u32;
        let mut generated = 0u32;
        let mut evicted = 0u32;
        let mut frames_with_monsters = 0u32;
        let mut frame_hashes = Vec::with_capacity(64);

        // Diagonal sprint: 300 steps x 4 tiles = 1,200 tiles on each axis, ticking the
        // world and rendering a frame every 5 steps.
        for step in 1..=300u32 {
            if world.pan_camera(1, 1, true) {
                stream_events += 1;
                generated += world.stream_stats.generated_chunks;
                evicted += world.stream_stats.evicted_chunks;
                assert_eq!(world.chunk_mgr.center_chunk, world.camera.chunk_coord());
                assert_eq!(world.loaded_chunks(), 25, "ring must stay full after step {}", step);
                // Every chunk in the Chebyshev ring must be resident.
                let c = world.camera.chunk_coord();
                for dy in -WORLD_VIEW_RADIUS..=WORLD_VIEW_RADIUS {
                    for dx in -WORLD_VIEW_RADIUS..=WORLD_VIEW_RADIUS {
                        assert!(world.chunk_mgr.chunk_at(ChunkCoord::new(c.x + dx, c.y + dy)).is_some());
                    }
                }
            }
            world.step();
            if world.monsters_alive() > 0 {
                frames_with_monsters += 1;
            }
            if step % 5 == 0 {
                world.render(&mut fb, world.tick);
                // Every visible tile is backed by a loaded chunk (no dimmed fallback cells).
                let (x0, y0, x1, y1) = world.camera.visible_tile_range();
                for wy in y0..y1 {
                    for wx in x0..x1 {
                        assert!(sample_world_cell(&world.chunk_mgr, wx, wy).loaded);
                    }
                }
                frame_hashes.push(fnv(&fb));
            }
        }

        let travelled_x = world.camera.world_x - start.0;
        let travelled_y = world.camera.world_y - start.1;
        assert_eq!((travelled_x, travelled_y), (1200, 1200));
        assert!(stream_events >= 37, "expected >= 37 chunk re-centres, got {}", stream_events);
        assert!(generated >= 37 * 9, "generated {}", generated);
        assert!(evicted >= 37 * 9, "evicted {}", evicted);
        assert_eq!(world.chunk_mgr.total_generated - base_generated, generated as u64);
        assert_eq!(world.chunk_mgr.total_evicted - base_evicted, evicted as u64);
        assert!(frames_with_monsters > 0, "ambient spawner never produced a monster");
        // Monsters that are alive always live inside the streamed ring.
        for m in world.spawner.monsters.iter().filter(|m| m.is_alive()) {
            assert!(m.home().chebyshev_dist(world.chunk_mgr.center_chunk) <= world.spawner.despawn_radius);
        }
        // Scrolling terrain must actually change the picture.
        let distinct = {
            let mut v = frame_hashes.clone();
            v.sort_unstable();
            v.dedup();
            v.len()
        };
        assert!(distinct >= frame_hashes.len() / 2, "distinct frames {}", distinct);

        // Terrain raster must match the manager's authoritative tile_at for a sample row.
        let (x0, y0, x1, _) = world.camera.visible_tile_range();
        for wx in x0..x1 {
            let cell = sample_world_cell(&world.chunk_mgr, wx, y0);
            assert_eq!(cell.tile, world.chunk_mgr.tile_at(wx, y0));
            assert_eq!(cell.biome, world.chunk_mgr.biome_at(wx, y0));
            assert_eq!((cell.deposit, cell.richness), world.chunk_mgr.deposit_at(wx, y0));
        }

        // Determinism: a fresh world driven by the same pan script reaches the same state
        // and pixels.
        let mut replay = OpenWorldView::new(seed);
        let mut fb2 = FrameBuffer::new(WIN_WIDTH, WIN_HEIGHT);
        for _ in 1..=300u32 {
            replay.pan_camera(1, 1, true);
            replay.step();
        }
        replay.render(&mut fb2, replay.tick);
        world.render(&mut fb, world.tick);
        assert_eq!(replay.compute_hash(), world.compute_hash());
        assert_eq!(fnv(&fb2), fnv(&fb));

        // Return trip: chunks around the stronghold regenerate bit-identically.
        let home_chunk = ChunkCoord::new(
            (start.0).div_euclid(32) as i32,
            (start.1).div_euclid(32) as i32,
        );
        let pristine = axiom_core::Chunk::<32>::generate(seed, home_chunk, 0).compute_hash();
        world.camera.set_position(start.0, start.1);
        assert!(world.sync_streaming());
        assert_eq!(world.chunk_mgr.chunk_at(home_chunk).unwrap().compute_hash(), pristine);
        assert!(world.visible_screen_pos(world.defense.heart_x as i64, world.defense.heart_y as i64).is_some());
        world.render(&mut fb, world.tick);
    }

    #[test]
    fn test_world_palette_covers_all_biomes_and_deposits() {
        use axiom_core::BiomeKind::*;
        let biomes = [
            TemperatePlains, BorealForest, AridDesert, FrozenTundra, VolcanicCrag, SwampMarsh,
            OceanDepths, MushroomGlade,
        ];
        let tiles = [TileType::Plains, TileType::Forest, TileType::Rock, TileType::Water];
        for &b in &biomes {
            for &t in &tiles {
                let a = terrain_color(b, t, true);
                let c = terrain_color(b, t, false);
                assert_eq!(a >> 24, 0xFF);
                assert_eq!(c >> 24, 0xFF);
                assert_ne!(a, c, "checker shades must differ for {:?}/{:?}", b, t);
            }
            assert!(!biome_short_name(b).is_empty());
            assert!(biome_short_name(b).len() <= 9);
        }
        for a in MonsterArchetype::ALL {
            let (kind, tint) = archetype_sprite_style(a);
            assert!(kind <= 2);
            assert_eq!(tint >> 24, 0xFF);
        }
        for k in 1..=6u8 {
            assert_ne!(deposit_color(ResourceDeposit::from_u8(k)), 0);
        }
        assert_eq!(deposit_color(ResourceDeposit::None), 0);
    }
}
