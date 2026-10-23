//! # 3D Voxel Dynamic Fluid & Porous Infiltration Rasterizer
//!
//! Renders real-time 3D fluid surfaces and porous rock infiltration dampness
//! directly to the framebuffer using Reversed-Z depth testing and zero-float
//! fixed-point perspective projection.

use crate::framebuffer::FrameBuffer;
use crate::math3d::{Mat4Fixed, Vec3Fixed};
use axiom_core::Fixed;
use axiom_core::voxel_destruct::VOXEL_DIM;
use axiom_core::voxel_fluid::VoxelFluidField16;

/// Evaluates fluid water ARGB color and opacity for a given integer level (1..255).
#[inline]
pub fn sample_fluid_color(level: u8) -> (u32, u32) {
    if level >= 180 {
        // Deep cerulean / sapphire water (higher opacity)
        (0x001D_4ED8, 210)
    } else if level >= 80 {
        // Medium cobalt / azure water
        (0x0025_63EB, 175)
    } else {
        // Shallow sky cyan puddle / spray
        (0x0038_BDF8, 140)
    }
}

/// Evaluates infiltrated wet rock darkening color and alpha for dampness (1..255).
#[inline]
pub fn sample_infiltrated_color(infiltrated: u8) -> (u32, u32) {
    // Dark slate dampness sheen (0x0F172A)
    let alpha = (40 + ((infiltrated as u32) * 100 / 255)).min(140);
    (0x000F_172A, alpha)
}

/// Renders 3D voxel fluid surfaces and porous rock dampness to the framebuffer.
///
/// Returns the number of fluid voxels successfully rasterized.
pub fn render_voxel_fluid(
    fb: &mut FrameBuffer,
    depth_buffer: &mut [i32],
    vp: &Mat4Fixed,
    fluid: &VoxelFluidField16,
    voxel_scale: Fixed,
    world_origin: Vec3Fixed,
) -> usize {
    let mut rendered_count = 0;
    let hw = (fb.width as i32) / 2;
    let hh = (fb.height as i32) / 2;

    for y in 0..VOXEL_DIM {
        for z in 0..VOXEL_DIM {
            for x in 0..VOXEL_DIM {
                let level = fluid.get_level(x, y, z);
                let infiltrated = fluid.get_infiltrated(x, y, z);

                if level == 0 && infiltrated == 0 {
                    continue;
                }

                let (base_rgb, alpha) = if level > 0 {
                    sample_fluid_color(level)
                } else {
                    sample_infiltrated_color(infiltrated)
                };

                // Offset relative to voxel grid center (8, 8, 8)
                let rx = Fixed::from_int((x as i64) - 8) * voxel_scale;
                let ry = Fixed::from_int((y as i64) - 8) * voxel_scale;
                let rz = Fixed::from_int((z as i64) - 8) * voxel_scale;

                let world_pos = Vec3Fixed::new(
                    world_origin.x + rx,
                    world_origin.y + ry,
                    world_origin.z + rz,
                );

                let clip = vp.transform_point(world_pos);
                if clip.z.raw() <= 0 {
                    continue;
                }

                // Perspective division to screen coordinates
                let z_recip = Fixed::ONE / clip.z;
                let sx = hw + (clip.x * z_recip * Fixed::from_int(hw as i64)).to_int() as i32;
                let sy = hh - (clip.y * z_recip * Fixed::from_int(hh as i64)).to_int() as i32;

                let size = (voxel_scale * z_recip * Fixed::from_int(hw as i64)).to_int().clamp(1, 8) as i32;
                let sz_q16 = clip.z.raw() as i32;

                let fr = (base_rgb >> 16) & 0xFF;
                let fg = (base_rgb >> 8) & 0xFF;
                let fb_col = base_rgb & 0xFF;
                let inv_a = 255 - alpha;

                for py in (sy - size / 2)..=(sy + size / 2) {
                    if py < 0 || py >= fb.height as i32 {
                        continue;
                    }
                    for px in (sx - size / 2)..=(sx + size / 2) {
                        if px < 0 || px >= fb.width as i32 {
                            continue;
                        }
                        let p_idx = (py as usize) * fb.width + (px as usize);

                        // Reversed-Z depth testing (closer objects have higher depth value)
                        // Allow slight offset for water layer over solid terrain
                        if sz_q16 >= depth_buffer[p_idx].saturating_sub(64) {
                            let cur_pixel = fb.pixels[p_idx];
                            let cr = (cur_pixel >> 16) & 0xFF;
                            let cg = (cur_pixel >> 8) & 0xFF;
                            let cb = cur_pixel & 0xFF;

                            let out_r = ((cr * inv_a + fr * alpha) / 255).min(255);
                            let out_g = ((cg * inv_a + fg * alpha) / 255).min(255);
                            let out_b = ((cb * inv_a + fb_col * alpha) / 255).min(255);

                            fb.pixels[p_idx] = 0xFF00_0000 | (out_r << 16) | (out_g << 8) | out_b;
                            if sz_q16 > depth_buffer[p_idx] {
                                depth_buffer[p_idx] = sz_q16;
                            }
                        }
                    }
                }

                rendered_count += 1;
            }
        }
    }

    rendered_count
}
