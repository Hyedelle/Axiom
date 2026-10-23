//! Software 3D Rasterizer with Z-buffering and Cel-shading.
//!
//! Renders 3D mecha models directly to `FrameBuffer` using deterministic
//! fixed-point integer mathematics with zero external GPU dependencies.

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

/// Software 3D Rasterizer Context.
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
            // In our camera view, camera is looking down the +Z axis from `cam_dist`
            let mut view_verts: Vec<(i64, i64, i32, Vec3Fixed)> =
                Vec::with_capacity(part.vertices.len());

            for v in &part.vertices {
                let p_world = world_mat.transform_point(v.pos);
                let n_world = world_mat.transform_vector(v.norm);

                // Camera offset along Z
                let vz = (p_world.z + cam_dist).raw();

                if vz <= 65536 / 10 {
                    // Behind or too close to near clipping plane
                    view_verts.push((0, 0, i32::MIN, n_world));
                    continue;
                }

                // Perspective projection: sx = cx + (vx * focal) / vz
                let sx = cx + (p_world.x.raw() * focal_length) / vz;
                let sy = cy - (p_world.y.raw() * focal_length) / vz; // Invert Y for screen coords

                // Fixed-point 1/Z representation for depth buffer (larger = closer)
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

                // Backface culling: 2D cross product of screen coordinates
                let cross2d = (x1 - x0) * (y2 - y0) - (y1 - y0) * (x2 - x0);
                if self.cull_backfaces && cross2d <= 0 {
                    continue; // Cull back-facing triangle
                }
                if cross2d == 0 {
                    continue; // Degenerate triangle
                }

                // Calculate average face normal for flat / cel lighting
                let avg_norm = Vec3Fixed::new(
                    (n0.x + n1.x + n2.x) * Fixed::from_raw(21845), // divide by 3 (1/3 in Q16 = 21845)
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

                self.rasterize_triangle(fb, x0, y0, z0, x1, y1, z1, x2, y2, z2, color);
            }
        }
    }

    /// Rasterizes a 2D projected triangle with Z-buffer depth testing.
    fn rasterize_triangle(
        &mut self,
        fb: &mut FrameBuffer,
        x0: i64,
        y0: i64,
        z0: i32,
        x1: i64,
        y1: i64,
        z1: i32,
        x2: i64,
        y2: i64,
        z2: i32,
        color: u32,
    ) {
        let min_x = (x0.min(x1).min(x2)).max(0).min(self.width as i64 - 1);
        let max_x = (x0.max(x1).max(x2)).max(0).min(self.width as i64 - 1);
        let min_y = (y0.min(y1).min(y2)).max(0).min(self.height as i64 - 1);
        let max_y = (y0.max(y1).max(y2)).max(0).min(self.height as i64 - 1);

        if min_x > max_x || min_y > max_y {
            return;
        }

        let area = (x1 - x0) * (y2 - y0) - (y1 - y0) * (x2 - x0);
        if area == 0 {
            return;
        }

        for y in min_y..=max_y {
            let row_idx = y as usize * self.width;

            for x in min_x..=max_x {
                // Edge functions
                let w0 = (x2 - x1) * (y - y1) - (y2 - y1) * (x - x1);
                let w1 = (x0 - x2) * (y - y2) - (y0 - y2) * (x - x2);
                let w2 = (x1 - x0) * (y - y0) - (y1 - y0) * (x - x0);

                if (w0 >= 0 && w1 >= 0 && w2 >= 0) || (w0 <= 0 && w1 <= 0 && w2 <= 0) {
                    // Interpolate depth
                    let depth = ((w0 * z0 as i64 + w1 * z1 as i64 + w2 * z2 as i64) / area) as i32;

                    let pixel_idx = row_idx + x as usize;
                    if depth > self.depth_buffer[pixel_idx] {
                        self.depth_buffer[pixel_idx] = depth;
                        fb.set_pixel(x as i32, y as i32, color);
                    }
                }
            }
        }
    }
}
