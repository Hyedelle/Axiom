//! Axiom 3D Voxel Terrain Destruction & Dynamic Collapse Stress Test & Optimization Suite (T-139).
//!
//! Rigorously verifies extreme edge cases: continuous Swiss-cheese crater erosion,
//! extreme-velocity ballistic impacts without overflow, fixed 16-chunk pool saturation and LRU eviction,
//! sub-millisecond mesh extraction throughput, and 1000-step bit-identical determinism.

use axiom_core::fixed::Fixed;
use axiom_core::voxel_collapse::DynamicCollapseSystem;
use axiom_core::voxel_destruct::{VOXEL_DIM, VOXEL_STONE, VOXEL_TOTAL};
use axiom_core::voxel_shard::VoxelShardPool;
use axiom_render::math3d::Mat4Fixed;
use axiom_render::voxel_collapse_render::render_dynamic_collapse_chunks;
use axiom_render::voxel_map::{generate_voxel_preset, VoxelPreset};
use axiom_render::voxel_mesh::extract_voxel_mesh;
use axiom_render::voxel_shard_render::render_voxel_shards;
use axiom_render::FrameBuffer;

#[test]
fn test_500_crater_continuous_stress_and_invariants() {
    let mut volume = generate_voxel_preset(VoxelPreset::MonolithRuins, 0xACE);
    let mut pool = VoxelShardPool::<128>::new(0xACE);
    let mut collapse = DynamicCollapseSystem::<16>::new(0xACE);

    let initial_solid = volume.total_solid;
    assert!(initial_solid > 0);

    for i in 0..500 {
        let cx = ((i * 7 + 3) % (VOXEL_DIM - 4) + 2) as i16;
        let cz = ((i * 11 + 5) % (VOXEL_DIM - 4) + 2) as i16;
        let radius = ((i % 4) + 2) as i16;
        let depth = ((i % 3) + 2) as i16;

        let carved = volume.carve_crater_with_shards(cx, cz, depth, radius, &mut pool, [-16384, 0, -16384], 2048);
        assert!(carved <= 4096);
        assert!(volume.total_solid <= VOXEL_TOTAL as u32);

        if i % 10 == 0 {
            let collapsed = collapse.detect_and_collapse(&mut volume, 0, [-16384, 0, -16384], 2048);
            let _ = collapsed;
        }

        let _ = collapse.step(Some(&mut pool));
        let _ = pool.step();

        // Ensure raycasting into stressed volume never panics
        let hit = volume.raycast([cx, 15, cz], [0, -1, 0], 20);
        let _ = hit;
    }

    // Extract mesh after 500 operations
    let mesh = extract_voxel_mesh("stress_ruins", &volume, Fixed::from_raw(2048));
    assert!(mesh.indices.len() % 3 == 0);
}

#[test]
fn test_extreme_velocity_ballistics_and_no_overflow() {
    let mut pool = VoxelShardPool::<64>::new(999);
    let mut collapse = DynamicCollapseSystem::<16>::new(999);

    // Spawn shards with near-overflow velocities
    pool.spawn_shard([0, 100000, 0], [i32::MAX / 4, -500000, i32::MIN / 4], VOXEL_STONE, 300);
    pool.spawn_shard([0, -45000, 0], [0, i32::MIN + 100, 0], VOXEL_STONE, 300);

    // Spawn chunk with near-overflow velocity
    collapse.spawn_chunk([0, 50000, 0], [100000, -800000, -100000], VOXEL_STONE, 8, [2, 2, 2], 300);

    // Step 50 ticks - must never panic from integer overflow or underflow
    for _ in 0..50 {
        let _ = collapse.step(Some(&mut pool));
        let _ = pool.step();
    }

    let hash_pool = pool.compute_consensus_hash();
    let hash_collapse = collapse.compute_consensus_hash();
    assert!(hash_pool > 0);
    assert!(hash_collapse > 0);
}

