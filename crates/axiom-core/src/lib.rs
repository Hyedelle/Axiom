//! # Axiom Core (`axiom-core`)
//!
//! The deterministic simulation foundation of the Axiom Engine:
//! - **Absolute Determinism**: Pure transition function $NextState = f(CurrentState, Input)$.
//! - **Cache-Line Resident**: 64-byte aligned entity definitions matching CPU L1 line geometry.
//! - **Zero Floating Point**: Fixed-point arithmetic for zero-error cross-platform consistency.
//! - **Time-Travel Debugging**: Instantaneous O(1) state recall and rollback replay.

#![no_std]

pub mod audio;
pub mod chunk;
pub mod combat;
pub mod effector;
pub mod entity;
pub mod fixed;
pub mod hash;
pub mod input;
pub mod logistics;
pub mod netcode;
pub mod npc_crowd;
pub mod pathfinding;
pub mod pcg;
pub mod quest;
pub mod schema;
pub mod serialization;
pub mod spatial;
pub mod state;
pub mod storage;
pub mod tick;
pub mod time_machine;
pub mod world_time_machine;

pub use audio::{
    AudioMixFrame, SoundCategory, SoundEvent, SpatialAudioSimulator, SpatialVoice,
    DEFAULT_MAX_VOICES, MAX_AUDIO_QUEUE,
};
pub use chunk::{
    sample_world_tile, Chunk, ChunkCoord, ChunkManager, ChunkSlot, ChunkStreamStats,
    DEFAULT_CHUNK_SIZE, DEFAULT_MAX_CHUNKS, DEFAULT_VIEW_RADIUS,
};
pub use combat::{BattleArena, CombatUnit, DEFAULT_MAX_UNITS};
pub use effector::{
    ActiveStatus, AtbArena, AtbUnit, CombatEffector, Element, StatusKind, ATB_MAX_GAUGE,
    MAX_ACTIVE_UNITS, MAX_STATUS_EFFECTS,
};
pub use entity::Entity;
pub use fixed::{Fixed, FixedVec2};
pub use hash::{deterministic_hash, Fnv1aHasher};
pub use input::InputState;
pub use logistics::{
    LogisticsCell, LogisticsGrid, DIR_EAST, DIR_NORTH, DIR_SOUTH, DIR_WEST, ITEM_COAL,
    ITEM_COPPER_ORE, ITEM_IRON_ORE, ITEM_NONE,
};
pub use netcode::{
    PlayerInputQueue, RollbackFrameStats, RollbackSession, SimulatedNetworkPipe,
    DEFAULT_NETCODE_BUFFER_SIZE, MAX_SUPPORTED_PLAYERS,
};
pub use npc_crowd::{
    Npc, NpcActivity, NpcCrowdPool, NpcTable, DEFAULT_NPC_TABLE_CAPACITY, TICKS_PER_DAY,
};
pub use pathfinding::{
    Path, PathFinder, DEFAULT_GRID_H, DEFAULT_GRID_W, DEFAULT_HEAP_CAPACITY, MAX_PATH_WAYPOINTS,
};
pub use pcg::{Rng, TileMap, TileType};
pub use quest::{
    DialogueAction, DialogueChoice, DialogueNode, DialogueSession, ObjectiveType, QuestJournal,
    QuestRecord, QuestStatus, DEFAULT_MAX_ACTIVE_QUESTS, DEFAULT_MAX_COMPLETED_QUESTS,
};
pub use serialization::{
    SaveHeader, SerializationError, WorldSerializer, AXM_MAGIC, AXM_VERSION,
};
pub use schema::{
    category as schema_category, RecipeDefinition, RecipeRegistry, SchemaHeader, TypeDefinition,
    TypeRegistry, DEFAULT_RECIPE_REGISTRY_CAPACITY, DEFAULT_TYPE_REGISTRY_CAPACITY, SCHEMA_MAGIC,
};
pub use spatial::{GridCell, SpatialGrid};
pub use state::{TickState, DEFAULT_MAX_ENTITIES};
pub use storage::{EntitySlabPool, EntityTable, DEFAULT_TABLE_CAPACITY};
pub use tick::{Tick, DELTA_TIME_MICROS, TICK_RATE_HZ};
pub use time_machine::{RollbackError, TimeMachine};
pub use world_time_machine::{
    WorldSnapshot, WorldTimeMachine, DEFAULT_WORLD_ROLLBACK_CAPACITY,
};
