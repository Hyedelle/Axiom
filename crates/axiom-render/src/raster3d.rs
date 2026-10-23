//! Software 3D Rasterizer with Juan Pineda 8x8 Tile Binning, Reversed-Z Affine Interpolation,
//! and Manga Inverted Hull Ink Outlines.
//!
//! Renders 3D mecha models directly to `FrameBuffer` using deterministic
//! fixed-point integer mathematics with zero external GPU dependencies.
//! Follows Steel-the-Show Color Grammar v1 (INK, IRON, AMBER, BRASS, RED, OLIVE).

use crate::framebuffer::FrameBuffer;
use crate::math3d::{Mat4Fixed, Vec3Fixed};
use crate::mesh3d::{MechaModel3D, PartMesh};
use axiom_core::Fixed;

/// Color palette for mecha cel-shading adhering to Steel-the-Show Color Grammar v1.
#[derive(Debug, Clone, Copy)]
pub struct CelShadingPalette {
    pub highlight: u32,
    pub base: u32,
    pub shadow: u32,
    pub outline: u32,
}

impl CelShadingPalette {
    /// Canon INK outline color from color-grammar-v1.md (#2B222A).
    pub const INK: u32 = 0xFF2B222A;
}

impl Default for CelShadingPalette {
    fn default() -> Self {
        // Canon STEEL armor palette
        Self {
            highlight: 0xFFD1D5DB, // Pale cool steel
            base: 0xFF64748B,      // Mid slate steel
            shadow: 0xFF334155,    // Dark naval steel
            outline: Self::INK,    // #2B222A Manga ink
        }
    }
}

