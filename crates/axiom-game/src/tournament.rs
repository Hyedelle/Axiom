//! Pure integer, zero-float Tournament Bracket progression engine.
//!
//! Provides deterministic single-elimination knockout brackets (4-fighter, 8-fighter),
//! automatic match seeding, winner advancement, and FNV-1a state consensus.

pub const MAX_TOURNAMENT_MATCHES: usize = 7; // Up to 8-fighter bracket (4 + 2 + 1)
pub const NO_NEXT_MATCH: u8 = 0xFF;

/// Status of a tournament match.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchStatus {
    Pending = 0,
    Ready = 1,
    InProgress = 2,
    Completed = 3,
}

/// A tournament contestant with fixed-size UTF-8 name and seed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TournamentFighter {
    pub id: u8,
    pub seed: u8,
    pub name: [u8; 16],
    pub wins: u8,
    pub losses: u8,
}

impl TournamentFighter {
    pub const fn new(id: u8, seed: u8, name_str: &[u8]) -> Self {
        let mut name = [0u8; 16];
        let mut i = 0;
        while i < name_str.len() && i < 16 {
            name[i] = name_str[i];
            i += 1;
        }
        Self {
            id,
            seed,
            name,
            wins: 0,
            losses: 0,
        }
    }

    pub fn name_as_str(&self) -> &str {
        let len = self.name.iter().position(|&b| b == 0).unwrap_or(16);
        core::str::from_utf8(&self.name[..len]).unwrap_or("UNKNOWN")
    }
}

/// A single match node within the tournament bracket tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TournamentMatch {
    pub match_id: u8,
    pub round: u8, // 0: Quarterfinal, 1: Semifinal, 2: Grand Final
    pub fighter_a: Option<TournamentFighter>,
    pub fighter_b: Option<TournamentFighter>,
    pub winner: Option<TournamentFighter>,
    pub score_a: u8,
    pub score_b: u8,
    pub status: MatchStatus,
    pub next_match_id: u8,
    pub next_slot: u8, // 0: slot A, 1: slot B
}

impl TournamentMatch {
    pub const fn empty(match_id: u8, round: u8, next_match_id: u8, next_slot: u8) -> Self {
        Self {
            match_id,
            round,
            fighter_a: None,
            fighter_b: None,
            winner: None,
            score_a: 0,
            score_b: 0,
            status: MatchStatus::Pending,
            next_match_id,
            next_slot,
        }
    }
}

/// 0-Float Deterministic Tournament Engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TournamentEngine<const N: usize = 4> {
    pub fighters: [Option<TournamentFighter>; N],
    pub matches: [TournamentMatch; MAX_TOURNAMENT_MATCHES],
    pub num_matches: usize,
    pub champion: Option<TournamentFighter>,
    pub current_match_idx: usize,
}

impl TournamentEngine<4> {
    /// Creates a 4-fighter knockout tournament bracket.
    ///
    /// Layout:
    /// - Match 0: Fighter 0 vs Fighter 1 -> Winner to Match 2 (Slot A)
    /// - Match 1: Fighter 2 vs Fighter 3 -> Winner to Match 2 (Slot B)
    /// - Match 2: Grand Final (Winner of M0 vs Winner of M1) -> Champion
    pub fn new_4_bracket(
        f0: TournamentFighter,
        f1: TournamentFighter,
        f2: TournamentFighter,
        f3: TournamentFighter,
    ) -> Self {
        let mut matches = [TournamentMatch::empty(0, 0, NO_NEXT_MATCH, 0); MAX_TOURNAMENT_MATCHES];

        // Semifinal 1
        matches[0] = TournamentMatch {
            match_id: 0,
            round: 0,
            fighter_a: Some(f0),
            fighter_b: Some(f1),
            winner: None,
            score_a: 0,
            score_b: 0,
            status: MatchStatus::Ready,
            next_match_id: 2,
            next_slot: 0,
        };

        // Semifinal 2
        matches[1] = TournamentMatch {
            match_id: 1,
            round: 0,
            fighter_a: Some(f2),
            fighter_b: Some(f3),
            winner: None,
            score_a: 0,
            score_b: 0,
            status: MatchStatus::Ready,
            next_match_id: 2,
            next_slot: 1,
        };

        // Grand Final
        matches[2] = TournamentMatch {
            match_id: 2,
            round: 1,
            fighter_a: None,
            fighter_b: None,
            winner: None,
            score_a: 0,
            score_b: 0,
            status: MatchStatus::Pending,
            next_match_id: NO_NEXT_MATCH,
            next_slot: 0,
        };

        Self {
            fighters: [Some(f0), Some(f1), Some(f2), Some(f3)],
            matches,
            num_matches: 3,
            champion: None,
            current_match_idx: 0,
        }
    }
}

