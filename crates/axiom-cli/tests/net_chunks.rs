//! Multiplayer Procedural Chunk Dynamic Hosting & Delta Sync Tests (T-034).
//!
//! Validates distributed chunk streaming, peer-to-peer terrain modification deltas,
//! and deterministic conflict resolution across moving players.

use axiom_core::chunk::{ChunkCoord, ChunkManager};
use axiom_core::fixed::FixedVec2;
use axiom_core::pcg::TileType;

const WORLD_SEED: u64 = 0xDEAD_BEEF_CAFE_0001;

/// Terrain modification delta packet transmitted across network peers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerrainDelta {
    pub sender_id: u32,
    pub tick: u64,
    pub tile_x: i64,
    pub tile_y: i64,
    pub new_tile: TileType,
}

impl TerrainDelta {
    /// Resolves conflict between two simultaneous modifications on the same tile.
    /// Higher tick wins; on tie, lowest sender_id wins.
    pub fn should_overwrite(&self, other: &Self) -> bool {
        if self.tick != other.tick {
            self.tick > other.tick
        } else {
            self.sender_id < other.sender_id
        }
    }
}

#[test]
fn test_multiplayer_distributed_chunk_streaming_and_delta_sync() {
    // Player 0 starts at (0, 0), Player 1 starts at distant (64, 64)
    let mut chunk_mgr_0 = ChunkManager::<32, 32>::new(WORLD_SEED, 1);
    let mut chunk_mgr_1 = ChunkManager::<32, 32>::new(WORLD_SEED, 1);

    let pos_0 = FixedVec2::from_ints(0, 0);
    let mut pos_1 = FixedVec2::from_ints(64, 64);

    chunk_mgr_0.update_player_pos(pos_0, 0);
    chunk_mgr_1.update_player_pos(pos_1, 0);

    // Initial state: Player 0 and Player 1 have loaded distinct chunk sets
    assert!(chunk_mgr_0.find_chunk_index(ChunkCoord::new(0, 0)).is_some());
    assert!(chunk_mgr_1.find_chunk_index(ChunkCoord::new(0, 0)).is_none());

    // 1. Player 0 executes 5 terrain modifications in Chunk (0, 0)
    let mut deltas: Vec<TerrainDelta> = Vec::new();
    for i in 0..5 {
        let tx = 5 + i * 2;
        let ty = 10;
        let new_tile = TileType::Plains; // Mine rock / pave road
        chunk_mgr_0.set_tile(tx, ty, new_tile);
        deltas.push(TerrainDelta {
            sender_id: 0,
            tick: 10 + i as u64,
            tile_x: tx,
            tile_y: ty,
            new_tile,
        });
    }

    // 2. Simulated network transport (latency = 5 ticks)
    let mut network_pipe: Vec<(u64, TerrainDelta)> = Vec::new();
    for delta in deltas {
        network_pipe.push((delta.tick + 5, delta));
    }

    // 3. Player 1 walks from (64, 64) toward (0, 0) over 32 ticks
    for tick in 1..=32 {
        pos_1.x -= axiom_core::fixed::Fixed::from_int(2);
        pos_1.y -= axiom_core::fixed::Fixed::from_int(2);
        chunk_mgr_1.update_player_pos(pos_1, tick);

        // Deliver arrived delta packets to Player 1
        let arrived: Vec<TerrainDelta> = network_pipe
            .iter()
            .filter(|(arrive_tick, _)| *arrive_tick <= tick)
            .map(|(_, d)| *d)
            .collect();
        network_pipe.retain(|(arrive_tick, _)| *arrive_tick > tick);

        for delta in arrived {
            chunk_mgr_1.set_tile(delta.tile_x, delta.tile_y, delta.new_tile);
        }
    }

    // 4. Player 1 has now arrived at (0, 0)
    assert_eq!(pos_1.x.to_int(), 0);
    assert_eq!(pos_1.y.to_int(), 0);
    assert!(chunk_mgr_1.find_chunk_index(ChunkCoord::new(0, 0)).is_some());

    // 5. Verify 100% Bit-for-Bit Chunk State Consensus between Peer 0 and Peer 1
    let chunk_0 = chunk_mgr_0.chunk_at(ChunkCoord::new(0, 0)).unwrap();
    let chunk_1 = chunk_mgr_1.chunk_at(ChunkCoord::new(0, 0)).unwrap();

    assert_eq!(
        chunk_0.compute_hash(),
        chunk_1.compute_hash(),
        "Chunk (0, 0) MUST have identical bit hashes on both peers after delta sync!"
    );

    for i in 0..5 {
        let tx = 5 + i * 2;
        let ty = 10;
        assert_eq!(chunk_mgr_0.tile_at(tx, ty), TileType::Plains);
        assert_eq!(chunk_mgr_1.tile_at(tx, ty), TileType::Plains);
    }
}

#[test]
fn test_multiplayer_simultaneous_terrain_edit_conflict_resolution() {
    let mut chunk_mgr_a = ChunkManager::<32, 32>::new(WORLD_SEED, 1);
    let mut chunk_mgr_b = ChunkManager::<32, 32>::new(WORLD_SEED, 1);

    let origin = FixedVec2::ZERO;
    chunk_mgr_a.update_player_pos(origin, 0);
    chunk_mgr_b.update_player_pos(origin, 0);

    // Conflict Scenario:
    // Peer 0 wants to build a Rock wall at (8, 8) at tick 12
    let edit_0 = TerrainDelta {
        sender_id: 0,
        tick: 12,
        tile_x: 8,
        tile_y: 8,
        new_tile: TileType::Rock,
    };

    // Peer 1 wants to dig a Water canal at (8, 8) at tick 14 (Later in time)
    let edit_1 = TerrainDelta {
        sender_id: 1,
        tick: 14,
        tile_x: 8,
        tile_y: 8,
        new_tile: TileType::Water,
    };

    // Apply edits in different arrival order on Peer A and Peer B
    // Peer A applies edit_0 first, then receives edit_1 later
    chunk_mgr_a.set_tile(edit_0.tile_x, edit_0.tile_y, edit_0.new_tile);
    if edit_1.should_overwrite(&edit_0) {
        chunk_mgr_a.set_tile(edit_1.tile_x, edit_1.tile_y, edit_1.new_tile);
    }

    // Peer B applies edit_1 first, then receives older edit_0 later
    chunk_mgr_b.set_tile(edit_1.tile_x, edit_1.tile_y, edit_1.new_tile);
    if edit_0.should_overwrite(&edit_1) {
        chunk_mgr_b.set_tile(edit_0.tile_x, edit_0.tile_y, edit_0.new_tile);
    }

    // Both peers MUST converge to the winner (edit_1, Water)
    assert_eq!(chunk_mgr_a.tile_at(8, 8), TileType::Water);
    assert_eq!(chunk_mgr_b.tile_at(8, 8), TileType::Water);

    let hash_a = chunk_mgr_a.compute_hash();
    let hash_b = chunk_mgr_b.compute_hash();

    assert_eq!(
        hash_a, hash_b,
        "Terrain hashes MUST be 100% bit-identical after deterministic conflict resolution!"
    );
}
