//! Tests for 3D Voxel Fluid & Infiltration Rasterizer.

use axiom_core::Fixed;
use axiom_core::voxel_destruct::{VoxelVolume16, VOXEL_STONE};
use axiom_core::voxel_fluid::VoxelFluidField16;
use axiom_render::fluid_render::{render_voxel_fluid, sample_fluid_color, sample_infiltrated_color};
use axiom_render::framebuffer::FrameBuffer;
use axiom_render::math3d::{Mat4Fixed, Vec3Fixed};

#[test]
fn test_sample_fluid_colors() {
    let (col_deep, alpha_deep) = sample_fluid_color(200);
    assert_eq!(col_deep, 0x001D_4ED8);
    assert_eq!(alpha_deep, 210);

    let (col_med, alpha_med) = sample_fluid_color(100);
    assert_eq!(col_med, 0x0025_63EB);
    assert_eq!(alpha_med, 175);

    let (col_shallow, alpha_shallow) = sample_fluid_color(30);
    assert_eq!(col_shallow, 0x0038_BDF8);
    assert_eq!(alpha_shallow, 140);
}

#[test]
fn test_sample_infiltrated_colors() {
    let (col_inf, alpha_inf) = sample_infiltrated_color(128);
    assert_eq!(col_inf, 0x000F_172A);
    assert!(alpha_inf >= 40 && alpha_inf <= 140);
}

#[test]
fn test_render_voxel_fluid_into_framebuffer() {
    let mut fb = FrameBuffer::new(320, 240);
    let mut depth_buffer = vec![i32::MIN; 320 * 240];
    let mut fluid = VoxelFluidField16::new();
    let mut volume = VoxelVolume16::new_empty();

    // Floor
    for x in 0..16 {
        for z in 0..16 {
            volume.set(x, 0, z, VOXEL_STONE);
        }
    }

    // Inject fluid at (8, 1, 8) and (8, 2, 8)
    fluid.inject_fluid(&mut volume, 8, 1, 8, 200);
    fluid.inject_fluid(&mut volume, 8, 2, 8, 150);

    // Matrix looking at center (8, 8, 8) with +Z translation
    let vp = Mat4Fixed::from_translation(Vec3Fixed::new(Fixed::ZERO, Fixed::ZERO, Fixed::from_int(15)));
    let voxel_scale = Fixed::from_int(1);
    let world_origin = Vec3Fixed::ZERO;

    let rendered = render_voxel_fluid(
        &mut fb,
        &mut depth_buffer,
        &vp,
        &fluid,
        voxel_scale,
        world_origin,
    );

    assert_eq!(rendered, 2, "Must render exactly 2 fluid voxels");
}

#[test]
fn test_render_fluid_under_heavy_saturation() {
    use axiom_core::voxel_destruct::VOXEL_DIM;

    let mut fb = FrameBuffer::new(704, 544);
    let mut depth_buffer = vec![i32::MIN; 704 * 544];
    let mut fluid = VoxelFluidField16::new();
    let mut volume = VoxelVolume16::new_empty();

    // Flood entire 4096-voxel volume with fluid and infiltration
    for y in 0..VOXEL_DIM {
        for z in 0..VOXEL_DIM {
            for x in 0..VOXEL_DIM {
                fluid.inject_fluid(&mut volume, x, y, z, 200);
                fluid.infiltrated[VoxelFluidField16::index(x, y, z)] = 150;
            }
        }
    }

    let vp = Mat4Fixed::from_translation(Vec3Fixed::new(Fixed::ZERO, Fixed::ZERO, Fixed::from_int(20)));
    let voxel_scale = Fixed::from_int(1);
    let world_origin = Vec3Fixed::ZERO;

    let start = std::time::Instant::now();
    let rendered = render_voxel_fluid(
        &mut fb,
        &mut depth_buffer,
        &vp,
        &fluid,
        voxel_scale,
        world_origin,
    );
    let elapsed = start.elapsed();

    assert_eq!(rendered, 4096, "All 4096 saturated fluid voxels must be rendered");
    assert!(
        elapsed.as_millis() < 50,
        "4096-voxel fluid rasterization must complete in under 50ms (took {:?})",
        elapsed
    );
}