impl<const N: usize> TournamentEngine<N> {
    /// Records the outcome of a match and automatically advances the winner.
    pub fn record_match_result(
        &mut self,
        match_id: u8,
        score_a: u8,
        score_b: u8,
    ) -> Result<TournamentFighter, &'static str> {
        let idx = match_id as usize;
        if idx >= self.num_matches {
            return Err("Match ID out of bounds");
        }

        let m = &mut self.matches[idx];
        if m.status == MatchStatus::Completed {
            return Err("Match already completed");
        }

        let fa = m.fighter_a.ok_or("Fighter A not present")?;
        let fb = m.fighter_b.ok_or("Fighter B not present")?;

        let mut winner = if score_a >= score_b { fa } else { fb };
        winner.wins += 1;

        m.winner = Some(winner);
        m.score_a = score_a;
        m.score_b = score_b;
        m.status = MatchStatus::Completed;

        let next_id = m.next_match_id;
        let next_slot = m.next_slot;

        if next_id == NO_NEXT_MATCH {
            // Grand final concluded!
            self.champion = Some(winner);
        } else {
            // Advance winner to the downstream match node
            let next_idx = next_id as usize;
            if next_idx < self.num_matches {
                let next_m = &mut self.matches[next_idx];
                if next_slot == 0 {
                    next_m.fighter_a = Some(winner);
                } else {
                    next_m.fighter_b = Some(winner);
                }

                // If both fighters are now set, mark match as Ready
                if next_m.fighter_a.is_some() && next_m.fighter_b.is_some() {
                    next_m.status = MatchStatus::Ready;
                }
            }
        }

        // Advance current match pointer to next ready match
        for i in 0..self.num_matches {
            if self.matches[i].status == MatchStatus::Ready {
                self.current_match_idx = i;
                break;
            }
        }

        Ok(winner)
    }

    /// Computes deterministic 64-bit FNV-1a hash of the entire tournament state.
    pub fn compute_hash(&self) -> u64 {
        let mut hash: u64 = 0xcbf29ce484222325;
        const FNV_PRIME: u64 = 0x100000001b3;

        for m in &self.matches[..self.num_matches] {
            hash ^= m.match_id as u64;
            hash = hash.wrapping_mul(FNV_PRIME);
            hash ^= m.status as u8 as u64;
            hash = hash.wrapping_mul(FNV_PRIME);
            hash ^= m.score_a as u64;
            hash = hash.wrapping_mul(FNV_PRIME);
            hash ^= m.score_b as u64;
            hash = hash.wrapping_mul(FNV_PRIME);

            if let Some(w) = &m.winner {
                hash ^= w.id as u64;
                hash = hash.wrapping_mul(FNV_PRIME);
            }
        }

        if let Some(champ) = &self.champion {
            hash ^= champ.id as u64;
            hash = hash.wrapping_mul(FNV_PRIME);
        }

        hash
    }
}

// ---------------------------------------------------------------------------
// Automated Tournament Broadcast & Spectator Stream Director
// ---------------------------------------------------------------------------

use crate::boxing::{ActionKind, BoxingRingEngine};

/// Broadcast phase of the automated tournament stream.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BroadcastPhase {
    /// Idle / setup
    Idle = 0,
    /// Live match fighting
    MatchActive = 1,
    /// Match concluded (KO or decision), displaying victor banner
    MatchConcluded = 2,
    /// Grand champion crowned sequence
    GrandChampionCrowned = 3,
}

/// 64-Byte L1 cache aligned real-time telemetry frame for tournament spectators.
#[repr(C, align(64))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TournamentBroadcastTelemetry {
    pub current_match_id: u8,
    pub round: u8,
    pub broadcast_phase: u8,
    pub fighter_a_id: u8,
    pub fighter_b_id: u8,
    pub fighter_a_hp: u8,
    pub fighter_b_hp: u8,
    pub fighter_a_heat: u8,
    pub fighter_b_heat: u8,
    pub winner_id: u8,
    pub champion_id: u8,
    pub spectator_count: u32,
    pub match_frame: u32,
    pub state_hash: u64,
}

impl Default for TournamentBroadcastTelemetry {
    fn default() -> Self {
        Self {
            current_match_id: 0,
            round: 0,
            broadcast_phase: 0,
            fighter_a_id: 0,
            fighter_b_id: 1,
            fighter_a_hp: 100,
            fighter_b_hp: 100,
            fighter_a_heat: 0,
            fighter_b_heat: 0,
            winner_id: 0xFF,
            champion_id: 0xFF,
            spectator_count: 1,
            match_frame: 0,
            state_hash: 0,
        }
    }
}

