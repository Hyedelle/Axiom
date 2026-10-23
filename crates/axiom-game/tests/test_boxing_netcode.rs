//! Integration tests for 2-Player P2P Mecha Boxing Rollback Netcode Engine (T-099).

use axiom_game::boxing::{ActionKind, BoxingRingEngine};
use axiom_game::boxing_netcode::BoxingRollbackSession;

#[test]
fn test_boxing_pvp_combat_interactions() {
    let mut engine = BoxingRingEngine::new(42);

    // 1. Clean hit: P0 Jabs while P1 is Idle
    let p1_start_hp = engine.opponent.hp;
    let (_ev0, _ev1) = engine.step_pvp(ActionKind::Jab, ActionKind::Idle);
    assert_eq!(engine.player.action.kind, ActionKind::Jab);

    // Advance to contact frame (18 / 2 = 9)
    for _ in 1..9 {
        engine.step_pvp(ActionKind::Idle, ActionKind::Idle);
    }
    let (ev0_hit, _) = engine.step_pvp(ActionKind::Idle, ActionKind::Idle);
    assert!(
        matches!(ev0_hit, axiom_game::boxing::BoxingEvent::PlayerPunched { is_counter: false, .. }),
        "Expected clean hit, got {ev0_hit:?}"
    );
    assert!(engine.opponent.hp < p1_start_hp, "P1 should take damage from clean hit");

    // Wait for stagger/recovery
    for _ in 0..25 {
        engine.step_pvp(ActionKind::Idle, ActionKind::Idle);
    }

    // 2. Evasion: P0 Crosses while P1 slips left
    let p1_hp_before_slip = engine.opponent.hp;
    engine.step_pvp(ActionKind::Cross, ActionKind::SlipLeft);
    // Advance to Cross contact frame (24 / 2 = 12)
    let mut evaded = false;
    for _ in 1..15 {
        let (e0, _) = engine.step_pvp(ActionKind::Idle, ActionKind::Idle);
        if matches!(e0, axiom_game::boxing::BoxingEvent::PlayerEvaded) {
            evaded = true;
        }
    }
    assert!(evaded, "P1 slip should cause P0 Cross to be evaded");
    assert_eq!(engine.opponent.hp, p1_hp_before_slip, "Evaded punch deals 0 damage");

    // Wait for actions to clear
    for _ in 0..25 {
        engine.step_pvp(ActionKind::Idle, ActionKind::Idle);
    }

    // 3. Guard: P0 Hooks while P1 guards
    let p1_hp_before_guard = engine.opponent.hp;
    engine.step_pvp(ActionKind::Hook, ActionKind::Guard);
    let mut blocked = false;
    for _ in 1..20 {
        let (e0, _) = engine.step_pvp(ActionKind::Idle, ActionKind::Guard);
        if matches!(e0, axiom_game::boxing::BoxingEvent::PlayerBlocked { .. }) {
            blocked = true;
        }
    }
    assert!(blocked, "P1 holding Guard should block Hook");
    assert!(engine.opponent.hp < p1_hp_before_guard, "Guard should take slight chip damage");
    assert!(
        p1_hp_before_guard - engine.opponent.hp < ActionKind::Hook.base_damage() as i32,
        "Chip damage should be much smaller than full damage"
    );
}

#[test]
fn test_boxing_rollback_zero_latency_parity() {
    let initial_state = BoxingRingEngine::new(12345);
    let mut session0 = BoxingRollbackSession::<64>::new(0, initial_state);
    let mut session1 = BoxingRollbackSession::<64>::new(1, initial_state);

    assert_eq!(session0.current_state.compute_hash(), session1.current_state.compute_hash());

    let p0_inputs = [ActionKind::Jab, ActionKind::Idle, ActionKind::Cross, ActionKind::Guard, ActionKind::Idle];
    let p1_inputs = [ActionKind::Idle, ActionKind::SlipLeft, ActionKind::Idle, ActionKind::Jab, ActionKind::Idle];

    for frame in 0..50 {
        let in0 = p0_inputs[frame % p0_inputs.len()];
        let in1 = p1_inputs[frame % p1_inputs.len()];

        // Synchronous exchange: both receive remote inputs before advancing frame
        session0.receive_remote_input(1, frame as u64, in1);
        session1.receive_remote_input(0, frame as u64, in0);

        session0.advance_frame(in0);
        session1.advance_frame(in1);

        assert_eq!(
            session0.current_state.compute_hash(),
            session1.current_state.compute_hash(),
            "Hash divergence on zero-latency frame {frame}"
        );
    }

    assert_eq!(session0.total_rollbacks, 0);
    assert_eq!(session1.total_rollbacks, 0);
}

