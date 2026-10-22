//! # Steel The Show (Axiom Refactored Edition)
//!
//! Complete, 100% zero-float, 64-byte L1 cache-aligned refactoring of the Steel The Show
//! mecha boxing roguelite system.
//!
//! Includes:
//! - 4-Slot Assembly (`Head`, `Core`, `Arm`, `Leg`) & 5 Drive Types (`Steam`, `Gear`, `Hydraulic`, `Spring`, `Condenser`)
//! - 3-Set Synergy Mechanics (`SteamBarrage`, `GearAimLock`, `HydraulicDeflect`)
//! - Duplicate Part Synthesis & Upgrade System (Tier 1 ➔ Tier 2, Attack+1, Defense+1, Heat-1)
//! - Inbound Reverse-Engineering Draft System (3 choices drafted from opponents)
//! - 4-Robot Foundry League (3 round-robin matches, standings, tie-breakers)
//! - Playoff Ladder: 1st/2nd Decider Match ➔ Title Match vs Champion THESEUS
//! - Direct integration with `BoxingRingEngine`.

pub mod league;
pub mod part;
pub mod robot;
pub mod season;

pub use league::{ContenderId, LeagueMatch, PlayoffState, StandingRecord, StsLeague};
pub use part::{can_upgrade, upgrade_part, DriveTag, Part, PartId, Slot, UpgradeBonus};
pub use robot::{RobotAssembly, SetSynergy};
pub use season::{InboundDraftState, StsPhase, StsSeasonEngine};
