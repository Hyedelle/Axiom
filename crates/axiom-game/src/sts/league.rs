//! 4-Robot League, round-robin match scheduler, standings table, and playoff ladder.
//!
//! Models D-308, D-315, and D-318 from steel-the-show. Zero-float pure integer logic.

use super::robot::RobotAssembly;

/// 4 League contenders + 1 Champion.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ContenderId {
    Elpis = 0,    // Player (The Rising Contender)
    Tinker = 1,   // Opponent A (Speed / Outboxer)
    Cipher = 2,   // Opponent B (Precision / Controller)
    Pike = 3,     // Opponent C (Heavy / Slugger)
    Theseus = 4,  // The Champion of the Foundry
}

impl ContenderId {
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Elpis => "ELPIS (Player)",
            Self::Tinker => "TINKER",
            Self::Cipher => "CIPHER",
            Self::Pike => "PIKE",
            Self::Theseus => "THESEUS (Champion)",
        }
    }

    pub fn default_assembly(&self) -> RobotAssembly {
        match self {
            Self::Elpis => RobotAssembly::elpis_starter(),
            Self::Tinker => RobotAssembly::tinker_assembly(),
            Self::Cipher => RobotAssembly::cipher_assembly(),
            Self::Pike => RobotAssembly::pike_assembly(),
            Self::Theseus => RobotAssembly::theseus_champion_assembly(),
        }
    }
}

/// A scheduled league match between two contenders.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LeagueMatch {
    pub round: u8,
    pub home: ContenderId,
    pub away: ContenderId,
    pub completed: bool,
    pub winner: Option<ContenderId>,
    pub home_score: u8,
    pub away_score: u8,
    pub home_ko: bool,
    pub away_ko: bool,
}

/// Standing record for one league robot.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StandingRecord {
    pub contender: ContenderId,
    pub matches_played: u8,
    pub wins: u8,
    pub draws: u8,
    pub losses: u8,
    pub points: u8, // Win = 3, Draw = 1, Loss = 0
    pub kos: u8,
    pub score_diff: i16,
}

impl StandingRecord {
    pub const fn new(contender: ContenderId) -> Self {
        Self {
            contender,
            matches_played: 0,
            wins: 0,
            draws: 0,
            losses: 0,
            points: 0,
            kos: 0,
            score_diff: 0,
        }
    }
}

/// Current state of the season playoff ladder (D-315).
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayoffState {
    LeagueInProgress = 0,
    DeciderReady = 1,
    TitleMatchReady = 2,
    ChampionshipWon = 3,
    EliminatedDefeat = 4,
}

/// 4-Robot League manager orchestrating the 3 league rounds and the playoff ladder.
#[repr(C, align(64))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StsLeague {
    pub matches: [LeagueMatch; 6],
    pub current_match_idx: usize,
    pub standings: [StandingRecord; 4],
    pub playoff_state: PlayoffState,
    pub decider_winner: Option<ContenderId>,
    pub title_winner: Option<ContenderId>,
}

impl Default for StsLeague {
    fn default() -> Self {
        Self::new()
    }
}

impl StsLeague {
    /// Creates the standard 4-robot league schedule (3 rounds, 6 matches).
    pub fn new() -> Self {
        let matches = [
            // Round 1
            LeagueMatch {
                round: 1,
                home: ContenderId::Elpis,
                away: ContenderId::Tinker,
                completed: false,
                winner: None,
                home_score: 0,
                away_score: 0,
                home_ko: false,
                away_ko: false,
            },
            LeagueMatch {
                round: 1,
                home: ContenderId::Cipher,
                away: ContenderId::Pike,
                completed: false,
                winner: None,
                home_score: 0,
                away_score: 0,
                home_ko: false,
                away_ko: false,
            },
            // Round 2
            LeagueMatch {
                round: 2,
                home: ContenderId::Elpis,
                away: ContenderId::Cipher,
                completed: false,
                winner: None,
                home_score: 0,
                away_score: 0,
                home_ko: false,
                away_ko: false,
            },
            LeagueMatch {
                round: 2,
                home: ContenderId::Tinker,
                away: ContenderId::Pike,
                completed: false,
                winner: None,
                home_score: 0,
                away_score: 0,
                home_ko: false,
                away_ko: false,
            },
            // Round 3
            LeagueMatch {
                round: 3,
                home: ContenderId::Elpis,
                away: ContenderId::Pike,
                completed: false,
                winner: None,
                home_score: 0,
                away_score: 0,
                home_ko: false,
                away_ko: false,
            },
            LeagueMatch {
                round: 3,
                home: ContenderId::Tinker,
                away: ContenderId::Cipher,
                completed: false,
                winner: None,
                home_score: 0,
                away_score: 0,
                home_ko: false,
                away_ko: false,
            },
        ];

        let standings = [
            StandingRecord::new(ContenderId::Elpis),
            StandingRecord::new(ContenderId::Tinker),
            StandingRecord::new(ContenderId::Cipher),
            StandingRecord::new(ContenderId::Pike),
        ];

        Self {
            matches,
            current_match_idx: 0,
            standings,
            playoff_state: PlayoffState::LeagueInProgress,
            decider_winner: None,
            title_winner: None,
        }
    }

