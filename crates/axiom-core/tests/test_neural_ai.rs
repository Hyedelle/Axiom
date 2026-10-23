use axiom_core::neural_ai::{
    infer_action, CombatObservation, NeuralAiWeights, NN_HIDDEN_DIM, NN_INPUT_DIM, NN_OUTPUT_DIM,
};
use core::mem::{align_of, size_of};

#[test]
fn test_neural_ai_weights_geometry_and_alignment() {
    assert_eq!(size_of::<NeuralAiWeights>(), 320);
    assert_eq!(align_of::<NeuralAiWeights>(), 64);
    assert_eq!(NN_INPUT_DIM, 8);
    assert_eq!(NN_HIDDEN_DIM, 16);
    assert_eq!(NN_OUTPUT_DIM, 8);

    let default_weights = NeuralAiWeights::default();
    assert_eq!(default_weights.model_version, 1);
    assert_eq!(default_weights.checksum, 0xA101_B202);
}

#[test]
fn test_deterministic_neural_inference_reproducibility() {
    let weights = NeuralAiWeights::default();

    let obs_danger = CombatObservation {
        rel_distance: 12,
        player_hp_pct: 350,
        opponent_hp_pct: 900,
        player_heat_pct: 200,
        player_action: 0,
        opponent_action: 3, // Heavy punch incoming
        player_combo_streak: 0,
        opponent_telegraph_rem: 15, // Active telegraph
    };

    // Forward inference 100 times must yield identical output
    let action_first = infer_action(&weights, &obs_danger);
    for _ in 0..100 {
        let action = infer_action(&weights, &obs_danger);
        assert_eq!(action, action_first);
    }
}

#[test]
fn test_neural_inference_variations() {
    let weights = NeuralAiWeights::default();

    let obs_neutral = CombatObservation::default();
    let action_neutral = infer_action(&weights, &obs_neutral);
    assert!(action_neutral < NN_OUTPUT_DIM as u8);

    let obs_close_combat = CombatObservation {
        rel_distance: 5,
        player_hp_pct: 1000,
        opponent_hp_pct: 200,
        player_heat_pct: 100,
        player_action: 1,
        opponent_action: 0,
        player_combo_streak: 4,
        opponent_telegraph_rem: 0,
    };
    let action_close = infer_action(&weights, &obs_close_combat);
    assert!(action_close < NN_OUTPUT_DIM as u8);
}
