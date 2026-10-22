//! High-performance, zero-bloat software FrameBuffer.
//!
//! Provides pixel plotting, primitive rasterization, and direct
//! fixed-point entity rendering without requiring a GPU context.

use axiom_core::storage::EntitySlabPool;

/// 32-bit ARGB FrameBuffer for headless visualization and software rasterization.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameBuffer {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u32>,
}

impl FrameBuffer {
    /// Creates a new framebuffer initialized to pitch black (`0xFF000000`).
    pub fn new(width: usize, height: usize) -> Self {
        let size = width * height;
        Self {
            width,
            height,
            pixels: vec![0xFF000000; size],
        }
    }

    /// Clears the entire framebuffer with a specific 32-bit ARGB color.
    #[inline(always)]
    pub fn clear(&mut self, color: u32) {
        self.pixels.fill(color);
    }

    /// Plots a single pixel with boundary clipping.
    #[inline(always)]
    pub fn set_pixel(&mut self, x: i32, y: i32, color: u32) {
        if x >= 0 && (x as usize) < self.width && y >= 0 && (y as usize) < self.height {
            let idx = (y as usize) * self.width + (x as usize);
            self.pixels[idx] = color;
        }
    }

    /// Draws a solid rectangle with boundary clipping.
    pub fn draw_rect(&mut self, x: i32, y: i32, w: i32, h: i32, color: u32) {
        let x_start = x.max(0) as usize;
        let x_end = ((x + w).min(self.width as i32)).max(0) as usize;
        let y_start = y.max(0) as usize;
        let y_end = ((y + h).min(self.height as i32)).max(0) as usize;

        for cur_y in y_start..y_end {
            let row_offset = cur_y * self.width;
            for cur_x in x_start..x_end {
                self.pixels[row_offset + cur_x] = color;
            }
        }
    }

    /// Renders a 16x16 ARGB sprite onto the framebuffer with alpha testing (`alpha > 0`)
    /// and boundary clipping.
    pub fn draw_sprite_16x16(&mut self, x: i32, y: i32, sprite: &[u32; 256]) {
        for sy in 0..16 {
            let py = y + sy as i32;
            if py < 0 || py >= self.height as i32 {
                continue;
            }
            let row_idx = py as usize * self.width;
            for sx in 0..16 {
                let px = x + sx as i32;
                if px < 0 || px >= self.width as i32 {
                    continue;
                }
                let color = sprite[sy * 16 + sx];
                if (color >> 24) > 0 {
                    self.pixels[row_idx + px as usize] = color;
                }
            }
        }
    }

    /// Renders a 16x16 ARGB sprite with color modulation (integer multiplicative tint)
    /// and boundary clipping.
    pub fn draw_sprite_16x16_tinted(&mut self, x: i32, y: i32, sprite: &[u32; 256], tint: u32) {
        let tr = (tint >> 16) & 0xFF;
        let tg = (tint >> 8) & 0xFF;
        let tb = tint & 0xFF;
        let ta = (tint >> 24) & 0xFF;

        for sy in 0..16 {
            let py = y + sy as i32;
            if py < 0 || py >= self.height as i32 {
                continue;
            }
            let row_idx = py as usize * self.width;
            for sx in 0..16 {
                let px = x + sx as i32;
                if px < 0 || px >= self.width as i32 {
                    continue;
                }
                let color = sprite[sy * 16 + sx];
                let a = (color >> 24) & 0xFF;
                if a > 0 {
                    let r = (((color >> 16) & 0xFF) * tr) / 255;
                    let g = (((color >> 8) & 0xFF) * tg) / 255;
                    let b = ((color & 0xFF) * tb) / 255;
                    let out_a = if ta > 0 { (a * ta) / 255 } else { a };
                    self.pixels[row_idx + px as usize] = (out_a << 24) | (r << 16) | (g << 8) | b;
                }
            }
        }
    }

