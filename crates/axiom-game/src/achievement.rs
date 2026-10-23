//! Steamworks Achievements & In-Game Meta Challenge Tracker.
//!
//! Zero-float, 64-byte L1 cache aligned achievement state machine and toast notification pipeline.

extern crate alloc;

use alloc::vec::Vec;

/// Total number of tracked achievements.
pub const TOTAL_ACHIEVEMENTS: usize = 12;

/// Identifiers for all Steamworks and in-game achievements.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AchievementId {
    FirstBlood = 0,       // Win first match
    CleanSweep = 1,       // Win match taking 0 damage
    SteamOverdrive = 2,   // Finisher with heat >= 80%
    CounterPuncher = 3,   // 3 counter hits in 1 match
    TitanSlayer = 4,      // Defeat THESEUS
    WorkshopPioneer = 5,  // Equip custom workshop part
    ReplayArchivist = 6,  // Inspect match replay
    PerfectParry = 7,     // Guard heavy attack
    LabScholar = 8,       // Research 3 lab nodes
    DeciderChamp = 9,     // Win playoff decider
    IronWall = 10,        // Mitigate 100+ damage
    Centurion = 11,       // 100 punches thrown
}

impl AchievementId {
    pub const fn name(&self) -> &'static str {
        match self {
            Self::FirstBlood => "First Blood",
            Self::CleanSweep => "Clean Sweep",
            Self::SteamOverdrive => "Steam Overdrive",
            Self::CounterPuncher => "Counter Specialist",
            Self::TitanSlayer => "Titan Slayer",
            Self::WorkshopPioneer => "Workshop Pioneer",
            Self::ReplayArchivist => "Replay Archivist",
            Self::PerfectParry => "Ironclad Guard",
            Self::LabScholar => "Foundry Scholar",
            Self::DeciderChamp => "Playoff Victor",
            Self::IronWall => "Indomitable Bulwark",
            Self::Centurion => "Centurion Striker",
        }
    }

    pub const fn desc(&self) -> &'static str {
        match self {
            Self::FirstBlood => "Win your first Foundry League match.",
            Self::CleanSweep => "Achieve a Flawless Victory without taking damage.",
            Self::SteamOverdrive => "Unleash a Nova Finisher at extreme boiler pressure (Heat >= 80%).",
            Self::CounterPuncher => "Land 3 counter-hits in a single match.",
            Self::TitanSlayer => "Dethrone Champion THESEUS in the Title Match.",
            Self::WorkshopPioneer => "Equip a custom Steam Workshop mecha part.",
            Self::ReplayArchivist => "Record and inspect an .axrp match replay.",
            Self::PerfectParry => "Successfully mitigate incoming damage with Guard.",
            Self::LabScholar => "Unlock 3 technological nodes in the Meta Lab.",
            Self::DeciderChamp => "Win the 1st vs 2nd Decider playoff match.",
            Self::IronWall => "Mitigate over 100 total damage across your career.",
            Self::Centurion => "Throw 100 punches across your league career.",
        }
    }

    pub const fn api_name(&self) -> &'static str {
        match self {
            Self::FirstBlood => "ACH_FIRST_BLOOD",
            Self::CleanSweep => "ACH_CLEAN_SWEEP",
            Self::SteamOverdrive => "ACH_STEAM_OVERDRIVE",
            Self::CounterPuncher => "ACH_COUNTER_PUNCHER",
            Self::TitanSlayer => "ACH_TITAN_SLAYER",
            Self::WorkshopPioneer => "ACH_WORKSHOP_PIONEER",
            Self::ReplayArchivist => "ACH_REPLAY_ARCHIVIST",
            Self::PerfectParry => "ACH_PERFECT_PARRY",
            Self::LabScholar => "ACH_LAB_SCHOLAR",
            Self::DeciderChamp => "ACH_DECIDER_CHAMP",
            Self::IronWall => "ACH_IRON_WALL",
            Self::Centurion => "ACH_CENTURION",
        }
    }
}

/// 64-byte L1 cache aligned persistent achievement & telemetry state.
#[repr(C, align(64))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AchievementState {
    pub unlocked_mask: u32,          // 4B: Bitmask of unlocked achievements
    pub total_punches: u32,          // 4B: Cumulative punches thrown
    pub total_kos: u32,              // 4B: Cumulative KOs
    pub total_counter_hits: u32,     // 4B: Cumulative counter punches
    pub total_damage_mitigated: u32, // 4B: Cumulative damage blocked
    pub lab_nodes_unlocked: u32,     // 4B: Number of lab nodes purchased
    pub max_win_streak: u32,         // 4B: Best consecutive wins
    pub current_win_streak: u32,     // 4B: Current win streak
    pub matches_won: u32,            // 4B: Total matches won
    pub flawless_victories: u32,     // 4B: Matches won with 0 damage
    pub checksum: u64,               // 8B: 64-bit FNV-1a checksum
    pub _padding: [u8; 16],          // 16B: Exact 64-byte boundary
}

