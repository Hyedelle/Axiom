use axiom_core::neural_ai::NeuralAiWeights;
use axiom_game::boxing::BoxingRingEngine;
use axiom_game::neural_fighter::NeuralFighterController;
use core::mem::{align_of, size_of};

#[test]
fn test_neural_fighter_geometry_and_alignment() {
    assert_eq!(size_of::<NeuralFighterController>(), 384);
    assert_eq!(align_of::<NeuralFighterController>(), 64);

    let controller = NeuralFighterController::default();
    assert_eq!(controller.inference_count, 0);
    assert_eq!(controller.weights.model_version, 1);
}

#[test]
fn test_neural_fighter_decision_loop() {
    let mut ring = BoxingRingEngine::new(42);
    let mut ai = NeuralFighterController::default();

    // Run 60 simulation ticks powered by neural network inference
    for _ in 0..60 {
        let _event = ai.step_player(&mut ring);
    }

    assert_eq!(ai.inference_count, 60);
    assert_eq!(ring.tick, 60);
}

#[test]
fn test_neural_fighter_observation_mapping() {
    let ring = BoxingRingEngine::new(99);
    let ai = NeuralFighterController::default();
    let obs = ai.observe_player(&ring);

    assert_eq!(obs.player_hp_pct, 1000);
    assert_eq!(obs.opponent_hp_pct, 1000);
    assert_eq!(obs.player_heat_pct, 0);
}
