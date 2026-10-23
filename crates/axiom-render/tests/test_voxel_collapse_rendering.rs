//! Unit and integration tests for 3D Dynamic Voxel Collapse & Structural Fragment Software Rasterizer.

use axiom_core::Fixed;
use axiom_core::voxel_collapse::DynamicCollapseSystem;
use axiom_core::voxel_destruct::VOXEL_STONE;
use axiom_render::framebuffer::FrameBuffer;
use axiom_render::math3d::Mat4Fixed;
use axiom_render::voxel_collapse_render::render_dynamic_collapse_chunks;

#[test]
fn test_dynamic_collapse_rendering_and_depth_culling() {
    let mut fb = FrameBuffer::new(128, 128);
    let mut depth_buffer = vec![i32::MIN; 128 * 128];

    let mut collapse = DynamicCollapseSystem::<4>::new(101);

    // Spawn chunk in front of camera
    collapse.spawn_chunk(
        [0, 0, 8000],
        [0, 0, 0],
        VOXEL_STONE,
        8,
        [2, 3, 2],
        100,
    );

    let view_mat = Mat4Fixed::IDENTITY;
    let cam_dist = Fixed::from_raw(65536);
    let focal = 65536;

    let rendered = render_dynamic_collapse_chunks(
        &mut fb,
        &mut depth_buffer,
        &collapse,
        &view_mat,
        cam_dist,
        focal,
    );

    assert_eq!(rendered, 1, "Front chunk must be rendered");

    // Center pixel should be drawn and have positive depth in Reversed-Z
    let center_idx = 64 * 128 + 64;
    assert!(depth_buffer[center_idx] > i32::MIN);
    assert_ne!(fb.pixels[center_idx], 0);

    // Now spawn another chunk right behind it (farther along Z, so vz is larger and inv_z is smaller)
    collapse.spawn_chunk(
        [0, 0, -25000],
        [0, 0, 0],
        VOXEL_STONE,
        8,
        [2, 3, 2],
        100,
    );

    let prev_center_depth = depth_buffer[center_idx];
    let prev_center_pixel = fb.pixels[center_idx];

    let _rendered_again = render_dynamic_collapse_chunks(
        &mut fb,
        &mut depth_buffer,
        &collapse,
        &view_mat,
        cam_dist,
        focal,
    );

    // The center pixel must NOT be overwritten by the occluded chunk behind it
    assert_eq!(depth_buffer[center_idx], prev_center_depth);
    assert_eq!(fb.pixels[center_idx], prev_center_pixel);
}
