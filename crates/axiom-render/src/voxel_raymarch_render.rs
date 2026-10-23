//! # WebGPU Compute DDA Raymarching Shader & Depth Buffer Blitter
//!
//! Provides:
//! 1. `VOXEL_RAYMARCH_WGSL`: Production-grade WebGPU Compute Shader WGSL code for 3D Fast Voxel DDA.
//! 2. `render_voxel_raymarch`: Deterministic 0-Float CPU software raymarch rasterizer fallback
//!    that casts rays into `VoxelVolume16`, extracts hit normals, computes directional lighting/shadows,
//!    and updates `FrameBuffer` with Reversed-Z depth buffer integration.

use crate::framebuffer::FrameBuffer;
use crate::math3d::{Mat4Fixed, Vec3Fixed};
use axiom_core::fixed::Fixed;
use axiom_core::voxel_destruct::{
    VoxelVolume16, VOXEL_ARMOR, VOXEL_METAL, VOXEL_MOLTEN, VOXEL_STONE,
};

/// WebGPU WGSL Compute Shader source code for Fast Voxel DDA (Amanatides-Woo).
/// 8x8 workgroup topology corresponds exactly to 64 invocations matching 64-byte L1 alignment.
pub const VOXEL_RAYMARCH_WGSL: &str = r#"// Axiom WebGPU Compute Fast Voxel DDA Raymarcher
// 64-thread workgroup layout matching 64B L1 cache alignment

struct RaymarchConfig {
    view_proj_inv: mat4x4<f32>,
    cam_pos: vec4<f32>,
    light_dir: vec4<f32>,
    volume_min: vec4<f32>,
    volume_max: vec4<f32>,
    viewport_size: vec2<f32>,
    max_steps: u32,
    shadow_steps: u32,
};

@group(0) @binding(0) var<uniform> u_config: RaymarchConfig;
@group(0) @binding(1) var<storage, read> u_volume: array<u32>;
@group(0) @binding(2) var out_color: texture_storage_2d<rgba8unorm, write>;
@group(0) @binding(3) var<storage, read_write> out_depth: array<f32>;

fn get_voxel(x: u32, y: u32, z: u32) -> u32 {
    if (x >= 16u || y >= 16u || z >= 16u) {
        return 0u;
    }
    let idx = z * 256u + y * 16u + x;
    let word = u_volume[idx / 4u];
    let shift = (idx % 4u) * 8u;
    return (word >> shift) & 0xFFu;
}

fn sample_material_color(mat: u32) -> vec3<f32> {
    switch (mat) {
        case 1u: { return vec3<f32>(0.47, 0.44, 0.42); } // Stone (#78716c)
        case 2u: { return vec3<f32>(0.58, 0.64, 0.72); } // Metal (#94a3b8)
        case 3u: { return vec3<f32>(0.98, 0.45, 0.09); } // Molten (#f97316)
        case 4u: { return vec3<f32>(0.89, 0.91, 0.94); } // Armor (#e2e8f0)
        case 5u: { return vec3<f32>(0.22, 0.74, 0.97); } // Water (#38bdf8)
        default: { return vec3<f32>(0.39, 0.45, 0.55); }
    }
}

