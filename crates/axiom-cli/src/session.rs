//! Complete Integrated Open-World Headless Session (T-016).
//!
//! Orchestrates all 4 Axiom Phase subsystems in a unified deterministic simulation loop:
//! - Phase 1: Auto-Battler Combat Arena (`BattleArena`)
//! - Phase 2: Factory Resource Logistics Grid (`LogisticsGrid`)
//! - Phase 3: Infinite Procedural Chunk Streaming (`ChunkManager`)
//! - Phase 4: 100,000 NPC Living Ecology Simulation (`NpcCrowdPool`)
//! - Time-Travel: Instantaneous deterministic state hashing and rollback.

extern crate alloc;

use alloc::boxed::Box;
use axiom_core::combat::BattleArena;
use axiom_core::fixed::{Fixed, FixedVec2};
use axiom_core::hash::deterministic_hash;
use axiom_core::logistics::{LogisticsGrid, DIR_EAST};
use axiom_core::npc_crowd::NpcCrowdPool;
use std::time::Instant;

/// Summary metrics returned from a headless simulation run.
#[derive(Debug, Clone)]
pub struct SessionReport {
    pub total_ticks: u64,
    pub final_hash: u64,
    pub total_npcs: usize,
    pub loaded_chunks: usize,
    pub total_chunks_generated: u64,
    pub total_logistics_items: u32,
    pub combat_units_alive: usize,
    pub elapsed_micros: u128,
    pub avg_micros_per_tick: f64,
    pub fps_equivalent: f64,
}

/// Unified open-world simulation session orchestrating all Axiom subsystems.
pub struct HeadlessSession {
    pub world_seed: u64,
    pub tick: u64,
    pub player_pos: FixedVec2,
    pub chunk_mgr: axiom_core::chunk::ChunkManager<32, 32>,
    pub npcs: Box<NpcCrowdPool<100>>,
    pub logistics: Box<LogisticsGrid<64, 64>>,
    pub arena: Box<BattleArena>,
}

impl HeadlessSession {
    /// Initializes the full open-world simulation environment.
    pub fn new(world_seed: u64) -> Self {
        // 1. Allocate 6.5MB NPC crowd pool safely on heap
        let npc_layout = core::alloc::Layout::new::<NpcCrowdPool<100>>();
        let npc_ptr = unsafe { alloc::alloc::alloc_zeroed(npc_layout) as *mut NpcCrowdPool<100> };
        assert!(!npc_ptr.is_null(), "Failed to allocate NpcCrowdPool on heap");
        let mut npcs = unsafe { Box::from_raw(npc_ptr) };

        // 2. Populate 100,000 NPCs
        for i in 0..100_000 {
            let id = (i + 1) as u32;
            let home = ((i % 1000) as i32, (i / 1000) as i32);
            let work = (((i + 500) % 1000) as i32, ((i + 500) / 1000) as i32);
            let shift = (i % 3) as u8;
            npcs.spawn(id, home, work, shift, 256);
        }

        // 3. Initialize chunk manager around player
        let player_pos = FixedVec2::new(Fixed::ZERO, Fixed::ZERO);
        let mut chunk_mgr = axiom_core::chunk::ChunkManager::new(world_seed, 2);
        chunk_mgr.update_player_pos(player_pos, 0);

        // 4. Initialize factory logistics grid with automated production lines
        let mut logistics = Box::new(LogisticsGrid::<64, 64>::new());
        // Drill at (10, 10), Belts from (11, 10) to (20, 10), Chest at (21, 10)
        logistics.place_drill(10, 10, DIR_EAST, 25);
        for x in 11..=20 {
            logistics.place_belt(x, 10, DIR_EAST, 20); // Flow East
        }
        logistics.place_chest(21, 10);

        // 5. Initialize auto-battler arena with 100 vs 100 combat units
        let mut arena = Box::new(BattleArena::new());
        for i in 0..100 {
            // Team 1 (Knights)
            arena.spawn_unit(
                (i + 1) as u32,
                1,
                100,
                20,
                3,
                2,
                FixedVec2::from_ints((i % 10) as i64, (i / 10) as i64),
            );
            // Team 2 (Orcs)
            arena.spawn_unit(
                (i + 101) as u32,
                2,
                120,
                18,
                2,
                3,
                FixedVec2::from_ints((i % 10 + 20) as i64, (i / 10) as i64),
            );
        }

        Self {
            world_seed,
            tick: 0,
            player_pos,
            chunk_mgr,
            npcs,
            logistics,
            arena,
        }
    }

    /// Advances the entire open world by 1 discrete tick.
    pub fn step(&mut self, player_move: FixedVec2) {
        self.tick += 1;

        // Subsystem 1: Player & Chunk Streaming
        self.player_pos.x += player_move.x;
        self.player_pos.y += player_move.y;
        self.chunk_mgr.update_player_pos(self.player_pos, self.tick);

        // Subsystem 2: 100,000 NPC Living Ecology
        self.npcs.update(self.tick);

        // Subsystem 3: 4,096-Cell Factory Logistics
        self.logistics.step();

        // Subsystem 4: Auto-Battler Combat Arena
        self.arena.step();
    }

    /// Computes deterministic master state hash combining all subsystems.
    pub fn compute_master_hash(&self) -> u64 {
        let mut h = self.world_seed;
        h = h.rotate_left(7) ^ self.tick.wrapping_mul(0x9E3779B97F4A7C15);
        h ^= deterministic_hash(&self.player_pos);
        h ^= self.chunk_mgr.compute_hash();
        h ^= self.npcs.compute_hash();
        h ^= self.logistics.compute_hash();
        h ^= self.arena.compute_hash();
        h
    }

    /// Executes `num_ticks` ticks continuously and returns detailed benchmark report.
    pub fn run_session(&mut self, num_ticks: u64) -> SessionReport {
        let start = Instant::now();
        let step_move = FixedVec2::new(Fixed::from_int(1), Fixed::ZERO);

        for _ in 0..num_ticks {
            self.step(step_move);
        }

        let elapsed = start.elapsed();
        let elapsed_micros = elapsed.as_micros();
        let avg_micros = elapsed_micros as f64 / num_ticks as f64;
        let fps_equiv = if avg_micros > 0.0 {
            1_000_000.0 / avg_micros
        } else {
            0.0
        };

        SessionReport {
            total_ticks: self.tick,
            final_hash: self.compute_master_hash(),
            total_npcs: self.npcs.len(),
            loaded_chunks: self.chunk_mgr.loaded_count(),
            total_chunks_generated: self.chunk_mgr.total_generated,
            total_logistics_items: self.logistics.total_belt_items(),
            combat_units_alive: self.arena.alive_count(),
            elapsed_micros,
            avg_micros_per_tick: avg_micros,
            fps_equivalent: fps_equiv,
        }
    }
}