    /// Renders loaded procedural chunks onto the viewport relative to camera position.
    pub fn render_chunks<const S: usize, const MAX_CHUNKS: usize>(
        &mut self,
        chunk_mgr: &axiom_core::chunk::ChunkManager<S, MAX_CHUNKS>,
        camera_pos: axiom_core::fixed::FixedVec2,
        tile_size: usize,
    ) {
        let cam_x = camera_pos.x.to_int() as i32;
        let cam_y = camera_pos.y.to_int() as i32;
        let half_w = (self.width / 2) as i32;
        let half_h = (self.height / 2) as i32;

        for slot in &chunk_mgr.slots {
            if !slot.active {
                continue;
            }
            let chunk_origin_x = slot.chunk.coord.x * S as i32;
            let chunk_origin_y = slot.chunk.coord.y * S as i32;

            for ly in 0..S {
                for lx in 0..S {
                    let world_tile_x = chunk_origin_x + lx as i32;
                    let world_tile_y = chunk_origin_y + ly as i32;

                    let screen_x = half_w + (world_tile_x - cam_x) * tile_size as i32;
                    let screen_y = half_h + (world_tile_y - cam_y) * tile_size as i32;

                    // Frustum culling check
                    if screen_x + tile_size as i32 <= 0
                        || screen_x >= self.width as i32
                        || screen_y + tile_size as i32 <= 0
                        || screen_y >= self.height as i32
                    {
                        continue;
                    }

                    let color = match slot.chunk.tiles[ly][lx] {
                        axiom_core::pcg::TileType::Plains => 0xFF3CB371, // Medium Sea Green
                        axiom_core::pcg::TileType::Forest => 0xFF195E35, // Forest Green
                        axiom_core::pcg::TileType::Rock => 0xFF708090,   // Slate Grey
                        axiom_core::pcg::TileType::Water => 0xFF1E90FF,  // Dodger Blue
                    };

                    self.draw_rect(screen_x, screen_y, tile_size as i32, tile_size as i32, color);
                }
            }
        }
    }

    /// Renders a real-time minimap density scatter of 100,000 NPCs.
    pub fn render_npc_minimap<const SLABS: usize>(
        &mut self,
        pool: &axiom_core::npc_crowd::NpcCrowdPool<SLABS>,
        minimap_x: i32,
        minimap_y: i32,
        minimap_w: usize,
        minimap_h: usize,
        world_scale: i32,
    ) {
        // Draw minimap background border
        self.draw_rect(
            minimap_x - 1,
            minimap_y - 1,
            minimap_w as i32 + 2,
            minimap_h as i32 + 2,
            0xFF2F4F4F, // Dark Slate Gray Border
        );
        self.draw_rect(
            minimap_x,
            minimap_y,
            minimap_w as i32,
            minimap_h as i32,
            0xCC111111, // Semi-dark transparent background
        );

        let scale = if world_scale > 0 { world_scale } else { 1 };

        for slab in &pool.slabs {
            if slab.active_count == 0 {
                continue;
            }
            for npc in &slab.npcs {
                if !npc.is_active() {
                    continue;
                }
                let wx = (npc.pos.x.to_int() / scale as i64) as i32;
                let wy = (npc.pos.y.to_int() / scale as i64) as i32;

                let mx = minimap_x + wx.rem_euclid(minimap_w as i32);
                let my = minimap_y + wy.rem_euclid(minimap_h as i32);

                let color = match npc.activity {
                    axiom_core::npc_crowd::NpcActivity::Rest => 0xFF4169E1,      // Royal Blue
                    axiom_core::npc_crowd::NpcActivity::Commute => 0xFFFFD700,   // Gold
                    axiom_core::npc_crowd::NpcActivity::Work => 0xFFFF8C00,      // Dark Orange
                    axiom_core::npc_crowd::NpcActivity::Socialize => 0xFF32CD32, // Lime Green
                    axiom_core::npc_crowd::NpcActivity::Idle => 0xFFD3D3D3,      // Light Gray
                };

                self.set_pixel(mx, my, color);
            }
        }
    }

    /// Renders all active entities from a multi-slab pool onto the framebuffer.
    /// Converts Q32.16 fixed-point coordinates into screen coordinates.
    pub fn render_slab_pool<const SLABS: usize>(
        &mut self,
        pool: &EntitySlabPool<SLABS>,
        color: u32,
    ) {
        for slab in pool.slabs.iter() {
            for entity in slab.entities.iter() {
                if entity.is_active() {
                    // Q32.16 fixed-point to screen pixel integer
                    let screen_x = entity.pos.x.to_int() as i32;
                    let screen_y = entity.pos.y.to_int() as i32;
                    self.set_pixel(screen_x, screen_y, color);
                }
            }
        }
    }

