//! Unit & Integration Tests for Tournament Bracket Engine and Spectator Relay Session (T-105).

use axiom_game::boxing::{ActionKind, BoxingRingEngine};
use axiom_game::boxing_netcode::{
    BoxingRollbackSession, NetboxWirePacket, SpectatorRelaySession, NETBOX_WIRE_MAGIC,
};
use axiom_game::tournament::{MatchStatus, TournamentEngine, TournamentFighter};

#[test]
fn test_tournament_bracket_progression_and_champion() {
    let f0 = TournamentFighter::new(0, 1, b"Tinker");
    let f1 = TournamentFighter::new(1, 4, b"Cipher");
    let f2 = TournamentFighter::new(2, 2, b"Pike");
    let f3 = TournamentFighter::new(3, 3, b"Theseus");

    let mut tournament = TournamentEngine::new_4_bracket(f0, f1, f2, f3);
    assert_eq!(tournament.num_matches, 3);
    assert_eq!(tournament.matches[0].status, MatchStatus::Ready);
    assert_eq!(tournament.matches[1].status, MatchStatus::Ready);
    assert_eq!(tournament.matches[2].status, MatchStatus::Pending);
    assert!(tournament.champion.is_none());

    let initial_hash = tournament.compute_hash();

    // Semifinal 1: Tinker vs Cipher -> Tinker wins (10 - 8)
    let winner_m0 = tournament
        .record_match_result(0, 10, 8)
        .expect("M0 result valid");
    assert_eq!(winner_m0.id, 0);
    assert_eq!(tournament.matches[0].status, MatchStatus::Completed);
    assert_eq!(tournament.matches[2].fighter_a.unwrap().id, 0);
    assert_eq!(tournament.matches[2].status, MatchStatus::Pending); // Waiting for M1

    // Semifinal 2: Pike vs Theseus -> Theseus wins (7 - 10)
    let winner_m1 = tournament
        .record_match_result(1, 7, 10)
        .expect("M1 result valid");
    assert_eq!(winner_m1.id, 3);
    assert_eq!(tournament.matches[1].status, MatchStatus::Completed);
    assert_eq!(tournament.matches[2].fighter_b.unwrap().id, 3);
    assert_eq!(tournament.matches[2].status, MatchStatus::Ready); // Grand Final ready!

    // Grand Final: Tinker vs Theseus -> Theseus wins championship (8 - 10)
    let champ = tournament
        .record_match_result(2, 8, 10)
        .expect("M2 result valid");
    assert_eq!(champ.id, 3);
    assert_eq!(champ.name_as_str(), "Theseus");
    assert_eq!(tournament.matches[2].status, MatchStatus::Completed);
    assert_eq!(tournament.champion.unwrap().id, 3);

    let final_hash = tournament.compute_hash();
    assert_ne!(initial_hash, final_hash);

    // Determinism test: Replaying the exact same results produces identical hash
    let mut replay = TournamentEngine::new_4_bracket(f0, f1, f2, f3);
    replay.record_match_result(0, 10, 8).unwrap();
    replay.record_match_result(1, 7, 10).unwrap();
    replay.record_match_result(2, 8, 10).unwrap();
    assert_eq!(replay.compute_hash(), final_hash);
}

#[test]
fn test_spectator_relay_session_synchronization_and_forwarding() {
    let p0_engine = BoxingRingEngine::new(777);
    let p1_engine = BoxingRingEngine::new(777);
    let spectator_engine = BoxingRingEngine::new(777);

    let mut session_p0 = BoxingRollbackSession::<64>::new(0, p0_engine);
    let mut session_p1 = BoxingRollbackSession::<64>::new(1, p1_engine);
    let mut spectator = SpectatorRelaySession::<64>::new(spectator_engine);

    // Simulate 30 frames of combat between P0 and P1 with wire packet transmission
    for f in 0..30 {
        let p0_act = if f % 4 == 0 { ActionKind::Jab } else { ActionKind::Idle };
        let p1_act = if f % 6 == 0 { ActionKind::Cross } else { ActionKind::Guard };

        let p0_pkt = session_p0.create_wire_packet(p0_act, f as u16);
        let p1_pkt = session_p1.create_wire_packet(p1_act, f as u16);

        session_p0.advance_frame(p0_act);
        session_p1.advance_frame(p1_act);

        // Spectator receives both packets
        spectator.receive_packet(p0_pkt);
        spectator.receive_packet(p1_pkt);

        // Cross receive between players
        session_p0.receive_redundant_packet(&p1_pkt);
        session_p1.receive_redundant_packet(&p0_pkt);
    }

    // Step spectator until caught up
    let mut spectator_frames_stepped = 0;
    while let Some(stats) = spectator.step_frame() {
        spectator_frames_stepped += 1;
        assert_eq!(stats.rolled_back, false);
    }

    assert_eq!(spectator_frames_stepped, 30);
    assert_eq!(spectator.current_frame, 30);

    // Bit-exact consensus check between Player 0, Player 1, and Spectator!
    let p0_hash = session_p0.current_state.compute_hash();
    let p1_hash = session_p1.current_state.compute_hash();
    let spec_hash = spectator.last_state_hash;

    assert_eq!(p0_hash, p1_hash, "P0 and P1 must agree on state hash");
    assert_eq!(spec_hash, p0_hash, "Spectator must achieve 100% bit-exact consensus with players");

    // Verify relay outbox draining for downstream spectator peers
    let mut relay_buf = [NetboxWirePacket {
        magic: 0,
        sender_id: 0,
        sequence: 0,
        current_frame: 0,
        ack_frame: 0,
        window_inputs: [0; 4],
        checksum: 0,
    }; 16];

    let relayed_count = spectator.drain_relay_packets(&mut relay_buf);
    assert!(relayed_count > 0, "Spectator must have staged relay packets");
    assert_eq!(relay_buf[0].magic, NETBOX_WIRE_MAGIC);
}

#[test]
fn test_tournament_director_automated_broadcast() {
    use axiom_game::tournament::BroadcastPhase;
    use axiom_game::tournament::TournamentDirector;

    let f0 = TournamentFighter::new(0, 1, b"Elpis");
    let f1 = TournamentFighter::new(1, 4, b"Tinker");
    let f2 = TournamentFighter::new(2, 2, b"Cipher");
    let f3 = TournamentFighter::new(3, 3, b"Theseus");

    let mut director = TournamentDirector::new_4(f0, f1, f2, f3);
    assert_eq!(director.phase, BroadcastPhase::MatchActive);

    let initial_telemetry = director.telemetry();
    assert_eq!(initial_telemetry.current_match_id, 0);
    assert_eq!(initial_telemetry.round, 0);

    // Simulate director ticks until champion is crowned
    let mut champion_crowned = false;
    for _ in 0..6000 {
        let ev = director.step();
        if ev == 3 || director.phase == BroadcastPhase::GrandChampionCrowned {
            champion_crowned = true;
            break;
        }
    }

    assert!(champion_crowned, "Tournament must conclude with crowned champion");
    assert!(director.tournament.champion.is_some());
    let final_telemetry = director.telemetry();
    assert_ne!(final_telemetry.champion_id, 0xFF);
    assert!(final_telemetry.state_hash > 0);
}