/// Autonomous tournament match runner and spectator broadcast director.
#[derive(Debug, Clone)]
pub struct TournamentDirector {
    pub tournament: TournamentEngine<4>,
    pub ring: BoxingRingEngine,
    pub phase: BroadcastPhase,
    pub phase_timer: u32,
    pub spectator_count: u32,
    pub match_frame: u32,
    pub last_winner_id: u8,
}

impl TournamentDirector {
    /// Creates a new autonomous tournament broadcast session for 4 fighters.
    pub fn new_4(
        f0: TournamentFighter,
        f1: TournamentFighter,
        f2: TournamentFighter,
        f3: TournamentFighter,
    ) -> Self {
        let tournament = TournamentEngine::new_4_bracket(f0, f1, f2, f3);
        let ring = BoxingRingEngine::new(0xABCD_EF01);

        Self {
            tournament,
            ring,
            phase: BroadcastPhase::MatchActive,
            phase_timer: 0,
            spectator_count: 12, // Default audience
            match_frame: 0,
            last_winner_id: 0xFF,
        }
    }

    /// Advances the tournament broadcast simulation by 1 tick.
    /// Returns an event code:
    /// - 0: normal step
    /// - 1: match starting / active
    /// - 2: match finished (winner decided)
    /// - 3: grand champion crowned
    pub fn step(&mut self) -> u8 {
        match self.phase {
            BroadcastPhase::Idle => 0,
            BroadcastPhase::MatchActive => {
                self.match_frame += 1;

                // Autonomous bot combat simulation
                let p0_action = match (self.match_frame * 7) % 60 {
                    10 => ActionKind::Jab,
                    25 => ActionKind::Cross,
                    40 => ActionKind::Hook,
                    55 => ActionKind::SlipLeft,
                    _ => ActionKind::Idle,
                };
                let p1_action = match (self.match_frame * 13) % 60 {
                    12 => ActionKind::Jab,
                    28 => ActionKind::Cross,
                    44 => ActionKind::Guard,
                    _ => ActionKind::Idle,
                };

                self.ring.step_pvp(p0_action, p1_action);

                let p0_hp = self.ring.player.hp;
                let p1_hp = self.ring.opponent.hp;

                // Match conclusion check (KO or 1200 frame limit)
                let match_over = p0_hp <= 0 || p1_hp <= 0 || self.match_frame >= 1200;

                if match_over {
                    let cur_idx = self.tournament.current_match_idx;
                    let score_a = p0_hp.clamp(0, 100) as u8;
                    let score_b = p1_hp.clamp(0, 100) as u8;
                    let winner = self
                        .tournament
                        .record_match_result(cur_idx as u8, score_a, score_b)
                        .unwrap_or(TournamentFighter::new(0, 0, b"UNKNOWN"));

                    self.last_winner_id = winner.id;

                    if self.tournament.champion.is_some() {
                        self.phase = BroadcastPhase::GrandChampionCrowned;
                        self.phase_timer = 300; // 5 seconds display
                        3
                    } else {
                        self.phase = BroadcastPhase::MatchConcluded;
                        self.phase_timer = 180; // 3 seconds transition
                        2
                    }
                } else {
                    0
                }
            }
            BroadcastPhase::MatchConcluded => {
                if self.phase_timer > 0 {
                    self.phase_timer -= 1;
                    0
                } else {
                    // Start next ready match
                    self.ring = BoxingRingEngine::new(0xABCD_EF01 ^ (self.match_frame as u64));
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

    /// Obtains current broadcast telemetry for downstream spectator relay.
    pub fn telemetry(&self) -> TournamentBroadcastTelemetry {
        let cur_m = &self.tournament.matches[self.tournament.current_match_idx];
        let fa_id = cur_m.fighter_a.map(|f| f.id).unwrap_or(0);
        let fb_id = cur_m.fighter_b.map(|f| f.id).unwrap_or(1);

        TournamentBroadcastTelemetry {
            current_match_id: cur_m.match_id,
            round: cur_m.round,
            broadcast_phase: self.phase as u8,
            fighter_a_id: fa_id,
            fighter_b_id: fb_id,
            fighter_a_hp: self.ring.player.hp.clamp(0, 100) as u8,
            fighter_b_hp: self.ring.opponent.hp.clamp(0, 100) as u8,
            fighter_a_heat: self.ring.player.heat.min(100) as u8,
            fighter_b_heat: self.ring.opponent.heat.min(100) as u8,
            winner_id: self.last_winner_id,
            champion_id: self.tournament.champion.map(|c| c.id).unwrap_or(0xFF),
            spectator_count: self.spectator_count,
            match_frame: self.match_frame,
            state_hash: self.tournament.compute_hash(),
        }
    }
}
