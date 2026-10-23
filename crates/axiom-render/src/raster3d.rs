//! Software 3D Rasterizer with Juan Pineda 8x8 Tile Binning and Reversed-Z Affine Interpolation.
//!
//! Renders 3D mecha models directly to `FrameBuffer` using deterministic
//! fixed-point integer mathematics with zero external GPU dependencies.
//! Optimized for 100% CPU L1 Data Cache residency and zero per-pixel divisions.

use crate::framebuffer::FrameBuffer;
use crate::math3d::{Mat4Fixed, Vec3Fixed};
use crate::mesh3d::MechaModel3D;
use axiom_core::Fixed;

/// Color palette for mecha cel-shading.
#[derive(Debug, Clone, Copy)]
pub struct CelShadingPalette {
    pub highlight: u32,
    pub base: u32,
    pub shadow: u32,
    pub outline: u32,
}

impl Default for CelShadingPalette {
    fn default() -> Self {
        // High-contrast industrial mecha steel palette
        Self {
            highlight: 0xFFE2E8F0, // Bright metallic chrome
            base: 0xFF64748B,      // Cool slate steel
            shadow: 0xFF1E293B,    // Dark gunmetal shadow
            outline: 0xFF0F172A,   // Deep ink outline
        }
    }
}

/// Palette presets for various mecha parts.
impl CelShadingPalette {
    pub fn for_part(name: &str) -> Self {
        if name.contains("eye") {
            // Glowing cyan optic visor
            Self {
                highlight: 0xFFE0F2FE,
                base: 0xFF0284C7,
                shadow: 0xFF0369A1,
                outline: 0xFF082F49,
            }
        } else if name.contains("glove") {
            // Heavy crimson boxing gloves
            Self {
                highlight: 0xFFFCA5A5,
                base: 0xFFDC2626,
                shadow: 0xFF7F1D1D,
                outline: 0xFF450A0A,
            }
        } else if name.contains("torso") || name.contains("pelvis") {
            // Core frame dark iron
            Self {
                highlight: 0xFF94A3B8,
                base: 0xFF475569,
                shadow: 0xFF1E293B,
                outline: 0xFF0F172A,
            }
        } else {
            // Primary mecha armor (Elpis steel)
            Self::default()
        }
    }
}

/// Software 3D Rasterizer Context using Juan Pineda 8x8 Tile Binning.
pub struct Rasterizer3D {
    pub width: usize,
    pub height: usize,
    pub depth_buffer: Vec<i32>,
    pub light_dir: Vec3Fixed,
    pub cull_backfaces: bool,
}

impl Rasterizer3D {
    pub fn new(width: usize, height: usize) -> Self {
        // Default directional light shining from top-right-front
        let light = Vec3Fixed::from_f32(0.577, 0.577, 0.577);
        Self {
            width,
            height,
            depth_buffer: vec![i32::MIN; width * height],
            light_dir: light,
            cull_backfaces: false,
        }
    }

    /// Clears the Z-buffer.
    pub fn clear_depth(&mut self) {
        self.depth_buffer.fill(i32::MIN);
    }

