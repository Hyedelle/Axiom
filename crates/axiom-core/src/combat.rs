//! Deterministic Auto-Battler Combat Ruleset (Phase 1 Gameplay).
//!
//! Features:
//! - 64-byte cache-line aligned `CombatUnit` structures.
//! - Pure-function deterministic battle step with zero floating-point math.
//! - Autonomous targeting, pathing, attack range, cooldowns, and damage resolution.

use crate::fixed::{Fixed, FixedVec2};

pub const DEFAULT_MAX_UNITS: usize = 256;

/// 64-byte cache-line aligned combat unit record for auto-battler simulation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(C, align(64))]
pub struct CombatUnit {
    pub id: u32,
    pub target_id: u32,
    pub current_hp: u16,
    pub max_hp: u16,
    pub attack_power: u16,
    pub attack_range: u16,
    pub attack_cooldown: u16,
    pub cooldown_timer: u16,
    pub kills: u16,
    pub team: u8,
    pub _pad1: [u8; 9],
    pub pos: FixedVec2, // 16 bytes
    pub vel: FixedVec2, // 16 bytes
}

impl Default for CombatUnit {
    fn default() -> Self {
        Self::empty()
    }
}

impl CombatUnit {
    pub const fn empty() -> Self {
        Self {
            id: 0,
            target_id: 0,
            current_hp: 0,
            max_hp: 0,
            attack_power: 0,
            attack_range: 0,
            attack_cooldown: 0,
            cooldown_timer: 0,
            kills: 0,
            team: 0,
            _pad1: [0; 9],
            pos: FixedVec2::ZERO,
            vel: FixedVec2::ZERO,
        }
    }

    #[inline(always)]
    pub fn is_alive(&self) -> bool {
        self.id != 0 && self.current_hp > 0
    }
}

/// Headless, deterministic battle arena simulation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C, align(64))]
pub struct BattleArena<const CAP: usize = DEFAULT_MAX_UNITS> {
    pub units: [CombatUnit; CAP],
    pub active_count: usize,
    pub current_tick: u64,
}

