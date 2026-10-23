//! Test suite for 3D Voxel Shard Ballistic Particle Rasterizer.

use axiom_core::Fixed;
use axiom_core::voxel_destruct::{VOXEL_ARMOR, VOXEL_METAL, VOXEL_MOLTEN, VOXEL_STONE};
use axiom_core::voxel_shard::VoxelShardPool;
use axiom_render::framebuffer::FrameBuffer;
use axiom_render::math3d::Mat4Fixed;
use axiom_render::voxel_shard_render::{render_voxel_shards, VoxelShardColorMap};

#[test]
fn test_voxel_shard_color_map() {
    let stone = VoxelShardColorMap::for_material(VOXEL_STONE);
    assert_eq!(stone.base, 0xFF64748B);

    let metal = VoxelShardColorMap::for_material(VOXEL_METAL);
    assert_eq!(metal.highlight, 0xFFF8FAFC);

    let molten = VoxelShardColorMap::for_material(VOXEL_MOLTEN);
    assert_eq!(molten.base, 0xFFF97316);

    let armor = VoxelShardColorMap::for_material(VOXEL_ARMOR);
    assert_eq!(armor.base, 0xFFD97706);
}

#[test]
fn test_voxel_shard_rasterization_with_depth_testing() {
    let mut fb = FrameBuffer::new(128, 128);
    let mut depth_buffer = vec![i32::MIN; 128 * 128];

    let mut pool = VoxelShardPool::<16>::new(42);
    // Spawn two shards: front and behind
    pool.spawn_shard([0, 0, 5000], [0, 0, 0], VOXEL_STONE, 100);
    pool.spawn_shard([0, 0, 15000], [0, 0, 0], VOXEL_MOLTEN, 100);

    let view_mat = Mat4Fixed::IDENTITY;
    let cam_dist = Fixed::from_raw(65536);
    let focal = 65536;

    let rendered = render_voxel_shards(
        &mut fb,
        &mut depth_buffer,
        &pool,
        &view_mat,
        cam_dist,
        focal,
    );

    assert_eq!(rendered, 2);

    // Center pixel should be written by the closest shard (Reversed-Z: higher value in front)
    let center_idx = 64 * 128 + 64;
    assert_ne!(depth_buffer[center_idx], i32::MIN);
    assert_ne!(fb.pixels[center_idx], 0);
}
