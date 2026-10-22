//! Robot assembly, 4-part loadout, drive synergies, and inventory management.
//!
//! 100% zero-float, L1 cache-friendly DOD structures.

use core::fmt;

use super::part::{can_upgrade, upgrade_part, DriveTag, Part, PartId, Slot, UpgradeBonus};

/// Active set synergies triggered when 3 or more parts share the same drive tag (D-287).
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetSynergy {
    None = 0,
    SteamBarrage = 1,       // 증기 난타 (Finisher heat cost halved, finisher damage amplified)
    GearAimLock = 2,        // 기어 조준고정 (Counter & crit damage doubled)
    HydraulicDeflect = 3,   // 유압 받아치기 (Guard reflects 50% damage back to attacker)
}

impl SetSynergy {
    pub const fn name(&self) -> &'static str {
        match self {
            Self::None => "None",
            Self::SteamBarrage => "Steam Finisher Barrage (증기 난타)",
            Self::GearAimLock => "Gear Aim Lock (기어 조준고정)",
            Self::HydraulicDeflect => "Hydraulic Counter Deflect (유압 받아치기)",
        }
    }
}

/// 64-byte aligned robot assembly loadout and inventory ledger.
#[repr(C, align(64))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RobotAssembly {
    pub head: Part,
    pub core: Part,
    pub arm: Part,
    pub leg: Part,
    pub inventory: [Option<Part>; 16],
    pub inventory_count: u8,
    pub _pad: [u8; 7],
}

impl RobotAssembly {
    /// Creates an assembly from 4 equipped parts.
    pub fn new(head: Part, core: Part, arm: Part, leg: Part) -> Self {
        debug_assert!(head.slot == Slot::Head);
        debug_assert!(core.slot == Slot::Core);
        debug_assert!(arm.slot == Slot::Arm);
        debug_assert!(leg.slot == Slot::Leg);

        Self {
            head,
            core,
            arm,
            leg,
            inventory: [None; 16],
            inventory_count: 0,
            _pad: [0; 7],
        }
    }

    /// Total effective attack power.
    pub const fn total_attack(&self) -> u16 {
        self.head.effective_attack()
            .saturating_add(self.core.effective_attack())
            .saturating_add(self.arm.effective_attack())
            .saturating_add(self.leg.effective_attack())
    }

    /// Total effective defense rating.
    pub const fn total_defense(&self) -> u16 {
        self.head.effective_defense()
            .saturating_add(self.core.effective_defense())
            .saturating_add(self.arm.effective_defense())
            .saturating_add(self.leg.effective_defense())
    }

    /// Total effective base heat generation.
    pub const fn total_heat(&self) -> u16 {
        self.head.effective_heat()
            .saturating_add(self.core.effective_heat())
            .saturating_add(self.arm.effective_heat())
            .saturating_add(self.leg.effective_heat())
    }

    /// Counts parts belonging to a specific drive tag across the 4 equipped slots.
    pub const fn drive_count(&self, tag: DriveTag) -> u8 {
        let mut count = 0;
        if self.head.drive as u8 == tag as u8 { count += 1; }
        if self.core.drive as u8 == tag as u8 { count += 1; }
        if self.arm.drive as u8 == tag as u8 { count += 1; }
        if self.leg.drive as u8 == tag as u8 { count += 1; }
        count
    }

    /// Evaluates the active 3-set synergy (D-287 / D-294).
    pub const fn active_synergy(&self) -> SetSynergy {
        if self.drive_count(DriveTag::Steam) >= 3 {
            SetSynergy::SteamBarrage
        } else if self.drive_count(DriveTag::Gear) >= 3 {
            SetSynergy::GearAimLock
        } else if self.drive_count(DriveTag::Hydraulic) >= 3 {
            SetSynergy::HydraulicDeflect
        } else {
            SetSynergy::None
        }
    }

    /// Returns a reference to the equipped part in the given slot.
    pub const fn get_equipped(&self, slot: Slot) -> &Part {
        match slot {
            Slot::Head => &self.head,
            Slot::Core => &self.core,
            Slot::Arm => &self.arm,
            Slot::Leg => &self.leg,
        }
    }

    /// Swaps an equipped part with a part from inventory.
    pub fn equip_from_inventory(&mut self, inv_idx: usize) -> bool {
        if inv_idx >= 16 {
            return false;
        }
        let Some(new_part) = self.inventory[inv_idx] else {
            return false;
        };

        let old_part = match new_part.slot {
            Slot::Head => core::mem::replace(&mut self.head, new_part),
            Slot::Core => core::mem::replace(&mut self.core, new_part),
            Slot::Arm => core::mem::replace(&mut self.arm, new_part),
            Slot::Leg => core::mem::replace(&mut self.leg, new_part),
        };
        self.inventory[inv_idx] = Some(old_part);
        true
    }

