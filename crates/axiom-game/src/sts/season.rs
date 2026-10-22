//! Complete Steel The Show Season Runner.
//!
//! Orchestrates the Garage, Assembly Synergies, Part Upgrades, Inbound Reverse-Engineering Draft,
//! 4-Robot League, and the Champion Title Match against THESEUS.
//! Connects directly to the zero-float `BoxingRingEngine`.

use core::fmt;

use crate::boxing::{ActionKind, BoxingRingEngine};
use super::league::{ContenderId, PlayoffState, StsLeague};
use super::part::{Part, Slot, UpgradeBonus};
use super::robot::{RobotAssembly, SetSynergy};

/// Phase of the Steel The Show Season lifecycle.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StsPhase {
    Garage = 0,             // Tuning assembly, equipping parts, synthesizing duplicates
    Combat = 1,             // Active punch-out boxing match in progress
    InboundDraft = 2,       // 3-choice reverse-engineering draft after each match
    PlayoffDeciderReady = 3,// Ready for 1st vs 2nd Decider Match
    TitleMatchReady = 4,    // Ready for Champion Title Match vs THESEUS
    Victory = 5,            // Championship Victory (Beat THESEUS)
    Defeat = 6,             // Eliminated / Lost in playoffs
}

/// Inbound draft choices offered to the player (3 parts sampled from opponent).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InboundDraftState {
    pub choices: [Option<Part>; 3],
    pub opponent: ContenderId,
}

impl InboundDraftState {
    pub const fn empty() -> Self {
        Self {
            choices: [None; 3],
            opponent: ContenderId::Tinker,
        }
    }
}

/// 64-byte aligned Steel The Show Full Season Simulator.
#[repr(C, align(64))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StsSeasonEngine {
    pub player: RobotAssembly,
    pub current_opponent: ContenderId,
    pub opponent_assembly: RobotAssembly,
    pub ring: BoxingRingEngine,
    pub league: StsLeague,
    pub phase: StsPhase,
    pub draft: InboundDraftState,
    pub current_league_match_idx: usize,
    pub match_ticks: u32,
    pub rng_seed: u64,
    pub matches_won: u8,
    pub matches_lost: u8,
    pub _pad: [u8; 2],
}

impl StsSeasonEngine {
    /// Initializes a brand new season run with starting ELPIS mecha.
    pub fn new(seed: u64) -> Self {
        let player = RobotAssembly::elpis_starter();
        let first_opponent = ContenderId::Tinker;
        let opp_assembly = first_opponent.default_assembly();
        let ring = BoxingRingEngine::new(seed);
        let league = StsLeague::new();

        let mut engine = Self {
            player,
            current_opponent: first_opponent,
            opponent_assembly: opp_assembly,
            ring,
            league,
            phase: StsPhase::Garage,
            draft: InboundDraftState::empty(),
            current_league_match_idx: 0,
            match_ticks: 0,
            rng_seed: seed.wrapping_add(0x9E3779B97F4A7C15),
            matches_won: 0,
            matches_lost: 0,
            _pad: [0; 2],
        };
        engine.sync_ring_stats();
        engine
    }

    /// Next deterministic PRNG tick.
    fn next_rand(&mut self) -> u64 {
        self.rng_seed = self.rng_seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        self.rng_seed
    }

    /// Synchronizes assembly stats and active synergies into the underlying `BoxingRingEngine`.
    pub fn sync_ring_stats(&mut self) {
        // Player stats scaling:
        // Base HP = 100 + (Defense * 2)
        let player_max_hp = 100i32.saturating_add((self.player.total_defense() as i32).saturating_mul(2));
        self.ring.player.max_hp = player_max_hp;
        self.ring.player.hp = player_max_hp;

        // Opponent stats scaling
        let opp_max_hp = 100i32.saturating_add((self.opponent_assembly.total_defense() as i32).saturating_mul(2));
        self.ring.opponent.max_hp = opp_max_hp;
        self.ring.opponent.hp = opp_max_hp;

        // Reset heat and cooldown
        self.ring.player.heat = 0;
        self.ring.opponent.heat = 0;
    }

