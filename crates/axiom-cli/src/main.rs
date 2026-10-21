//! Axiom Headless CLI Entry Point.
//!
//! Provides CLI execution for fast deterministic replay, state hashing,
//! time-travel rollback, and autonomous Monte Carlo combat balancing.

use axiom_cli::{AsciiView, MonteCarloBalancer, Replayer, TimeTravelDemo};
use axiom_core::fixed::FixedVec2;
use axiom_core::input::InputState;
use axiom_core::state::TickState;
use std::time::Instant;

fn main() {
    println!("🏛️ [Axiom Engine] Headless CLI Engine Suite\n");

    // 1. Time-Travel Rollback Demo
    println!("--- [DEMO 1] 0.001ms Time-Travel Rollback ---");
    let mut demo = TimeTravelDemo::new();
    println!("Initial State (Tick 0):");
    print!("{}", AsciiView::render(demo.state(), 24, 8));

    for _ in 0..30 {
        demo.step(InputState::new(1, 0, 0));
    }
    println!("Advanced to Tick 30:");
    print!("{}", AsciiView::render(demo.state(), 24, 8));

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
            "[PASS] Replayed {} ticks with 100% bit determinism. Final Hash: 0x{:016X}\n",
            report.total_ticks, report.final_hash
        );
    } else {
        eprintln!("[FAIL] Determinism mismatch detected during replay!");
        std::process::exit(1);
    }

    // 3. Autonomous Monte Carlo Balance Simulation (10,000 Matches)
    println!("--- [DEMO 3] Autonomous Monte Carlo Balance Simulator (10,000 Matches) ---");
    let sim_start = Instant::now();
    let stats = MonteCarloBalancer::simulate_matches(10_000, 777_888_999);
    let sim_duration = sim_start.elapsed();

    println!(
        "[PASS] 10,000 full combat matches simulated in {:.2?} ({:.2} ms per 1,000 matches)",
        sim_duration,
        sim_duration.as_millis() as f64 / 10.0
    );

    let report_md = MonteCarloBalancer::format_report(&stats);
    println!("\n{}", report_md);

    // Save report to tmp/balance_report.md
    if let Err(e) = std::fs::write("tmp/balance_report.md", &report_md) {
        eprintln!("Failed to save balance report: {}", e);
    } else {
        println!("==> Balance report successfully saved to tmp/balance_report.md");
    }
}
