//! Axiom Headless CLI Entry Point.
//!
//! Provides CLI execution for fast deterministic replay, state hashing, and verification.

use axiom_cli::Replayer;
use axiom_core::fixed::FixedVec2;
use axiom_core::input::InputState;
use axiom_core::state::TickState;

fn main() {
    println!("🏛️ [Axiom Engine] Headless CLI Replayer");

    let mut initial_state = TickState::<32>::new();
    initial_state.spawn(1, FixedVec2::from_ints(10, 20), FixedVec2::ZERO);

    // Generate 1,000 deterministic inputs
    let inputs: Vec<InputState> = (0..1000)
        .map(|i| {
            let mx = if i % 2 == 0 { 1 } else { -1 };
            let my = if i % 3 == 0 { 1 } else { 0 };
            InputState::new(mx, my, 0)
        })
        .collect();

    let replayer = Replayer::new(initial_state, inputs);

    if replayer.verify_determinism() {
        let report = replayer.run();
        println!(
            "[PASS] Replayed {} ticks with 100% determinism. Final Hash: 0x{:016X}",
            report.total_ticks, report.final_hash
        );
    } else {
        eprintln!("[FAIL] Determinism mismatch detected during replay!");
        std::process::exit(1);
    }
}