    /// Starts the next scheduled combat match.
    pub fn start_match(&mut self, opponent: ContenderId) {
        self.current_opponent = opponent;
        self.opponent_assembly = opponent.default_assembly();
        self.sync_ring_stats();
        self.match_ticks = 0;
        self.phase = StsPhase::Combat;
    }

    /// Steps the simulation by 1 frame (1/60th second).
    pub fn step_frame(&mut self, player_action: ActionKind) {
        if self.phase != StsPhase::Combat {
            return;
        }

        self.match_ticks = self.match_ticks.saturating_add(1);

        // Apply Synergy Buffs before stepping:
        let synergy = self.player.active_synergy();
        let is_finisher = player_action == ActionKind::Finisher;

        // 1. Steam Finisher Barrage: Halves finisher heat cost
        if synergy == SetSynergy::SteamBarrage && is_finisher {
            if self.ring.player.heat >= 10 {
                self.ring.player.heat = self.ring.player.heat.saturating_sub(10);
            }
        }

        let pre_opp_hp = self.ring.opponent.hp;
        let pre_player_hp = self.ring.player.hp;

        // Step combat engine
        self.ring.step(player_action);

        // 2. Gear Aim Lock: If player landed a counter hit, amplify damage
        if synergy == SetSynergy::GearAimLock && self.ring.opponent.hp < pre_opp_hp {
            let _dmg_dealt = pre_opp_hp.saturating_sub(self.ring.opponent.hp);
            // Extra critical damage bonus based on attack power
            let extra = (self.player.total_attack() / 4).max(1) as i32;
            self.ring.opponent.hp = self.ring.opponent.hp.saturating_sub(extra);
        }

        // 3. Hydraulic Counter Deflect: If player guarded, reflect 50% damage back to opponent
        if synergy == SetSynergy::HydraulicDeflect
            && self.ring.player.action.kind == ActionKind::Guard
            && self.ring.player.hp < pre_player_hp
        {
            let dmg_taken = pre_player_hp.saturating_sub(self.ring.player.hp);
            let reflected = (dmg_taken / 2).max(1);
            self.ring.opponent.hp = self.ring.opponent.hp.saturating_sub(reflected);
        }

        // Check Match Conclusion (KO or 60s timeout = 3600 frames)
        if self.ring.opponent.hp <= 0 || self.ring.player.hp <= 0 || self.match_ticks >= 3600 {
            self.handle_match_conclusion();
        }
    }

    /// Resolves combat outcome, updates league standings, and generates inbound draft.
    fn handle_match_conclusion(&mut self) {
        let player_won = self.ring.opponent.hp == 0 || self.ring.player.hp > self.ring.opponent.hp;
        let player_ko = self.ring.opponent.hp == 0;
        let opp_ko = self.ring.player.hp == 0;

        if player_won {
            self.matches_won += 1;
        } else {
            self.matches_lost += 1;
        }

        match self.league.playoff_state {
            PlayoffState::LeagueInProgress => {
                // Record match in league
                let home_ko = player_ko;
                let away_ko = opp_ko;
                let home_score = if player_won { 10 } else { 5 };
                let away_score = if player_won { 5 } else { 10 };

                self.league.record_match_result(
                    self.current_league_match_idx,
                    home_score,
                    away_score,
                    home_ko,
                    away_ko,
                );

                // Simulate the other parallel match in this round if applicable
                self.simulate_other_league_matches();

                // Generate Inbound Draft (3 choices from defeated/fought opponent)
                self.generate_inbound_draft(self.current_opponent);
                self.phase = StsPhase::InboundDraft;
            }
            PlayoffState::DeciderReady => {
                let winner = if player_won { ContenderId::Elpis } else { self.current_opponent };
                self.league.record_decider_result(winner);
                if winner == ContenderId::Elpis {
                    self.generate_inbound_draft(self.current_opponent);
                    self.phase = StsPhase::InboundDraft;
                } else {
                    self.phase = StsPhase::Defeat;
                }
            }
            PlayoffState::TitleMatchReady => {
                let winner = if player_won { ContenderId::Elpis } else { ContenderId::Theseus };
                self.league.record_title_result(winner);
                if winner == ContenderId::Elpis {
                    // Won Championship! Player drafts 1 of THESEUS's champion parts
                    self.generate_inbound_draft(ContenderId::Theseus);
                    self.phase = StsPhase::Victory;
                } else {
                    self.phase = StsPhase::Defeat;
                }
            }
            _ => {}
        }
    }

