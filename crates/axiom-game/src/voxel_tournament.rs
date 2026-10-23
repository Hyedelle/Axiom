//! # 3D Voxel Destructible Arena Autonomous Championship Director
//!
//! Integrates 0-Float quantized neural AI fighter inference (`NeuralFighterController`),
//! destructible voxel terrain geometry (`VoxelVolume16`), and single-elimination
//! tournament progression (`TournamentEngine<4>`) into an autonomous championship engine.
//!
//! Fully deterministic, zero floating-point arithmetic, cache-friendly 64-byte L1 telemetry.

use axiom_core::voxel_destruct::{VoxelVolume16, VOXEL_STONE};
use axiom_core::voxel_shard::VoxelShardPool;
use axiom_core::neural_ai::NeuralAiWeights;
use crate::boxing::{ActionKind, BoxingEvent, BoxingRingEngine};
use crate::neural_fighter::NeuralFighterController;
use crate::tournament::{BroadcastPhase, TournamentEngine, TournamentFighter};

/// 64-Byte L1 Cache Line Aligned Voxel Championship Telemetry.
#[repr(C, align(64))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VoxelChampionshipTelemetry {
    pub match_id: u8,
    pub round: u8,
    pub broadcast_phase: u8,
    pub fighter_a_id: u8,
    pub fighter_b_id: u8,
    pub fighter_a_hp: u8,
    pub fighter_b_hp: u8,
    pub winner_id: u8,

    pub champion_id: u8,
    pub _reserved1: u8,
    pub damaged_voxels: u16,
    pub solid_voxels: u16,
    pub craters_carved: u16,

    pub debris_count: u16,
    pub _reserved2: u16,
    pub match_frame: u32,

    pub state_hash: u64,
    pub arena_checksum: u64,

    pub spectator_count: u32,
    pub reserved_flags: u32,

    pub final_score_a: u16,
    pub final_score_b: u16,
    pub _pad: [u8; 12],
}

const _: () = assert!(core::mem::size_of::<VoxelChampionshipTelemetry>() == 64);

/// Autonomous 3D Voxel Destructible Arena Championship Engine.
pub struct VoxelChampionshipDirector {
    pub tournament: TournamentEngine<4>,
    pub ring: BoxingRingEngine,
    pub arena: VoxelVolume16,
    pub shards: VoxelShardPool<128>,
    pub controller_a: NeuralFighterController,
    pub controller_b: NeuralFighterController,
    pub phase: BroadcastPhase,
    pub phase_timer: u32,
    pub match_frame: u32,
    pub craters_carved: u16,
    pub debris_count: u16,
    pub spectator_count: u32,
    pub last_winner_id: u8,
}

impl VoxelChampionshipDirector {
    /// Creates a new autonomous 4-fighter championship on a destructible voxel arena.
    pub fn new(
        f0: TournamentFighter,
        f1: TournamentFighter,
        f2: TournamentFighter,
        f3: TournamentFighter,
        seed: u64,
    ) -> Self {
        let tournament = TournamentEngine::new_4_bracket(f0, f1, f2, f3);
        let ring = BoxingRingEngine::new(seed);
        let arena = VoxelVolume16::new_solid(VOXEL_STONE);
        let shards = VoxelShardPool::new(seed);

        let mut weights_b = NeuralAiWeights::default();
        weights_b.b1[0] = 30;
        weights_b.b1[1] = -25;
        weights_b.b2[5] = 40; // Bias toward Cross

        Self {
            tournament,
            ring,
            arena,
            shards,
            controller_a: NeuralFighterController::new(NeuralAiWeights::default()),
            controller_b: NeuralFighterController::new(weights_b),
            phase: BroadcastPhase::MatchActive,
            phase_timer: 0,
            match_frame: 0,
            craters_carved: 0,
            debris_count: 0,
            spectator_count: 48,
            last_winner_id: 0xFF,
        }
    }

