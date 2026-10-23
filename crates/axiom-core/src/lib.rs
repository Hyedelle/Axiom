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
pub mod cloud_save;
pub mod combat;
pub mod dlc;
pub mod effector;
pub mod entity;
pub mod event_bus;
pub mod fixed;
pub mod fluid;
pub mod grid_netcode;
pub mod hash;
pub mod hpa;
pub mod input;
pub mod job_system;
pub mod logistics;
pub mod monster_spawn;
pub mod netcode;
pub mod neural_ai;
pub mod npc_crowd;
pub mod pathfinding;
pub mod pcg;
pub mod power_grid;
pub mod quest;
pub mod replay;
pub mod schema;
pub mod serialization;
pub mod spatial;
pub mod spatial_grid;
pub mod spatial_index;
pub mod state;
pub mod storage;
pub mod thermal_stress;
pub mod tick;
pub mod tile_entity;
pub mod time_machine;
pub mod voxel_collapse;
pub mod voxel_destruct;
pub mod voxel_fluid;
pub mod voxel_fluid_simd;
pub mod voxel_shard;
pub mod worker_pool;
pub mod world_time_machine;

pub use voxel_fluid::{FluidStepResult, VoxelFluidField16, FLUID_MAX_LEVEL};
pub use voxel_fluid_simd::{SimdFluidEngine, SimdFluidStepResult};
pub use thermal_stress::{
    ThermalCollapseResult, ThermalStressField16, TEMP_AMBIENT, TEMP_INCANDESCENT_THRESHOLD,
    TEMP_PHASE_MELT_THRESHOLD, TEMP_WARM_THRESHOLD, STRESS_FRACTURE_THRESHOLD,
};
pub use job_system::{
    JobContext, JobDescriptor64, JobFn, JobHandle, JobPriority, JobStatus, JobSystem,
    DEFAULT_JOB_CAPACITY,
};
pub use worker_pool::{
    WorkStealingDeque, WorkerPoolCoordinator, WorkerTelemetry64, DEQUE_CAPACITY,
};
pub use event_bus::{
    channel, component, dispatch_events_with_jobs, filter_entities, filter_entities_with_jobs,
    ComponentMask64, ComponentQuery, DeterministicEventBus, EventSlab64, DEFAULT_EVENT_CAPACITY,
};
pub use spatial_grid::{
    spatial_hash_3d, AabbSlab64, CollisionPair64, SpatialCell64, SpatialHashGrid,
    COLLISION_FLAG_DYNAMIC, COLLISION_FLAG_SLEEPING, COLLISION_FLAG_STATIC, COLLISION_FLAG_TRIGGER,
    DEFAULT_CELL_CAPACITY, DEFAULT_GRID_TABLE_CAP,
};

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
pub use cloud_save::{
    resolve_sync_conflict, CloudSaveError, CloudSaveHeader, CloudSavePackage, SyncResolution,
    CLOUD_SAVE_MAGIC, CLOUD_SAVE_VERSION,
};
pub use combat::{BattleArena, CombatUnit, DEFAULT_MAX_UNITS};
pub use dlc::{DlcId, DlcState};
pub use effector::{
    ActiveStatus, AtbArena, AtbUnit, CombatEffector, Element, StatusKind, ATB_MAX_GAUGE,
    MAX_ACTIVE_UNITS, MAX_STATUS_EFFECTS,
};
pub use entity::Entity;
pub use fixed::{Fixed, FixedVec2};
pub use fluid::{
    dir_offset, opposite_dir, FluidNetwork, FluidStats, FluidType, PipeKind, PipeNode,
    DEFAULT_MAX_PIPES, FLUID_INDEX_BUCKETS, FLUID_NONE, PRESSURE_FULL,
};
pub use hash::{deterministic_hash, Fnv1aHasher};
pub use hpa::{
    chunk_bfs, ChunkPortalCache, HierarchicalPathfinder, HpaStats, Portal,
    DEFAULT_HPA_MAX_CHUNKS, DEFAULT_HPA_MAX_PORTALS, HPA_CHUNK_TILES_MAX, HPA_HEAP_CAPACITY,
    HPA_INDEX_BUCKETS, HPA_MAX_CHUNK_SIZE, HPA_MAX_WAYPOINTS, HPA_NO_NODE, HPA_UNREACHABLE,
    MAX_PORTALS_PER_CHUNK, PORTAL_EDGE_EAST, PORTAL_EDGE_NORTH, PORTAL_EDGE_SOUTH,
    PORTAL_EDGE_WEST,
};
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
pub use neural_ai::{
    infer_action, CombatObservation, NeuralAiWeights, NN_HIDDEN_DIM, NN_INPUT_DIM, NN_OUTPUT_DIM,
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
pub use power_grid::{
    PowerGridManager, PowerNetwork, PowerNode, PowerRole, DEFAULT_MAX_POWER_NETS,
    DEFAULT_MAX_POWER_NODES, NO_NET, POWER_NET_ID_LIMIT,
};
pub use quest::{
    DialogueAction, DialogueChoice, DialogueNode, DialogueSession, ObjectiveType, QuestJournal,
    QuestRecord, QuestStatus, DEFAULT_MAX_ACTIVE_QUESTS, DEFAULT_MAX_COMPLETED_QUESTS,
};
pub use replay::{
    DeterministicReplay, ReplayError, ReplayHeader, RleInputRun, DEFAULT_KEYFRAME_INTERVAL,
    REPLAY_MAGIC, REPLAY_VERSION,
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
pub use voxel_collapse::{DynamicCollapseSystem, DynamicVoxelChunk};
pub use voxel_destruct::{
    SeverancePlane, VoxelRaymarchHit, VoxelVolume16, VOXEL_AIR, VOXEL_ARMOR, VOXEL_DIM, VOXEL_METAL, VOXEL_MOLTEN,
    VOXEL_STONE, VOXEL_TOTAL,
};
pub use voxel_shard::{VoxelShard, VoxelShardPool};
pub use world_time_machine::{
    WorldSnapshot, WorldTimeMachine, DEFAULT_WORLD_ROLLBACK_CAPACITY,
};
