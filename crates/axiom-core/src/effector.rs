//! # Deterministic Combat Effector & ATB Skill Combo System (Phase 4 RPG)
//!
//! Provides Active Time Battle (ATB) gauge progression, elemental damage matrix,
//! and fixed-point status effectors (DoT, Buffs, Debuffs) with 100% zero-float determinism.

use crate::fixed::Fixed;
use crate::hash::deterministic_hash;

pub const ATB_MAX_GAUGE: u16 = 1000;
pub const MAX_STATUS_EFFECTS: usize = 4;
pub const MAX_ACTIVE_UNITS: usize = 32;

/// Elemental damage affinity category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u8)]
pub enum Element {
    #[default]
    Physical = 0,
    Fire = 1,
    Ice = 2,
    Lightning = 3,
    Holy = 4,
    Dark = 5,
}

impl Element {
    /// Multiplier against target defense element in Q32.16 fixed-point.
    ///
    /// Fire beats Ice (1.5x), Ice beats Fire (0.5x), Holy beats Dark (1.5x), Dark beats Holy (1.5x).
    pub fn affinity_multiplier(self, defender: Self) -> Fixed {
        match (self, defender) {
            (Element::Fire, Element::Ice) => Fixed::from_raw(98_304),       // 1.5x
            (Element::Ice, Element::Fire) => Fixed::from_raw(32_768),       // 0.5x
            (Element::Lightning, Element::Physical) => Fixed::from_raw(81_920), // 1.25x
            (Element::Holy, Element::Dark) => Fixed::from_raw(98_304),      // 1.5x
            (Element::Dark, Element::Holy) => Fixed::from_raw(98_304),      // 1.5x
            _ => Fixed::ONE,                                                // 1.0x
        }
    }
}

/// Status effect kind attached to a combatant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u8)]
pub enum StatusKind {
    #[default]
    None = 0,
    Burn = 1,   // Fire DoT damage
    Poison = 2, // Natural DoT damage
    Slow = 3,   // 50% ATB speed penalty
    Haste = 4,  // 50% ATB speed boost
    Shield = 5, // Damage absorption pool
    Stun = 6,   // ATB freeze
}

/// Active status effect instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(C)]
pub struct ActiveStatus {
    pub kind: StatusKind,
    pub remaining_ticks: u16,
    pub potency: u16,
}

/// RPG Combat Unit with ATB gauge, stats, and status effect slots.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(C, align(64))]
pub struct AtbUnit {
    pub id: u32,
    pub team: u8,
    pub element: Element,
    pub hp: u32,
    pub max_hp: u32,
    pub attack_power: u32,
    pub defense_power: u32,
    pub speed: u16,
    pub atb_gauge: u16,
    pub combo_counter: u8,
    pub status_effects: [ActiveStatus; MAX_STATUS_EFFECTS],
    _padding: [u8; 10],
}

impl Default for AtbUnit {
    fn default() -> Self {
        Self::empty()
    }
}

impl AtbUnit {
    pub const fn empty() -> Self {
        Self {
            id: 0,
            team: 0,
            element: Element::Physical,
            hp: 0,
            max_hp: 0,
            attack_power: 0,
            defense_power: 0,
            speed: 0,
            atb_gauge: 0,
            combo_counter: 0,
            status_effects: [ActiveStatus {
                kind: StatusKind::None,
                remaining_ticks: 0,
                potency: 0,
            }; MAX_STATUS_EFFECTS],
            _padding: [0; 10],
        }
    }

    pub fn new(
        id: u32,
        team: u8,
        element: Element,
        hp: u32,
        attack: u32,
        defense: u32,
        speed: u16,
    ) -> Self {
        Self {
            id,
            team,
            element,
            hp,
            max_hp: hp,
            attack_power: attack,
            defense_power: defense,
            speed,
            atb_gauge: 0,
            combo_counter: 0,
            status_effects: [ActiveStatus::default(); MAX_STATUS_EFFECTS],
            _padding: [0; 10],
        }
    }

    #[inline(always)]
    pub fn is_alive(&self) -> bool {
        self.id != 0 && self.hp > 0
    }