#[test]
fn test_boxing_rollback_simulated_latency_and_reconciliation() {
    let initial_state = BoxingRingEngine::new(9999);
    let mut session0 = BoxingRollbackSession::<64>::new(0, initial_state);
    let mut session1 = BoxingRollbackSession::<64>::new(1, initial_state);

    // Buffer for simulated packet delay (4 frames = ~66ms delay)
    const DELAY: usize = 4;
    let mut p0_to_p1_pipe: Vec<(u64, ActionKind)> = Vec::new();
    let mut p1_to_p0_pipe: Vec<(u64, ActionKind)> = Vec::new();

    for frame in 0..100 {
        let in0 = if frame % 12 == 3 {
            ActionKind::Jab
        } else if frame % 20 == 8 {
            ActionKind::Guard
        } else {
            ActionKind::Idle
        };

        let in1 = if frame % 15 == 4 {
            ActionKind::SlipRight
        } else if frame % 18 == 6 {
            ActionKind::Cross
        } else {
            ActionKind::Idle
        };

        // Enqueue to simulated network pipe
        p0_to_p1_pipe.push((frame as u64, in0));
        p1_to_p0_pipe.push((frame as u64, in1));

        // Advance local frame (predicting opponent)
        session0.advance_frame(in0);
        session1.advance_frame(in1);

        // Deliver packets older than DELAY frames
        if frame >= DELAY {
            let (f0, act0) = p0_to_p1_pipe[frame - DELAY];
            let (f1, act1) = p1_to_p0_pipe[frame - DELAY];

            session1.receive_remote_input(0, f0, act0);
            session0.receive_remote_input(1, f1, act1);
        }
    }

    // Flush remaining delayed packets in the pipe
    for i in (100 - DELAY)..100 {
        let (f0, act0) = p0_to_p1_pipe[i];
        let (f1, act1) = p1_to_p0_pipe[i];
        session1.receive_remote_input(0, f0, act0);
        session0.receive_remote_input(1, f1, act1);
    }

    // Both sessions must have executed rollbacks
    assert!(session0.total_rollbacks > 0, "P0 must have executed speculative rollbacks");
    assert!(session1.total_rollbacks > 0, "P1 must have executed speculative rollbacks");

    // Absolute 100% bit-identical state parity after all delayed packets resolved
    assert_eq!(
        session0.current_state.compute_hash(),
        session1.current_state.compute_hash(),
        "State hashes must converge to identical bits after rollback reconciliation!"
    );
    assert_eq!(session0.current_state.player.hp, session1.current_state.player.hp);
    assert_eq!(session0.current_state.opponent.hp, session1.current_state.opponent.hp);
}

#[test]
fn test_boxing_rollback_spectator_parity() {
    let initial_state = BoxingRingEngine::new(777);
    let mut session0 = BoxingRollbackSession::<64>::new(0, initial_state);
    let mut session1 = BoxingRollbackSession::<64>::new(1, initial_state);
    let mut spectator = BoxingRollbackSession::<64>::new(2, initial_state); // Spectator = 2

    // Run 60 frames with variable packet arrival
    for frame in 0..60 {
        let in0 = if frame % 10 == 2 { ActionKind::Jab } else { ActionKind::Idle };
        let in1 = if frame % 14 == 5 { ActionKind::Hook } else { ActionKind::Idle };

        session0.advance_frame(in0);
        session1.advance_frame(in1);
        spectator.advance_frame(ActionKind::Idle); // Spectator advances time

        // Spectator receives inputs from both
        spectator.receive_remote_input(0, frame as u64, in0);
        spectator.receive_remote_input(1, frame as u64, in1);

        session0.receive_remote_input(1, frame as u64, in1);
        session1.receive_remote_input(0, frame as u64, in0);
    }

    let hash0 = session0.current_state.compute_hash();
    let hash1 = session1.current_state.compute_hash();
    let hash_spec = spectator.current_state.compute_hash();

    assert_eq!(hash0, hash1);
    assert_eq!(hash0, hash_spec, "Spectator state hash must match participant state hash 100%");
}