    /// Adds a newly drafted / acquired part to inventory.
    pub fn add_to_inventory(&mut self, part: Part) -> bool {
        for slot in &mut self.inventory {
            if slot.is_none() {
                *slot = Some(part);
                self.inventory_count = self.inventory_count.saturating_add(1);
                return true;
            }
        }
        false
    }

    /// Removes a part from inventory by index.
    pub fn remove_from_inventory(&mut self, inv_idx: usize) -> Option<Part> {
        if inv_idx >= 16 {
            return None;
        }
        let item = self.inventory[inv_idx].take();
        if item.is_some() {
            self.inventory_count = self.inventory_count.saturating_sub(1);
        }
        item
    }

    /// Synthesizes two duplicate parts in inventory (D-316 / D-319).
    pub fn synthesize_inventory(&mut self, base_idx: usize, mat_idx: usize, seed: u64) -> Option<UpgradeBonus> {
        if base_idx == mat_idx || base_idx >= 16 || mat_idx >= 16 {
            return None;
        }
        let (Some(base), Some(mat)) = (self.inventory[base_idx], self.inventory[mat_idx]) else {
            return None;
        };
        if !can_upgrade(&base, &mat) {
            return None;
        }

        let mut upgraded = base;
        let bonus = upgrade_part(&mut upgraded, seed);
        self.inventory[base_idx] = Some(upgraded);
        self.inventory[mat_idx] = None;
        self.inventory_count = self.inventory_count.saturating_sub(1);
        Some(bonus)
    }

    /// Synthesizes an equipped part with a matching material part from inventory (D-316).
    pub fn synthesize_equipped(&mut self, slot: Slot, mat_idx: usize, seed: u64) -> Option<UpgradeBonus> {
        if mat_idx >= 16 {
            return None;
        }
        let Some(mat) = self.inventory[mat_idx] else {
            return None;
        };

        let equipped_part = match slot {
            Slot::Head => &mut self.head,
            Slot::Core => &mut self.core,
            Slot::Arm => &mut self.arm,
            Slot::Leg => &mut self.leg,
        };

        if !can_upgrade(equipped_part, &mat) {
            return None;
        }

        let bonus = upgrade_part(equipped_part, seed);
        self.inventory[mat_idx] = None;
        self.inventory_count = self.inventory_count.saturating_sub(1);
        Some(bonus)
    }

    // --- Authentic Presets from steel-the-show ---

    /// Player's starting mecha (ELPIS).
    pub fn elpis_starter() -> Self {
        Self::new(
            PartId::EyeRadar.to_part(),
            PartId::CorePower.to_part(),
            PartId::ArmPrecision.to_part(),
            PartId::LegAnchor.to_part(),
        )
    }

    /// Opponent 1: TINKER (Speed & Rush outboxer).
    pub fn tinker_assembly() -> Self {
        Self::new(
            PartId::EyeTarget.to_part(),
            PartId::CoreBalanced.to_part(),
            PartId::ArmPiston.to_part(),
            PartId::LegSprint.to_part(),
        )
    }

    /// Opponent 2: CIPHER (Controller & Aim specialist).
    pub fn cipher_assembly() -> Self {
        Self::new(
            PartId::EyeTarget.to_part(),
            PartId::CoreBalanced.to_part(),
            PartId::ArmPrecision.to_part(),
            PartId::LegTread.to_part(),
        )
    }

    /// Opponent 3: PIKE (Heavy hydraulic slugger).
    pub fn pike_assembly() -> Self {
        Self::new(
            PartId::EyeVisor.to_part(),
            PartId::CoreHeavy.to_part(),
            PartId::ArmHammer.to_part(),
            PartId::LegHydraulic.to_part(),
        )
    }

    /// Final Boss: THESEUS (The Champion of the Foundry).
    pub fn theseus_champion_assembly() -> Self {
        Self::new(
            PartId::EyeHawk.to_part(),
            PartId::CorePhoenix.to_part(),
            PartId::ArmExecutioner.to_part(),
            PartId::LegGhost.to_part(),
        )
    }
}

impl fmt::Display for RobotAssembly {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Assembly [ATK:{} DEF:{} HEAT:{}] Synergy: {}",
            self.total_attack(),
            self.total_defense(),
            self.total_heat(),
            self.active_synergy().name()
        )
    }
}
