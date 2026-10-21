//! Deterministic Headless Replay Engine.
//!
//! Replays input sequences across discrete ticks without GUI or floating-point drift,
//! verifying bit-for-bit hash consistency.

use axiom_core::input::InputState;
use axiom_core::state::TickState;

/// Result summary of an executed replay session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayReport<const N: usize> {
    pub total_ticks: usize,
    pub final_state: TickState<N>,
    pub final_hash: u64,
}

/// Headless replay driver executing input streams against pure tick state transitions.
pub struct Replayer<const N: usize = 32> {
    initial_state: TickState<N>,
    inputs: Vec<InputState>,
}

impl<const N: usize> Replayer<N> {
    /// Creates a new replayer with an initial world state and pre-recorded inputs.
    pub fn new(initial_state: TickState<N>, inputs: Vec<InputState>) -> Self {
        Self {
            initial_state,
            inputs,
        }
    }

    /// Executes the full input stream step-by-step, returning the replay report.
    pub fn run(&self) -> ReplayReport<N> {
        let mut state = self.initial_state;

        for input in &self.inputs {
            state = state.step(input);
        }

        ReplayReport {
            total_ticks: self.inputs.len(),
            final_state: state,
            final_hash: state.compute_hash(),
        }
    }

    /// Verifies that two independent executions produce 100% bit-identical results.
    pub fn verify_determinism(&self) -> bool {
        let run_1 = self.run();
        let run_2 = self.run();

        run_1.final_hash == run_2.final_hash && run_1.final_state == run_2.final_state
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_core::fixed::FixedVec2;

    #[test]
    fn test_replay_determinism_consistency() {
        let mut state = TickState::<16>::new();
        state.spawn(1, FixedVec2::from_ints(0, 0), FixedVec2::ZERO);

        let inputs = vec![InputState::new(1, 0, 0); 100];

        let replayer = Replayer::new(state, inputs);
        assert!(
            replayer.verify_determinism(),
            "Replay must be 100% deterministic"
        );

        let report = replayer.run();
        assert_eq!(report.total_ticks, 100);
        assert_eq!(report.final_state.tick.0, 100);
    }
}