    /// Simulates off-screen league matches between AI contenders.
    fn simulate_other_league_matches(&mut self) {
        // Parallel matches in rounds 1, 2, 3:
        // Round 1 (match 1): Cipher vs Pike
        // Round 2 (match 3): Tinker vs Pike
        // Round 3 (match 5): Tinker vs Cipher
        let ai_match_idx = match self.current_league_match_idx {
            0 => 1,
            2 => 3,
            4 => 5,
            _ => return,
        };

        let roll = self.next_rand();
        let home_wins = (roll % 2) == 0;
        let home_score = if home_wins { 10 } else { 6 };
        let away_score = if home_wins { 6 } else { 10 };
        let home_ko = (roll % 4) == 0;
        let away_ko = !home_ko && ((roll % 4) == 1);

        self.league.record_match_result(ai_match_idx, home_score, away_score, home_ko, away_ko);
    }

    /// Generates 3 inbound draft choices from an opponent's parts (D-282 / D-283).
    fn generate_inbound_draft(&mut self, opp: ContenderId) {
        let opp_assembly = opp.default_assembly();
        let opp_parts = [
            opp_assembly.head,
            opp_assembly.core,
            opp_assembly.arm,
            opp_assembly.leg,
        ];

        let seed = self.next_rand();
        let skip_idx = (seed % 4) as usize; // Pick 3 out of 4

        let mut choices = [None; 3];
        let mut c = 0;
        for i in 0..4 {
            if i != skip_idx && c < 3 {
                choices[c] = Some(opp_parts[i]);
                c += 1;
            }
        }

        self.draft = InboundDraftState {
            choices,
            opponent: opp,
        };
    }

    /// Selects an inbound part from the 3 choices and stores it into the player's inventory.
    pub fn select_inbound_part(&mut self, choice_idx: usize) -> bool {
        if self.phase != StsPhase::InboundDraft && self.phase != StsPhase::Victory {
            return false;
        }
        if choice_idx >= 3 {
            return false;
        }
        let Some(picked_part) = self.draft.choices[choice_idx] else {
            return false;
        };

        let added = self.player.add_to_inventory(picked_part);
        if added {
            self.draft.choices[choice_idx] = None;
            // Advance to Garage or next tournament stage
            if self.league.playoff_state == PlayoffState::DeciderReady {
                self.phase = StsPhase::PlayoffDeciderReady;
            } else if self.league.playoff_state == PlayoffState::TitleMatchReady {
                self.phase = StsPhase::TitleMatchReady;
            } else if self.league.playoff_state == PlayoffState::ChampionshipWon {
                self.phase = StsPhase::Victory;
            } else if self.league.playoff_state == PlayoffState::EliminatedDefeat {
                self.phase = StsPhase::Defeat;
            } else {
                // Continue league
                self.current_league_match_idx = self.current_league_match_idx.saturating_add(2);
                self.phase = StsPhase::Garage;
            }
        }
        added
    }

    /// Synthesizes two duplicate parts from inventory into an upgraded Tier 2 part (D-316/319).
    pub fn synthesize_inventory(&mut self, base_idx: usize, mat_idx: usize) -> Option<UpgradeBonus> {
        let seed = self.next_rand();
        self.player.synthesize_inventory(base_idx, mat_idx, seed)
    }

    /// Synthesizes an equipped part with a matching duplicate from inventory (D-316/319).
    pub fn synthesize_equipped(&mut self, slot: Slot, mat_idx: usize) -> Option<UpgradeBonus> {
        let seed = self.next_rand();
        self.player.synthesize_equipped(slot, mat_idx, seed)
    }
}

impl fmt::Display for StsSeasonEngine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "STS Season [Phase: {:?}, Opp: {:?}, Won: {}, Lost: {}] {}",
            self.phase,
            self.current_opponent,
            self.matches_won,
            self.matches_lost,
            self.player
        )
    }
}