const _: () = assert!(core::mem::size_of::<AchievementState>() == 64);
const _: () = assert!(core::mem::align_of::<AchievementState>() == 64);

impl AchievementState {
    /// Serializes state directly into a 64-byte L1 cache aligned byte array.
    pub fn to_bytes(&self) -> [u8; 64] {
        let mut bytes = [0u8; 64];
        unsafe {
            core::ptr::copy_nonoverlapping(
                self as *const Self as *const u8,
                bytes.as_mut_ptr(),
                64,
            );
        }
        bytes
    }

    /// Deserializes state from a 64-byte array, verifying checksum integrity.
    pub fn from_bytes(bytes: &[u8; 64]) -> Option<Self> {
        let state = unsafe {
            core::ptr::read_unaligned(bytes.as_ptr() as *const Self)
        };
        if state.is_valid() {
            Some(state)
        } else {
            None
        }
    }

    /// Computes the 64-bit FNV-1a hash over all telemetry fields.
    pub fn compute_checksum(&self) -> u64 {
        let mut hash: u64 = 0xCBF29CE484222325;
        hash ^= self.unlocked_mask as u64; hash = hash.wrapping_mul(0x100000001B3);
        hash ^= self.total_punches as u64; hash = hash.wrapping_mul(0x100000001B3);
        hash ^= self.total_kos as u64; hash = hash.wrapping_mul(0x100000001B3);
        hash ^= self.total_counter_hits as u64; hash = hash.wrapping_mul(0x100000001B3);
        hash ^= self.total_damage_mitigated as u64; hash = hash.wrapping_mul(0x100000001B3);
        hash ^= self.lab_nodes_unlocked as u64; hash = hash.wrapping_mul(0x100000001B3);
        hash ^= self.max_win_streak as u64; hash = hash.wrapping_mul(0x100000001B3);
        hash ^= self.current_win_streak as u64; hash = hash.wrapping_mul(0x100000001B3);
        hash ^= self.matches_won as u64; hash = hash.wrapping_mul(0x100000001B3);
        hash ^= self.flawless_victories as u64; hash = hash.wrapping_mul(0x100000001B3);
        hash
    }

    /// Verifies whether the state has been tampered with.
    pub fn is_valid(&self) -> bool {
        self.checksum == self.compute_checksum()
    }

    /// Returns the number of unlocked achievements.
    pub fn unlocked_count(&self) -> usize {
        self.unlocked_mask.count_ones() as usize
    }
}

impl Default for AchievementState {
    fn default() -> Self {
        let mut state = Self {
            unlocked_mask: 0,
            total_punches: 0,
            total_kos: 0,
            total_counter_hits: 0,
            total_damage_mitigated: 0,
            lab_nodes_unlocked: 0,
            max_win_streak: 0,
            current_win_streak: 0,
            matches_won: 0,
            flawless_victories: 0,
            checksum: 0,
            _padding: [0; 16],
        };
        state.checksum = state.compute_checksum();
        state
    }
}

/// Active toast notification for on-screen HUD pop-up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AchievementToast {
    pub id: AchievementId,
    pub display_frames: u16,
}

/// Real-time Achievement & Meta Challenge Tracker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AchievementTracker {
    pub state: AchievementState,
    pub toast_queue: Vec<AchievementToast>,
    pub active_toast: Option<AchievementToast>,
    pub match_damage_taken: u32,
    pub match_counter_hits: u32,
}

impl Default for AchievementTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl AchievementTracker {
    /// Creates a new achievement tracker instance.
    pub fn new() -> Self {
        Self {
            state: AchievementState::default(),
            toast_queue: Vec::new(),
            active_toast: None,
            match_damage_taken: 0,
            match_counter_hits: 0,
        }
    }

    /// Checks if a specific achievement is already unlocked.
    #[inline(always)]
    pub fn is_unlocked(&self, id: AchievementId) -> bool {
        (self.state.unlocked_mask & (1 << (id as u8))) != 0
    }

    /// Unlocks an achievement, queuing a HUD toast notification if newly acquired.
    pub fn unlock(&mut self, id: AchievementId) -> bool {
        let bit = 1 << (id as u8);
        if (self.state.unlocked_mask & bit) == 0 {
            self.state.unlocked_mask |= bit;
            self.toast_queue.push(AchievementToast {
                id,
                display_frames: 180, // 3 seconds at 60 FPS
            });
            self.recompute_checksum();
            true
        } else {
            false
        }
    }