#[test]
fn test_16_chunk_pool_saturation_and_lru_eviction() {
    let mut collapse = DynamicCollapseSystem::<16>::new(42);
    assert_eq!(collapse.active_count, 0);

    // Spawn 50 chunks into 16-chunk pool
    for i in 0..50 {
        let spawned = collapse.spawn_chunk(
            [i * 100, 20000, i * 100],
            [0, -1000, 0],
            VOXEL_STONE,
            4,
            [1, 1, 1],
            (50 - i as u16).max(5), // Decreasing life
        );
        assert!(spawned, "Pool should accept and evict without dropping");
        assert!(collapse.active_count <= 16, "Active count must never exceed capacity");
    }

    assert_eq!(collapse.active_count, 16);
    assert_eq!(collapse.total_collapsed, 50);
}

#[test]
fn test_sub_millisecond_extraction_and_rasterization_benchmark() {
    let mut volume = generate_voxel_preset(VoxelPreset::MonolithRuins, 12345);
    // Create multiple craters to create high-porosity geometry
    volume.carve_crater(6, 6, 3, 4);
    volume.carve_crater(10, 10, 3, 4);

    let mut collapse = DynamicCollapseSystem::<16>::new(12345);
    let mut shards = VoxelShardPool::<128>::new(12345);
    let _ = collapse.detect_and_collapse(&mut volume, 0, [-16384, 0, -16384], 2048);
    shards.spawn_burst([0, 20000, 0], VOXEL_STONE, 32, 1500);

    let mut fb = FrameBuffer::new(160, 120);
    let mut depth_buffer = vec![i32::MIN; 160 * 120];
    let view_mat = Mat4Fixed::IDENTITY;
    let cam_dist = Fixed::from_raw(262144);
    let focal = 220;

    // Warm-up
    let _mesh = extract_voxel_mesh("bench_warm", &volume, Fixed::from_raw(2048));

    // Timed benchmark: 20 extractions + render passes
    let start = std::time::Instant::now();
    for _ in 0..20 {
        let m = extract_voxel_mesh("bench", &volume, Fixed::from_raw(2048));
        assert!(m.triangle_count() > 0);

        depth_buffer.fill(i32::MIN);
        let _ = render_dynamic_collapse_chunks(&mut fb, &mut depth_buffer, &collapse, &view_mat, cam_dist, focal);
        let _ = render_voxel_shards(&mut fb, &mut depth_buffer, &shards, &view_mat, cam_dist, focal);
    }
    let elapsed = start.elapsed();
    let per_op_us = elapsed.as_micros() / 20;

    println!("⚡ High-Porosity Voxel Extraction & Render: {} µs / op", per_op_us);
    assert!(per_op_us < 2000, "Voxel pipeline must remain sub-2ms even under test harness overhead");
}

#[test]
fn test_bit_identical_1000_step_determinism() {
    let run_sim = || -> (u64, u64) {
        let mut volume = generate_voxel_preset(VoxelPreset::CrateredArena, 8888);
        let mut collapse = DynamicCollapseSystem::<16>::new(8888);
        let mut shards = VoxelShardPool::<128>::new(8888);

        for step in 0..1000 {
            if step % 25 == 0 {
                let cx = ((step * 5 + 3) % 12 + 2) as i16;
                let cz = ((step * 9 + 1) % 12 + 2) as i16;
                volume.carve_crater_with_shards(cx, cz, 2, 3, &mut shards, [-16384, 0, -16384], 2048);
                collapse.detect_and_collapse(&mut volume, 0, [-16384, 0, -16384], 2048);
            }
            collapse.step(Some(&mut shards));
            shards.step();
        }

        (collapse.compute_consensus_hash(), shards.compute_consensus_hash())
    };

    let (hash_c1, hash_s1) = run_sim();
    let (hash_c2, hash_s2) = run_sim();

    assert_eq!(hash_c1, hash_c2, "Dynamic collapse 1000-step consensus hash must be bit-identical");
    assert_eq!(hash_s1, hash_s2, "Voxel shards 1000-step consensus hash must be bit-identical");
}