    /// Renders a complete `MechaModel3D` with the given transform matrix and camera parameters.
    pub fn render_model(
        &mut self,
        fb: &mut FrameBuffer,
        model: &MechaModel3D,
        world_mat: &Mat4Fixed,
        cam_dist: Fixed,
        focal_length: i64,
    ) {
        let cx = (self.width / 2) as i64;
        let cy = (self.height / 2) as i64;

        for part in &model.parts {
            let palette = CelShadingPalette::for_part(&part.name);

            // Transform vertices to view space
            let mut view_verts: Vec<(i64, i64, i32, Vec3Fixed)> =
                Vec::with_capacity(part.vertices.len());

            for v in &part.vertices {
                let p_world = world_mat.transform_point(v.pos);
                let n_world = world_mat.transform_vector(v.norm);

                let vz = (p_world.z + cam_dist).raw();

                if vz <= 65536 / 10 {
                    // Behind near clipping plane
                    view_verts.push((0, 0, i32::MIN, n_world));
                    continue;
                }

                // Perspective projection in fixed point
                let sx = cx + (p_world.x.raw() * focal_length) / vz;
                let sy = cy - (p_world.y.raw() * focal_length) / vz;

                // Affine linear 1/Z depth (Reversed-Z: larger = closer)
                let inv_z = ((1i64 << 30) / vz) as i32;

                view_verts.push((sx, sy, inv_z, n_world));
            }

            // Rasterize triangles
            let tri_count = part.indices.len() / 3;
            for t in 0..tri_count {
                let i0 = part.indices[t * 3] as usize;
                let i1 = part.indices[t * 3 + 1] as usize;
                let i2 = part.indices[t * 3 + 2] as usize;

                if i0 >= view_verts.len() || i1 >= view_verts.len() || i2 >= view_verts.len() {
                    continue;
                }

                let (x0, y0, z0, n0) = view_verts[i0];
                let (x1, y1, z1, n1) = view_verts[i1];
                let (x2, y2, z2, n2) = view_verts[i2];

                if z0 == i32::MIN || z1 == i32::MIN || z2 == i32::MIN {
                    continue;
                }

                // Calculate average face normal for flat / cel lighting
                let avg_norm = Vec3Fixed::new(
                    (n0.x + n1.x + n2.x) * Fixed::from_raw(21845),
                    (n0.y + n1.y + n2.y) * Fixed::from_raw(21845),
                    (n0.z + n1.z + n2.z) * Fixed::from_raw(21845),
                );

                let light_dot = avg_norm.dot(self.light_dir);

                // Cel-shading stepped tone selection
                let color = if light_dot.raw() > 24000 {
                    palette.highlight
                } else if light_dot.raw() > 4000 {
                    palette.base
                } else {
                    palette.shadow
                };

                self.rasterize_pineda_8x8(fb, x0, y0, z0, x1, y1, z1, x2, y2, z2, color);
            }
        }
    }

