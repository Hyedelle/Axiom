//! # axiom-game
//!
//! High-level game systems for Axiom Engine (Track 1: Open-World Factory RPG).
//! Fully deterministic, zero floating-point, cache-friendly DOD structures.

#![no_std]

pub mod factory;

pub use factory::{
    Belt, Chest, Direction, FactoryGrid, FactoryReport, FactoryStructure, Inserter, ItemType,
    MiningDrill, OreDeposit, Smelter, StructureKind, CYCLE, DEFAULT_FACTORY_H,
    DEFAULT_FACTORY_W, MAX_BELT_TRANSFERS, SMELTER_INPUT_CAP, SMELTER_OUTPUT_CAP,
};