    #[inline(always)]
    pub fn can_act(&self) -> bool {
        self.is_alive() && self.atb_gauge >= ATB_MAX_GAUGE && !self.has_status(StatusKind::Stun)
    }

    pub fn has_status(&self, kind: StatusKind) -> bool {
        self.status_effects.iter().any(|s| s.kind == kind && s.remaining_ticks > 0)
    }

    /// Attaches or refreshes a status effect.
    pub fn apply_status(&mut self, kind: StatusKind, duration_ticks: u16, potency: u16) -> bool {
        if !self.is_alive() || kind == StatusKind::None {
            return false;
        }

        // Refresh existing status if present
        for status in &mut self.status_effects {
            if status.kind == kind {
                status.remaining_ticks = status.remaining_ticks.max(duration_ticks);
                status.potency = status.potency.max(potency);
                return true;
            }
        }

        // Insert into first free slot
        for status in &mut self.status_effects {
            if status.kind == StatusKind::None || status.remaining_ticks == 0 {
                *status = ActiveStatus {
                    kind,
                    remaining_ticks: duration_ticks,
                    potency,
                };
                return true;
            }
        }
        false
    }

    /// Advances 1 simulation tick for this unit.
    pub fn tick(&mut self) -> u32 {
        if !self.is_alive() {
            return 0;
        }

        let mut total_damage_taken = 0;

        // 1. Process Status Effects (DoTs and duration decays)
        for status in &mut self.status_effects {
            if status.remaining_ticks == 0 {
                status.kind = StatusKind::None;
                continue;
            }

            match status.kind {
                StatusKind::Burn | StatusKind::Poison => {
                    let dot_damage = status.potency as u32;
                    self.hp = self.hp.saturating_sub(dot_damage);
                    total_damage_taken += dot_damage;
                }
                _ => {}
            }

            status.remaining_ticks = status.remaining_ticks.saturating_sub(1);
            if status.remaining_ticks == 0 {
                status.kind = StatusKind::None;
            }
        }

        if !self.is_alive() {
            self.atb_gauge = 0;
            return total_damage_taken;
        }

        // 2. Fill ATB Gauge based on speed modifier
        if !self.has_status(StatusKind::Stun) {
            let mut rate = self.speed as u32;
            if self.has_status(StatusKind::Haste) {
                rate = rate.saturating_mul(3) / 2; // +50%
            }
            if self.has_status(StatusKind::Slow) {
                rate = rate / 2; // -50%
            }
            self.atb_gauge = (self.atb_gauge as u32 + rate).min(ATB_MAX_GAUGE as u32) as u16;
        }

        total_damage_taken
    }
}

/// Deterministic Combat Effector resolving attacks, multipliers, and combos.
pub struct CombatEffector;

impl CombatEffector {
    /// Computes damage dealt from attacker to defender without floating point.
    pub fn calculate_damage(
        attacker: &AtbUnit,
        defender: &AtbUnit,
        skill_power: u32,
        skill_element: Element,
    ) -> u32 {
        // Base damage: (Attack * SkillPower) / (Defense + 100)
        let effective_atk = attacker.attack_power.max(1);
        let effective_def = defender.defense_power;
        let base_raw = (effective_atk as u64 * skill_power as u64) / (effective_def as u64 + 100);

        // Elemental Affinity Multiplier
        let affinity = skill_element.affinity_multiplier(defender.element);
        let elemental_damage = (Fixed::from_int(base_raw as i64) * affinity).to_int().max(1) as u32;

        // Combo scaling: +10% damage per accumulated combo hit
        let combo_bonus = (elemental_damage * attacker.combo_counter as u32) / 10;
        elemental_damage.saturating_add(combo_bonus)
    }

    /// Executes an attack action from attacker onto defender.
    pub fn execute_attack(
        attacker: &mut AtbUnit,
        defender: &mut AtbUnit,
        skill_power: u32,
        skill_element: Element,
        status_to_apply: StatusKind,
        status_duration: u16,
        status_potency: u16,
    ) -> u32 {
        if !attacker.can_act() || !defender.is_alive() {
            return 0;
        }

        let damage = Self::calculate_damage(attacker, defender, skill_power, skill_element);
        defender.hp = defender.hp.saturating_sub(damage);

        // Apply status effect if specified
        if status_to_apply != StatusKind::None {
            defender.apply_status(status_to_apply, status_duration, status_potency);
        }

        // Reset attacker ATB gauge and increment combo
        attacker.atb_gauge = 0;
        attacker.combo_counter = attacker.combo_counter.saturating_add(1).min(10);

        damage
    }
}

