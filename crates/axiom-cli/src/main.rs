//! Axiom Headless CLI Entry Point.
//!
//! Provides CLI execution for fast deterministic replay, state hashing, and time-travel rollback.

use axiom_cli::{AsciiView, Replayer, TimeTravelDemo};
use axiom_core::fixed::FixedVec2;
use axiom_core::input::InputState;
use axiom_core::state::TickState;

fn main() {
    println!("🏛️ [Axiom Engine] Headless CLI Replayer & Time-Travel Debugger\n");

    // 1. Time-Travel Rollback Demo
    println!("--- [DEMO 1] 0.001ms Time-Travel Rollback ---");
    let mut demo = TimeTravelDemo::new();
    println!("Initial State (Tick 0):");
    print!("{}", AsciiView::render(demo.state(), 24, 8));

    // Simulate 30 ticks forward
    for _ in 0..30 {
        demo.step(InputState::new(1, 0, 0));
    }
    println!("Advanced to Tick 30:");
    print!("{}", AsciiView::render(demo.state(), 24, 8));

    // Instantaneous rollback to tick 10
    println!("Rewinding time back to Tick 10 (Time-Travel Undo)...");
    demo.rollback_to(10).expect("Rollback failed");
    print!("{}", AsciiView::render(demo.state(), 24, 8));

    // 2. 1,000 Tick Replay Determinism Verification
    println!("--- [DEMO 2] 1,000-Tick Deterministic Replay ---");
    let mut initial_state = TickState::<32>::new();
    initial_state.spawn(1, FixedVec2::from_ints(10, 20), FixedVec2::ZERO);

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
            "[PASS] Replayed {} ticks with 100% bit determinism. Final Hash: 0x{:016X}",
            report.total_ticks, report.final_hash
        );
    } else {
        eprintln!("[FAIL] Determinism mismatch detected during replay!");
        std::process::exit(1);
    }
}
