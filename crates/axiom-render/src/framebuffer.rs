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
            let chunk_origin_x = slot.chunk.header.coord.x * S as i32;
            let chunk_origin_y = slot.chunk.header.coord.y * S as i32;

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

    /// Creates a 2x nearest-neighbor upscaled copy of this framebuffer.
    pub fn to_upscaled_2x(&self) -> Self {
        let mut dst = Self::new(self.width * 2, self.height * 2);
        upscale_nearest_2x(&self.pixels, self.width, self.height, &mut dst.pixels);
        dst
    }

    /// Alpha blends a solid color over existing framebuffer pixels using pure integer math.
    pub fn draw_rect_blend(&mut self, x: i32, y: i32, w: i32, h: i32, color: u32) {
        let a = (color >> 24) & 0xFF;
        if a == 0 {
            return;
        }
        if a == 255 {
            self.draw_rect(x, y, w, h, color);
            return;
        }
        let inv_a = 255 - a;
        let cr = (color >> 16) & 0xFF;
        let cg = (color >> 8) & 0xFF;
        let cb = color & 0xFF;

        let x_start = x.max(0) as usize;
        let x_end = ((x + w).min(self.width as i32)).max(0) as usize;
        let y_start = y.max(0) as usize;
        let y_end = ((y + h).min(self.height as i32)).max(0) as usize;

        for cur_y in y_start..y_end {
            let row_offset = cur_y * self.width;
            for cur_x in x_start..x_end {
                let idx = row_offset + cur_x;
                let bg = self.pixels[idx];
                let br = (bg >> 16) & 0xFF;
                let bg_g = (bg >> 8) & 0xFF;
                let bb = bg & 0xFF;

                let out_r = (cr * a + br * inv_a) / 255;
                let out_g = (cg * a + bg_g * inv_a) / 255;
                let out_b = (cb * a + bb * inv_a) / 255;

                self.pixels[idx] = (0xFF << 24) | (out_r << 16) | (out_g << 8) | out_b;
            }
        }
    }

    /// Draws a radial soft ellipse with pure integer falloff and alpha blending.
    /// Ideal for ambient occlusion contact shadows under boots, floor lighting, and halos.
    pub fn draw_ellipse_blend(&mut self, cx: i32, cy: i32, rx: i32, ry: i32, color: u32) {
        if rx <= 0 || ry <= 0 {
            return;
        }
        let base_a = (color >> 24) & 0xFF;
        if base_a == 0 {
            return;
        }
        let cr = (color >> 16) & 0xFF;
        let cg = (color >> 8) & 0xFF;
        let cb = color & 0xFF;

        let rx_sq = (rx as i64) * (rx as i64);
        let ry_sq = (ry as i64) * (ry as i64);

        let y_min = (cy - ry).max(0);
        let y_max = (cy + ry).min(self.height as i32 - 1);
        let x_min = (cx - rx).max(0);
        let x_max = (cx + rx).min(self.width as i32 - 1);

        for py in y_min..=y_max {
            let dy = (py - cy) as i64;
            let dy_sq = dy * dy;
            let row_offset = py as usize * self.width;

            for px in x_min..=x_max {
                let dx = (px - cx) as i64;
                let dx_sq = dx * dx;

                // Normalized squared distance scaled by 65536
                let d = (dx_sq * 65536) / rx_sq + (dy_sq * 65536) / ry_sq;
                if d < 65536 {
                    let falloff = 65536 - d; // 65536 at center, 0 at perimeter
                    let cur_a = ((base_a as i64 * falloff) / 65536) as u32;
                    if cur_a == 0 {
                        continue;
                    }
                    let inv_a = 255 - cur_a;

                    let idx = row_offset + px as usize;
                    let bg = self.pixels[idx];
                    let br = (bg >> 16) & 0xFF;
                    let bg_g = (bg >> 8) & 0xFF;
                    let bb = bg & 0xFF;

                    let out_r = (cr * cur_a + br * inv_a) / 255;
                    let out_g = (cg * cur_a + bg_g * inv_a) / 255;
                    let out_b = (cb * cur_a + bb * inv_a) / 255;

                    self.pixels[idx] = (0xFF << 24) | (out_r << 16) | (out_g << 8) | out_b;
                }
            }
        }
    }

    /// Draws a vertical volumetric light beam/cone trapezoid with linear vertical falloff.
    pub fn draw_cone_light_blend(
        &mut self,
        top_cx: i32,
        top_y: i32,
        top_w: i32,
        bot_cx: i32,
        bot_y: i32,
        bot_w: i32,
        color: u32,
    ) {
        if bot_y <= top_y {
            return;
        }
        let base_a = (color >> 24) & 0xFF;
        if base_a == 0 {
            return;
        }
        let cr = (color >> 16) & 0xFF;
        let cg = (color >> 8) & 0xFF;
        let cb = color & 0xFF;

        let total_h = (bot_y - top_y) as i64;
        let y_start = top_y.max(0);
        let y_end = bot_y.min(self.height as i32 - 1);

        for py in y_start..=y_end {
            let prog = (py - top_y) as i64; // 0..total_h
            let cx = top_cx as i64 + ((bot_cx - top_cx) as i64 * prog) / total_h;
            let half_w = (top_w as i64 + ((bot_w - top_w) as i64 * prog) / total_h) / 2;

            // Light beam fades slightly as it expands downward
            let cur_a = ((base_a as i64 * (total_h * 2 - prog)) / (total_h * 2)) as u32;
            let inv_a = 255 - cur_a;

            let x_start = (cx - half_w).max(0);
            let x_end = (cx + half_w).min(self.width as i64 - 1);
            let row_offset = py as usize * self.width;

            for px in x_start..=x_end {
                let idx = row_offset + px as usize;
                let bg = self.pixels[idx];
                let br = (bg >> 16) & 0xFF;
                let bg_g = (bg >> 8) & 0xFF;
                let bb = bg & 0xFF;

                let out_r = (cr * cur_a + br * inv_a) / 255;
                let out_g = (cg * cur_a + bg_g * inv_a) / 255;
                let out_b = (cb * cur_a + bb * inv_a) / 255;

                self.pixels[idx] = (0xFF << 24) | (out_r << 16) | (out_g << 8) | out_b;
            }
        }
    }

    /// Renders a single 3x5 font character.
    pub fn draw_char(&mut self, c: char, x: i32, y: i32, color: u32) {
        let glyph = glyph_3x5(c);
        if glyph == 0 {
            return;
        }
        for row in 0..5 {
            for col in 0..3 {
                let bit = 14 - (row * 3 + col);
                if (glyph & (1 << bit)) != 0 {
                    self.set_pixel(x + col, y + row, color);
                }
            }
        }
    }

    /// Renders a string of text using 3x5 bitmap font (4 pixels pitch).
    pub fn draw_text(&mut self, mut x: i32, y: i32, text: &str, color: u32) {
        for c in text.chars() {
            if c == ' ' {
                x += 3;
            } else {
                self.draw_char(c, x, y, color);
                x += 4;
            }
        }
    }

    /// Renders 2x scaled bold text using 3x5 bitmap font.
    pub fn draw_text_2x(&mut self, mut x: i32, y: i32, text: &str, color: u32) {
        for c in text.chars() {
            if c == ' ' {
                x += 6;
            } else {
                let glyph = glyph_3x5(c);
                if glyph != 0 {
                    for row in 0..5 {
                        for col in 0..3 {
                            let bit = 14 - (row * 3 + col);
                            if (glyph & (1 << bit)) != 0 {
                                let px = x + col * 2;
                                let py = y + row * 2;
                                self.set_pixel(px, py, color);
                                self.set_pixel(px + 1, py, color);
                                self.set_pixel(px, py + 1, color);
                                self.set_pixel(px + 1, py + 1, color);
                            }
                        }
                    }
                }
                x += 8;
            }
        }
    }
}