impl<const CAP: usize> Default for BattleArena<CAP> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const CAP: usize> BattleArena<CAP> {
    pub const fn new() -> Self {
        Self {
            units: [CombatUnit::empty(); CAP],
            active_count: 0,
            current_tick: 0,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn spawn_unit(
        &mut self,
        id: u32,
        team: u8,
        hp: u16,
        attack: u16,
        range: u16,
        cooldown: u16,
        pos: FixedVec2,
    ) -> Option<usize> {
        if self.active_count >= CAP || id == 0 {
            return None;
        }

        for (idx, slot) in self.units.iter_mut().enumerate() {
            if slot.id == 0 {
                *slot = CombatUnit {
                    id,
                    target_id: 0,
                    current_hp: hp,
                    max_hp: hp,
                    attack_power: attack,
                    attack_range: range,
                    attack_cooldown: cooldown,
                    cooldown_timer: 0,
                    kills: 0,
                    team,
                    _pad1: [0; 9],
                    pos,
                    vel: FixedVec2::ZERO,
                };
                self.active_count += 1;
                return Some(idx);
            }
        }
        None
    }

    /// Advances the auto-battler simulation by 1 discrete tick.
    ///
    /// Pure function mechanics:
    /// 1. Cooldown ticks down.
    /// 2. Alive units acquire nearest living enemy.
    /// 3. In range -> Attack and deal damage. Out of range -> Move towards target.
    pub fn step(&mut self) {
        self.current_tick += 1;

        // 1. Tick cooldowns and clear dead targets
        for unit in self.units.iter_mut() {
            if unit.is_alive() && unit.cooldown_timer > 0 {
                unit.cooldown_timer -= 1;
            }
        }

        // 2. Targeting, movement, and combat resolution
        for i in 0..CAP {
            if !self.units[i].is_alive() {
                continue;
            }

            let attacker_team = self.units[i].team;
            let attacker_pos = self.units[i].pos;
            let attack_range = self.units[i].attack_range as i64;
            let range_sq = Fixed::from_int(attack_range * attack_range);

            // Find closest enemy
            let mut closest_enemy_idx: Option<usize> = None;
            let mut min_dist_sq = Fixed::MAX;

            for j in 0..CAP {
                if i != j && self.units[j].is_alive() && self.units[j].team != attacker_team {
                    let dist_sq = self.units[i].pos.distance_squared(self.units[j].pos);
                    if dist_sq < min_dist_sq {
                        min_dist_sq = dist_sq;
                        closest_enemy_idx = Some(j);
                    }
                }
            }

            if let Some(target_idx) = closest_enemy_idx {
                self.units[i].target_id = self.units[target_idx].id;

                if min_dist_sq <= range_sq {
                    // In attack range: stop and attack if off cooldown
                    self.units[i].vel = FixedVec2::ZERO;

                    if self.units[i].cooldown_timer == 0 {
                        let damage = self.units[i].attack_power;
                        let target_hp = self.units[target_idx].current_hp;
                        self.units[target_idx].current_hp = target_hp.saturating_sub(damage);

                        if self.units[target_idx].current_hp == 0 {
                            self.units[i].kills += 1;
                        }
                        self.units[i].cooldown_timer = self.units[i].attack_cooldown;
                    }
                } else {
                    // Out of range: step towards target with fixed step
                    let target_pos = self.units[target_idx].pos;
                    let dx = if target_pos.x > attacker_pos.x {
                        1
                    } else if target_pos.x < attacker_pos.x {
                        -1
                    } else {
                        0
                    };
                    let dy = if target_pos.y > attacker_pos.y {
                        1
                    } else if target_pos.y < attacker_pos.y {
                        -1
                    } else {
                        0
                    };

                    self.units[i].vel = FixedVec2::from_ints(dx, dy);
                    self.units[i].pos += self.units[i].vel;
                }
            }
        }
    }

    /// Returns alive count for a given team (1 = Blue, 2 = Red).
    pub fn team_alive_count(&self, team: u8) -> usize {
        self.units
            .iter()
            .filter(|u| u.is_alive() && u.team == team)
            .count()
    }

    /// Number of living units across all teams.
    #[inline(always)]
    pub fn alive_count(&self) -> usize {
        self.units.iter().filter(|u| u.is_alive()).count()
    }

    /// Computes deterministic hash across living units.
    pub fn compute_hash(&self) -> u64 {
        let mut h = self.current_tick.wrapping_mul(0x9E3779B97F4A7C15);
        for unit in &self.units {
            if unit.is_alive() {
                h ^= crate::hash::deterministic_hash(unit);
                h = h.rotate_left(7);
            }
        }
        h
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::{align_of, size_of};

    #[test]
    fn test_combat_unit_geometry() {
        assert_eq!(
            size_of::<CombatUnit>(),
            64,
            "CombatUnit must be exactly 64 bytes"
        );
        assert_eq!(
            align_of::<CombatUnit>(),
            64,
            "CombatUnit must be 64-byte aligned"
        );
    }

    #[test]
    fn test_auto_battler_duel_resolution() {
        let mut arena = BattleArena::<16>::new();

        // Blue Knight: 100 HP, 25 ATK, 2 Range, 2 CD, Pos (0, 0)
        arena.spawn_unit(1, 1, 100, 25, 2, 2, FixedVec2::from_ints(0, 0));

        // Red Goblin: 40 HP, 10 ATK, 2 Range, 2 CD, Pos (10, 0)
        arena.spawn_unit(2, 2, 40, 10, 2, 2, FixedVec2::from_ints(10, 0));

        // Simulate 20 ticks of battle
        for _ in 0..20 {
            arena.step();
        }

        // Blue Knight should have defeated Red Goblin
        assert_eq!(arena.team_alive_count(2), 0, "Red Goblin should be dead");
        assert_eq!(arena.team_alive_count(1), 1, "Blue Knight should be alive");
        assert_eq!(arena.units[0].kills, 1);
    }
}
