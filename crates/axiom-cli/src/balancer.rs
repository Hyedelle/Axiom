//! Autonomous Monte Carlo Balance Simulation Engine (T-011).
//!
//! Executes thousands of headless auto-battler matches in seconds,
//! analyzing win rates, combat duration, and unit survivability without floats.

use axiom_core::combat::BattleArena;
use axiom_core::fixed::FixedVec2;
use axiom_core::pcg::Rng;

/// Statistics collected across thousands of simulated matches.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MatchStats {
    pub total_matches: u32,
    pub team_1_wins: u32,
    pub team_2_wins: u32,
    pub draws: u32,
    pub total_ticks: u64,
    pub team_1_survivors: u64,
    pub team_2_survivors: u64,
}

pub struct MonteCarloBalancer;

impl MonteCarloBalancer {
    /// Simulates `match_count` auto-battler matches with randomized starting positions.
    pub fn simulate_matches(match_count: usize, base_seed: u64) -> MatchStats {
        let mut stats = MatchStats {
            total_matches: match_count as u32,
            ..Default::default()
        };

        for m in 0..match_count {
            let mut rng =
                Rng::with_seed(base_seed.wrapping_add((m as u64).wrapping_mul(0x9E3779B97F4A7C15)));
            let mut arena = BattleArena::<32>::new();

            // Spawn 5 Blue Knights (Team 1): 100 HP, 20 ATK, 2 Range, 3 CD
            for i in 0..5 {
                let rx = rng.range_u32(0, 10) as i64;
                let ry = rng.range_u32(0, 20) as i64;
                arena.spawn_unit(
                    (i + 1) as u32,
                    1,
                    100,
                    20,
                    2,
                    3,
                    FixedVec2::from_ints(rx, ry),
                );
            }

            // Spawn 6 Red Goblins (Team 2 - Numerical advantage): 60 HP, 16 ATK, 2 Range, 2 CD
            for i in 0..6 {
                let rx = 30 + rng.range_u32(0, 10) as i64;
                let ry = rng.range_u32(0, 20) as i64;
                arena.spawn_unit(
                    (10 + i + 1) as u32,
                    2,
                    60,
                    16,
                    2,
                    2,
                    FixedVec2::from_ints(rx, ry),
                );
            }

            // Simulate match up to 100 ticks
            let mut match_ticks = 0;
            for _ in 0..100 {
                if arena.team_alive_count(1) == 0 || arena.team_alive_count(2) == 0 {
                    break;
                }
                arena.step();
                match_ticks += 1;
            }

            stats.total_ticks += match_ticks;
            let blue_alive = arena.team_alive_count(1) as u32;
            let red_alive = arena.team_alive_count(2) as u32;
            stats.team_1_survivors += blue_alive as u64;
            stats.team_2_survivors += red_alive as u64;

            if blue_alive > 0 && red_alive == 0 {
                stats.team_1_wins += 1;
            } else if red_alive > 0 && blue_alive == 0 {
                stats.team_2_wins += 1;
            } else {
                stats.draws += 1;
            }
        }

        stats
    }

    /// Formats the simulation results into a clean markdown report.
    pub fn format_report(stats: &MatchStats) -> String {
        let total = stats.total_matches.max(1) as u64;
        let blue_win_pct_x10 = (stats.team_1_wins as u64 * 1000) / total;
        let red_win_pct_x10 = (stats.team_2_wins as u64 * 1000) / total;
        let draw_pct_x10 = (stats.draws as u64 * 1000) / total;
        let avg_ticks = stats.total_ticks / total;

        format!(
            "# ⚔️ Axiom Auto-Battler Monte Carlo Balance Report\n\n\
            > **Total Simulated Matches:** {}\n\
            > **Average Match Length:** {} ticks\n\n\
            | Team | Composition | Wins | Win Rate | Total Survivors |\n\
            |---|---|---|:---:|:---:|\n\
            | **Blue Knights** (Team 1) | 5x [100 HP, 20 ATK, 3 CD] | {} | {}.{}% | {} |\n\
            | **Red Goblins** (Team 2) | 6x [60 HP, 16 ATK, 2 CD] | {} | {}.{}% | {} |\n\
            | **Draws / Timeouts** | - | {} | {}.{}% | - |\n\n\
            ### 🎯 Automated Balance Assessment\n\
            - Blue Win Rate: {}.{}%\n\
            - Red Win Rate: {}.{}%\n\
            - Status: {}\n",
            stats.total_matches,
            avg_ticks,
            stats.team_1_wins,
            blue_win_pct_x10 / 10,
            blue_win_pct_x10 % 10,
            stats.team_1_survivors,
            stats.team_2_wins,
            red_win_pct_x10 / 10,
            red_win_pct_x10 % 10,
            stats.team_2_survivors,
            stats.draws,
            draw_pct_x10 / 10,
            draw_pct_x10 % 10,
            blue_win_pct_x10 / 10,
            blue_win_pct_x10 % 10,
            red_win_pct_x10 / 10,
            red_win_pct_x10 % 10,
            if (blue_win_pct_x10 as i64 - red_win_pct_x10 as i64).abs() < 100 {
                "🟢 EXCELLENT BALANCE (Within 10% delta)"
            } else {
                "🟡 ASYMMETRICAL MATCHUP (Needs tuning)"
            }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_monte_carlo_small_batch() {
        let stats = MonteCarloBalancer::simulate_matches(50, 42);
        assert_eq!(stats.total_matches, 50);
        assert_eq!(stats.team_1_wins + stats.team_2_wins + stats.draws, 50);

        let report = MonteCarloBalancer::format_report(&stats);
        assert!(report.contains("Total Simulated Matches"));
    }
}
