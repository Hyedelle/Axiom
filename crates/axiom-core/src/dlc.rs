//! Steamworks DLC & Expansion Pack Management System.
//!
//! 100% deterministic, zero-float, 64-byte L1 cache aligned expansion entitlement states.

use crate::hash::Fnv1aHasher;
use core::hash::Hasher;

/// Canonical Steamworks DLC App Identifier Enum.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DlcId {
    /// DLC 1: Titan Chronicles (Boss KRONOS, Titanium Hull Parts, Giga Finisher)
    TitanChronicles = 0,
    /// DLC 2: Void Frontier (Boss AETHER, Plasma Cannon, Phase Weave)
    VoidFrontier = 1,
    /// DLC 3: Golden Decal & Sponsor Pack (Exclusive Workshop Textures & Skins)
    GoldenDecals = 2,
    /// DLC 4: Foundry Overdrive (Infinite Fusion Reactor & Hyper Logistics)
    FoundryOverdrive = 3,
}

impl DlcId {
    pub const TOTAL_DLCS: usize = 4;

    pub fn bitmask(self) -> u32 {
        1 << (self as u8)
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::TitanChronicles => "Titan Chronicles",
            Self::VoidFrontier => "Void Frontier",
            Self::GoldenDecals => "Golden Decals & Sponsor Pack",
            Self::FoundryOverdrive => "Foundry Overdrive",
        }
    }

    pub fn steam_dlc_id(self) -> u32 {
        match self {
            Self::TitanChronicles => 3910010,
            Self::VoidFrontier => 3910020,
            Self::GoldenDecals => 3910030,
            Self::FoundryOverdrive => 3910040,
        }
    }
}

/// Exact 64-byte L1 cache aligned Steam DLC entitlement and license state.
#[repr(C, align(64))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DlcState {
    pub owned_mask: u32,         // 4B: Bitmask of owned DLCs
    pub active_mask: u32,        // 4B: Bitmask of actively enabled DLCs
    pub steam_app_id: u32,       // 4B: Base Game Steam AppID (e.g. 3910000)
    pub build_id: u32,           // 4B: Steam Depot Build ID
    pub install_timestamp: u64,  // 8B: Discrete Unix epoch installation timestamp
    pub signature_hash: u64,     // 8B: 64-bit FNV-1a anti-tamper entitlement checksum
    pub _padding: [u8; 32],      // 32B: Exact padding to 64 bytes
}

const _: () = assert!(core::mem::size_of::<DlcState>() == 64);
const _: () = assert!(core::mem::align_of::<DlcState>() == 64);

impl Default for DlcState {
    fn default() -> Self {
        let mut state = Self {
            owned_mask: 0,
            active_mask: 0,
            steam_app_id: 3910000,
            build_id: 20261023,
            install_timestamp: 0,
            signature_hash: 0,
            _padding: [0u8; 32],
        };
        state.signature_hash = state.compute_signature();
        state
    }
}

impl DlcState {
    /// Creates a new DLC entitlement manager with given base Steam AppID.
    pub fn new(steam_app_id: u32, build_id: u32) -> Self {
        let mut state = Self {
            owned_mask: 0,
            active_mask: 0,
            steam_app_id,
            build_id,
            install_timestamp: 0,
            signature_hash: 0,
            _padding: [0u8; 32],
        };
        state.signature_hash = state.compute_signature();
        state
    }

    /// Computes the 64-bit FNV-1a cryptographic integrity signature.
    pub fn compute_signature(&self) -> u64 {
        let mut buf = [0u8; 24];
        buf[0..4].copy_from_slice(&self.owned_mask.to_le_bytes());
        buf[4..8].copy_from_slice(&self.active_mask.to_le_bytes());
        buf[8..12].copy_from_slice(&self.steam_app_id.to_le_bytes());
        buf[12..16].copy_from_slice(&self.build_id.to_le_bytes());
        buf[16..24].copy_from_slice(&self.install_timestamp.to_le_bytes());
        let mut hasher = Fnv1aHasher::new();
        hasher.write(&buf);
        hasher.finish()
    }

    /// Verifies whether the entitlement license has been tampered with.
    pub fn verify_integrity(&self) -> bool {
        self.signature_hash == self.compute_signature()
    }

    /// Unlocks a DLC upon Steamworks license verification.
    pub fn unlock(&mut self, dlc: DlcId, timestamp: u64) {
        self.owned_mask |= dlc.bitmask();
        self.active_mask |= dlc.bitmask();
        if self.install_timestamp == 0 {
            self.install_timestamp = timestamp;
        }
        self.signature_hash = self.compute_signature();
    }

    /// Sets active/disabled state of an owned DLC.
    pub fn set_active(&mut self, dlc: DlcId, active: bool) -> bool {
        if !self.is_owned(dlc) {
            return false;
        }
        if active {
            self.active_mask |= dlc.bitmask();
        } else {
            self.active_mask &= !dlc.bitmask();
        }
        self.signature_hash = self.compute_signature();
        true
    }

    /// Checks if a specific DLC is owned.
    pub fn is_owned(&self, dlc: DlcId) -> bool {
        (self.owned_mask & dlc.bitmask()) != 0
    }

    /// Checks if a specific DLC is currently enabled in gameplay.
    pub fn is_active(&self, dlc: DlcId) -> bool {
        (self.active_mask & dlc.bitmask()) != 0
    }

    /// Checks if a mod or asset requires DLCs that are not owned.
    pub fn meets_requirements(&self, required_mask: u32) -> bool {
        (self.owned_mask & required_mask) == required_mask
    }
}