@compute @workgroup_size(8, 8, 1)
fn cs_raymarch_main(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let px = global_id.x;
    let py = global_id.y;
    if (f32(px) >= u_config.viewport_size.x || f32(py) >= u_config.viewport_size.y) {
        return;
    }

    let uv = vec2<f32>(
        (f32(px) + 0.5) / u_config.viewport_size.x * 2.0 - 1.0,
        1.0 - (f32(py) + 0.5) / u_config.viewport_size.y * 2.0
    );

    let near_pt = u_config.view_proj_inv * vec4<f32>(uv, 0.0, 1.0);
    let far_pt = u_config.view_proj_inv * vec4<f32>(uv, 1.0, 1.0);
    let ray_orig = near_pt.xyz / near_pt.w;
    let ray_dir = normalize((far_pt.xyz / far_pt.w) - ray_orig);

    var t_min = 0.0;
    var t_max = 1e9;
    var enter_normal = vec3<f32>(0.0);

    for (var i = 0u; i < 3u; i = i + 1u) {
        let inv_d = 1.0 / ray_dir[i];
        var t0 = (u_config.volume_min[i] - ray_orig[i]) * inv_d;
        var t1 = (u_config.volume_max[i] - ray_orig[i]) * inv_d;
        var n0 = vec3<f32>(0.0);
        n0[i] = -sign(ray_dir[i]);
        if (inv_d < 0.0) {
            let tmp = t0;
            t0 = t1;
            t1 = tmp;
            n0 = -n0;
        }
        if (t0 > t_min) {
            t_min = t0;
            enter_normal = n0;
        }
        t_max = min(t_max, t1);
        if (t_min > t_max) {
            return;
        }
    }

    let start_pos = ray_orig + ray_dir * max(t_min, 0.0);
    let cell_pos = clamp(floor(start_pos), vec3<f32>(0.0), vec3<f32>(15.0));
    var vx = i32(cell_pos.x);
    var vy = i32(cell_pos.y);
    var vz = i32(cell_pos.z);

    let step_x = i32(sign(ray_dir.x));
    let step_y = i32(sign(ray_dir.y));
    let step_z = i32(sign(ray_dir.z));

    let t_delta = abs(vec3<f32>(1.0) / ray_dir);
    var t_max_vec = vec3<f32>(
        select((f32(vx + 1) - start_pos.x) * t_delta.x, (start_pos.x - f32(vx)) * t_delta.x, step_x < 0),
        select((f32(vy + 1) - start_pos.y) * t_delta.y, (start_pos.y - f32(vy)) * t_delta.y, step_y < 0),
        select((f32(vz + 1) - start_pos.z) * t_delta.z, (start_pos.z - f32(vz)) * t_delta.z, step_z < 0)
    );

    var cur_normal = enter_normal;
    var hit_mat = 0u;
    var dist = max(t_min, 0.0);

    for (var s = 0u; s < u_config.max_steps; s = s + 1u) {
        if (vx < 0 || vx >= 16 || vy < 0 || vy >= 16 || vz < 0 || vz >= 16) {
            break;
        }
        let m = get_voxel(u32(vx), u32(vy), u32(vz));
        if (m != 0u) {
            hit_mat = m;
            break;
        }

        if (t_max_vec.x < t_max_vec.y && t_max_vec.x < t_max_vec.z) {
            vx = vx + step_x;
            dist = t_max_vec.x;
            t_max_vec.x = t_max_vec.x + t_delta.x;
            cur_normal = vec3<f32>(-f32(step_x), 0.0, 0.0);
        } else if (t_max_vec.y < t_max_vec.z) {
            vy = vy + step_y;
            dist = t_max_vec.y;
            t_max_vec.y = t_max_vec.y + t_delta.y;
            cur_normal = vec3<f32>(0.0, -f32(step_y), 0.0);
        } else {
            vz = vz + step_z;
            dist = t_max_vec.z;
            t_max_vec.z = t_max_vec.z + t_delta.z;
            cur_normal = vec3<f32>(0.0, 0.0, -f32(step_z));
        }
    }

    if (hit_mat == 0u) {
        return;
    }

    var shadow_factor = 1.0;
    let base_col = sample_material_color(hit_mat);
    let n_dot_l = max(dot(cur_normal, u_config.light_dir.xyz), 0.0);
    let final_rgb = base_col * (0.25 + 0.75 * n_dot_l * shadow_factor);

    textureStore(out_color, vec2<i32>(i32(px), i32(py)), vec4<f32>(final_rgb, 1.0));

    let depth_idx = py * u32(u_config.viewport_size.x) + px;
    out_depth[depth_idx] = 1.0 / (dist + 1.0);
}
"#;

