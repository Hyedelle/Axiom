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
pub mod grid_netcode;
pub mod hash;
pub mod input;
pub mod logistics;
pub mod monster_spawn;
pub mod netcode;
pub mod npc_crowd;
pub mod pathfinding;
pub mod pcg;
pub mod quest;
pub mod schema;
pub mod serialization;
pub mod spatial;
pub mod spatial_index;
pub mod state;
pub mod storage;
pub mod tick;
pub mod tile_entity;
pub mod time_machine;
pub mod world_time_machine;

pub use audio::{
    AudioMixFrame, SoundCategory, SoundEvent, SpatialAudioSimulator, SpatialVoice,
    DEFAULT_MAX_VOICES, MAX_AUDIO_QUEUE,
};
pub use chunk::{
    pack_resource, sample_world_tile, unpack_resource, Chunk, ChunkCoord, ChunkDelta,
    ChunkDeltaMap, ChunkHeader, ChunkManager, ChunkSlot, ChunkStreamStats, CHUNK_INDEX_BUCKETS,
    DEFAULT_CHUNK_SIZE, DEFAULT_DELTA_CAPACITY, DEFAULT_MAX_CHUNKS, DEFAULT_SPAWN_MIN_OPEN_AREA,
    DEFAULT_VIEW_RADIUS, SPAWN_WINDOW_MAX,
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
pub use monster_spawn::{
    biome_allows, pick_spawn_entry, tier_for_chunk_distance, DefensePost, DefenseTickReport,
    MonsterArchetype, MonsterSpawner, MonsterState, SpawnEntry, SpawnStats, TerritoryDefense,
    WorldMonster, BIOME_SPAWN_DENSITY, BIOME_SPAWN_TABLE, DEFAULT_DESPAWN_RADIUS,
    DEFAULT_MAX_DEFENSE_POSTS, DEFAULT_MAX_MONSTERS_PER_CHUNK, DEFAULT_MAX_WORLD_MONSTERS,
    DEFAULT_SPAWN_INTERVAL, DEFAULT_STRONGHOLD_HP, DEFAULT_TERRITORY_RADIUS, GUARDIAN_DEPOSIT_THRESHOLD,
    MAX_TIER, TIER_CHUNK_STEP, WAVE_RING_MARGIN,
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
pub use pcg::{
    classify_biome, hash2d, sample_value_noise, sample_world_biome, sample_world_biome_cached,
    BiomeKind, BiomeNoiseContext, BiomeSample, ResourceDeposit, Rng, TileMap, TileType,
    BIOME_DEPOSIT_TABLE, NOISE_MAX, VEIN_CELL_SIZE, VEIN_MAX_RADIUS, VEIN_NO_DISTANCE,
};
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
pub use spatial::{
    GridCell, SpatialGrid, SpatialIndex, DEFAULT_SPATIAL_BUCKETS, DEFAULT_SPATIAL_MAX_ENTITIES,
    EMPTY_INDEX,
};
pub use state::{TickState, DEFAULT_MAX_ENTITIES};
pub use storage::{EntitySlabPool, EntityTable, DEFAULT_TABLE_CAPACITY};
pub use tick::{Tick, DELTA_TIME_MICROS, TICK_RATE_HZ};
pub use tile_entity::{grid_action_kind, tile_flags, GridAction, TileCell, TileGrid};
pub use grid_netcode::{
    GridFrameStats, GridInputQueue, GridRollbackSession, SimulatedGridNetworkPipe,
    DEFAULT_GRID_NETCODE_BUFFER_SIZE, MAX_SUPPORTED_GRID_PLAYERS,
};
pub use time_machine::{RollbackError, TimeMachine};
pub use world_time_machine::{
    WorldSnapshot, WorldTimeMachine, DEFAULT_WORLD_ROLLBACK_CAPACITY,
};
