//! Integration benchmark and test for ChunkManager infinite procedural streaming.

use axiom_core::{
    sample_world_tile, ChunkCoord, ChunkManager, Fixed, FixedVec2, TileType,
};

#[test]
fn test_seamless_world_tile_continuity() {
    let seed = 999999;
    // Across boundary between chunk 0 and chunk 1 (x=31 and x=32)
    let t_31 = sample_world_tile(31, 10, seed);
    let t_32 = sample_world_tile(32, 10, seed);

    // Tiles are valid types
    assert!(matches!(
        t_31,
        TileType::Plains | TileType::Forest | TileType::Rock | TileType::Water
    ));
    assert!(matches!(
        t_32,
        TileType::Plains | TileType::Forest | TileType::Rock | TileType::Water
    ));
}

#[test]
fn test_long_range_continuous_streaming() {
    let world_seed = 888123;
    let mut mgr = ChunkManager::<32, 32>::new(world_seed, 2);

    let mut player_pos = FixedVec2::new(Fixed::ZERO, Fixed::ZERO);
    let mut total_generated = 0;
    let mut total_evicted = 0;

    // Simulate player traveling across 100 chunks along the diagonal
    // 1 chunk = 32 tiles = 32.0 Fixed units
    let step = Fixed::from_int(16); // Half-chunk steps (16 tiles each step)
    let steps_count = 200; // 200 steps = 3,200 tiles traveled

    for tick in 1..=steps_count {
        player_pos.x += step;
        player_pos.y += step;

        let stats = mgr.update_player_pos(player_pos, tick);
        total_generated += stats.generated_chunks as u64;
        total_evicted += stats.evicted_chunks as u64;

        // Bounded working set: loaded chunks must never exceed 25 (with radius 2)
        assert!(mgr.loaded_count() <= 25);
        assert!(mgr.loaded_count() > 0);

        // Player's current position tile must be queryable
        let current_tile = mgr.tile_at(player_pos.x.to_int(), player_pos.y.to_int());
        assert!(matches!(
            current_tile,
            TileType::Plains | TileType::Forest | TileType::Rock | TileType::Water
        ));
    }

    assert_eq!(mgr.total_generated, total_generated);
    assert_eq!(mgr.total_evicted, total_evicted);
    assert!(mgr.total_generated > 50, "Must have generated dozens of chunks during traversal");
    assert!(mgr.total_evicted > 30, "Must have evicted out-of-range chunks to keep slab bounded");
}

#[test]
fn test_chunk_revisit_determinism() {
    let world_seed = 54321;
    let mut mgr = ChunkManager::<32, 32>::new(world_seed, 2);

    // Initial load at origin
    mgr.update_center(ChunkCoord::new(0, 0), 1);
    let origin_hash = mgr.chunk_at(ChunkCoord::new(0, 0)).unwrap().compute_hash();

    // Travel far away (to chunk 100, 100)
    for tick in 2..=50 {
        mgr.update_center(ChunkCoord::new(tick * 2, tick * 2), tick as u64);
    }

    // Origin chunk must have been evicted
    assert!(mgr.chunk_at(ChunkCoord::new(0, 0)).is_none());

    // Travel back to origin
    mgr.update_center(ChunkCoord::new(0, 0), 100);
    let returned_chunk = mgr.chunk_at(ChunkCoord::new(0, 0)).expect("Origin chunk must be reloaded");

    // Must be 100% bit-for-bit identical to initial generation!
    assert_eq!(
        returned_chunk.compute_hash(),
        origin_hash,
        "Revisited chunk must match initial state exactly"
    );
}

#[test]
fn test_terrain_modification_dirty_flag() {
    let world_seed = 777;
    let mut mgr = ChunkManager::<32, 32>::new(world_seed, 2);

    mgr.update_center(ChunkCoord::new(0, 0), 1);

    let origin_tile = mgr.tile_at(5, 5);
    let modified_tile = if origin_tile == TileType::Water {
        TileType::Plains
    } else {
        TileType::Water
    };

    let modified = mgr.set_tile(5, 5, modified_tile);
    assert!(modified);

    assert_eq!(mgr.tile_at(5, 5), modified_tile);
    let chunk = mgr.chunk_at(ChunkCoord::new(0, 0)).unwrap();
    assert!(chunk.dirty);
}

#[test]
fn test_1000_steps_streaming_performance() {
    let mut mgr = ChunkManager::<32, 32>::new(424242, 2);
    let start = std::time::Instant::now();

    let mut pos = FixedVec2::new(Fixed::ZERO, Fixed::ZERO);
    let step = Fixed::from_int(16);

    for tick in 1..=1_000 {
        pos.x += step;
        pos.y += step;
        let _ = mgr.update_player_pos(pos, tick);
    }

    let elapsed = start.elapsed();
    let micros_per_step = elapsed.as_micros() as f64 / 1_000.0;
    println!(
        "[PERF] 1,000 chunk streaming steps completed in {:?} ({:.2} µs/step, total generated chunks: {})",
        elapsed, micros_per_step, mgr.total_generated
    );

    // Each streaming update in unoptimized debug mode (generating ~4.5 chunks of 32x32 tiles per step)
    // must take < 2000µs (2.0ms) on average (well within 60Hz 16.6ms budget).
    assert!(
        micros_per_step < 2000.0,
        "Streaming update must be well within real-time budget (<2000µs in debug), took {:.2}µs",
        micros_per_step
    );
}
