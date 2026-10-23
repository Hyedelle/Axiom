//! Steamworks Multiplayer Lobby & Global ELO Leaderboard Systems.
//!
//! Zero-float, 64-byte L1 cache aligned leaderboard records and lobby state machines.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

/// Default initial ELO rating for new competitors.
pub const DEFAULT_ELO: u32 = 1200;

/// Standard ELO K-factor weight for rating changes.
pub const ELO_K_FACTOR: i32 = 32;

/// Default maximum members inside a Steam lobby room.
pub const MAX_LOBBY_MEMBERS: usize = 8;

/// Exact 64-byte L1 cache aligned player record for global leaderboards.
#[repr(C, align(64))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LeaderboardEntry {
    pub steam_id: u64,           // 8B: Steamworks 64-bit community ID
    pub rating: u32,             // 4B: Integer ELO skill rating (base 1200)
    pub matches_won: u32,        // 4B: Total career victories
    pub matches_lost: u32,       // 4B: Total career defeats
    pub kos: u32,                // 4B: Total knockout finishes delivered
    pub flawless_victories: u32, // 4B: Victories taking 0 damage
    pub highest_win_streak: u32, // 4B: Peak consecutive win streak
    pub current_win_streak: u32, // 4B: Ongoing consecutive win streak
    pub name: [u8; 24],          // 24B: UTF-8 null-terminated player moniker
    pub _padding: [u8; 4],       // 4B: Padding to exact 64 bytes
}

const _: () = assert!(core::mem::size_of::<LeaderboardEntry>() == 64);
const _: () = assert!(core::mem::align_of::<LeaderboardEntry>() == 64);

impl Default for LeaderboardEntry {
    fn default() -> Self {
        Self {
            steam_id: 0,
            rating: 1200,
            matches_won: 0,
            matches_lost: 0,
            kos: 0,
            flawless_victories: 0,
            highest_win_streak: 0,
            current_win_streak: 0,
            name: [0u8; 24],
            _padding: [0u8; 4],
        }
    }
}

impl LeaderboardEntry {
    /// Creates a new player entry with starting 1200 ELO.
    pub fn new(steam_id: u64, name_bytes: &[u8]) -> Self {
        let mut entry = Self::default();
        entry.steam_id = steam_id;
        let copy_len = name_bytes.len().min(23);
        entry.name[..copy_len].copy_from_slice(&name_bytes[..copy_len]);
        entry.name[copy_len] = 0; // null-terminated
        entry
    }

    /// Returns the player handle string slice.
    pub fn name_str(&self) -> &str {
        let len = self.name.iter().position(|&b| b == 0).unwrap_or(self.name.len());
        core::str::from_utf8(&self.name[..len]).unwrap_or("Unknown")
    }

    /// Returns competitive tier tier name: Bronze, Silver, Gold, Platinum, Grandmaster.
    pub fn tier_name(&self) -> &'static str {
        if self.rating >= 2000 {
            "Grandmaster"
        } else if self.rating >= 1700 {
            "Platinum"
        } else if self.rating >= 1400 {
            "Gold"
        } else if self.rating >= 1100 {
            "Silver"
        } else {
            "Bronze"
        }
    }
}

/// Computes deterministic zero-float ELO rating updates.
/// Returns (new_rating_a, new_rating_b).
pub fn calculate_elo(rating_a: u32, rating_b: u32, a_won: bool) -> (u32, u32) {
    let diff = (rating_a as i32) - (rating_b as i32);
    // Expected win probability permille (0..1000)
    // 500 when equal, each 40 rating points difference shifts expected score by ~57 permille
    let expected_a = (500 + (diff * 57 / 40)).clamp(50, 950);
    let actual_a = if a_won { 1000 } else { 0 };
    let k = ELO_K_FACTOR;
    let delta = (k * (actual_a - expected_a)) / 1000;

    let new_a = ((rating_a as i32) + delta).max(100) as u32;
    let new_b = ((rating_b as i32) - delta).max(100) as u32;
    (new_a, new_b)
}

/// Bounded deterministic Leaderboard storage table.
#[derive(Debug, Clone)]
pub struct LeaderboardTable<const CAP: usize> {
    pub entries: [LeaderboardEntry; CAP],
    pub len: usize,
}

