//! # Deterministic Open-World Time-Travel & Rollback Engine
//!
//! Synchronizes infinite chunk streaming, terrain modifications, and NPC ecology
//! with instantaneous O(1) state recall and deterministic time-travel rollback.

extern crate alloc;

use alloc::vec::Vec;

use crate::chunk::{ChunkManager, DEFAULT_CHUNK_SIZE, DEFAULT_MAX_CHUNKS};
use crate::fixed::{Fixed, FixedVec2};
use crate::hash::deterministic_hash;
use crate::npc_crowd::{NpcTable, DEFAULT_NPC_TABLE_CAPACITY};
use crate::pcg::TileType;
use crate::tick::Tick;
use crate::time_machine::RollbackError;

pub const DEFAULT_WORLD_ROLLBACK_CAPACITY: usize = 32;

/// Complete deterministic snapshot of the open world at a discrete simulation tick.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorldSnapshot<
    const S: usize = DEFAULT_CHUNK_SIZE,
    const MAX_CHUNKS: usize = DEFAULT_MAX_CHUNKS,
    const NPC_CAP: usize = DEFAULT_NPC_TABLE_CAPACITY,
> {
    pub tick: u64,
    pub player_pos: FixedVec2,
    pub chunk_mgr: ChunkManager<S, MAX_CHUNKS>,
    pub npcs: NpcTable<NPC_CAP>,
}

impl<const S: usize, const MAX_CHUNKS: usize, const NPC_CAP: usize> WorldSnapshot<S, MAX_CHUNKS, NPC_CAP> {
    /// Initializes a new open-world snapshot at tick 0.
    pub fn new(world_seed: u64, view_radius: i32) -> Self {
        let mut chunk_mgr = ChunkManager::new(world_seed, view_radius);
        let player_pos = FixedVec2::new(Fixed::ZERO, Fixed::ZERO);
        chunk_mgr.update_player_pos(player_pos, 0);

        Self {
            tick: 0,
            player_pos,
            chunk_mgr,
            npcs: NpcTable::new(),
        }
    }

    /// Advances the open world simulation by 1 discrete tick.
    pub fn step(&mut self, player_move: FixedVec2) {
        self.tick += 1;
        self.player_pos.x += player_move.x;
        self.player_pos.y += player_move.y;

        // 1. Update procedural chunk streaming around player
        self.chunk_mgr.update_player_pos(self.player_pos, self.tick);

        // 2. Update NPC schedules and movements
        self.npcs.update(self.tick);
    }

    /// Sets tile at world coordinates (e.g. mining or building) and marks chunk dirty.
    #[inline]
    pub fn set_tile(&mut self, world_x: i64, world_y: i64, tile: TileType) -> bool {
        self.chunk_mgr.set_tile(world_x, world_y, tile)
    }

    /// Retrieves tile at world coordinates.
    #[inline]
    pub fn tile_at(&self, world_x: i64, world_y: i64) -> TileType {
        self.chunk_mgr.tile_at(world_x, world_y)
    }

    /// Computes deterministic state hash across chunks, tiles, NPCs, and player.
    pub fn compute_hash(&self) -> u64 {
        let mut h = self.tick.wrapping_mul(0x9E3779B97F4A7C15);
        h ^= deterministic_hash(&self.player_pos);
        h ^= self.chunk_mgr.compute_hash();
        for npc in &self.npcs.npcs {
            if npc.is_active() {
                h ^= deterministic_hash(npc);
            }
        }
        h
    }
}

/// Ring-buffer time machine recording full open-world snapshots for instantaneous rollback.
pub struct WorldTimeMachine<
    const S: usize = DEFAULT_CHUNK_SIZE,
    const MAX_CHUNKS: usize = DEFAULT_MAX_CHUNKS,
    const NPC_CAP: usize = DEFAULT_NPC_TABLE_CAPACITY,
> {
    snapshots: Vec<Option<WorldSnapshot<S, MAX_CHUNKS, NPC_CAP>>>,
    capacity: usize,
    oldest_tick: u64,
    newest_tick: u64,
    count: usize,
}

impl<const S: usize, const MAX_CHUNKS: usize, const NPC_CAP: usize>
    WorldTimeMachine<S, MAX_CHUNKS, NPC_CAP>
{
    /// Creates a new time machine with a fixed capacity ring buffer.
    pub fn new(capacity: usize) -> Self {
        let mut snapshots = Vec::with_capacity(capacity);
        for _ in 0..capacity {
            snapshots.push(None);
        }

        Self {
            snapshots,
            capacity,
            oldest_tick: 0,
            newest_tick: 0,
            count: 0,
        }
    }

    /// Records a world snapshot into history.
    pub fn record(&mut self, snapshot: WorldSnapshot<S, MAX_CHUNKS, NPC_CAP>) {
        let slot = (snapshot.tick as usize) % self.capacity;
        let tick = snapshot.tick;
        self.snapshots[slot] = Some(snapshot);

        if self.count == 0 {
            self.oldest_tick = tick;
            self.newest_tick = tick;
            self.count = 1;
        } else {
            if tick > self.newest_tick {
                self.newest_tick = tick;
            }
            if self.count < self.capacity {
                self.count += 1;
            } else {
                self.oldest_tick = self.newest_tick.saturating_sub(self.capacity as u64 - 1);
            }
        }
    }

    /// Instantaneously recalls the exact world snapshot at `target_tick`.
    pub fn recall(&self, target_tick: u64) -> Result<&WorldSnapshot<S, MAX_CHUNKS, NPC_CAP>, RollbackError> {
        if self.count == 0 {
            return Err(RollbackError::TickNotFound(Tick(target_tick)));
        }
        if target_tick > self.newest_tick {
            return Err(RollbackError::TickInFuture(Tick(target_tick)));
        }
        if target_tick < self.oldest_tick {
            return Err(RollbackError::HistoryTooOld(Tick(target_tick)));
        }

        let slot = (target_tick as usize) % self.capacity;
        if let Some(snapshot) = &self.snapshots[slot] {
            if snapshot.tick == target_tick {
                return Ok(snapshot);
            }
        }

        Err(RollbackError::TickNotFound(Tick(target_tick)))
    }

    /// Returns the number of recorded snapshots in buffer.
    #[inline]
    pub fn len(&self) -> usize {
        self.count
    }

    /// Returns true if no snapshots are recorded.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }
}
