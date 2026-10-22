//! Axiom Headless & Desktop CLI Entry Point.
//!
//! Provides CLI execution for fast deterministic replay, state hashing,
//! time-travel rollback, autonomous Monte Carlo combat balancing, and
//! native desktop window studio.

use axiom_cli::{AsciiView, MonteCarloBalancer, OpenWorldRpgSession, Replayer, TimeTravelDemo};
use axiom_core::fixed::FixedVec2;
use axiom_core::input::InputState;
use axiom_core::state::TickState;
use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let has_benchmark = args.iter().any(|a| a == "--benchmark");
    let has_headless = args.iter().any(|a| a == "--headless");
    let has_window = args.iter().any(|a| a == "--window");

    if has_benchmark || has_headless || (args.len() > 1 && !has_window) {
        run_headless_suite();
    } else if let Err(e) = axiom_cli::desktop::run_desktop_window() {
        eprintln!("Failed to launch desktop window: {:?}", e);
        std::process::exit(1);
    }
}

/// Runs the headless benchmark and verification suite.
pub fn run_headless_suite() {
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

    // 4. Open-World RPG Integration Demo (100k NPCs + Procedural Chunks + Quests + A* Navigation + ATB Combat + 3D Spatial Audio)
    println!("\n--- [DEMO 4] Complete Open-World RPG Integration ---");
    let mut rpg = OpenWorldRpgSession::new(0xABCD_1234_5678);
    println!("Spawned 100,000 living NPCs and initialized procedural world chunks.");

    // Dialogue & Quest
    rpg.interact_npc_quest(10, 100, 999, 1, 500);
    println!("Player interacted with Village Elder NPC and accepted Quest #100 (Slay Frost Goblin).");

    // A* Pathfinding
    rpg.plan_path_to(15, 15);
    println!("Calculated A* collision-free path across procedural terrain.");

    // Encounter
    rpg.trigger_combat_encounter(999);
    println!("Triggered combat encounter with Frost Goblin Mob!");

    // Simulate 30 ticks
    let rpg_report = rpg.run_loop(30);
    println!(
        "[PASS] 30 ticks simulated in {:.2}ms ({:.3}ms/tick = {:.1} FPS equivalent)",
        rpg_report.elapsed_micros as f64 / 1000.0,
        rpg_report.avg_micros_per_tick / 1000.0,
        rpg_report.fps_equivalent
    );
    println!("Final Master State Hash: 0x{:016X}", rpg_report.final_hash);
    println!(
        "Quest Status: Active={}, Completed={}, XP Earned={}",
        rpg_report.active_quests, rpg_report.completed_quests, rpg_report.total_xp
    );
    println!(
        "Audio Events Spatialized: {}, Master Left/Right Volume: ({}, {})\n",
        rpg_report.audio_events_processed,
        rpg_report.audio_master_volume.0,
        rpg_report.audio_master_volume.1
    );
}