impl<const CAP: usize> Default for LeaderboardTable<CAP> {
    fn default() -> Self {
        Self {
            entries: [LeaderboardEntry::default(); CAP],
            len: 0,
        }
    }
}

impl<const CAP: usize> LeaderboardTable<CAP> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Finds an existing entry index or inserts a new one if capacity permits.
    pub fn find_or_create(&mut self, steam_id: u64, name: &[u8]) -> Option<usize> {
        for i in 0..self.len {
            if self.entries[i].steam_id == steam_id {
                return Some(i);
            }
        }
        if self.len < CAP {
            let idx = self.len;
            self.entries[idx] = LeaderboardEntry::new(steam_id, name);
            self.len += 1;
            Some(idx)
        } else {
            None
        }
    }

    /// Records match outcome, updates win/loss streaks and recalculates ELO ratings.
    pub fn record_match(
        &mut self,
        winner_id: u64,
        loser_id: u64,
        is_ko: bool,
        is_flawless: bool,
    ) -> bool {
        let (win_idx, lose_idx) = match (self.find_entry_idx(winner_id), self.find_entry_idx(loser_id)) {
            (Some(w), Some(l)) => (w, l),
            _ => return false,
        };

        let (new_win_elo, new_lose_elo) = calculate_elo(
            self.entries[win_idx].rating,
            self.entries[lose_idx].rating,
            true,
        );

        // Update Winner
        self.entries[win_idx].rating = new_win_elo;
        self.entries[win_idx].matches_won = self.entries[win_idx].matches_won.saturating_add(1);
        self.entries[win_idx].current_win_streak = self.entries[win_idx].current_win_streak.saturating_add(1);
        if self.entries[win_idx].current_win_streak > self.entries[win_idx].highest_win_streak {
            self.entries[win_idx].highest_win_streak = self.entries[win_idx].current_win_streak;
        }
        if is_ko {
            self.entries[win_idx].kos = self.entries[win_idx].kos.saturating_add(1);
        }
        if is_flawless {
            self.entries[win_idx].flawless_victories = self.entries[win_idx].flawless_victories.saturating_add(1);
        }

        // Update Loser
        self.entries[lose_idx].rating = new_lose_elo;
        self.entries[lose_idx].matches_lost = self.entries[lose_idx].matches_lost.saturating_add(1);
        self.entries[lose_idx].current_win_streak = 0;

        self.sort_by_rank();
        true
    }

    /// Sorts leaderboard descending by rating, wins, KOs, then steam_id.
    pub fn sort_by_rank(&mut self) {
        if self.len <= 1 {
            return;
        }
        // Insertion sort for deterministic zero-alloc operation
        for i in 1..self.len {
            let mut j = i;
            while j > 0 && is_better(&self.entries[j], &self.entries[j - 1]) {
                self.entries.swap(j, j - 1);
                j -= 1;
            }
        }
    }

    /// Finds 1-based rank position of a player (1..=len), or None if not found.
    pub fn rank_of(&self, steam_id: u64) -> Option<usize> {
        for (i, entry) in self.entries[..self.len].iter().enumerate() {
            if entry.steam_id == steam_id {
                return Some(i + 1);
            }
        }
        None
    }

    fn find_entry_idx(&self, steam_id: u64) -> Option<usize> {
        for (i, entry) in self.entries[..self.len].iter().enumerate() {
            if entry.steam_id == steam_id {
                return Some(i);
            }
        }
        None
    }
}

#[inline(always)]
fn is_better(a: &LeaderboardEntry, b: &LeaderboardEntry) -> bool {
    if a.rating != b.rating {
        a.rating > b.rating
    } else if a.matches_won != b.matches_won {
        a.matches_won > b.matches_won
    } else if a.kos != b.kos {
        a.kos > b.kos
    } else {
        a.steam_id < b.steam_id
    }
}

/// Role of a participant inside a Steam multiplayer lobby.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LobbyRole {
    Player1 = 0,
    Player2 = 1,
    Spectator = 2,
}

/// Lifecycle state of a multiplayer lobby session.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LobbyState {
    #[default]
    WaitingForPlayers = 0,
    ReadyToStart = 1,
    MatchInProgress = 2,
    MatchFinished = 3,
}

/// Participant data inside a multiplayer match room.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LobbyMember {
    pub steam_id: u64,
    pub rating: u32,
    pub role: LobbyRole,
    pub ready: bool,
    pub name: [u8; 24],
}

