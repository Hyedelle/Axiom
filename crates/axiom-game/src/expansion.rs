//! Steamworks Expansion Pack Gameplay Systems.
//!
//! Exposes 100% deterministic, 64-byte L1 cache aligned expansion contenders,
//! exclusive passive perks, and DLC-gated game mechanics.

extern crate alloc;

use alloc::vec::Vec;
use axiom_core::dlc::{DlcId, DlcState};

/// Exclusive DLC Expansion Boss Contenders.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ExpansionBoss {
    /// DLC 1: Titan Overlord KRONOS (Superheavy 180 HP Juggernaut)
    Kronos = 0,
    /// DLC 2: Celestial Phantom AETHER (High-Frequency 140 HP Phase Striker)
    Aether = 1,
}

impl ExpansionBoss {
    pub fn required_dlc(self) -> DlcId {
        match self {
            Self::Kronos => DlcId::TitanChronicles,
            Self::Aether => DlcId::VoidFrontier,
        }
    }

    pub fn moniker(self) -> &'static str {
        match self {
            Self::Kronos => "KRONOS (The Iron Colossus)",
            Self::Aether => "AETHER (The Void Phantom)",
        }
    }

    pub fn base_hp(self) -> i32 {
        match self {
            Self::Kronos => 180,
            Self::Aether => 140,
        }
    }
}

/// 64-byte L1 cache aligned Expansion Gameplay Tracker.
#[repr(C, align(64))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExpansionTracker {
    pub dlc_state: DlcState,       // 64B: Embedded 64B L1 aligned DLC state
}

const _: () = assert!(core::mem::size_of::<ExpansionTracker>() == 64);
const _: () = assert!(core::mem::align_of::<ExpansionTracker>() == 64);

impl Default for ExpansionTracker {
    fn default() -> Self {
        Self {
            dlc_state: DlcState::default(),
        }
    }
}

impl ExpansionTracker {
    pub fn new(steam_app_id: u32, build_id: u32) -> Self {
        Self {
            dlc_state: DlcState::new(steam_app_id, build_id),
        }
    }

    /// Checks if an expansion boss is unlocked and enabled for matchmaking.
    pub fn is_boss_unlocked(&self, boss: ExpansionBoss) -> bool {
        self.dlc_state.is_active(boss.required_dlc())
    }

    /// Unlocks a DLC license.
    pub fn unlock_dlc(&mut self, dlc: DlcId, timestamp: u64) {
        self.dlc_state.unlock(dlc, timestamp);
    }

    /// Returns list of currently active expansion bosses.
    pub fn active_bosses(&self) -> Vec<ExpansionBoss> {
        let mut list = Vec::new();
        if self.is_boss_unlocked(ExpansionBoss::Kronos) {
            list.push(ExpansionBoss::Kronos);
        }
        if self.is_boss_unlocked(ExpansionBoss::Aether) {
            list.push(ExpansionBoss::Aether);
        }
        list
    }
}
