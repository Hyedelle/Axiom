//! Deterministic Unit Tests for Axiom Punch-Out Boxing Engine (T-075).

use axiom_game::boxing::{ActionKind, AiPhase, BoxingEvent, BoxingRingEngine};
use axiom_game::card::{CardKind, DeckState, TacticalCard};

#[test]
fn test_boxing_mechanics_and_determinism() {
    let seed = 0xDEAD_BEEF_1234;
    let mut engine1 = BoxingRingEngine::new(seed);
    let mut engine2 = BoxingRingEngine::new(seed);

    assert_eq!(engine1.player.hp, 100);
    assert_eq!(engine1.opponent.hp, 120);
    assert_eq!(engine1.compute_hash(), engine2.compute_hash());

    // 1. Advance until opponent enters telegraph phase
    let mut saw_telegraph = false;
    let mut saw_evasion = false;
    let mut saw_counter = false;

    for tick in 0..300 {
        let input = if engine1.ai_phase == AiPhase::Telegraph {
            saw_telegraph = true;
            ActionKind::SlipLeft
        } else if saw_evasion && !saw_counter {
            ActionKind::Hook
        } else if tick % 30 == 0 {
            ActionKind::Jab
        } else {
            ActionKind::Idle
        };

        let ev1 = engine1.step(input);
        let _ev2 = engine2.step(input);

        // Bit-for-bit identical state hash
        assert_eq!(
            engine1.compute_hash(),
            engine2.compute_hash(),
            "Engines diverged at tick {}",
            tick
        );

        match ev1 {
            BoxingEvent::PlayerEvaded => {
                saw_evasion = true;
            }
            BoxingEvent::PlayerPunched { damage: _, is_counter } => {
                if is_counter {
                    saw_counter = true;
                }
            }
            _ => {}
        }
    }

    assert!(saw_telegraph, "Opponent AI must telegraph attack");
    assert!(saw_evasion, "Player must successfully slip evade");
    assert!(saw_counter, "Player must land counter attack after evasion");
    assert!(engine1.opponent.hp < 120, "Opponent must have taken damage");
}

#[test]
fn test_guard_blocks_and_reduces_damage() {
    let mut engine = BoxingRingEngine::new(42);

    // Fast-forward until opponent attacks
    while engine.ai_phase != AiPhase::Telegraph {
        engine.step(ActionKind::Idle);
    }

    let hp_before = engine.player.hp;

    // Hold guard through the attack
    let mut saw_block = false;
    for _ in 0..80 {
        let ev = engine.step(ActionKind::Guard);
        if let BoxingEvent::PlayerBlocked { chip } = ev {
            saw_block = true;
            assert!(chip <= 8, "Blocked chip damage must be small (<= 8)");
        }
    }

    assert!(saw_block, "Holding guard must trigger PlayerBlocked");
    assert!(
        engine.player.hp >= hp_before - 8,
        "Player HP must be protected by guard"
    );
}

#[test]
fn test_tactical_card_deck() {
    let mut deck = DeckState::<16>::new();
    assert_eq!(deck.count, 3);
    assert_eq!(deck.active_card().kind, CardKind::Straight);

    deck.select_active(1);
    assert_eq!(deck.active_card().kind, CardKind::Guard);

    deck.select_active(2);
    assert_eq!(deck.active_card().kind, CardKind::OverheatHook);

    // Add loot cards
    assert!(deck.add_card(TacticalCard::counter_slip()));
    assert!(deck.add_card(TacticalCard::plasma_upper()));
    assert_eq!(deck.count, 5);
}

#[test]
fn test_ai_difficulty_balancing() {
    use axiom_game::boxing::AiDifficulty;

    let mut normal_engine = BoxingRingEngine::new(0x1234_5678);
    let mut hard_engine = BoxingRingEngine::new(0x1234_5678);
    let mut nightmare_engine = BoxingRingEngine::new(0x1234_5678);

    hard_engine.set_difficulty(AiDifficulty::Hard);
    nightmare_engine.set_difficulty(AiDifficulty::Nightmare);

    assert_eq!(normal_engine.difficulty, AiDifficulty::Normal);
    assert_eq!(hard_engine.difficulty, AiDifficulty::Hard);
    assert_eq!(nightmare_engine.difficulty, AiDifficulty::Nightmare);

    // Initial hashes must differ due to difficulty state
    assert_ne!(normal_engine.compute_hash(), hard_engine.compute_hash());

    // Advance 60 frames
    for _ in 0..60 {
        normal_engine.step(ActionKind::Idle);
        hard_engine.step(ActionKind::Idle);
        nightmare_engine.step(ActionKind::Idle);
    }

    // Hard & Nightmare AI should transition to Telegraph faster than Normal
    assert_eq!(normal_engine.difficulty.next(), AiDifficulty::Hard);
    assert_eq!(hard_engine.difficulty.next(), AiDifficulty::Nightmare);
    assert_eq!(nightmare_engine.difficulty.next(), AiDifficulty::Normal);
}
