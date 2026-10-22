//! # axiom-game
//!
//! High-level game systems for Axiom Engine (Track 1: Open-World Factory RPG).
//! Fully deterministic, zero floating-point, cache-friendly DOD structures.

#![no_std]

pub mod boxing;
pub mod card;
pub mod defense;
pub mod factory;
pub mod schema_catalog;
pub mod world_factory;
pub mod worker;

pub use boxing::{ActionKind, AiPhase, BoxerAction, BoxerState, BoxingEvent, BoxingRingEngine};
pub use card::{CardKind, DeckState, TacticalCard};

pub use factory::{
    Belt, Chest, Direction, FactoryGrid, FactoryReport, FactoryStructure, FilterInserter,
    Inserter, ItemType, MiningDrill, OreDeposit, Smelter, StructureKind, CYCLE, DEFAULT_FACTORY_H,
    DEFAULT_FACTORY_W, MAX_BELT_TRANSFERS, SMELTER_INPUT_CAP, SMELTER_OUTPUT_CAP,
};

pub use world_factory::{
    chunk_ore_seed, for_each_vein_tile, generate_ore_veins, survey_chunk, CrossChunkLogistics,
    DefaultWorldFactory, FactoryChunkSlot, OreSurvey, OreVein, OreVeinSet, WorldFactory,
    WorldFactoryReport, DEFAULT_MAX_FACTORY_CHUNKS, MAX_CROSS_CHUNK_TRANSFERS,
    MAX_VEINS_PER_CHUNK, MAX_VEIN_RADIUS, MIN_VEINS_PER_CHUNK, VEIN_RICHNESS_MAX,
    VEIN_RICHNESS_MIN,
};

pub use worker::{
    ColonyWorkforce, WorkerAssignment, WorkerRole, WorkerState, WorkforceReport,
    DEFAULT_WORKFORCE_CAP, EXHAUSTED_THRESHOLD, HAUL_SPEED, MAX_SKILL, RESTED_THRESHOLD,
    REST_STAMINA_GAIN, SKILL_BUFF_PCT, STAMINA_MAX, WORK_STAMINA_COST, XP_PER_SKILL,
};

pub use defense::{
    BaseHeart, DefenseReport, LaserTurret, MonsterKind, Turret, Wall, WaveDirector, BASE_HEART_HP,
    DEFAULT_BASE_WAVE_SIZE, DEFAULT_FIRST_WAVE_TICK, DEFAULT_MAX_MONSTERS, DEFAULT_WAVE_GROWTH,
    DEFAULT_WAVE_INTERVAL, LASER_TURRET_DAMAGE, LASER_TURRET_ENERGY_CAP, LASER_TURRET_HP,
    LASER_TURRET_RANGE, MAX_DEFENSE_TILES, MONSTER_ATTACK_GROWTH, MONSTER_BASE_ATTACK,
    MONSTER_BASE_HP, MONSTER_COOLDOWN, MONSTER_HP_GROWTH, MONSTER_MOVE_INTERVAL, MONSTER_RANGE,
    MONSTER_TEAM, TURRET_AMMO_CAP, TURRET_COOLDOWN, TURRET_DAMAGE, TURRET_HP, TURRET_RANGE,
    TURRET_SALVAGE, WALL_HP, WALL_SALVAGE,
};

pub use schema_catalog::{factory_types, register_factory_recipes, register_factory_types};