    /// Records the result of a match and updates standings.
    pub fn record_match_result(
        &mut self,
        match_idx: usize,
        home_score: u8,
        away_score: u8,
        home_ko: bool,
        away_ko: bool,
    ) {
        if match_idx >= 6 {
            return;
        }
        let m = &mut self.matches[match_idx];
        m.completed = true;
        m.home_score = home_score;
        m.away_score = away_score;
        m.home_ko = home_ko;
        m.away_ko = away_ko;

        let winner = if home_ko || home_score > away_score {
            Some(m.home)
        } else if away_ko || away_score > home_score {
            Some(m.away)
        } else {
            None
        };
        m.winner = winner;

        let home_idx = m.home as usize;
        let away_idx = m.away as usize;

        // Update home stats
        self.standings[home_idx].matches_played += 1;
        self.standings[home_idx].score_diff += (home_score as i16) - (away_score as i16);
        if home_ko { self.standings[home_idx].kos += 1; }

        // Update away stats
        self.standings[away_idx].matches_played += 1;
        self.standings[away_idx].score_diff += (away_score as i16) - (home_score as i16);
        if away_ko { self.standings[away_idx].kos += 1; }

        if let Some(w) = winner {
            if w == m.home {
                self.standings[home_idx].wins += 1;
                self.standings[home_idx].points += 3;
                self.standings[away_idx].losses += 1;
            } else {
                self.standings[away_idx].wins += 1;
                self.standings[away_idx].points += 3;
                self.standings[home_idx].losses += 1;
            }
        } else {
            // Draw
            self.standings[home_idx].draws += 1;
            self.standings[home_idx].points += 1;
            self.standings[away_idx].draws += 1;
            self.standings[away_idx].points += 1;
        }

        self.current_match_idx = match_idx + 1;

        // Check if all 6 league matches are completed
        if self.current_match_idx >= 6 {
            self.evaluate_league_conclusion();
        }
    }

    /// Returns the 4 standings records sorted by:
    /// (Points DESC, KOs DESC, ScoreDiff DESC, ContenderId ASC).
    pub fn sorted_standings(&self) -> [StandingRecord; 4] {
        let mut sorted = self.standings;
        sorted.sort_by(|a, b| {
            b.points.cmp(&a.points)
                .then_with(|| b.kos.cmp(&a.kos))
                .then_with(|| b.score_diff.cmp(&a.score_diff))
                .then_with(|| (a.contender as u8).cmp(&(b.contender as u8)))
        });
        sorted
    }

    /// Evaluates promotion to playoffs (D-308 / D-315).
    ///
    /// 3rd & 4th are eliminated (Game Over).
    /// 1st & 2nd play in the Decider Match.
    fn evaluate_league_conclusion(&mut self) {
        let sorted = self.sorted_standings();
        let p1 = sorted[0].contender;
        let p2 = sorted[1].contender;

        // If player (Elpis) is not in Top 2, player is eliminated.
        if p1 != ContenderId::Elpis && p2 != ContenderId::Elpis {
            self.playoff_state = PlayoffState::EliminatedDefeat;
        } else {
            self.playoff_state = PlayoffState::DeciderReady;
        }
    }

    /// Records the result of the 1st vs 2nd Decider Match.
    pub fn record_decider_result(&mut self, winner: ContenderId) {
        debug_assert!(self.playoff_state == PlayoffState::DeciderReady);
        self.decider_winner = Some(winner);
        if winner == ContenderId::Elpis {
            self.playoff_state = PlayoffState::TitleMatchReady;
        } else {
            self.playoff_state = PlayoffState::EliminatedDefeat;
        }
    }

    /// Records the result of the Title Match vs THESEUS.
    pub fn record_title_result(&mut self, winner: ContenderId) {
        debug_assert!(self.playoff_state == PlayoffState::TitleMatchReady);
        self.title_winner = Some(winner);
        if winner == ContenderId::Elpis {
            self.playoff_state = PlayoffState::ChampionshipWon;
        } else {
            self.playoff_state = PlayoffState::EliminatedDefeat;
        }
    }
}
