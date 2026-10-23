//! # 3D Thermal Stress Field & Incandescent Emissive Rasterizer
//!
//! Renders thermal heat radiance and incandescent emissive glow from 3D voxel
//! volumes and thermal stress fields, using Reversed-Z depth testing and additive
//! integer color blending.

use crate::framebuffer::FrameBuffer;
use crate::math3d::{Mat4Fixed, Vec3Fixed};
use axiom_core::Fixed;
use axiom_core::thermal_stress::{
    ThermalStressField16, TEMP_INCANDESCENT_THRESHOLD, TEMP_WARM_THRESHOLD,
};
use axiom_core::voxel_destruct::VOXEL_DIM;

/// Evaluates thermal emission ARGB color for a given integer temperature (0..1000).
#[inline]
pub fn sample_thermal_glow_color(temp: u16) -> Option<u32> {
    if temp < TEMP_WARM_THRESHOLD {
        None
    } else if temp < TEMP_INCANDESCENT_THRESHOLD {
        // Warm amber to bright orange gradient (0xFFEA580C -> 0xFFFD9F07)
        let t = (temp - TEMP_WARM_THRESHOLD) as u32;
        let r = (234 + (t * 19 / 400)).min(255);
        let g = (88 + (t * 71 / 400)).min(255);
        let b = 12;
        Some(0xFF00_0000 | (r << 16) | (g << 8) | b)
    } else {
        // High-temperature white-hot incandescent emission (0xFFFEF08A -> 0xFFFFFBEB)
        let t = (temp - TEMP_INCANDESCENT_THRESHOLD) as u32;
        let r = 255;
        let g = (240 + (t * 11 / 300)).min(255);
        let b = (138 + (t * 97 / 300)).min(235);
        Some(0xFF00_0000 | (r << 16) | (g << 8) | b)
    }
}

/// Renders 3D thermal incandescent emissive voxels directly to the framebuffer.
///
/// Returns the number of emissive glowing thermal voxels successfully rasterized.
pub fn render_thermal_glow(
    fb: &mut FrameBuffer,
    depth_buffer: &mut [i32],
    vp: &Mat4Fixed,
    thermal: &ThermalStressField16,
    voxel_scale: Fixed,
    world_origin: Vec3Fixed,
) -> usize {
    let mut rendered_count = 0;
    let hw = (fb.width as i32) / 2;
    let hh = (fb.height as i32) / 2;

    for y in 0..VOXEL_DIM {
        for z in 0..VOXEL_DIM {
            for x in 0..VOXEL_DIM {
                let temp = thermal.get_temp(x, y, z);
                let glow_color = match sample_thermal_glow_color(temp) {
                    Some(c) => c,
                    None => continue,
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

                for py in (sy - size / 2)..=(sy + size / 2) {
                    if py < 0 || py >= fb.height as i32 {
                        continue;
                    }
                    for px in (sx - size / 2)..=(sx + size / 2) {
                        if px < 0 || px >= fb.width as i32 {
                            continue;
                        }
                        let p_idx = (py as usize) * fb.width + (px as usize);

                        // Reversed-Z depth testing (closer objects have higher 1/Z or depth value)
                        if sz_q16 > depth_buffer[p_idx] {
                            depth_buffer[p_idx] = sz_q16;

                            // Additive thermal emissive blending
                            let cur_pixel = fb.pixels[p_idx];
                            let cr = (cur_pixel >> 16) & 0xFF;
                            let cg = (cur_pixel >> 8) & 0xFF;
                            let cb = cur_pixel & 0xFF;

                            let gr = (glow_color >> 16) & 0xFF;
                            let gg = (glow_color >> 8) & 0xFF;
                            let gb = glow_color & 0xFF;

                            let out_r = (cr + gr / 2).min(255);
                            let out_g = (cg + gg / 2).min(255);
                            let out_b = (cb + gb / 2).min(255);

                            fb.pixels[p_idx] = 0xFF00_0000 | (out_r << 16) | (out_g << 8) | out_b;
                        }
                    }
                }

                rendered_count += 1;
            }
        }
    }

    rendered_count
}