impl LobbyMember {
    pub fn name_str(&self) -> &str {
        let len = self.name.iter().position(|&b| b == 0).unwrap_or(self.name.len());
        core::str::from_utf8(&self.name[..len]).unwrap_or("Unknown")
    }
}

/// Steamworks P2P Multiplayer Room & Matchmaking Lobby.
#[derive(Debug, Clone)]
pub struct SteamLobby<const MAX_MEMBERS: usize = 8> {
    pub room_code: [u8; 8],
    pub host_id: u64,
    pub state: LobbyState,
    pub members: [Option<LobbyMember>; MAX_MEMBERS],
}

impl<const MAX_MEMBERS: usize> Default for SteamLobby<MAX_MEMBERS> {
    fn default() -> Self {
        Self {
            room_code: *b"AXM-0001",
            host_id: 0,
            state: LobbyState::WaitingForPlayers,
            members: [None; MAX_MEMBERS],
        }
    }
}

impl<const MAX_MEMBERS: usize> SteamLobby<MAX_MEMBERS> {
    /// Creates a new lobby hosted by host_id.
    pub fn new(host_id: u64, host_name: &[u8], room_code: &[u8; 8]) -> Self {
        let mut lobby = Self {
            room_code: *room_code,
            host_id,
            state: LobbyState::WaitingForPlayers,
            members: [None; MAX_MEMBERS],
        };
        let mut name_buf = [0u8; 24];
        let copy_len = host_name.len().min(23);
        name_buf[..copy_len].copy_from_slice(&host_name[..copy_len]);

        lobby.members[0] = Some(LobbyMember {
            steam_id: host_id,
            rating: 1200,
            role: LobbyRole::Player1,
            ready: true,
            name: name_buf,
        });
        lobby
    }

    /// Member joins lobby with requested role (P1, P2, or Spectator).
    pub fn join(&mut self, steam_id: u64, name: &[u8], rating: u32, role: LobbyRole) -> bool {
        // Prevent duplicate joins
        if self.find_member_idx(steam_id).is_some() {
            return false;
        }

        // Find available slot
        for slot in &mut self.members {
            if slot.is_none() {
                let mut name_buf = [0u8; 24];
                let copy_len = name.len().min(23);
                name_buf[..copy_len].copy_from_slice(&name[..copy_len]);

                *slot = Some(LobbyMember {
                    steam_id,
                    rating,
                    role,
                    ready: role == LobbyRole::Spectator, // Spectators are always ready
                    name: name_buf,
                });
                self.update_state();
                return true;
            }
        }
        false
    }

    /// Member leaves lobby.
    pub fn leave(&mut self, steam_id: u64) -> bool {
        if let Some(idx) = self.find_member_idx(steam_id) {
            self.members[idx] = None;
            // Shift down members or compact
            self.update_state();
            true
        } else {
            false
        }
    }

    /// Toggles or sets member readiness.
    pub fn set_ready(&mut self, steam_id: u64, ready: bool) -> bool {
        if let Some(idx) = self.find_member_idx(steam_id) {
            if let Some(m) = &mut self.members[idx] {
                m.ready = ready;
                self.update_state();
                return true;
            }
        }
        false
    }

    /// Returns the number of connected members.
    pub fn member_count(&self) -> usize {
        self.members.iter().filter(|m| m.is_some()).count()
    }

    /// Checks if both Player 1 and Player 2 are present and ready to start the match.
    pub fn can_start_match(&self) -> bool {
        let mut p1_ready = false;
        let mut p2_ready = false;

        for m in self.members.iter().flatten() {
            if m.role == LobbyRole::Player1 && m.ready {
                p1_ready = true;
            } else if m.role == LobbyRole::Player2 && m.ready {
                p2_ready = true;
            }
        }
        p1_ready && p2_ready
    }

    fn find_member_idx(&self, steam_id: u64) -> Option<usize> {
        for (i, slot) in self.members.iter().enumerate() {
            if let Some(m) = slot {
                if m.steam_id == steam_id {
                    return Some(i);
                }
            }
        }
        None
    }

    fn update_state(&mut self) {
        if self.can_start_match() {
            self.state = LobbyState::ReadyToStart;
        } else {
            self.state = LobbyState::WaitingForPlayers;
        }
    }
}