    /// Renders a deterministic procedural tilemap onto the framebuffer.
    pub fn render_tilemap<const W: usize, const H: usize>(
        &mut self,
        map: &axiom_core::pcg::TileMap<W, H>,
        tile_size: usize,
    ) {
        for y in 0..H {
            for x in 0..W {
                let color = match map.tiles[y][x] {
                    axiom_core::pcg::TileType::Plains => 0xFF3CB371, // Medium Sea Green
                    axiom_core::pcg::TileType::Forest => 0xFF195E35, // Forest Green
                    axiom_core::pcg::TileType::Rock => 0xFF708090,   // Slate Grey
                    axiom_core::pcg::TileType::Water => 0xFF1E90FF,  // Dodger Blue
                };
                self.draw_rect(
                    (x * tile_size) as i32,
                    (y * tile_size) as i32,
                    tile_size as i32,
                    tile_size as i32,
                    color,
                );
            }
        }
    }

    /// Returns the raw pixel slice (ARGB format).
    #[inline(always)]
    pub fn as_slice(&self) -> &[u32] {
        &self.pixels
    }

    /// Returns the mutable raw pixel slice.
    #[inline(always)]
    pub fn as_mut_slice(&mut self) -> &mut [u32] {
        &mut self.pixels
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_core::fixed::FixedVec2;

    #[test]
    fn test_framebuffer_allocation_and_clear() {
        let mut fb = FrameBuffer::new(800, 600);
        assert_eq!(fb.pixels.len(), 800 * 600);

        fb.clear(0xFFFF0000); // Red
        assert_eq!(fb.pixels[0], 0xFFFF0000);
        assert_eq!(fb.pixels[800 * 600 - 1], 0xFFFF0000);
    }

    #[test]
    fn test_pixel_clipping_bounds() {
        let mut fb = FrameBuffer::new(100, 100);
        fb.clear(0);

        // Within bounds
        fb.set_pixel(10, 10, 0xFFFFFFFF);
        assert_eq!(fb.pixels[10 * 100 + 10], 0xFFFFFFFF);

        // Out of bounds (should not panic or write)
        fb.set_pixel(-1, 0, 0xFFFFFFFF);
        fb.set_pixel(100, 50, 0xFFFFFFFF);
        fb.set_pixel(50, 100, 0xFFFFFFFF);
    }

    #[test]
    fn test_render_10k_entities_throughput() {
        let mut fb = FrameBuffer::new(1920, 1080);
        let mut pool = EntitySlabPool::<10>::new();

        // Spawn 10,000 entities positioned within 1920x1080 viewport
        for i in 0..10_000 {
            let x = (i % 1920) as i64;
            let y = ((i / 1920) * 100) as i64;
            pool.spawn((i + 1) as u64, FixedVec2::from_ints(x, y), FixedVec2::ZERO)
                .unwrap();
        }

        // Render 10k entities
        fb.render_slab_pool(&pool, 0xFF00FF00); // Green

        // Verify entity 0 is at (0, 0)
        assert_eq!(fb.pixels[0], 0xFF00FF00);
    }

    #[test]
    fn test_draw_sprite_16x16_and_tint() {
        let mut fb = FrameBuffer::new(32, 32);
        fb.clear(0xFF000000);

        let mut sprite = [0u32; 256];
        // Transparent background with a 4x4 white box at top-left
        for y in 0..4 {
            for x in 0..4 {
                sprite[y * 16 + x] = 0xFFFFFFFF;
            }
        }

        // Draw at (4, 4)
        fb.draw_sprite_16x16(4, 4, &sprite);
        assert_eq!(fb.pixels[4 * 32 + 4], 0xFFFFFFFF);
        assert_eq!(fb.pixels[7 * 32 + 7], 0xFFFFFFFF);
        // Untouched outside the 4x4 box
        assert_eq!(fb.pixels[4 * 32 + 8], 0xFF000000);

        // Test clipping: draw sprite overlapping left and top edges (-2, -2)
        fb.draw_sprite_16x16(-2, -2, &sprite);
        // (0, 0) should receive the sprite pixel from (2, 2)
        assert_eq!(fb.pixels[0], 0xFFFFFFFF);

        // Test tinting with pure red (0xFFFF0000)
        fb.clear(0xFF000000);
        fb.draw_sprite_16x16_tinted(0, 0, &sprite, 0xFFFF0000);
        assert_eq!(fb.pixels[0], 0xFFFF0000);
    }
}