    /// Updates active toast timer and advances the toast queue.
    pub fn update_toast(&mut self) {
        if let Some(active) = &mut self.active_toast {
            if active.display_frames > 0 {
                active.display_frames -= 1;
            } else {
                self.active_toast = None;
            }
        }

        if self.active_toast.is_none() && !self.toast_queue.is_empty() {
            self.active_toast = Some(self.toast_queue.remove(0));
        }
    }

    /// Resets match-scoped counters when a new bout begins.
    pub fn on_match_start(&mut self) {
        self.match_damage_taken = 0;
        self.match_counter_hits = 0;
    }

    /// Triggered on every punch action thrown.
    pub fn on_punch_thrown(&mut self) {
        self.state.total_punches = self.state.total_punches.saturating_add(1);
        if self.state.total_punches >= 100 {
            self.unlock(AchievementId::Centurion);
        }
        self.recompute_checksum();
    }

    /// Triggered when damage is mitigated via guard.
    pub fn on_damage_mitigated(&mut self, amount: u32) {
        self.unlock(AchievementId::PerfectParry);
        self.state.total_damage_mitigated = self.state.total_damage_mitigated.saturating_add(amount);
        if self.state.total_damage_mitigated >= 100 {
            self.unlock(AchievementId::IronWall);
        }
        self.recompute_checksum();
    }

    /// Triggered when damage is taken by player.
    pub fn on_damage_taken(&mut self, amount: u32) {
        self.match_damage_taken = self.match_damage_taken.saturating_add(amount);
    }

    /// Triggered when player lands a counter hit.
    pub fn on_counter_hit(&mut self) {
        self.match_counter_hits = self.match_counter_hits.saturating_add(1);
        self.state.total_counter_hits = self.state.total_counter_hits.saturating_add(1);
        if self.match_counter_hits >= 3 {
            self.unlock(AchievementId::CounterPuncher);
        }
        self.recompute_checksum();
    }

    /// Triggered when player executes a Finisher.
    pub fn on_finisher_executed(&mut self, heat: u8) {
        if heat >= 80 {
            self.unlock(AchievementId::SteamOverdrive);
        }
    }

    /// Triggered when a match concludes.
    pub fn on_match_concluded(
        &mut self,
        won: bool,
        is_ko: bool,
        is_theseus: bool,
        is_decider: bool,
    ) {
        if won {
            self.state.matches_won = self.state.matches_won.saturating_add(1);
            self.state.current_win_streak = self.state.current_win_streak.saturating_add(1);
            if self.state.current_win_streak > self.state.max_win_streak {
                self.state.max_win_streak = self.state.current_win_streak;
            }

            self.unlock(AchievementId::FirstBlood);

            if self.match_damage_taken == 0 {
                self.state.flawless_victories = self.state.flawless_victories.saturating_add(1);
                self.unlock(AchievementId::CleanSweep);
            }

            if is_ko {
                self.state.total_kos = self.state.total_kos.saturating_add(1);
            }

            if is_decider {
                self.unlock(AchievementId::DeciderChamp);
            }

            if is_theseus {
                self.unlock(AchievementId::TitanSlayer);
            }
        } else {
            self.state.current_win_streak = 0;
        }

        self.recompute_checksum();
    }

    /// Triggered when equipping a workshop part.
    pub fn on_workshop_equipped(&mut self) {
        self.unlock(AchievementId::WorkshopPioneer);
    }

    /// Triggered when inspecting a match replay.
    pub fn on_replay_inspected(&mut self) {
        self.unlock(AchievementId::ReplayArchivist);
    }

    /// Triggered when lab research nodes are unlocked.
    pub fn on_lab_node_purchased(&mut self, total_nodes: u32) {
        self.state.lab_nodes_unlocked = total_nodes;
        if total_nodes >= 3 {
            self.unlock(AchievementId::LabScholar);
        }
        self.recompute_checksum();
    }

    /// Checks whether state integrity matches its checksum.
    pub fn is_valid(&self) -> bool {
        self.state.is_valid()
    }

    /// Returns unlocked achievements count.
    pub fn unlocked_count(&self) -> usize {
        self.state.unlocked_mask.count_ones() as usize
    }

    /// Recomputes 64-bit FNV-1a checksum over state fields.
    fn recompute_checksum(&mut self) {
        self.state.checksum = self.state.compute_checksum();
    }
}
