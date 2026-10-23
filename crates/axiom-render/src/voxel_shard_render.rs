//! # 3D Voxel Shard Ballistic Particle Rasterizer
//!
//! Renders rigid tumbling voxel shards expelled by craters and explosions
//! with Reversed-Z depth buffer testing and faceted lighting.

use crate::framebuffer::FrameBuffer;
use crate::math3d::{Mat4Fixed, Vec3Fixed};
use axiom_core::Fixed;
use axiom_core::voxel_destruct::{VOXEL_ARMOR, VOXEL_METAL, VOXEL_MOLTEN, VOXEL_STONE};
use axiom_core::voxel_shard::VoxelShardPool;

/// Palette colors for distinct voxel shard materials.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VoxelShardColorMap {
    pub highlight: u32,
    pub base: u32,
    pub shadow: u32,
}

impl VoxelShardColorMap {
    pub const fn for_material(material: u8) -> Self {
        match material {
            VOXEL_METAL => Self {
                highlight: 0xFFF8FAFC, // Specular Platinum
                base: 0xFFCBD5E1,      // Metallic Steel
                shadow: 0xFF475569,    // Deep Slate
            },
            VOXEL_ARMOR => Self {
                highlight: 0xFFFDE047, // Golden Luster
                base: 0xFFD97706,      // Armored Brass
                shadow: 0xFF78350F,    // Dark Bronze
            },
            VOXEL_MOLTEN => Self {
                highlight: 0xFFFFFBEB, // White-hot incandescent
                base: 0xFFF97316,      // Magma Orange
                shadow: 0xFFB45309,    // Cooling Crust
            },
            VOXEL_STONE | _ => Self {
                highlight: 0xFF94A3B8, // Granite Surface
                base: 0xFF64748B,      // Dense Rock
                shadow: 0xFF334155,    // Dark Basalt
            },
        }
    }
}

/// Renders all active voxel shards from the pool into the frame buffer with Reversed-Z depth test.
pub fn render_voxel_shards<const CAP: usize>(
    fb: &mut FrameBuffer,
    depth_buffer: &mut [i32],
    pool: &VoxelShardPool<CAP>,
    view_mat: &Mat4Fixed,
    cam_dist: Fixed,
    focal_length: i64,
) -> usize {
    let cx = (fb.width / 2) as i64;
    let cy = (fb.height / 2) as i64;
    let mut rendered_count = 0;

    for s in &pool.shards {
        if !s.is_active {
            continue;
        }

        let p_world = Vec3Fixed::from_raw(s.pos[0] as i64, s.pos[1] as i64, s.pos[2] as i64);
        let p_cam = view_mat.transform_point(p_world);
        let vz = (cam_dist - p_cam.z).raw();

        // Cull if behind near camera plane
        if vz <= 65536 / 10 {
            continue;
        }

        let sx = cx + (p_cam.x.raw() * focal_length) / vz;
        let sy = cy - (p_cam.y.raw() * focal_length) / vz;
        let inv_z = ((1i64 << 30) / vz) as i32;

        let col_map = VoxelShardColorMap::for_material(s.material);

        // Perspective-scaled shard extent
        let world_size = (s.scale_permille as i64 * 350) / 1000;
        let rad = ((world_size * focal_length) / vz).clamp(1, 16);

        let min_px = (sx - rad).max(0).min(fb.width as i64 - 1);
        let max_px = (sx + rad).max(0).min(fb.width as i64 - 1);
        let min_py = (sy - rad).max(0).min(fb.height as i64 - 1);
        let max_py = (sy + rad).max(0).min(fb.height as i64 - 1);

        // Tumbling rotation phase offset from BAMS yaw/pitch
        let rot_phase = (s.rot_bams[0] as i64 + s.rot_bams[1] as i64) % 64;

        for y in min_py..=max_py {
            let row_offset = y as usize * fb.width;
            let rel_y = y - sy;

            for x in min_px..=max_px {
                let pixel_idx = row_offset + x as usize;
                let rel_x = x - sx;

                // Reversed-Z occlusion test
                if inv_z > depth_buffer[pixel_idx] {
                    depth_buffer[pixel_idx] = inv_z;

                    // Faceted 3D shading based on rotated coordinates
                    let facet = rel_x + rel_y + (rot_phase / 8);
                    let color = if facet < -1 {
                        col_map.highlight
                    } else if facet > 1 {
                        col_map.shadow
                    } else {
                        col_map.base
                    };

                    fb.pixels[pixel_idx] = color;
                }
            }
        }

        rendered_count += 1;
    }

    rendered_count
}