/// 3x5 Bitmap font glyph lookup table.
pub fn glyph_3x5(c: char) -> u16 {
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
        '!' => 0x2482,
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
        '+' => 0x0BA0,
        '%' => 0x52A5,
        _ => 0,
    }
}

/// Upscales a source pixel buffer by a factor of 2 using nearest-neighbor replication.
///
/// Replicates each source pixel into a 2x2 square in the destination buffer.
/// Destination width must be `src_w * 2`, and destination height must be `src_h * 2`.
/// Zero floats, bounds checked and cache-friendly.
///
/// # Panics
/// Panics if `src.len() < src_w * src_h` or `dst.len() < (src_w * 2) * (src_h * 2)`.
pub fn upscale_nearest_2x(src: &[u32], src_w: usize, src_h: usize, dst: &mut [u32]) {
    let dst_w = src_w * 2;
    let dst_h = src_h * 2;
    let total_src = src_w * src_h;
    let total_dst = dst_w * dst_h;

    assert!(
        src.len() >= total_src,
        "Source buffer too small: required {}, got {}",
        total_src,
        src.len()
    );
    assert!(
        dst.len() >= total_dst,
        "Destination buffer too small: required {}, got {}",
        total_dst,
        dst.len()
    );

    for sy in 0..src_h {
        let src_row = &src[sy * src_w..(sy + 1) * src_w];
        let dst_row0_start = (sy * 2) * dst_w;
        let dst_row1_start = dst_row0_start + dst_w;

        for sx in 0..src_w {
            let px = src_row[sx];
            let dx = sx * 2;
            dst[dst_row0_start + dx] = px;
            dst[dst_row0_start + dx + 1] = px;
            dst[dst_row1_start + dx] = px;
            dst[dst_row1_start + dx + 1] = px;
        }
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

    #[test]
    fn test_upscale_nearest_2x_exactness() {
        let src_w = 4;
        let src_h = 4;
        let mut src = vec![0u32; src_w * src_h];
        for y in 0..src_h {
            for x in 0..src_w {
                src[y * src_w + x] = 0xFF00_0000 | ((y as u32) << 8) | (x as u32);
            }
        }

        let dst_w = src_w * 2;
        let dst_h = src_h * 2;
        let mut dst = vec![0u32; dst_w * dst_h];

        upscale_nearest_2x(&src, src_w, src_h, &mut dst);

        for sy in 0..src_h {
            for sx in 0..src_w {
                let expected = src[sy * src_w + sx];
                let dy0 = sy * 2;
                let dy1 = dy0 + 1;
                let dx0 = sx * 2;
                let dx1 = dx0 + 1;

                assert_eq!(dst[dy0 * dst_w + dx0], expected, "Mismatch at ({}, {})", dx0, dy0);
                assert_eq!(dst[dy0 * dst_w + dx1], expected, "Mismatch at ({}, {})", dx1, dy0);
                assert_eq!(dst[dy1 * dst_w + dx0], expected, "Mismatch at ({}, {})", dx0, dy1);
                assert_eq!(dst[dy1 * dst_w + dx1], expected, "Mismatch at ({}, {})", dx1, dy1);
            }
        }
    }

    #[test]
    fn test_framebuffer_to_upscaled_2x() {
        let mut fb = FrameBuffer::new(2, 2);
        fb.set_pixel(0, 0, 0xFF112233);
        fb.set_pixel(1, 0, 0xFF445566);
        fb.set_pixel(0, 1, 0xFF778899);
        fb.set_pixel(1, 1, 0xFFAABBCC);

        let up = fb.to_upscaled_2x();
        assert_eq!(up.width, 4);
        assert_eq!(up.height, 4);
        assert_eq!(up.pixels.len(), 16);

        // Top-left 2x2 should all be 0xFF112233
        assert_eq!(up.pixels[0 * 4 + 0], 0xFF112233);
        assert_eq!(up.pixels[0 * 4 + 1], 0xFF112233);
        assert_eq!(up.pixels[1 * 4 + 0], 0xFF112233);
        assert_eq!(up.pixels[1 * 4 + 1], 0xFF112233);

        // Bottom-right 2x2 should all be 0xFFAABBCC
        assert_eq!(up.pixels[2 * 4 + 2], 0xFFAABBCC);
        assert_eq!(up.pixels[2 * 4 + 3], 0xFFAABBCC);
        assert_eq!(up.pixels[3 * 4 + 2], 0xFFAABBCC);
        assert_eq!(up.pixels[3 * 4 + 3], 0xFFAABBCC);
    }

    #[test]
    fn test_draw_ellipse_blend_falloff() {
        let mut fb = FrameBuffer::new(50, 50);
        fb.clear(0xFFFFFFFF); // White background

        // Draw black contact shadow ellipse centered at (25, 25), rx=10, ry=5
        fb.draw_ellipse_blend(25, 25, 10, 5, 0xC0000000);

        // Center should be darkest
        let center_px = fb.pixels[25 * 50 + 25];
        let cr = (center_px >> 16) & 0xFF;
        assert!(cr < 100, "Center of contact shadow must be dark, got {}", cr);

        // Corner of bounding box outside ellipse should remain white
        let outside_px = fb.pixels[15 * 50 + 15];
        assert_eq!(outside_px, 0xFFFFFFFF, "Outside ellipse must remain white");
    }

    #[test]
    fn test_draw_cone_light_blend() {
        let mut fb = FrameBuffer::new(60, 60);
        fb.clear(0xFF000000); // Black background

        // Draw brass cone light from top (30, 0) w=10 to bottom (30, 40) w=30
        fb.draw_cone_light_blend(30, 0, 10, 30, 40, 30, 0x40FED7AA);

        // Center ray should be illuminated
        let center_px = fb.pixels[20 * 60 + 30];
        let cr = (center_px >> 16) & 0xFF;
        assert!(cr > 0, "Cone light must illuminate center path");

        // Edge outside cone should remain black
        let edge_px = fb.pixels[20 * 60 + 5];
        assert_eq!(edge_px, 0xFF000000, "Outside cone light must remain black");
    }
}
