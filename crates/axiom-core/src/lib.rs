//! # Axiom Core (`axiom-core`)
//!
//! The deterministic simulation foundation of the Axiom Engine:
//! - **Absolute Determinism**: Pure transition function $NextState = f(CurrentState, Input)$.
//! - **Cache-Line Resident**: 64-byte aligned entity definitions matching CPU L1 line geometry.
//! - **Zero Floating Point**: Fixed-point arithmetic for zero-error cross-platform consistency.
//! - **Time-Travel Debugging**: Instantaneous O(1) state recall and rollback replay.

#![no_std]

pub mod audio;
pub mod ccd;
pub mod chunk;
pub mod cloud_save;
pub mod combat;
pub mod debris_particle;
pub mod shockwave;
pub mod deformable_mesh;
pub mod destruction_audio;
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
pub mod ragdoll;
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
pub mod xpbd_ragdoll;
pub mod soft_body;
pub mod actuator;
pub mod aerodynamics;
pub mod turbulence;

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
    broadphase_batch_job, dispatch_broadphase_with_jobs, emit_collision_pairs_to_event_bus,
    spatial_hash_3d, AabbSlab64, CollisionPair64, SpatialCell64, SpatialHashGrid,
    COLLISION_FLAG_DYNAMIC, COLLISION_FLAG_SLEEPING, COLLISION_FLAG_STATIC, COLLISION_FLAG_TRIGGER,
    DEFAULT_CELL_CAPACITY, DEFAULT_GRID_TABLE_CAP, EVENT_BROADPHASE_PAIR,
};
pub use ccd::{
    ccd_batch_job, compute_ccd_consensus_hash, dispatch_ccd_hitboxes_with_jobs,
    emit_ccd_hits_to_event_bus, raycast_aabb_toi, swept_aabb_toi, swept_hitbox_vs_hurtbox,
    ContactNormal6, HitType, HitboxSlab64, SweptAabbResult64, EVENT_CCD_HIT, HITBOX_FLAG_ACTIVE,
    HITBOX_FLAG_COUNTER_HIT, HITBOX_FLAG_GUARD_BREAK, HITBOX_FLAG_PIERCING, HITBOX_FLAG_SWEPT_CCD,
    TOI_ONE_Q16,
};
pub use ragdoll::{
    dispatch_ragdoll_with_jobs, emit_ragdoll_impacts_to_event_bus, isqrt_i64, ragdoll_batch_job,
    solve_distance_constraint, solve_two_bone_ik, RagdollBoneSlab64, RagdollEngine,
    RagdollImpactReport, RagdollJointSlab64, RagdollJointType, EVENT_RAGDOLL_IMPACT,
    EVENT_RAGDOLL_SEVER, Q16_ONE as RAGDOLL_Q16_ONE, RAGDOLL_BONE_FLAG_ACTIVE,
    RAGDOLL_BONE_FLAG_FROZEN, RAGDOLL_BONE_FLAG_GROUNDED, RAGDOLL_BONE_FLAG_SEVERED,
    RAGDOLL_JOINT_FLAG_ACTIVE, RAGDOLL_JOINT_FLAG_LIMITED, RAGDOLL_JOINT_FLAG_MOTOR,
};
pub use deformable_mesh::{
    dispatch_terrain_deform_with_jobs, emit_crater_to_event_bus, icbrt_u64,
    terrain_deform_batch_job, CraterSlab64, DeformableMeshGrid, TerrainCraterReport,
    TerrainVertexSlab64, CRATER_FLAG_ACTIVE, CRATER_FLAG_PERMANENT, CRATER_FLAG_SHARDS_SPAWNED,
    DEFAULT_GRID_DIM, DEFAULT_MAX_CRATERS, DEFAULT_MESH_VERTICES, EVENT_TERRAIN_CRATER,
    TERRAIN_MAT_DIRT, TERRAIN_MAT_METAL, TERRAIN_MAT_MOLTEN, TERRAIN_MAT_SAND,
    TERRAIN_MAT_STONE, VERTEX_FLAG_FRACTURED, VERTEX_FLAG_MODIFIED, VERTEX_FLAG_MOLTEN,
    VERTEX_FLAG_PINNED,
};
pub use debris_particle::{
    debris_batch_job, dispatch_debris_with_jobs, emit_debris_burst_to_event_bus, DebrisBurstReport,
    DebrisParticlePool, DebrisParticleSlab64, ParticleContactSlab64, CONTACT_TYPE_ENTITY,
    CONTACT_TYPE_FLOOR, CONTACT_TYPE_TERRAIN, CONTACT_TYPE_WALL, DEBRIS_FLAG_ACTIVE,
    DEBRIS_FLAG_BOUNCED, DEBRIS_FLAG_CRITICAL, DEBRIS_FLAG_GROUNDED, DEBRIS_FLAG_SLEEPING,
    DEBRIS_MAT_DIRT, DEBRIS_MAT_METAL, DEBRIS_MAT_MOLTEN, DEBRIS_MAT_SAND, DEBRIS_MAT_SPARK,
    DEBRIS_MAT_STONE, DEFAULT_MAX_DEBRIS_PARTICLES, EVENT_DEBRIS_BURST, GRAVITY_MMS2,
    GRAVITY_PER_TICK_MMS,
};
pub use destruction_audio::{
    destruction_audio_batch_job, deterministic_noise, dispatch_destruction_audio_with_jobs,
    emit_destruction_audio_to_event_bus, synthesize_destruction_impact_pcm, AcousticZoneSlab64,
    DestructionAudioEngine, DestructionAudioReport, DestructionVoiceSlab64, AUDIO_MAT_DIRT,
    AUDIO_MAT_METAL, AUDIO_MAT_MOLTEN, AUDIO_MAT_SAND, AUDIO_MAT_STONE, AUDIO_SAMPLE_RATE_48K,
    DEFAULT_MAX_ACOUSTIC_ZONES, DEFAULT_MAX_DESTRUCTION_VOICES, EVENT_AUDIO_DESTRUCTION,
    VOICE_FLAG_ACTIVE, VOICE_FLAG_CRACK_BURST, VOICE_FLAG_HIGH_PRIORITY, VOICE_FLAG_REVERB_TAIL,
    ZONE_FLAG_ACTIVE, ZONE_FLAG_DAMPED, ZONE_FLAG_ENCLOSED,
};
pub use shockwave::{
    dispatch_shockwave_with_jobs, emit_shockwave_to_event_bus, shockwave_batch_job,
    DustEmitterSlab64, ShockwaveReport, ShockwaveSlab64, VolumetricShockwaveEngine,
    DEFAULT_MAX_DUST_EMITTERS, DEFAULT_MAX_SHOCKWAVES, DUST_FLAG_ACTIVE, DUST_FLAG_DISSIPATED,
    DUST_FLAG_SETTLING, EVENT_SHOCKWAVE, SHOCKWAVE_FLAG_ACTIVE, SHOCKWAVE_FLAG_GROUND_BURST,
    SHOCKWAVE_FLAG_REFLECTED, SHOCKWAVE_TYPE_CONICAL, SHOCKWAVE_TYPE_SPHERICAL,
    SHOCKWAVE_TYPE_TOROIDAL, SPEED_OF_SOUND_MMS,
};
pub use xpbd_ragdoll::{
    dispatch_xpbd_with_jobs, emit_xpbd_to_event_bus, isqrt_i64 as xpbd_isqrt_i64,
    solve_xpbd_distance, xpbd_batch_job, XpbdBoneSlab64, XpbdConstraintSlab64,
    XpbdConstraintType, XpbdEventReport, XpbdRagdollEngine, EVENT_XPBD_STRAIN, EVENT_XPBD_TEAR,
    Q16_ONE as XPBD_Q16_ONE, XPBD_BONE_FLAG_ACTIVE, XPBD_BONE_FLAG_FROZEN,
    XPBD_BONE_FLAG_GROUNDED, XPBD_BONE_FLAG_SEVERED, XPBD_CONSTRAINT_FLAG_ACTIVE,
    XPBD_CONSTRAINT_FLAG_FATIGUED, XPBD_CONSTRAINT_FLAG_PLASTIC, XPBD_CONSTRAINT_FLAG_SEVERED,
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
