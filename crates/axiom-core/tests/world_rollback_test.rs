//! Open-World Time-Travel Rollback & Chunk Integration Test (T-015)
//!
//! Validates:
//! - Full integration between ChunkManager, NpcTable, and WorldTimeMachine
//! - Terrain modification and subsequent rollback
//! - NPC circadian schedule rollback
//! - 100% bit-for-bit state hash match after rolling back 50 ticks

use axiom_core::{
    Fixed, FixedVec2, TileType, WorldSnapshot, WorldTimeMachine,
};

#[test]
fn test_open_world_full_rollback_integration() {
    let world_seed = 133788;
    let mut world = WorldSnapshot::<32, 32, 1024>::new(world_seed, 2);
    let mut time_machine = WorldTimeMachine::<32, 32, 1024>::new(64);

    // Spawn 50 NPCs with diverse schedules
    for i in 0..50 {
        let id = (i + 1) as u32;
        let home = (i as i32 * 2, i as i32);
        let work = (i as i32 * 2 + 10, i as i32 + 20);
        let shift = (i % 3) as u8;
        world.npcs.spawn(id, home, work, shift, 256);
    }

    // Step 1: Simulate ticks 1..=30 and record history
    let step_vec = FixedVec2::new(Fixed::from_int(1), Fixed::ZERO);
    let mut hash_at_tick_30 = 0;

    for tick in 1..=30 {
        world.step(step_vec);

        // At tick 15, player modifies terrain (e.g. mines tile at (5, 5))
        if tick == 15 {
            world.set_tile(5, 5, TileType::Plains);
        }

        time_machine.record(world.clone());

        if tick == 30 {
            hash_at_tick_30 = world.compute_hash();
        }
    }

    assert_eq!(world.tick, 30);
    assert_eq!(world.tile_at(5, 5), TileType::Plains);
    let player_pos_at_30 = world.player_pos;

    // Step 2: Simulate ticks 31..=60 (advance into the future)
    for tick in 31..=60 {
        world.step(step_vec);

        // At tick 45, player alters another tile at (20, 20)
        if tick == 45 {
            world.set_tile(20, 20, TileType::Water);
        }

        time_machine.record(world.clone());
    }

    assert_eq!(world.tick, 60);
    assert_eq!(world.tile_at(20, 20), TileType::Water);
    assert_ne!(world.compute_hash(), hash_at_tick_30);

    // Step 3: Instantaneous Rollback to tick 30!
    let rolled_back_world = time_machine
        .recall(30)
        .expect("Snapshot at tick 30 must exist")
        .clone();

    // Verification 1: Clock & Player Position
    assert_eq!(rolled_back_world.tick, 30);
    assert_eq!(rolled_back_world.player_pos, player_pos_at_30);

    // Verification 2: Terrain State
    // Tile (5, 5) modified at tick 15 MUST remain Plains
    assert_eq!(rolled_back_world.tile_at(5, 5), TileType::Plains);
    // Tile (20, 20) modified in the future at tick 45 MUST NOT be Water!
    assert_ne!(
        rolled_back_world.tile_at(20, 20),
        TileType::Water,
        "Future terrain modification must be undone by rollback"
    );

    // Verification 3: 100% Bit-for-bit Deterministic Hash Match
    assert_eq!(
        rolled_back_world.compute_hash(),
        hash_at_tick_30,
        "Rolled back state must bit-for-bit match recorded tick 30 hash"
    );

    // Step 4: Alternate Timeline Branching from tick 30
    let mut alternate_world = rolled_back_world;
    let alternate_step = FixedVec2::new(Fixed::ZERO, Fixed::from_int(1)); // Move North instead of East

    for _ in 31..=60 {
        alternate_world.step(alternate_step);
    }

    assert_eq!(alternate_world.tick, 60);
    // Player is in an entirely different location in the alternate timeline
    assert_ne!(alternate_world.player_pos, world.player_pos);
}
