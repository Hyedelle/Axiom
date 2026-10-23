//! # 3D Dynamic Voxel Collapse & Structural Fragment Software Rasterizer
//!
//! Renders rigid dynamic voxel chunks detaching from collapsed structures
//! with Reversed-Z depth buffer testing, faceted tumbling shading, and edge beveling.

use crate::framebuffer::FrameBuffer;
use crate::math3d::{Mat4Fixed, Vec3Fixed};
use crate::voxel_shard_render::VoxelShardColorMap;
use axiom_core::Fixed;
use axiom_core::voxel_collapse::DynamicCollapseSystem;

/// Renders all active collapsed dynamic voxel chunks into the frame buffer with Reversed-Z depth testing.
pub fn render_dynamic_collapse_chunks<const MAX_CHUNKS: usize>(
    fb: &mut FrameBuffer,
    depth_buffer: &mut [i32],
    collapse: &DynamicCollapseSystem<MAX_CHUNKS>,
    view_mat: &Mat4Fixed,
    cam_dist: Fixed,
    focal_length: i64,
) -> usize {
    let cx = (fb.width / 2) as i64;
    let cy = (fb.height / 2) as i64;
    let mut rendered_count = 0;

    for chunk in &collapse.chunks {
        if !chunk.is_active {
            continue;
        }

        let p_world = Vec3Fixed::from_raw(chunk.pos[0] as i64, chunk.pos[1] as i64, chunk.pos[2] as i64);
        let p_cam = view_mat.transform_point(p_world);
        let vz = (cam_dist - p_cam.z).raw();

        // Near plane clipping
        if vz <= 65536 / 10 {
            continue;
        }

        let sx = cx + (p_cam.x.raw() * focal_length) / vz;
        let sy = cy - (p_cam.y.raw() * focal_length) / vz;
        let inv_z = ((1i64 << 30) / vz) as i32;

        let col_map = VoxelShardColorMap::for_material(chunk.material);

        // Perspective-projected extent in screen pixels
        let ext_x = chunk.shape_extent[0] as i64 * 800;
        let ext_y = chunk.shape_extent[1] as i64 * 800;

        let rad_x = ((ext_x * focal_length) / vz).clamp(2, 40);
        let rad_y = ((ext_y * focal_length) / vz).clamp(2, 40);

        let min_px = (sx - rad_x).max(0).min(fb.width as i64 - 1);
        let max_px = (sx + rad_x).max(0).min(fb.width as i64 - 1);
        let min_py = (sy - rad_y).max(0).min(fb.height as i64 - 1);
        let max_py = (sy + rad_y).max(0).min(fb.height as i64 - 1);

        // Faceted tumbling lighting based on chunk rotation BAMS
        let rot_yaw = chunk.rot_bams[0] as i64;
        let rot_pitch = chunk.rot_bams[1] as i64;
        let light_phase = (rot_yaw + rot_pitch * 2) % 256;

        let mut drew_any_pixel = false;

        for py in min_py..=max_py {
            let row_offset = py as usize * fb.width;
            let dy = py - sy;

            for px in min_px..=max_px {
                let p_idx = row_offset + px as usize;
                let dx = px - sx;

                // Diamond/Box faceted metric
                let dist_metric = (dx.abs() * rad_y + dy.abs() * rad_x) / rad_y.max(1);
                if dist_metric > rad_x {
                    continue;
                }

                // Reversed-Z Depth Test
                if inv_z > depth_buffer[p_idx] {
                    depth_buffer[p_idx] = inv_z;

                    // Blocky edge bevel outline
                    let is_border = dist_metric >= rad_x - 1;

                    let pixel_color = if is_border {
                        0xFF0F172A // Dark Slate Outline
                    } else if (dx + dy + light_phase) % 64 < 24 {
                        col_map.highlight
                    } else if (dx - dy + light_phase) % 64 < 48 {
                        col_map.base
                    } else {
                        col_map.shadow
                    };

                    fb.set_pixel(px as i32, py as i32, pixel_color);
                    drew_any_pixel = true;
                }
            }
        }

        if drew_any_pixel {
            rendered_count += 1;
        }
    }

    rendered_count
}
