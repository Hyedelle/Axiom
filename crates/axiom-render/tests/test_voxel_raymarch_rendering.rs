use axiom_core::fixed::Fixed;
use axiom_core::voxel_destruct::{
    VoxelVolume16, VOXEL_ARMOR, VOXEL_METAL, VOXEL_MOLTEN, VOXEL_STONE,
};
use axiom_render::framebuffer::FrameBuffer;
use axiom_render::gpu_backend::VoxelComputeDispatch64;
use axiom_render::math3d::{Mat4Fixed, Vec3Fixed};
use axiom_render::voxel_raymarch_render::{
    apply_raymarch_lighting, render_voxel_raymarch, sample_raymarch_material_color,
    VoxelRaymarchRenderConfig, VOXEL_RAYMARCH_WGSL,
};

#[test]
fn test_wgsl_compute_shader_integrity() {
    assert!(!VOXEL_RAYMARCH_WGSL.is_empty());
    assert!(VOXEL_RAYMARCH_WGSL.contains("@compute @workgroup_size(8, 8, 1)"));
    assert!(VOXEL_RAYMARCH_WGSL.contains("struct RaymarchConfig"));
    assert!(VOXEL_RAYMARCH_WGSL.contains("fn cs_raymarch_main"));
    assert!(VOXEL_RAYMARCH_WGSL.contains("textureStore(out_color"));
    assert!(VOXEL_RAYMARCH_WGSL.contains("out_depth[depth_idx] = 1.0 / (dist + 1.0);"));

    // Verify getter on VoxelComputeDispatch64
    assert_eq!(VoxelComputeDispatch64::wgsl_shader_source(), VOXEL_RAYMARCH_WGSL);
}

#[test]
fn test_material_color_and_lighting() {
    let stone = sample_raymarch_material_color(VOXEL_STONE);
    let metal = sample_raymarch_material_color(VOXEL_METAL);
    let molten = sample_raymarch_material_color(VOXEL_MOLTEN);
    let armor = sample_raymarch_material_color(VOXEL_ARMOR);
    let water = sample_raymarch_material_color(5);

    assert_eq!(stone, 0xFF78716C);
    assert_eq!(metal, 0xFF94A3B8);
    assert_eq!(molten, 0xFFF97316);
    assert_eq!(armor, 0xFFE2E8F0);
    assert_eq!(water, 0xFF38BDF8);

    let light_dir = [0, 0, 1000]; // Direct light along +Z
    let facing_normal = [0, 0, 1]; // Facing light
    let lit = apply_raymarch_lighting(stone, facing_normal, light_dir, false);
    let shadowed = apply_raymarch_lighting(stone, facing_normal, light_dir, true);

    // Unshadowed must be strictly brighter than shadowed
    assert!((lit & 0xFF) >= (shadowed & 0xFF));
    assert!(((lit >> 8) & 0xFF) >= ((shadowed >> 8) & 0xFF));
    assert!(((lit >> 16) & 0xFF) >= ((shadowed >> 16) & 0xFF));
}

#[test]
fn test_render_voxel_raymarch_software_rasterizer() {
    let mut fb = FrameBuffer::new(64, 64);
    fb.clear(0xFF0F172A); // Dark slate clear color
    let mut depth_buffer = vec![0i32; 64 * 64];

    let mut vol = VoxelVolume16::new_solid(VOXEL_METAL);
    vol.carve_crater(8, 8, 4, 5);

    let vp = Mat4Fixed::from_translation(Vec3Fixed::from_raw(0, 0, 10000));
    let config = VoxelRaymarchRenderConfig::default();
    let world_origin = Vec3Fixed::new(Fixed::ZERO, Fixed::ZERO, Fixed::from_int(10000));
    let voxel_scale = Fixed::from_int(500);

    let rendered_fragments = render_voxel_raymarch(
        &mut fb,
        &mut depth_buffer,
        &vol,
        &vp,
        &config,
        world_origin,
        voxel_scale,
    );

    assert!(rendered_fragments > 0, "Must render solid voxel fragments");

    // Check that some pixels were overwritten and depth buffer values were updated
    let mut non_clear_pixels = 0;
    let mut non_zero_depths = 0;

    for i in 0..(64 * 64) {
        if fb.pixels[i] != 0xFF0F172A {
            non_clear_pixels += 1;
        }
        if depth_buffer[i] > 0 {
            non_zero_depths += 1;
        }
    }

    assert!(non_clear_pixels > 0, "FrameBuffer must contain rendered pixels");
    assert_eq!(non_clear_pixels, non_zero_depths, "Pixels and depth buffer entries must match");
}

#[test]
fn test_render_voxel_raymarch_downsampled_tiles() {
    let mut fb = FrameBuffer::new(64, 64);
    fb.clear(0xFF000000);
    let mut depth_buffer = vec![0i32; 64 * 64];

    let vol = VoxelVolume16::new_solid(VOXEL_STONE);
    let vp = Mat4Fixed::from_translation(Vec3Fixed::from_raw(0, 0, 10000));
    let mut config = VoxelRaymarchRenderConfig::default();
    config.tile_scale = 2; // 2x2 downsampled fast raymarch
    let world_origin = Vec3Fixed::new(Fixed::ZERO, Fixed::ZERO, Fixed::from_int(10000));
    let voxel_scale = Fixed::from_int(500);

    let rendered = render_voxel_raymarch(
        &mut fb,
        &mut depth_buffer,
        &vol,
        &vp,
        &config,
        world_origin,
        voxel_scale,
    );

    assert!(rendered > 0);
}
