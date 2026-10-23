//! Test suite for 3D Voxel Shard ballistic physics engine.

use axiom_core::voxel_destruct::{VoxelVolume16, VOXEL_ARMOR, VOXEL_METAL, VOXEL_MOLTEN, VOXEL_STONE};
use axiom_core::voxel_shard::{VoxelShard, VoxelShardPool};
use core::mem::{align_of, size_of};

#[test]
fn test_voxel_shard_l1_alignment_and_size() {
    assert_eq!(size_of::<VoxelShard>(), 64);
    assert_eq!(align_of::<VoxelShard>(), 64);
    assert_eq!(align_of::<VoxelShardPool<64>>(), 64);
}

#[test]
fn test_voxel_shard_spawning_and_life() {
    let mut pool = VoxelShardPool::<16>::new(42);
    assert_eq!(pool.active_count, 0);

    let spawned = pool.spawn_shard([0, 10000, 0], [100, 200, 300], VOXEL_STONE, 50);
    assert!(spawned);
    assert_eq!(pool.active_count, 1);
    assert_eq!(pool.total_spawned, 1);

    let shard = &pool.shards[0];
    assert!(shard.is_active);
    assert_eq!(shard.material, VOXEL_STONE);
    assert_eq!(shard.restitution_permille, 350);
    assert_eq!(shard.life, 50);
}

#[test]
fn test_voxel_shard_ballistic_flight_and_bounce() {
    let mut pool = VoxelShardPool::<16>::new(12345);
    pool.floor_y_q16 = 0;
    pool.gravity_q16 = 50;

    // Spawn shard 100 units above floor with negative Y velocity
    pool.spawn_shard([0, 100, 0], [10, -80, 0], VOXEL_METAL, 100);

    let mut bounced = false;
    for _ in 0..50 {
        let b = pool.step();
        if b > 0 {
            bounced = true;
            break;
        }
    }

    assert!(bounced, "Metal shard must impact floor and bounce upward");
    let shard = &pool.shards[0];
    assert!(shard.bounces >= 1);
    assert!(shard.pos[1] >= pool.floor_y_q16);
    assert!(shard.vel[1] > 0, "Velocity must reverse upward upon rebound");
}

#[test]
fn test_voxel_shard_burst_radial_dispersion() {
    let mut pool = VoxelShardPool::<32>::new(0xABCD);
    pool.spawn_burst([0, 0, 0], VOXEL_ARMOR, 12, 1000);

    assert_eq!(pool.active_count, 12);
    assert_eq!(pool.total_spawned, 12);

    let hashes = pool.compute_consensus_hash();
    assert_ne!(hashes, 0);

    for s in &pool.shards[..12] {
        assert!(s.is_active);
        assert_eq!(s.material, VOXEL_ARMOR);
        assert_eq!(s.scale_permille, 1200);
    }
}

#[test]
fn test_voxel_volume_carve_crater_with_shards() {
    let mut vol = VoxelVolume16::new_solid(VOXEL_STONE);
    let mut pool = VoxelShardPool::<64>::new(0x9876);

    let carved = vol.carve_crater_with_shards(8, 8, 3, 4, &mut pool, [0, 0, 0], 2048);
    assert!(carved > 0);
    assert!(pool.active_count > 0, "Carving crater must expel active voxel shards");
    assert!(pool.total_spawned > 0);

    let bounces = pool.step();
    assert_eq!(pool.active_count, pool.total_spawned as usize);
    let _ = bounces;
}

#[test]
fn test_voxel_shard_consensus_hash_determinism() {
    let mut p1 = VoxelShardPool::<32>::new(0xFEED_FACE);
    let mut p2 = VoxelShardPool::<32>::new(0xFEED_FACE);

    p1.spawn_burst([1000, 2000, 3000], VOXEL_MOLTEN, 8, 800);
    p2.spawn_burst([1000, 2000, 3000], VOXEL_MOLTEN, 8, 800);

    assert_eq!(p1.compute_consensus_hash(), p2.compute_consensus_hash());

    for _ in 0..20 {
        p1.step();
        p2.step();
    }

    assert_eq!(p1.compute_consensus_hash(), p2.compute_consensus_hash());
}