    /// Advances the autonomous voxel championship simulation by 1 tick.
    /// Returns an event code:
    /// - 0: normal combat tick
    /// - 1: crater carved by heavy impact
    /// - 2: match concluded (winner decided)
    /// - 3: grand champion crowned
    pub fn step(&mut self) -> u8 {
        match self.phase {
            BroadcastPhase::Idle => 0,
            BroadcastPhase::MatchActive => {
                self.match_frame += 1;

                // 1. Quantized Neural AI Observation & Deterministic Action Selection
                let neural_action = self.controller_a.decide_player_action(&self.ring);

                let action_a = match (self.match_frame * 7) % 60 {
                    10 => ActionKind::Jab,
                    25 => ActionKind::Cross,
                    40 => ActionKind::Hook,
                    50 => ActionKind::Finisher,
                    56 => neural_action,
                    _ => ActionKind::Idle,
                };

                let action_b = match (self.match_frame * 13) % 60 {
                    12 => ActionKind::Jab,
                    26 => ActionKind::Cross,
                    42 => ActionKind::Hook,
                    52 => ActionKind::Guard,
                    _ => ActionKind::Idle,
                };

                // 2. Step deterministic ring physics and capture events
                let (ev0, ev1) = self.ring.step_pvp(action_a, action_b);

                // Advance ballistic physics for active voxel shards
                self.shards.step();

                // 3. Dynamic Terrain Cratering on heavy combat strikes
                let mut crater_event = false;
                let is_heavy_0 = matches!(ev0, BoxingEvent::PlayerPunched { .. } | BoxingEvent::OpponentDowned | BoxingEvent::PlayerBlocked { .. });
                let is_heavy_1 = matches!(ev1, BoxingEvent::PlayerStruck { .. } | BoxingEvent::PlayerDowned | BoxingEvent::PlayerBlocked { .. });

                if is_heavy_0 {
                    let crater_x = ((self.match_frame * 3) % 12 + 2) as i16;
                    let crater_z = ((self.match_frame * 7) % 12 + 2) as i16;
                    let radius: i16 = if action_a == ActionKind::Finisher { 4 } else { 2 };
                    self.arena.carve_crater_with_shards(crater_x, crater_z, 2, radius, &mut self.shards, [0, -45000, 0], 2048);
                    self.craters_carved += 1;
                    self.debris_count = self.shards.active_count as u16;
                    crater_event = true;
                }
                if is_heavy_1 {
                    let crater_x = ((self.match_frame * 5) % 12 + 2) as i16;
                    let crater_z = ((self.match_frame * 11) % 12 + 2) as i16;
                    let radius: i16 = if action_b == ActionKind::Finisher { 4 } else { 2 };
                    self.arena.carve_crater_with_shards(crater_x, crater_z, 2, radius, &mut self.shards, [0, -45000, 0], 2048);
                    self.craters_carved += 1;
                    self.debris_count = self.shards.active_count as u16;
                    crater_event = true;
                }

                // 4. Match Conclusion Check (KO or 1200 frame limit)
                let cur_p0_hp = self.ring.player.hp;
                let cur_p1_hp = self.ring.opponent.hp;
                let match_over = cur_p0_hp <= 0 || cur_p1_hp <= 0 || self.match_frame >= 1200;
                if match_over {
                    let cur_idx = self.tournament.current_match_idx;
                    let score_a = cur_p0_hp.clamp(0, 100) as u8;
                    let score_b = cur_p1_hp.clamp(0, 100) as u8;
                    let winner = self
                        .tournament
                        .record_match_result(cur_idx as u8, score_a, score_b)
                        .unwrap_or(TournamentFighter::new(0, 0, b"UNKNOWN"));

                    self.last_winner_id = winner.id;

                    if self.tournament.champion.is_some() {
                        self.phase = BroadcastPhase::GrandChampionCrowned;
                        self.phase_timer = 300;
                        3
                    } else {
                        self.phase = BroadcastPhase::MatchConcluded;
                        self.phase_timer = 120;
                        2
                    }
                } else if crater_event {
                    1
                } else {
                    0
                }
            }
            BroadcastPhase::MatchConcluded => {
                if self.phase_timer > 0 {
                    self.phase_timer -= 1;
                    0
                } else {
                    // Next match setup with fresh seed and continuous persistent arena destruction
                    let next_seed = 0xABCD_EF01 ^ ((self.match_frame as u64) << 16);
                    self.ring = BoxingRingEngine::new(next_seed);
                    self.match_frame = 0;
                    self.phase = BroadcastPhase::MatchActive;
                    1
                }
            }
            BroadcastPhase::GrandChampionCrowned => {
                if self.phase_timer > 0 {
                    self.phase_timer -= 1;
                }
                0
            }
        }
    }

    /// Computes full championship 64-bit FNV-1a deterministic hash.
    pub fn compute_consensus_hash(&self) -> u64 {
        let mut hash: u64 = 0xCBF2_9CE4_8422_2325;
        let t_hash = self.tournament.compute_hash();
        hash ^= t_hash;
        hash = hash.wrapping_mul(0x100_0000_01B3);

        let v_damaged = self.arena.damaged_count as u64;
        hash ^= v_damaged;
        hash = hash.wrapping_mul(0x100_0000_01B3);

        let v_solid = self.arena.total_solid as u64;
        hash ^= v_solid;
        hash = hash.wrapping_mul(0x100_0000_01B3);

        let craters = self.craters_carved as u64;
        hash ^= craters;
        hash = hash.wrapping_mul(0x100_0000_01B3);

        let p0_hp = (self.ring.player.hp as u64) & 0xFFFF;
        let p1_hp = (self.ring.opponent.hp as u64) & 0xFFFF;
        hash ^= (p0_hp << 16) | p1_hp;
        hash = hash.wrapping_mul(0x100_0000_01B3);

        let shard_hash = self.shards.compute_consensus_hash();
        hash ^= shard_hash;
        hash = hash.wrapping_mul(0x100_0000_01B3);

        hash
    }

    /// Obtains 64-byte L1 aligned championship telemetry.
    pub fn telemetry(&self) -> VoxelChampionshipTelemetry {
        let cur_m = &self.tournament.matches[self.tournament.current_match_idx];
        let fa_id = cur_m.fighter_a.map(|f| f.id).unwrap_or(0);
        let fb_id = cur_m.fighter_b.map(|f| f.id).unwrap_or(1);

        VoxelChampionshipTelemetry {
            match_id: cur_m.match_id,
            round: cur_m.round,
            broadcast_phase: self.phase as u8,
            fighter_a_id: fa_id,
            fighter_b_id: fb_id,
            fighter_a_hp: self.ring.player.hp.clamp(0, 100) as u8,
            fighter_b_hp: self.ring.opponent.hp.clamp(0, 100) as u8,
            winner_id: self.last_winner_id,
            champion_id: self.tournament.champion.map(|c| c.id).unwrap_or(0xFF),
            _reserved1: 0,
            damaged_voxels: self.arena.damaged_count as u16,
            solid_voxels: self.arena.total_solid as u16,
            craters_carved: self.craters_carved,
            debris_count: self.debris_count,
            _reserved2: 0,
            match_frame: self.match_frame,
            state_hash: self.compute_consensus_hash(),
            arena_checksum: (self.arena.damaged_count as u64) ^ ((self.arena.total_solid as u64) << 32),
            spectator_count: self.spectator_count,
            reserved_flags: 0,
            final_score_a: self.ring.player.hp.max(0) as u16,
            final_score_b: self.ring.opponent.hp.max(0) as u16,
            _pad: [0u8; 12],
        }
    }
}
