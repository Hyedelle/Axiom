//! Unit and integration test suite for 3D Voxel Connectivity Analysis & Dynamic Mesh Collapse Simulation.

use axiom_core::voxel_collapse::{DynamicCollapseSystem, DynamicVoxelChunk};
use axiom_core::voxel_destruct::{VoxelVolume16, VOXEL_AIR, VOXEL_STONE};
use axiom_core::voxel_shard::VoxelShardPool;
use core::mem::{align_of, size_of};

#[test]
fn test_dynamic_voxel_chunk_l1_alignment_and_size() {
    assert_eq!(size_of::<DynamicVoxelChunk>(), 64);
    assert_eq!(align_of::<DynamicVoxelChunk>(), 64);
}

#[test]
fn test_dynamic_collapse_detect_unsupported_pillar() {
    let mut volume = VoxelVolume16::new_empty();

    // 1. Bedrock base at y = 0
    for z in 0..16 {
        for x in 0..16 {
            volume.set(x, 0, z, VOXEL_STONE);
        }
    }

    // 2. Vertical pillar from y = 1 to 10 at (8, z, 8)
    for y in 1..=10 {
        volume.set(8, y, 8, VOXEL_STONE);
    }
    let initial_solid = volume.total_solid;

    let mut collapse = DynamicCollapseSystem::<8>::new(12345);

    // Initial check: pillar is firmly rooted to bedrock (anchor_y = 0), so 0 chunks collapse
    let collapsed = collapse.detect_and_collapse(&mut volume, 0, [0, 0, 0], 2048);
    assert_eq!(collapsed, 0);
    assert_eq!(collapse.active_count, 0);
    assert_eq!(volume.total_solid, initial_solid);

    // 3. Cut / destroy pillar base at y = 1 and y = 2 (trench/carve)
    volume.set(8, 1, 8, VOXEL_AIR);
    volume.set(8, 2, 8, VOXEL_AIR);
    volume.total_solid -= 2;

    // 4. Now voxels from y = 3..10 are completely floating in mid-air
    let collapsed = collapse.detect_and_collapse(&mut volume, 0, [0, 0, 0], 2048);
    assert_eq!(collapsed, 1, "Floating pillar segment must be detected and collapsed");
    assert_eq!(collapse.active_count, 1);

    let chunk = collapse.chunks[0];
    assert!(chunk.is_active);
    assert_eq!(chunk.voxel_count, 8, "Pillar voxels y=3..10 equals 8 voxels");
    assert_eq!(chunk.material, VOXEL_STONE);

    // Verify all floating voxels were cleared from static volume
    for y in 3..=10 {
        assert_eq!(volume.get(8, y, 8), VOXEL_AIR);
    }
}

#[test]
fn test_dynamic_collapse_ballistic_flight_and_bounce() {
    let mut collapse = DynamicCollapseSystem::<4>::new(42);

    // Spawn chunk high above floor
    let spawned = collapse.spawn_chunk(
        [0, 50000, 0],
        [100, -2000, 0],
        VOXEL_STONE,
        5,
        [2, 2, 2],
        100,
    );
    assert!(spawned);
    assert_eq!(collapse.active_count, 1);

    let initial_y = collapse.chunks[0].pos[1];
    let (bounces, shatters) = collapse.step::<64>(None);
    assert_eq!(bounces, 0);
    assert_eq!(shatters, 0);
    assert!(collapse.chunks[0].pos[1] < initial_y, "Chunk must fall downward");
}

#[test]
fn test_dynamic_collapse_high_velocity_shatter_into_shards() {
    let mut collapse = DynamicCollapseSystem::<4>::new(999);
    let mut shard_pool = VoxelShardPool::<64>::new(777);

    // Spawn chunk right above ground with high downward velocity
    let spawned = collapse.spawn_chunk(
        [0, -40000, 0],
        [0, -25000, 0], // Exceeds shatter threshold (16000)
        VOXEL_STONE,
        12,
        [3, 3, 3],
        100,
    );
    assert!(spawned);

    // Step physics with shard pool
    let (_bounces, shatters) = collapse.step(Some(&mut shard_pool));
    assert_eq!(shatters, 1, "High velocity impact must shatter chunk");
    assert_eq!(collapse.active_count, 0, "Shattered chunk deactivates");
    assert!(collapse.total_shattered >= 1);
    assert!(shard_pool.active_count > 0, "Shatter must populate shard pool with fine fragments");
}

#[test]
fn test_dynamic_collapse_consensus_hash_determinism() {
    let mut sys_a = DynamicCollapseSystem::<8>::new(0xABCD);
    let mut sys_b = DynamicCollapseSystem::<8>::new(0xABCD);

    sys_a.spawn_chunk([1000, 2000, 3000], [50, -500, 10], VOXEL_STONE, 6, [2, 2, 2], 80);
    sys_b.spawn_chunk([1000, 2000, 3000], [50, -500, 10], VOXEL_STONE, 6, [2, 2, 2], 80);

    for _ in 0..50 {
        sys_a.step::<32>(None);
        sys_b.step::<32>(None);
    }

    assert_eq!(sys_a.compute_consensus_hash(), sys_b.compute_consensus_hash());
    assert_eq!(sys_a.chunks[0].pos, sys_b.chunks[0].pos);
    assert_eq!(sys_a.chunks[0].rot_bams, sys_b.chunks[0].rot_bams);
}