/// Palette presets mapped to canon materials from Steel-the-Show `color-grammar-v1.md`.
impl CelShadingPalette {
    pub fn for_part(name: &str) -> Self {
        let n = name.to_ascii_lowercase();
        if n.contains("eye") {
            // Canon glowing orange porthole eye (#FF660A / glowing warm gold highlight #FFD973)
            Self {
                highlight: 0xFFFFD973,
                base: 0xFFFF660A,
                shadow: 0xFFEA580C,
                outline: Self::INK,
            }
        } else if n.contains("head") {
            // Canon Gunmetal Gray dome (#514447)
            Self {
                highlight: 0xFF7D7275,
                base: 0xFF514447,
                shadow: 0xFF352B2D,
                outline: Self::INK,
            }
        } else if n.contains("glove") {
            // Canon Weathered Red boxing gloves (#8A2B17)
            Self {
                highlight: 0xFFB9442C,
                base: 0xFF8A2B17,
                shadow: 0xFF541508,
                outline: Self::INK,
            }
        } else if n.contains("torso") {
            // Canon Olive Drab chassis (#7F772B)
            Self {
                highlight: 0xFFA8A03E,
                base: 0xFF7F772B,
                shadow: 0xFF4D4818,
                outline: Self::INK,
            }
        } else if n.contains("pelvis") || n.contains("boot") || n.contains("leg") {
            // Canon Brown boots & chassis (#6F4430)
            Self {
                highlight: 0xFF8F5F49,
                base: 0xFF6F4430,
                shadow: 0xFF442619,
                outline: Self::INK,
            }
        } else if n.contains("arm") {
            // Canon Dark worn iron chassis (#5A5148)
            Self {
                highlight: 0xFF7E7367,
                base: 0xFF5A5148,
                shadow: 0xFF332D28,
                outline: Self::INK,
            }
        } else if n.contains("brass") || n.contains("piston") {
            // Canon Brass fittings (#A06432)
            Self {
                highlight: 0xFFFED7AA,
                base: 0xFFA06432,
                shadow: 0xFF6E5320,
                outline: Self::INK,
            }
        } else {
            // Primary mecha armor default
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
    pub custom_palette: Option<CelShadingPalette>,
    pub draw_outline: bool,
    pub outline_width: Fixed,
    pub outline_color: u32,
    pub rim_light_color: Option<u32>,
    pub eye_glow_boost: bool,
    pub custom_texture: Option<crate::texture::TextureMap>,
    pub texture_wrap_mode: crate::texture::TextureWrapMode,
}

impl Rasterizer3D {
    pub fn new(width: usize, height: usize) -> Self {
        let light = Vec3Fixed::from_f32(0.577, 0.577, 0.577);
        Self {
            width,
            height,
            depth_buffer: vec![i32::MIN; width * height],
            light_dir: light,
            cull_backfaces: false,
            custom_palette: None,
            draw_outline: true,
            outline_width: Fixed::from_raw(524), // ~0.008m extrusion
            outline_color: CelShadingPalette::INK,
            rim_light_color: None,
            eye_glow_boost: true,
            custom_texture: None,
            texture_wrap_mode: crate::texture::TextureWrapMode::Repeat,
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
        let default_transforms = crate::kinematics3d::PartTransformTable::default();
        self.render_model_posed(
            fb,
            model,
            &default_transforms,
            world_mat,
            cam_dist,
            focal_length,
        );
    }

    /// Renders a complete `MechaModel3D` with individual part kinematics transforms.
    pub fn render_model_posed(
        &mut self,
        fb: &mut FrameBuffer,
        model: &MechaModel3D,
        transforms: &crate::kinematics3d::PartTransformTable,
        world_mat: &Mat4Fixed,
        cam_dist: Fixed,
        focal_length: i64,
    ) {
        let cx = (self.width / 2) as i64;
        let cy = (self.height / 2) as i64;

        // Pass 1: Inverted Hull Manga Ink Outline (Extruded backface shell)
        if self.draw_outline && self.outline_width.raw() > 0 {
            for part in &model.parts {
                let part_mat = crate::kinematics3d::get_part_matrix(transforms, &part.name);
                let effective_world = world_mat.mul_mat(&part_mat);

                let mut outline_verts: Vec<(i64, i64, i32)> =
                    Vec::with_capacity(part.vertices.len());

                for v in &part.vertices {
                    // Displace vertex outward along its normal
                    let p_extruded = Vec3Fixed::new(
                        v.pos.x + v.norm.x * self.outline_width,
                        v.pos.y + v.norm.y * self.outline_width,
                        v.pos.z + v.norm.z * self.outline_width,
                    );

                    let p_world = effective_world.transform_point(p_extruded);
                    let vz = (cam_dist - p_world.z).raw();

                    if vz <= 65536 / 10 {
                        outline_verts.push((0, 0, i32::MIN));
                        continue;
                    }

                    let sx = cx + (p_world.x.raw() * focal_length) / vz;
                    let sy = cy - (p_world.y.raw() * focal_length) / vz;
                    // Slightly depress depth so body always wins
                    let inv_z = (((1i64 << 30) / vz) as i32).saturating_sub(400);

                    outline_verts.push((sx, sy, inv_z));
                }

                let tri_count = part.indices.len() / 3;
                for t in 0..tri_count {
                    let i0 = part.indices[t * 3] as usize;
                    let i1 = part.indices[t * 3 + 1] as usize;
                    let i2 = part.indices[t * 3 + 2] as usize;

                    if i0 >= outline_verts.len() || i1 >= outline_verts.len() || i2 >= outline_verts.len() {
                        continue;
                    }

                    let (x0, y0, z0) = outline_verts[i0];
                    let (x1, y1, z1) = outline_verts[i1];
                    let (x2, y2, z2) = outline_verts[i2];

                    if z0 == i32::MIN || z1 == i32::MIN || z2 == i32::MIN {
                        continue;
                    }

                    // Inverted hull: ONLY draw backfaces (area < 0 in original screen orientation)
                    let area = (x1 - x0) * (y2 - y0) - (y1 - y0) * (x2 - x0);
                    if area >= 0 {
                        continue; // Cull front-facing triangles
                    }

                    // Rasterize outline triangle in canon INK
                    let line_color = self.outline_color;
                    self.rasterize_pineda_8x8(
                        fb,
                        x0, y0, z0,
                        x1, y1, z1,
                        x2, y2, z2,
                        line_color,
                    );
                }
            }
        }

        // Pass 2: Main Mecha Body Cel-Shading
        for part in &model.parts {
            let palette = self
                .custom_palette
                .unwrap_or_else(|| CelShadingPalette::for_part(&part.name));
            let part_mat = crate::kinematics3d::get_part_matrix(transforms, &part.name);
            let effective_world = world_mat.mul_mat(&part_mat);

            let mut view_verts: Vec<(i64, i64, i32, Vec3Fixed, i32, i32)> =
                Vec::with_capacity(part.vertices.len());

            for v in &part.vertices {
                let p_world = effective_world.transform_point(v.pos);
                let n_world = effective_world.transform_vector(v.norm);

                let vz = (cam_dist - p_world.z).raw();

                if vz <= 65536 / 10 {
                    view_verts.push((0, 0, i32::MIN, n_world, v.u, v.v));
                    continue;
                }

                let sx = cx + (p_world.x.raw() * focal_length) / vz;
                let sy = cy - (p_world.y.raw() * focal_length) / vz;
                let inv_z = ((1i64 << 30) / vz) as i32;

                view_verts.push((sx, sy, inv_z, n_world, v.u, v.v));
            }

            let tri_count = part.indices.len() / 3;
            for t in 0..tri_count {
                let i0 = part.indices[t * 3] as usize;
                let i1 = part.indices[t * 3 + 1] as usize;
                let i2 = part.indices[t * 3 + 2] as usize;

                if i0 >= view_verts.len() || i1 >= view_verts.len() || i2 >= view_verts.len() {
                    continue;
                }

                let (x0, y0, z0, n0, u0, v0) = view_verts[i0];
                let (x1, y1, z1, n1, u1, v1) = view_verts[i1];
                let (x2, y2, z2, n2, u2, v2) = view_verts[i2];

                if z0 == i32::MIN || z1 == i32::MIN || z2 == i32::MIN {
                    continue;
                }

                let avg_norm = Vec3Fixed::new(
                    (n0.x + n1.x + n2.x) * Fixed::from_raw(21845),
                    (n0.y + n1.y + n2.y) * Fixed::from_raw(21845),
                    (n0.z + n1.z + n2.z) * Fixed::from_raw(21845),
                );

                let light_dot = avg_norm.dot(self.light_dir);

                // Cel-shading stepped tone selection with Visor Eye Glow and Rim Lighting
                let color = if self.eye_glow_boost && (part.name == "visor" || part.name.contains("eye")) {
                    0xFFFFF59D // Incandescent Oracle Visor beacon
                } else if let Some(rim_col) = self.rim_light_color {
                    // Grazing angle silhouette rim highlight
                    if avg_norm.z.raw().abs() < 16000 {
                        rim_col
                    } else if light_dot.raw() > 24000 {
                        palette.highlight
                    } else if light_dot.raw() > 4000 {
                        palette.base
                    } else {
                        palette.shadow
                    }
                } else if light_dot.raw() > 24000 {
                    palette.highlight
                } else if light_dot.raw() > 4000 {
                    palette.base
                } else {
                    palette.shadow
                };

                if self.custom_texture.is_some() {
                    self.rasterize_textured_pineda_8x8(
                        fb,
                        x0, y0, z0, u0, v0,
                        x1, y1, z1, u1, v1,
                        x2, y2, z2, u2, v2,
                        color,
                    );
                } else {
                    self.rasterize_pineda_8x8(fb, x0, y0, z0, x1, y1, z1, x2, y2, z2, color);
                }
            }
        }
    }

    /// Renders a single `PartMesh` (e.g. dynamic crater floor or severed mecha piece).
    pub fn render_part(
        &mut self,
        fb: &mut FrameBuffer,
        part: &PartMesh,
        world_mat: &Mat4Fixed,
        cam_dist: Fixed,
        focal_length: i64,
    ) {
        let cx = (self.width / 2) as i64;
        let cy = (self.height / 2) as i64;

        let palette = self
            .custom_palette
            .unwrap_or_else(|| CelShadingPalette::for_part(&part.name));

        let mut view_verts: Vec<(i64, i64, i32, Vec3Fixed, i32, i32)> =
            Vec::with_capacity(part.vertices.len());

        for v in &part.vertices {
            let p_world = world_mat.transform_point(v.pos);
            let n_world = world_mat.transform_vector(v.norm);

            let vz = (cam_dist - p_world.z).raw();

            if vz <= 65536 / 10 {
                view_verts.push((0, 0, i32::MIN, n_world, v.u, v.v));
                continue;
            }

            let sx = cx + (p_world.x.raw() * focal_length) / vz;
            let sy = cy - (p_world.y.raw() * focal_length) / vz;
            let inv_z = ((1i64 << 30) / vz) as i32;

            view_verts.push((sx, sy, inv_z, n_world, v.u, v.v));
        }

        let tri_count = part.indices.len() / 3;
        for t in 0..tri_count {
            let i0 = part.indices[t * 3] as usize;
            let i1 = part.indices[t * 3 + 1] as usize;
            let i2 = part.indices[t * 3 + 2] as usize;

            if i0 >= view_verts.len() || i1 >= view_verts.len() || i2 >= view_verts.len() {
                continue;
            }

            let (x0, y0, z0, n0, u0, v0) = view_verts[i0];
            let (x1, y1, z1, n1, _u1, _v1) = view_verts[i1];
            let (x2, y2, z2, n2, _u2, _v2) = view_verts[i2];

            if z0 == i32::MIN || z1 == i32::MIN || z2 == i32::MIN {
                continue;
            }

            let avg_norm = Vec3Fixed::new(
                (n0.x + n1.x + n2.x) * Fixed::from_raw(21845),
                (n0.y + n1.y + n2.y) * Fixed::from_raw(21845),
                (n0.z + n1.z + n2.z) * Fixed::from_raw(21845),
            );

            let light_dot = avg_norm.dot(self.light_dir);

            let color = if u0 >= 49152 || v0 >= 49152 {
                0xFFFFAA00 // Molten glowing slag / severed edge
            } else if light_dot.raw() > 24000 {
                palette.highlight
            } else if light_dot.raw() > 4000 {
                palette.base
            } else {
                palette.shadow
            };

            self.rasterize_pineda_8x8(fb, x0, y0, z0, x1, y1, z1, x2, y2, z2, color);
        }
    }

    /// Juan Pineda 8x8 Tile Binning Triangle Rasterizer with Affine 1/Z and UV interpolation.
    pub fn rasterize_textured_pineda_8x8(
        &mut self,
        fb: &mut FrameBuffer,
        x0: i64,
        y0: i64,
        z0: i32,
        u0: i32,
        v0: i32,
        mut x1: i64,
        mut y1: i64,
        mut z1: i32,
        mut u1: i32,
        mut v1: i32,
        mut x2: i64,
        mut y2: i64,
        mut z2: i32,
        mut u2: i32,
        mut v2: i32,
        base_color: u32,
    ) {
        let (width, height, wrap_mode, cull_backfaces) = (
            self.width,
            self.height,
            self.texture_wrap_mode,
            self.cull_backfaces,
        );
        let texture = match &self.custom_texture {
            Some(t) => t,
            None => return,
        };
        let depth_buffer = &mut self.depth_buffer;

        let area_init = (x1 - x0) * (y2 - y0) - (y1 - y0) * (x2 - x0);
        if area_init == 0 {
            return;
        }

        if cull_backfaces && area_init <= 0 {
            return;
        }

        if area_init < 0 {
            core::mem::swap(&mut x1, &mut x2);
            core::mem::swap(&mut y1, &mut y2);
            core::mem::swap(&mut z1, &mut z2);
            core::mem::swap(&mut u1, &mut u2);
            core::mem::swap(&mut v1, &mut v2);
        }

        let area = (x1 - x0) * (y2 - y0) - (y1 - y0) * (x2 - x0);
        if area <= 0 {
            return;
        }

        let min_x = (x0.min(x1).min(x2)).max(0).min(width as i64 - 1);
        let max_x = (x0.max(x1).max(x2)).max(0).min(width as i64 - 1);
        let min_y = (y0.min(y1).min(y2)).max(0).min(height as i64 - 1);
        let max_y = (y0.max(y1).max(y2)).max(0).min(height as i64 - 1);

        if min_x > max_x || min_y > max_y {
            return;
        }

        let dx0 = x2 - x1;
        let dy0 = y2 - y1;
        let dx1 = x0 - x2;
        let dy1 = y0 - y2;
        let dx2 = x1 - x0;
        let dy2 = y1 - y0;

        let dz10 = z1 as i64 - z0 as i64;
        let dz20 = z2 as i64 - z0 as i64;
        let d_zinv_dx = (dz10 * (y2 - y0) - dz20 * (y1 - y0)) / area;
        let d_zinv_dy = (dz20 * (x1 - x0) - dz10 * (x2 - x0)) / area;

        let du10 = u1 as i64 - u0 as i64;
        let du20 = u2 as i64 - u0 as i64;
        let du_dx = (du10 * (y2 - y0) - du20 * (y1 - y0)) / area;
        let du_dy = (du20 * (x1 - x0) - du10 * (x2 - x0)) / area;

        let dv10 = v1 as i64 - v0 as i64;
        let dv20 = v2 as i64 - v0 as i64;
        let dv_dx = (dv10 * (y2 - y0) - dv20 * (y1 - y0)) / area;
        let dv_dy = (dv20 * (x1 - x0) - dv10 * (x2 - x0)) / area;

        let tile_min_x = min_x / 8;
        let tile_max_x = max_x / 8;
        let tile_min_y = min_y / 8;
        let tile_max_y = max_y / 8;

        for ty in tile_min_y..=tile_max_y {
            let t_y0 = ty * 8;
            let t_y1 = (t_y0 + 7).min(height as i64 - 1);

            for tx in tile_min_x..=tile_max_x {
                let t_x0 = tx * 8;
                let t_x1 = (t_x0 + 7).min(width as i64 - 1);

                // Tile Trivial Reject Test at 4 corners
                let e0_c00 = dx0 * (t_y0 - y1) - dy0 * (t_x0 - x1);
                let e0_c10 = dx0 * (t_y0 - y1) - dy0 * (t_x1 - x1);
                let e0_c01 = dx0 * (t_y1 - y1) - dy0 * (t_x0 - x1);
                let e0_c11 = dx0 * (t_y1 - y1) - dy0 * (t_x1 - x1);
                if e0_c00 < 0 && e0_c10 < 0 && e0_c01 < 0 && e0_c11 < 0 {
                    continue;
                }

                let e1_c00 = dx1 * (t_y0 - y2) - dy1 * (t_x0 - x2);
                let e1_c10 = dx1 * (t_y0 - y2) - dy1 * (t_x1 - x2);
                let e1_c01 = dx1 * (t_y1 - y2) - dy1 * (t_x0 - x2);
                let e1_c11 = dx1 * (t_y1 - y2) - dy1 * (t_x1 - x2);
                if e1_c00 < 0 && e1_c10 < 0 && e1_c01 < 0 && e1_c11 < 0 {
                    continue;
                }

                let e2_c00 = dx2 * (t_y0 - y0) - dy2 * (t_x0 - x0);
                let e2_c10 = dx2 * (t_y0 - y0) - dy2 * (t_x1 - x0);
                let e2_c01 = dx2 * (t_y1 - y0) - dy2 * (t_x0 - x0);
                let e2_c11 = dx2 * (t_y1 - y0) - dy2 * (t_x1 - x0);
                if e2_c00 < 0 && e2_c10 < 0 && e2_c01 < 0 && e2_c11 < 0 {
                    continue;
                }

                // Incremental evaluation within tile
                let mut row_e0 = dx0 * (t_y0 - y1) - dy0 * (t_x0 - x1);
                let mut row_e1 = dx1 * (t_y0 - y2) - dy1 * (t_x0 - x2);
                let mut row_e2 = dx2 * (t_y0 - y0) - dy2 * (t_x0 - x0);
                let mut row_zinv = z0 as i64 + (t_x0 - x0) * d_zinv_dx + (t_y0 - y0) * d_zinv_dy;
                let mut row_u = u0 as i64 + (t_x0 - x0) * du_dx + (t_y0 - y0) * du_dy;
                let mut row_v = v0 as i64 + (t_x0 - x0) * dv_dx + (t_y0 - y0) * dv_dy;

                for py in t_y0..=t_y1 {
                    let row_idx = py as usize * width;
                    let mut e0 = row_e0;
                    let mut e1 = row_e1;
                    let mut e2 = row_e2;
                    let mut zinv = row_zinv;
                    let mut u = row_u;
                    let mut v = row_v;

                    for px in t_x0..=t_x1 {
                        if (e0 | e1 | e2) >= 0 {
                            let pixel_idx = row_idx + px as usize;
                            let z_val = zinv as i32;

                            if z_val > depth_buffer[pixel_idx] {
                                depth_buffer[pixel_idx] = z_val;
                                let tex_col = texture.sample_nearest(
                                    u as i32,
                                    v as i32,
                                    wrap_mode,
                                );
                                let final_col = crate::texture::blend_decal(base_color, tex_col);
                                fb.pixels[pixel_idx] = final_col;
                            }
                        }

                        e0 -= dy0;
                        e1 -= dy1;
                        e2 -= dy2;
                        zinv += d_zinv_dx;
                        u += du_dx;
                        v += dv_dx;
                    }

                    row_e0 += dx0;
                    row_e1 += dx1;
                    row_e2 += dx2;
                    row_zinv += d_zinv_dy;
                    row_u += du_dy;
                    row_v += dv_dy;
                }
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
        let area_init = (x1 - x0) * (y2 - y0) - (y1 - y0) * (x2 - x0);
        if area_init == 0 {
            return;
        }

        if self.cull_backfaces && area_init <= 0 {
            return;
        }

        if area_init < 0 {
            core::mem::swap(&mut x1, &mut x2);
            core::mem::swap(&mut y1, &mut y2);
            core::mem::swap(&mut z1, &mut z2);
        }

        let area = (x1 - x0) * (y2 - y0) - (y1 - y0) * (x2 - x0);
        if area <= 0 {
            return;
        }

        let min_x = (x0.min(x1).min(x2)).max(0).min(self.width as i64 - 1);
        let max_x = (x0.max(x1).max(x2)).max(0).min(self.width as i64 - 1);
        let min_y = (y0.min(y1).min(y2)).max(0).min(self.height as i64 - 1);
        let max_y = (y0.max(y1).max(y2)).max(0).min(self.height as i64 - 1);

        if min_x > max_x || min_y > max_y {
            return;
        }

        let dx0 = x2 - x1;
        let dy0 = y2 - y1;
        let dx1 = x0 - x2;
        let dy1 = y0 - y2;
        let dx2 = x1 - x0;
        let dy2 = y1 - y0;

        let dz10 = z1 as i64 - z0 as i64;
        let dz20 = z2 as i64 - z0 as i64;
        let d_zinv_dx = (dz10 * (y2 - y0) - dz20 * (y1 - y0)) / area;
        let d_zinv_dy = (dz20 * (x1 - x0) - dz10 * (x2 - x0)) / area;

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
                let e0_c00 = dx0 * (t_y0 - y1) - dy0 * (t_x0 - x1);
                let e0_c10 = dx0 * (t_y0 - y1) - dy0 * (t_x1 - x1);
                let e0_c01 = dx0 * (t_y1 - y1) - dy0 * (t_x0 - x1);
                let e0_c11 = dx0 * (t_y1 - y1) - dy0 * (t_x1 - x1);
                if e0_c00 < 0 && e0_c10 < 0 && e0_c01 < 0 && e0_c11 < 0 {
                    continue;
                }

                let e1_c00 = dx1 * (t_y0 - y2) - dy1 * (t_x0 - x2);
                let e1_c10 = dx1 * (t_y0 - y2) - dy1 * (t_x1 - x2);
                let e1_c01 = dx1 * (t_y1 - y2) - dy1 * (t_x0 - x2);
                let e1_c11 = dx1 * (t_y1 - y2) - dy1 * (t_x1 - x2);
                if e1_c00 < 0 && e1_c10 < 0 && e1_c01 < 0 && e1_c11 < 0 {
                    continue;
                }

                let e2_c00 = dx2 * (t_y0 - y0) - dy2 * (t_x0 - x0);
                let e2_c10 = dx2 * (t_y0 - y0) - dy2 * (t_x1 - x0);
                let e2_c01 = dx2 * (t_y1 - y0) - dy2 * (t_x0 - x0);
                let e2_c11 = dx2 * (t_y1 - y0) - dy2 * (t_x1 - x0);
                if e2_c00 < 0 && e2_c10 < 0 && e2_c01 < 0 && e2_c11 < 0 {
                    continue;
                }

                // Incremental evaluation within tile
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
                        if (e0 | e1 | e2) >= 0 {
                            let pixel_idx = row_idx + px as usize;
                            let z_val = zinv as i32;

                            if z_val > self.depth_buffer[pixel_idx] {
                                self.depth_buffer[pixel_idx] = z_val;
                                fb.pixels[pixel_idx] = color;
                            }
                        }

                        e0 -= dy0;
                        e1 -= dy1;
                        e2 -= dy2;
                        zinv += d_zinv_dx;
                    }

                    row_e0 += dx0;
                    row_e1 += dx1;
                    row_e2 += dx2;
                    row_zinv += d_zinv_dy;
                }
            }
        }
    }
}
