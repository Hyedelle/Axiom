//! Comprehensive Integrated Open-World Headless Session Test (T-016)
//!
//! Validates:
//! - Full integration of all 4 Phase subsystems:
//!   1. Chunk streaming
//!   2. 100,000 NPC Living Ecology
//!   3. Factory Logistics conveyor belt network
//!   4. Auto-Battler combat units
//! - 100% deterministic bit-identical master hash across separate runs
//! - Real-time performance well within 60Hz frame budget (<16.6ms)

use axiom_cli::HeadlessSession;

#[test]
fn test_headless_session_full_integration_and_determinism() {
    let builder = std::thread::Builder::new()
        .name("session_test".into())
        .stack_size(16 * 1024 * 1024);

    let handle = builder
        .spawn(|| {
            run_session_integration();
        })
        .unwrap();

    handle.join().unwrap();
}

fn run_session_integration() {
    let seed = 424242;

    // Run Session A: 60 full ticks
    let mut session_a = HeadlessSession::new(seed);
    assert_eq!(session_a.npcs.len(), 100_000);
    assert!(session_a.chunk_mgr.loaded_count() > 0);

    let report_a = session_a.run_session(60);

    println!(
        "\n==> [SESSION A] 60 ticks completed in {:.2}ms ({:.2} µs/tick, {:.1} FPS equivalent)",
        report_a.elapsed_micros as f64 / 1000.0,
        report_a.avg_micros_per_tick,
        report_a.fps_equivalent
    );
    println!("==> Master State Hash: 0x{:016X}", report_a.final_hash);
    println!("==> Total Active NPCs: {}", report_a.total_npcs);
    println!("==> Chunks Loaded: {}", report_a.loaded_chunks);
    println!("==> Logistics Items: {}", report_a.total_logistics_items);

    // Assertions on subsystem evolution
    assert_eq!(report_a.total_ticks, 60);
    assert_eq!(report_a.total_npcs, 100_000);
    assert!(report_a.total_chunks_generated > 0);

    // Performance budget check: Must easily run under 16.6ms 60Hz frame budget
    assert!(
        report_a.avg_micros_per_tick < 16_000.0,
        "Integrated tick must be <16.0ms, was {:.2}ms",
        report_a.avg_micros_per_tick / 1000.0
    );

    // Run Session B with identical seed: 60 full ticks
    let mut session_b = HeadlessSession::new(seed);
    let report_b = session_b.run_session(60);

    // Assert 100% Bit-for-bit master hash match!
    assert_eq!(
        report_a.final_hash, report_b.final_hash,
        "Master state hash across all 4 combined subsystems must be bit-identical!"
    );
}
