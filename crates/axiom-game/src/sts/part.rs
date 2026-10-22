//! Authentic part catalog, slot architecture, and synthesis upgrade mechanics.
//!
//! Conforms to Axiom Zero-Float Constitution: pure integer stats, deterministic PRNG.

use core::fmt;

/// 4 Assembly slots of a Foundry Combat Robot.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Slot {
    Head = 0,
    Core = 1,
    Arm = 2,
    Leg = 3,
}

impl Slot {
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Head => "HEAD",
            Self::Core => "CORE",
            Self::Arm => "ARM",
            Self::Leg => "LEG",
        }
    }
}

/// 5 Drive types corresponding to set synergies.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DriveTag {
    Steam = 0,      // 증기 (Rush / Finisher focus)
    Gear = 1,       // 기어 (Precision / Aim Lock)
    Hydraulic = 2,  // 유압 (Heavy / Counter Deflect)
    Spring = 3,     // 반동 (Rapid recoil)
    Condenser = 4,  // 응축 (Cooling)
}

impl DriveTag {
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Steam => "STEAM",
            Self::Gear => "GEAR",
            Self::Hydraulic => "HYDRAULIC",
            Self::Spring => "SPRING",
            Self::Condenser => "CONDENSER",
        }
    }
}

/// Stat upgrade bonus applied upon part synthesis (D-316 / D-319).
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpgradeBonus {
    None = 0,
    AttackPlus1 = 1,
    DefensePlus1 = 2,
    HeatMinus1 = 3,
}

/// Authentic Part ID catalog derived from steel-the-show data/parts.json.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PartId {
    // Head / Eye parts
    EyeTarget = 0,
    EyeRadar = 1,
    EyeVisor = 2,
    EyeHawk = 3,        // THESEUS Champion Part
    EyeEco = 4,
    EyeOverclock = 5,

    // Core parts
    CorePower = 10,
    CoreBalanced = 11,
    CoreHeavy = 12,
    CoreReactor = 13,
    CorePhoenix = 14,    // THESEUS Champion Part
    CoreBulwark = 15,

    // Arm parts
    ArmPiston = 20,
    ArmPrecision = 21,
    ArmGuard = 22,
    ArmHammer = 23,
    ArmExecutioner = 24, // THESEUS Champion Part

    // Leg parts
    LegSprint = 30,
    LegAnchor = 31,
    LegHydraulic = 32,
    LegTread = 33,
    LegGhost = 34,       // THESEUS Champion Part
}

impl PartId {
    pub const fn name(&self) -> &'static str {
        match self {
            Self::EyeTarget => "Targeting Eye",
            Self::EyeRadar => "Radar Eye",
            Self::EyeVisor => "Heavy Visor",
            Self::EyeHawk => "Hawk Eye (THESEUS)",
            Self::EyeEco => "Eco Sensor",
            Self::EyeOverclock => "Overclock Eye",

            Self::CorePower => "Steam Boiler Core",
            Self::CoreBalanced => "Balanced Flywheel",
            Self::CoreHeavy => "Cast-Iron Core",
            Self::CoreReactor => "High-Pressure Reactor",
            Self::CorePhoenix => "Phoenix Heart (THESEUS)",
            Self::CoreBulwark => "Bulwark Bastion",

            Self::ArmPiston => "Steam Piston Arm",
            Self::ArmPrecision => "Precision Gear Arm",
            Self::ArmGuard => "Reinforced Guard Arm",
            Self::ArmHammer => "Hydraulic Hammer",
            Self::ArmExecutioner => "Executioner (THESEUS)",

            Self::LegSprint => "Sprint Pistons",
            Self::LegAnchor => "Cast-Iron Anchors",
            Self::LegHydraulic => "Hydraulic Struts",
            Self::LegTread => "Caterpillar Treads",
            Self::LegGhost => "Ghost Rollers (THESEUS)",
        }
    }

    pub const fn slot(&self) -> Slot {
        match self {
            Self::EyeTarget | Self::EyeRadar | Self::EyeVisor | Self::EyeHawk
            | Self::EyeEco | Self::EyeOverclock => Slot::Head,

            Self::CorePower | Self::CoreBalanced | Self::CoreHeavy | Self::CoreReactor
            | Self::CorePhoenix | Self::CoreBulwark => Slot::Core,

            Self::ArmPiston | Self::ArmPrecision | Self::ArmGuard | Self::ArmHammer
            | Self::ArmExecutioner => Slot::Arm,

            Self::LegSprint | Self::LegAnchor | Self::LegHydraulic | Self::LegTread
            | Self::LegGhost => Slot::Leg,
        }
    }

    pub const fn drive(&self) -> DriveTag {
        match self {
            Self::EyeTarget => DriveTag::Gear,
            Self::EyeRadar => DriveTag::Steam,
            Self::EyeVisor => DriveTag::Hydraulic,
            Self::EyeHawk => DriveTag::Gear,
            Self::EyeEco => DriveTag::Condenser,
            Self::EyeOverclock => DriveTag::Spring,

            Self::CorePower => DriveTag::Steam,
            Self::CoreBalanced => DriveTag::Gear,
            Self::CoreHeavy => DriveTag::Hydraulic,
            Self::CoreReactor => DriveTag::Steam,
            Self::CorePhoenix => DriveTag::Steam,
            Self::CoreBulwark => DriveTag::Hydraulic,

            Self::ArmPiston => DriveTag::Steam,
            Self::ArmPrecision => DriveTag::Gear,
            Self::ArmGuard => DriveTag::Hydraulic,
            Self::ArmHammer => DriveTag::Hydraulic,
            Self::ArmExecutioner => DriveTag::Steam,

            Self::LegSprint => DriveTag::Steam,
            Self::LegAnchor => DriveTag::Hydraulic,
            Self::LegHydraulic => DriveTag::Hydraulic,
            Self::LegTread => DriveTag::Gear,
            Self::LegGhost => DriveTag::Gear,
        }
    }

    pub const fn base_stats(&self) -> (u16, u16, u16) {
        // Returns (attack, defense, heat)
        match self {
            Self::EyeTarget => (12, 1, 4),
            Self::EyeRadar => (8, 3, 3),
            Self::EyeVisor => (5, 6, 2),
            Self::EyeHawk => (18, 5, 5),
            Self::EyeEco => (6, 2, 1),
            Self::EyeOverclock => (14, 0, 7),

            Self::CorePower => (10, 6, 8),
            Self::CoreBalanced => (6, 8, 4),
            Self::CoreHeavy => (4, 14, 3),
            Self::CoreReactor => (15, 4, 10),
            Self::CorePhoenix => (16, 12, 6),
            Self::CoreBulwark => (2, 18, 2),

            Self::ArmPiston => (14, 4, 6),
            Self::ArmPrecision => (11, 5, 3),
            Self::ArmGuard => (4, 12, 2),
            Self::ArmHammer => (18, 2, 8),
            Self::ArmExecutioner => (22, 6, 7),

            Self::LegSprint => (8, 4, 4),
            Self::LegAnchor => (3, 10, 2),
            Self::LegHydraulic => (6, 8, 4),
            Self::LegTread => (5, 12, 3),
            Self::LegGhost => (12, 8, 4),
        }
    }
}