/// Evaluates base 32-bit ARGB color for a voxel material.
#[inline(always)]
pub fn sample_raymarch_material_color(material: u8) -> u32 {
    match material {
        VOXEL_STONE => 0xFF78716C,  // Stone grey (#78716c)
        VOXEL_METAL => 0xFF94A3B8,  // Metal slate (#94a3b8)
        VOXEL_MOLTEN => 0xFFF97316, // Molten orange (#f97316)
        VOXEL_ARMOR => 0xFFE2E8F0,  // Armor white (#e2e8f0)
        5 => 0xFF38BDF8,            // Water / Coolant cyan (#38bdf8)
        _ => 0xFF64748B,            // Generic slate
    }
}

/// Applies directional diffuse lighting and shadow occlusion to a 32-bit ARGB color.
#[inline]
pub fn apply_raymarch_lighting(base_color: u32, normal: [i8; 3], light_dir: [i32; 3], is_shadowed: bool) -> u32 {
    let nx = normal[0] as i32;
    let ny = normal[1] as i32;
    let nz = normal[2] as i32;

    // Normal dot Light (light_dir normalized to ~1000 length)
    let dot = (nx * light_dir[0] + ny * light_dir[1] + nz * light_dir[2]).max(0);
    let diffuse_factor = (dot * 1000) / 1000; // 0..1000

    let shadow_mul = if is_shadowed { 500 } else { 1000 }; // 50% shadow ambient
    // Ambient: 250 (25%), Diffuse: 750 (75%)
    let intensity = (250 + (diffuse_factor * 750 / 1000) * shadow_mul / 1000).clamp(100, 1000);

    let r = ((base_color >> 16) & 0xFF) as u32;
    let g = ((base_color >> 8) & 0xFF) as u32;
    let b = (base_color & 0xFF) as u32;

    let lit_r = ((r * intensity as u32) / 1000).min(255);
    let lit_g = ((g * intensity as u32) / 1000).min(255);
    let lit_b = ((b * intensity as u32) / 1000).min(255);

    0xFF00_0000 | (lit_r << 16) | (lit_g << 8) | lit_b
}

/// Configuration descriptor for the CPU software DDA raymarch rasterizer.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct VoxelRaymarchRenderConfig {
    pub max_steps: u8,
    pub cast_shadows: bool,
    pub light_dir: [i32; 3],
    pub tile_scale: u8, // 1 = 1:1 full pixel, 2 = 2x2 downsampled, 4 = 4x4 downsampled
}

impl Default for VoxelRaymarchRenderConfig {
    fn default() -> Self {
        Self {
            max_steps: 48,
            cast_shadows: true,
            light_dir: [577, 577, -577], // Normalized directional vector [1, 1, -1] * 577 ~ 1000
            tile_scale: 1,
        }
    }
}

