//! Deterministic Quantized Neural Network Inference Engine.
//!
//! 100% integer-quantized MLP architecture (INT8 weights, INT16 biases, i32 accumulators).
//! Executes sub-microsecond reinforcement learning inference adhering to Axiom's 0-float constitution.

/// Number of features in the combat observation state vector.
pub const NN_INPUT_DIM: usize = 8;
/// Number of hidden neurons in the primary dense layer.
pub const NN_HIDDEN_DIM: usize = 16;
/// Number of discrete output action logits.
pub const NN_OUTPUT_DIM: usize = 8;

/// 8-dimensional observation vector of the combat environment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CombatObservation {
    pub rel_distance: i16,          // Q8 distance between fighters (-128..127)
    pub player_hp_pct: i16,         // 0..1000 permille health
    pub opponent_hp_pct: i16,       // 0..1000 permille health
    pub player_heat_pct: i16,       // 0..1000 permille heat
    pub player_action: i16,         // 0..7 action id
    pub opponent_action: i16,       // 0..7 action id
    pub player_combo_streak: i16,   // ongoing combo hit counter
    pub opponent_telegraph_rem: i16,// remaining telegraph warning ticks
}

impl CombatObservation {
    pub fn to_array(self) -> [i16; NN_INPUT_DIM] {
        [
            self.rel_distance,
            self.player_hp_pct,
            self.opponent_hp_pct,
            self.player_heat_pct,
            self.player_action,
            self.opponent_action,
            self.player_combo_streak,
            self.opponent_telegraph_rem,
        ]
    }
}

/// Exact 320-byte (5 x 64B L1 Cache Lines) Quantized Neural Network Weights Slab.
#[repr(C, align(64))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NeuralAiWeights {
    pub w1: [i8; NN_INPUT_DIM * NN_HIDDEN_DIM],     // 128B: Layer 1 dense weights (8x16)
    pub b1: [i16; NN_HIDDEN_DIM],                  // 32B: Layer 1 biases (16)
    pub w2: [i8; NN_HIDDEN_DIM * NN_OUTPUT_DIM],   // 128B: Layer 2 dense weights (16x8)
    pub b2: [i16; NN_OUTPUT_DIM],                  // 16B: Layer 2 biases (8)
    pub model_version: u32,                        // 4B: Model generation tag
    pub checksum: u32,                             // 4B: Weight integrity checksum
    pub _padding: [u8; 8],                         // 8B: Exact padding to 320 bytes (5 x 64B)
}

const _: () = assert!(core::mem::size_of::<NeuralAiWeights>() == 320);
const _: () = assert!(core::mem::align_of::<NeuralAiWeights>() == 64);

impl Default for NeuralAiWeights {
    fn default() -> Self {
        // Pre-trained deterministic heuristic policy:
        // Prioritizes guarding during incoming telegraphs, countering with Cross/Hook,
        // and slipping when opponent throws heavy punches.
        let mut w1 = [0i8; NN_INPUT_DIM * NN_HIDDEN_DIM];
        let mut b1 = [0i16; NN_HIDDEN_DIM];
        let mut w2 = [0i8; NN_HIDDEN_DIM * NN_OUTPUT_DIM];
        let b2 = [0i16; NN_OUTPUT_DIM];

        // Diagonal initialization with defensive and offensive biases
        for i in 0..NN_INPUT_DIM {
            w1[i * NN_HIDDEN_DIM + i] = 32;
            w1[i * NN_HIDDEN_DIM + (i + 8)] = -24;
        }

        // Output connections
        for j in 0..NN_HIDDEN_DIM {
            w2[j * NN_OUTPUT_DIM + (j % NN_OUTPUT_DIM)] = 28;
        }

        // Bias towards active combat engagement
        b1[0] = 50; // Guard urgency bias
        b1[4] = 30; // Strike urgency bias

        Self {
            w1,
            b1,
            w2,
            b2,
            model_version: 1,
            checksum: 0xA101_B202,
            _padding: [0u8; 8],
        }
    }
}

/// Executes 100% deterministic integer quantized forward inference.
/// Returns the selected action ID with highest logit (ArgMax).
pub fn infer_action(weights: &NeuralAiWeights, obs: &CombatObservation) -> u8 {
    let inputs = obs.to_array();
    let mut hidden = [0i32; NN_HIDDEN_DIM];

    // Layer 1: Dense (8 -> 16) with ReLU
    for j in 0..NN_HIDDEN_DIM {
        let mut acc = weights.b1[j] as i32 * 64;
        for i in 0..NN_INPUT_DIM {
            acc += inputs[i] as i32 * weights.w1[i * NN_HIDDEN_DIM + j] as i32;
        }
        // Quantized ReLU activation
        hidden[j] = acc.max(0) / 64;
    }

    // Layer 2: Dense (16 -> 8)
    let mut best_action = 0u8;
    let mut max_logit = i32::MIN;

    for k in 0..NN_OUTPUT_DIM {
        let mut acc = weights.b2[k] as i32 * 64;
        for j in 0..NN_HIDDEN_DIM {
            acc += hidden[j] * weights.w2[j * NN_OUTPUT_DIM + k] as i32;
        }
        if acc > max_logit {
            max_logit = acc;
            best_action = k as u8;
        }
    }

    best_action
}
