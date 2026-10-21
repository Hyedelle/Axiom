//! # Axiom Core (`axiom-core`)
//!
//! The deterministic simulation foundation of the Axiom Engine:
//! - **Absolute Determinism**: Pure transition function $NextState = f(CurrentState, Input)$.
//! - **Cache-Line Resident**: 64-byte aligned entity definitions matching CPU L1 line geometry.
//! - **Zero Floating Point**: Fixed-point arithmetic for zero-error cross-platform consistency.
//! - **Time-Travel Debugging**: Instantaneous O(1) state recall and rollback replay.

#![no_std]

pub mod chunk;
pub mod combat;
pub mod entity;
pub mod fixed;
pub mod hash;
pub mod input;
pub mod logistics;
pub mod npc_crowd;
pub mod pcg;
pub mod spatial;
pub mod state;
pub mod storage;
pub mod tick;
pub mod time_machine;
pub mod world_time_machine;

pub use chunk::{
    sample_world_tile, Chunk, ChunkCoord, ChunkManager, ChunkSlot, ChunkStreamStats,
    DEFAULT_CHUNK_SIZE, DEFAULT_MAX_CHUNKS, DEFAULT_VIEW_RADIUS,
};
pub use combat::{BattleArena, CombatUnit, DEFAULT_MAX_UNITS};
pub use entity::Entity;
pub use fixed::{Fixed, FixedVec2};
pub use hash::{deterministic_hash, Fnv1aHasher};
pub use input::InputState;
pub use logistics::{LogisticsCell, LogisticsGrid};
pub use npc_crowd::{
    Npc, NpcActivity, NpcCrowdPool, NpcTable, DEFAULT_NPC_TABLE_CAPACITY, TICKS_PER_DAY,
};
pub use pcg::{Rng, TileMap, TileType};
pub use spatial::{GridCell, SpatialGrid};
pub use state::{TickState, DEFAULT_MAX_ENTITIES};
pub use storage::{EntitySlabPool, EntityTable, DEFAULT_TABLE_CAPACITY};
pub use tick::{Tick, DELTA_TIME_MICROS, TICK_RATE_HZ};
pub use time_machine::{RollbackError, TimeMachine};
pub use world_time_machine::{
    WorldSnapshot, WorldTimeMachine, DEFAULT_WORLD_ROLLBACK_CAPACITY,
};