    /// Juan Pineda 8x8 Tile Binning Triangle Rasterizer with Affine 1/Z interpolation.
    fn rasterize_pineda_8x8(
        &mut self,
        fb: &mut FrameBuffer,
        x0: i64,
        y0: i64,
        z0: i32,
        mut x1: i64,
        mut y1: i64,
        mut z1: i32,
        mut x2: i64,
        mut y2: i64,
        mut z2: i32,
        color: u32,
    ) {
        // Initial 2D cross product for area
        let area_init = (x1 - x0) * (y2 - y0) - (y1 - y0) * (x2 - x0);
        if area_init == 0 {
            return; // Degenerate
        }

        if self.cull_backfaces && area_init <= 0 {
            return; // Cull back-facing triangle
        }

        // Guarantee area > 0 by swapping vertices if wound clockwise
        if area_init < 0 {
            core::mem::swap(&mut x1, &mut x2);
            core::mem::swap(&mut y1, &mut y2);
            core::mem::swap(&mut z1, &mut z2);
        }

        let area = (x1 - x0) * (y2 - y0) - (y1 - y0) * (x2 - x0);
        if area <= 0 {
            return;
        }

        // Bounding box clamped to screen
        let min_x = (x0.min(x1).min(x2)).max(0).min(self.width as i64 - 1);
        let max_x = (x0.max(x1).max(x2)).max(0).min(self.width as i64 - 1);
        let min_y = (y0.min(y1).min(y2)).max(0).min(self.height as i64 - 1);
        let max_y = (y0.max(y1).max(y2)).max(0).min(self.height as i64 - 1);

        if min_x > max_x || min_y > max_y {
            return;
        }

        // Edge vectors:
        // Edge 0: from v1 to v2
        let dx0 = x2 - x1;
        let dy0 = y2 - y1;
        // Edge 1: from v2 to v0
        let dx1 = x0 - x2;
        let dy1 = y0 - y2;
        // Edge 2: from v0 to v1
        let dx2 = x1 - x0;
        let dy2 = y1 - y0;

        // Affine Linear 1/Z gradients across screen space (Constant for the entire triangle)
        let dz10 = z1 as i64 - z0 as i64;
        let dz20 = z2 as i64 - z0 as i64;
        let d_zinv_dx = (dz10 * (y2 - y0) - dz20 * (y1 - y0)) / area;
        let d_zinv_dy = (dz20 * (x1 - x0) - dz10 * (x2 - x0)) / area;

        // 8x8 Tile bounds
        let tile_min_x = min_x / 8;
        let tile_max_x = max_x / 8;
        let tile_min_y = min_y / 8;
        let tile_max_y = max_y / 8;

        for ty in tile_min_y..=tile_max_y {
            let t_y0 = ty * 8;
            let t_y1 = (t_y0 + 7).min(self.height as i64 - 1);

            for tx in tile_min_x..=tile_max_x {
                let t_x0 = tx * 8;
                let t_x1 = (t_x0 + 7).min(self.width as i64 - 1);

                // Tile Trivial Reject Test at 4 corners
                // Edge 0: w0 = dx0 * (y - y1) - dy0 * (x - x1)
                let e0_c00 = dx0 * (t_y0 - y1) - dy0 * (t_x0 - x1);
                let e0_c10 = dx0 * (t_y0 - y1) - dy0 * (t_x1 - x1);
                let e0_c01 = dx0 * (t_y1 - y1) - dy0 * (t_x0 - x1);
                let e0_c11 = dx0 * (t_y1 - y1) - dy0 * (t_x1 - x1);
                if e0_c00 < 0 && e0_c10 < 0 && e0_c01 < 0 && e0_c11 < 0 {
                    continue; // Completely outside edge 0
                }

                // Edge 1: w1 = dx1 * (y - y2) - dy1 * (x - x2)
                let e1_c00 = dx1 * (t_y0 - y2) - dy1 * (t_x0 - x2);
                let e1_c10 = dx1 * (t_y0 - y2) - dy1 * (t_x1 - x2);
                let e1_c01 = dx1 * (t_y1 - y2) - dy1 * (t_x0 - x2);
                let e1_c11 = dx1 * (t_y1 - y2) - dy1 * (t_x1 - x2);
                if e1_c00 < 0 && e1_c10 < 0 && e1_c01 < 0 && e1_c11 < 0 {
                    continue; // Completely outside edge 1
                }

                // Edge 2: w2 = dx2 * (y - y0) - dy2 * (x - x0)
                let e2_c00 = dx2 * (t_y0 - y0) - dy2 * (t_x0 - x0);
                let e2_c10 = dx2 * (t_y0 - y0) - dy2 * (t_x1 - x0);
                let e2_c01 = dx2 * (t_y1 - y0) - dy2 * (t_x0 - x0);
                let e2_c11 = dx2 * (t_y1 - y0) - dy2 * (t_x1 - x0);
                if e2_c00 < 0 && e2_c10 < 0 && e2_c01 < 0 && e2_c11 < 0 {
                    continue; // Completely outside edge 2
                }

                // Tile overlaps triangle: rasterize 8x8 pixels with incremental additions
                let mut row_e0 = dx0 * (t_y0 - y1) - dy0 * (t_x0 - x1);
                let mut row_e1 = dx1 * (t_y0 - y2) - dy1 * (t_x0 - x2);
                let mut row_e2 = dx2 * (t_y0 - y0) - dy2 * (t_x0 - x0);
                let mut row_zinv = z0 as i64 + (t_x0 - x0) * d_zinv_dx + (t_y0 - y0) * d_zinv_dy;

                for py in t_y0..=t_y1 {
                    let row_idx = py as usize * self.width;
                    let mut e0 = row_e0;
                    let mut e1 = row_e1;
                    let mut e2 = row_e2;
                    let mut zinv = row_zinv;

                    for px in t_x0..=t_x1 {
                        // Check if pixel is inside all 3 edges (sign bit test)
                        if (e0 | e1 | e2) >= 0 {
                            let pixel_idx = row_idx + px as usize;
                            let z_val = zinv as i32;

                            // Reversed-Z depth test: closer is larger
                            if z_val > self.depth_buffer[pixel_idx] {
                                self.depth_buffer[pixel_idx] = z_val;
                                fb.pixels[pixel_idx] = color;
                            }
                        }

                        // Incremental step in X: w(x+1, y) = w(x, y) - dy
                        e0 -= dy0;
                        e1 -= dy1;
                        e2 -= dy2;
                        zinv += d_zinv_dx;
                    }

                    // Incremental step in Y: w(x, y+1) = w(x, y) + dx
                    row_e0 += dx0;
                    row_e1 += dx1;
                    row_e2 += dx2;
                    row_zinv += d_zinv_dy;
                }
            }
        }
    }
}