/// 16-byte packed POD representation of an installed or inventoried mecha part.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Part {
    pub id: PartId,
    pub slot: Slot,
    pub drive: DriveTag,
    pub tier: u8, // 1 = standard, 2 = synthesized upgrade
    pub bonus: UpgradeBonus,
    pub _pad0: u8,
    pub base_attack: u16,
    pub base_defense: u16,
    pub base_heat: u16,
    pub _pad1: [u8; 4],
}

impl Part {
    pub const fn effective_attack(&self) -> u16 {
        match self.bonus {
            UpgradeBonus::AttackPlus1 => self.base_attack.saturating_add(1),
            _ => self.base_attack,
        }
    }

    pub const fn effective_defense(&self) -> u16 {
        match self.bonus {
            UpgradeBonus::DefensePlus1 => self.base_defense.saturating_add(1),
            _ => self.base_defense,
        }
    }

    pub const fn effective_heat(&self) -> u16 {
        match self.bonus {
            UpgradeBonus::HeatMinus1 => self.base_heat.saturating_sub(1),
            _ => self.base_heat,
        }
    }
}

impl fmt::Display for Part {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} [T{}] (ATK:{}, DEF:{}, HEAT:{}, DRIVE:{})",
            self.id.name(),
            self.tier,
            self.effective_attack(),
            self.effective_defense(),
            self.effective_heat(),
            self.drive.name()
        )
    }
}

/// Checks whether two duplicate parts can be synthesized (D-316 / D-319).
pub fn can_upgrade(base: &Part, material: &Part) -> bool {
    base.id == material.id && base.tier == 1 && material.tier == 1
}

/// Synthesizes two duplicate parts into an upgraded Tier 2 part (D-316 / D-319).
///
/// Deterministically selects Attack +1, Defense +1, or Heat -1 via input seed.
pub fn upgrade_part(base: &mut Part, seed: u64) -> UpgradeBonus {
    debug_assert!(base.tier == 1, "Only Tier 1 parts can be upgraded");
    base.tier = 2;
    let roll = seed % 3;
    let bonus = match roll {
        0 => UpgradeBonus::AttackPlus1,
        1 => UpgradeBonus::DefensePlus1,
        _ => UpgradeBonus::HeatMinus1,
    };
    base.bonus = bonus;
    bonus
}

impl PartId {
    pub fn to_part(self) -> Part {
        let (atk, def, heat) = self.base_stats();
        Part {
            id: self,
            slot: self.slot(),
            drive: self.drive(),
            tier: 1,
            bonus: UpgradeBonus::None,
            _pad0: 0,
            base_attack: atk,
            base_defense: def,
            base_heat: heat,
            _pad1: [0; 4],
        }
    }
}
