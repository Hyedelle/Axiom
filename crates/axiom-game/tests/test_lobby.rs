use axiom_game::lobby::{
    calculate_elo, LeaderboardEntry, LeaderboardTable, LobbyRole, LobbyState, SteamLobby,
    DEFAULT_ELO,
};

#[test]
fn test_leaderboard_entry_64b_alignment() {
    assert_eq!(core::mem::size_of::<LeaderboardEntry>(), 64);
    assert_eq!(core::mem::align_of::<LeaderboardEntry>(), 64);

    let entry = LeaderboardEntry::new(76561198000000001, b"AxiomChampion");
    assert_eq!(entry.steam_id, 76561198000000001);
    assert_eq!(entry.rating, DEFAULT_ELO);
    assert_eq!(entry.name_str(), "AxiomChampion");
    assert_eq!(entry.tier_name(), "Silver");
}

#[test]
fn test_tier_names() {
    let mut entry = LeaderboardEntry::new(1, b"TierTester");
    entry.rating = 800;
    assert_eq!(entry.tier_name(), "Bronze");
    entry.rating = 1200;
    assert_eq!(entry.tier_name(), "Silver");
    entry.rating = 1500;
    assert_eq!(entry.tier_name(), "Gold");
    entry.rating = 1800;
    assert_eq!(entry.tier_name(), "Platinum");
    entry.rating = 2200;
    assert_eq!(entry.tier_name(), "Grandmaster");
}

#[test]
fn test_calculate_elo_equal_ratings() {
    // When ratings are equal (1200 vs 1200), expected score is 500 permille.
    // Winning increases rating by K * (1000 - 500) / 1000 = 32 * 0.5 = 16.
    let (new_a, new_b) = calculate_elo(1200, 1200, true);
    assert_eq!(new_a, 1216);
    assert_eq!(new_b, 1184);

    // Loser drops by 16, winner gains 16. Sum remains 2400.
    assert_eq!(new_a + new_b, 2400);
}

#[test]
fn test_calculate_elo_underdog_win() {
    // Underdog A (1000) beats favorite B (1400)
    // diff = 1000 - 1400 = -400.
    // expected_a = 500 + (-400 * 57 / 40) = 500 - 570 = -70 -> clamped to 50.
    // delta = 32 * (1000 - 50) / 1000 = 32 * 950 / 1000 = 30.
    let (new_a, new_b) = calculate_elo(1000, 1400, true);
    assert_eq!(new_a, 1030); // Major rating gain (+30)
    assert_eq!(new_b, 1370); // Major rating loss (-30)

    // Favorite B (1400) beats underdog A (1000)
    // expected_b = 500 + (400 * 57 / 40) = 500 + 570 = 1070 -> clamped to 950.
    // delta = 32 * (1000 - 950) / 1000 = 32 * 50 / 1000 = 1.
    let (new_fav, new_und) = calculate_elo(1400, 1000, true);
    assert_eq!(new_fav, 1401); // Minor rating gain (+1)
    assert_eq!(new_und, 999);  // Minor rating loss (-1)
}

#[test]
fn test_leaderboard_table_ranking_and_sorting() {
    let mut table = LeaderboardTable::<16>::new();
    assert_eq!(table.len, 0);

    let idx1 = table.find_or_create(1001, b"PlayerOne").unwrap();
    let idx2 = table.find_or_create(1002, b"PlayerTwo").unwrap();
    let idx3 = table.find_or_create(1003, b"PlayerThree").unwrap();
    assert_eq!(table.len, 3);
    assert_eq!(idx1, 0);
    assert_eq!(idx2, 1);
    assert_eq!(idx3, 2);

    // Initial state: all 1200
    table.sort_by_rank();
    // Tie-break is lower steam_id first: 1001, 1002, 1003
    assert_eq!(table.entries[0].steam_id, 1001);
    assert_eq!(table.entries[1].steam_id, 1002);
    assert_eq!(table.entries[2].steam_id, 1003);

    // Player 1002 beats Player 1001 with KO & Flawless
    let ok = table.record_match(1002, 1001, true, true);
    assert!(ok);

    // Player 1002 rating becomes 1216, 1001 becomes 1184
    assert_eq!(table.entries[0].steam_id, 1002);
    assert_eq!(table.entries[0].rating, 1216);
    assert_eq!(table.entries[0].matches_won, 1);
    assert_eq!(table.entries[0].kos, 1);
    assert_eq!(table.entries[0].flawless_victories, 1);
    assert_eq!(table.entries[0].current_win_streak, 1);

    // Ranks:
    // Rank 1: 1002 (1216)
    // Rank 2: 1003 (1200)
    // Rank 3: 1001 (1184)
    assert_eq!(table.rank_of(1002), Some(1));
    assert_eq!(table.rank_of(1003), Some(2));
    assert_eq!(table.rank_of(1001), Some(3));
    assert_eq!(table.rank_of(9999), None);
}

#[test]
fn test_steam_lobby_lifecycle() {
    let mut lobby = SteamLobby::<8>::new(1001, b"HostPro", b"AXM-ROOM");
    assert_eq!(lobby.host_id, 1001);
    assert_eq!(lobby.room_code, *b"AXM-ROOM");
    assert_eq!(lobby.member_count(), 1);
    assert_eq!(lobby.state, LobbyState::WaitingForPlayers);
    assert!(!lobby.can_start_match());

    // Challenger joins as Player2
    let joined = lobby.join(1002, b"Challenger", 1250, LobbyRole::Player2);
    assert!(joined);
    assert_eq!(lobby.member_count(), 2);
    assert!(!lobby.can_start_match()); // Challenger is not ready yet

    // Spectator joins
    let spec_joined = lobby.join(1003, b"Spectator", 1100, LobbyRole::Spectator);
    assert!(spec_joined);
    assert_eq!(lobby.member_count(), 3);
    // Spectators are auto-ready, but Player 2 isn't
    assert!(!lobby.can_start_match());

    // Challenger sets ready
    lobby.set_ready(1002, true);
    assert!(lobby.can_start_match());
    assert_eq!(lobby.state, LobbyState::ReadyToStart);

    // Challenger leaves
    lobby.leave(1002);
    assert_eq!(lobby.member_count(), 2);
    assert!(!lobby.can_start_match());
    assert_eq!(lobby.state, LobbyState::WaitingForPlayers);
}
