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
