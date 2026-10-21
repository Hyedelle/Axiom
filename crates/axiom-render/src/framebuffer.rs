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
}