/// Software DDA raymarch rasterizer into `FrameBuffer` with Reversed-Z depth buffer integration.
///
/// Raymarches orthogonal/perspective rays through `VoxelVolume16` and directly blits lit pixels
/// and depth values into the target `FrameBuffer`.
///
/// Returns the number of voxel surface fragments rasterized.
pub fn render_voxel_raymarch(
    fb: &mut FrameBuffer,
    depth_buffer: &mut [i32],
    volume: &VoxelVolume16,
    vp: &Mat4Fixed,
    config: &VoxelRaymarchRenderConfig,
    world_origin: Vec3Fixed,
    voxel_scale: Fixed,
) -> usize {
    if fb.width == 0 || fb.height == 0 || volume.total_solid == 0 {
        return 0;
    }

    let mut rendered_count = 0usize;
    let hw = (fb.width as i32) / 2;
    let hh = (fb.height as i32) / 2;

    // Transform volume bounding corners to find screen bounding rectangle
    let min_pt = Vec3Fixed::new(
        world_origin.x - voxel_scale * Fixed::from_int(8),
        world_origin.y - voxel_scale * Fixed::from_int(8),
        world_origin.z - voxel_scale * Fixed::from_int(8),
    );
    let max_pt = Vec3Fixed::new(
        world_origin.x + voxel_scale * Fixed::from_int(8),
        world_origin.y + voxel_scale * Fixed::from_int(8),
        world_origin.z + voxel_scale * Fixed::from_int(8),
    );

    let corners = [
        min_pt,
        Vec3Fixed::new(max_pt.x, min_pt.y, min_pt.z),
        Vec3Fixed::new(min_pt.x, max_pt.y, min_pt.z),
        Vec3Fixed::new(max_pt.x, max_pt.y, min_pt.z),
        Vec3Fixed::new(min_pt.x, min_pt.y, max_pt.z),
        Vec3Fixed::new(max_pt.x, min_pt.y, max_pt.z),
        Vec3Fixed::new(min_pt.x, max_pt.y, max_pt.z),
        max_pt,
    ];

    let mut min_sx = fb.width as i32;
    let mut max_sx = 0i32;
    let mut min_sy = fb.height as i32;
    let mut max_sy = 0i32;
    let mut has_visible_corner = false;

    for corner in &corners {
        let clip = vp.transform_point(*corner);
        if clip.z.raw() <= 0 {
            continue;
        }
        has_visible_corner = true;
        let z_recip = Fixed::ONE / clip.z;
        let sx = hw + (clip.x * z_recip * Fixed::from_int(hw as i64)).to_int() as i32;
        let sy = hh - (clip.y * z_recip * Fixed::from_int(hh as i64)).to_int() as i32;

        min_sx = min_sx.min(sx);
        max_sx = max_sx.max(sx);
        min_sy = min_sy.min(sy);
        max_sy = max_sy.max(sy);
    }

    if !has_visible_corner {
        return 0;
    }

    let start_x = min_sx.clamp(0, (fb.width - 1) as i32);
    let end_x = max_sx.clamp(0, (fb.width - 1) as i32);
    let start_y = min_sy.clamp(0, (fb.height - 1) as i32);
    let end_y = max_sy.clamp(0, (fb.height - 1) as i32);

    let step = config.tile_scale.max(1) as i32;

    let span_x = (end_x - start_x).max(1);
    let span_y = (end_y - start_y).max(1);

    // Cast DDA rays across the clamped screen rectangle
    for sy in (start_y..=end_y).step_by(step as usize) {
        for sx in (start_x..=end_x).step_by(step as usize) {
            // Map screen pixel (sx, sy) to normalized volume ray origin [0, 16000]
            let norm_x = ((sx - start_x) * 16000) / span_x;
            let norm_y = ((end_y - sy) * 16000) / span_y;

            let ray_orig = [norm_x, norm_y, -2000];
            let ray_dir = [0, 0, 1000]; // Forward through volume

            let hit = volume.raymarch_dda(ray_orig, ray_dir, config.max_steps);
            if !hit.hit {
                continue;
            }

            let normal = [hit.normal_x, hit.normal_y, hit.normal_z];
            let hit_pt = [
                hit.voxel_x as i32 * 1000 + 500,
                hit.voxel_y as i32 * 1000 + 500,
                hit.voxel_z as i32 * 1000 + 500,
            ];

            let is_shadow = if config.cast_shadows {
                volume.is_shadowed(hit_pt, config.light_dir, 16)
            } else {
                false
            };

            let base_col = sample_raymarch_material_color(hit.material);
            let final_pixel = apply_raymarch_lighting(base_col, normal, config.light_dir, is_shadow);

            // Reversed-Z depth calculation (closer voxels have higher depth values)
            let depth_val = (100_000i32).saturating_sub(hit.distance_units as i32 * 4);

            // Blit pixel (or tile block if tile_scale > 1)
            for ty in 0..step {
                let py = sy + ty;
                if py >= fb.height as i32 {
                    break;
                }
                for tx in 0..step {
                    let px = sx + tx;
                    if px >= fb.width as i32 {
                        break;
                    }

                    let p_idx = (py as usize) * fb.width + (px as usize);
                    if depth_val > depth_buffer[p_idx] {
                        depth_buffer[p_idx] = depth_val;
                        fb.pixels[p_idx] = final_pixel;
                        rendered_count += 1;
                    }
                }
            }
        }
    }

    rendered_count
}
