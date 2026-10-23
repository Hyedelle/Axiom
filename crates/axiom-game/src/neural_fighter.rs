//! High-Level Neural Network Boxing Fighter AI Controller.
//!
//! Integrates `axiom-core` deterministic INT8 quantized neural inference
//! with `BoxingRingEngine` mecha boxing combat simulation.

use axiom_core::neural_ai::{infer_action, CombatObservation, NeuralAiWeights};
use crate::boxing::{ActionKind, AiPhase, BoxingEvent, BoxingRingEngine};

/// 384-byte (6 x 64B L1 Cache Lines) Neural Fighter Controller.
#[repr(C, align(64))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NeuralFighterController {
    pub weights: NeuralAiWeights,          // 320B (offset 0..320)
    pub last_observation: CombatObservation, // 16B (offset 320..336)
    pub inference_count: u32,             // 4B (offset 336..340)
    pub confidence_score: i16,            // 2B (offset 340..342)
    pub last_action: ActionKind,          // 1B (offset 342..343)
    pub mode: u8,                         // 1B (offset 343..344)
    pub _pad: [u8; 40],                   // 40B (offset 344..384)
}

const _: () = assert!(core::mem::size_of::<NeuralFighterController>() == 384);
const _: () = assert!(core::mem::align_of::<NeuralFighterController>() == 64);

impl Default for NeuralFighterController {
    fn default() -> Self {
        Self {
            weights: NeuralAiWeights::default(),
            last_observation: CombatObservation::default(),
            inference_count: 0,
            confidence_score: 0,
            last_action: ActionKind::Idle,
            mode: 0,
            _pad: [0u8; 40],
        }
    }
}

impl NeuralFighterController {
    /// Creates a new controller with given network weights.
    pub const fn new(weights: NeuralAiWeights) -> Self {
        Self {
            weights,
            last_observation: CombatObservation {
                rel_distance: 0,
                player_hp_pct: 1000,
                opponent_hp_pct: 1000,
                player_heat_pct: 0,
                player_action: 0,
                opponent_action: 0,
                player_combo_streak: 0,
                opponent_telegraph_rem: 0,
            },
            inference_count: 0,
            confidence_score: 0,
            last_action: ActionKind::Idle,
            mode: 0,
            _pad: [0u8; 40],
        }
    }

    /// Converts neural network 0..7 output logit to discrete `ActionKind`.
    pub const fn action_from_logit(logit: u8) -> ActionKind {
        match logit {
            0 => ActionKind::Idle,
            1 => ActionKind::SlipLeft,
            2 => ActionKind::SlipRight,
            3 => ActionKind::Guard,
            4 => ActionKind::Jab,
            5 => ActionKind::Cross,
            6 => ActionKind::Hook,
            7 => ActionKind::Finisher,
            _ => ActionKind::Idle,
        }
    }

    /// Extracts normalized combat observation from the player mecha perspective.
    pub fn observe_player(&self, ring: &BoxingRingEngine) -> CombatObservation {
        let max_p_hp = ring.player.max_hp.max(1);
        let max_o_hp = ring.opponent.max_hp.max(1);
        let max_p_heat = (ring.player.max_heat as i32).max(1);

        let p_hp_pct = ((ring.player.hp.max(0) as i64 * 1000) / max_p_hp as i64) as i16;
        let o_hp_pct = ((ring.opponent.hp.max(0) as i64 * 1000) / max_o_hp as i64) as i16;
        let p_heat_pct = ((ring.player.heat as i64 * 1000) / max_p_heat as i64) as i16;

        let telegraph_rem = if ring.ai_phase == AiPhase::Telegraph {
            ring.ai_timer as i16
        } else {
            0
        };

        CombatObservation {
            rel_distance: (ring.opponent.x_offset_px - ring.player.x_offset_px).clamp(-128, 127),
            player_hp_pct: p_hp_pct.clamp(0, 1000),
            opponent_hp_pct: o_hp_pct.clamp(0, 1000),
            player_heat_pct: p_heat_pct.clamp(0, 1000),
            player_action: ring.player.action.kind.to_u8() as i16,
            opponent_action: ring.opponent.action.kind.to_u8() as i16,
            player_combo_streak: ring.player.combo as i16,
            opponent_telegraph_rem: telegraph_rem,
        }
    }

    /// Predicts next optimal action using integer-quantized neural inference.
    pub fn decide_player_action(&mut self, ring: &BoxingRingEngine) -> ActionKind {
        let obs = self.observe_player(ring);
        self.last_observation = obs;
        let action_id = infer_action(&self.weights, &obs);
        let action = Self::action_from_logit(action_id);
        self.last_action = action;
        self.inference_count = self.inference_count.wrapping_add(1);
        action
    }

    /// Advances the boxing ring simulation by 1 tick using neural inference.
    pub fn step_player(&mut self, ring: &mut BoxingRingEngine) -> BoxingEvent {
        let action = self.decide_player_action(ring);
        ring.step(action)
    }
}
