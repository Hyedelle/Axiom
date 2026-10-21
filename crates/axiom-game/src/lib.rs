//! # axiom-game
//!
//! High-level game systems for Axiom Engine (Track 1: Open-World Factory RPG).
//! Fully deterministic, zero floating-point, cache-friendly DOD structures.

#![no_std]

pub mod factory;
pub mod world_factory;

pub use factory::{
    Belt, Chest, Direction, FactoryGrid, FactoryReport, FactoryStructure, Inserter, ItemType,
    MiningDrill, OreDeposit, Smelter, StructureKind, CYCLE, DEFAULT_FACTORY_H,
    DEFAULT_FACTORY_W, MAX_BELT_TRANSFERS, SMELTER_INPUT_CAP, SMELTER_OUTPUT_CAP,
};

pub use world_factory::{
    chunk_ore_seed, for_each_vein_tile, generate_ore_veins, survey_chunk, CrossChunkLogistics,
    DefaultWorldFactory, FactoryChunkSlot, OreSurvey, OreVein, OreVeinSet, WorldFactory,
    WorldFactoryReport, DEFAULT_MAX_FACTORY_CHUNKS, MAX_CROSS_CHUNK_TRANSFERS,
    MAX_VEINS_PER_CHUNK, MAX_VEIN_RADIUS, MIN_VEINS_PER_CHUNK, VEIN_RICHNESS_MAX,
    VEIN_RICHNESS_MIN,
};