/// Headless ATB Combat Arena simulating multi-unit turn-based encounters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AtbArena<const CAP: usize = MAX_ACTIVE_UNITS> {
    pub units: [AtbUnit; CAP],
    pub current_tick: u64,
    pub total_damage_dealt: u64,
}

impl<const CAP: usize> Default for AtbArena<CAP> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const CAP: usize> AtbArena<CAP> {
    pub const fn new() -> Self {
        Self {
            units: [AtbUnit::empty(); CAP],
            current_tick: 0,
            total_damage_dealt: 0,
        }
    }

    pub fn spawn_unit(&mut self, unit: AtbUnit) -> bool {
        for slot in &mut self.units {
            if !slot.is_alive() {
                *slot = unit;
                return true;
            }
        }
        false
    }

    /// Advances 1 tick across all units and executes automatic combo attacks.
    pub fn step(&mut self) {
        self.current_tick += 1;

        // 1. Tick all units
        for unit in &mut self.units {
            if unit.is_alive() {
                self.total_damage_dealt += unit.tick() as u64;
            }
        }

        // 2. Resolve ready actions (first unit to fill ATB strikes enemy)
        for i in 0..CAP {
            if !self.units[i].can_act() {
                continue;
            }

            let attacker_team = self.units[i].team;
            let mut target_idx = None;

            // Find first living enemy
            for j in 0..CAP {
                if i != j && self.units[j].is_alive() && self.units[j].team != attacker_team {
                    target_idx = Some(j);
                    break;
                }
            }

            if let Some(target) = target_idx {
                let elem = self.units[i].element;
                let damage = CombatEffector::calculate_damage(&self.units[i], &self.units[target], 100, elem);
                self.units[target].hp = self.units[target].hp.saturating_sub(damage);
                self.units[i].atb_gauge = 0;
                self.units[i].combo_counter = self.units[i].combo_counter.saturating_add(1).min(10);
                self.total_damage_dealt += damage as u64;
            }
        }
    }

    /// Computes deterministic state hash across combatants.
    pub fn compute_hash(&self) -> u64 {
        let mut h = self.current_tick.wrapping_mul(0x9E3779B97F4A7C15) ^ self.total_damage_dealt;
        for unit in &self.units {
            if unit.is_alive() {
                h ^= deterministic_hash(unit);
                h = h.rotate_left(9);
            }
        }
        h
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_atb_unit_geometry() {
        assert_eq!(core::mem::size_of::<AtbUnit>(), 64);
        assert_eq!(core::mem::align_of::<AtbUnit>(), 64);
    }

    #[test]
    fn test_elemental_affinity_and_damage() {
        let fire_mage = AtbUnit::new(1, 1, Element::Fire, 500, 100, 20, 50);
        let ice_golem = AtbUnit::new(2, 2, Element::Ice, 1000, 40, 50, 20);

        // Fire against Ice: 1.5x damage bonus
        let damage = CombatEffector::calculate_damage(&fire_mage, &ice_golem, 100, Element::Fire);
        // Base: (100 * 100) / (50 + 100) = 10000 / 150 = 66
        // 66 * 1.5 = 99
        assert!(damage >= 90 && damage <= 110, "Fire must deal ~99 damage to Ice, got {damage}");
    }

    #[test]
    fn test_status_effects_and_atb_slow() {
        let mut hero = AtbUnit::new(1, 1, Element::Physical, 500, 80, 30, 100);
        // Apply Slow for 10 ticks
        hero.apply_status(StatusKind::Slow, 10, 0);

        // 1 tick: rate should be halved to 50
        hero.tick();
        assert_eq!(hero.atb_gauge, 50);

        // Apply Burn for 5 ticks with 10 potency
        hero.apply_status(StatusKind::Burn, 5, 10);
        let dmg = hero.tick();
        assert_eq!(dmg, 10);
        assert_eq!(hero.hp, 490);
    }
}
