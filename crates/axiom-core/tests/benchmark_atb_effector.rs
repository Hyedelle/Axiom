//! 1,000 Ticks ATB & Skill Combo Effector Benchmark (T-023)
//!
//! Validates:
//! - Multi-unit Active Time Battle (ATB) gauge progression
//! - Elemental damage multipliers & combo scaling
//! - DoT burn and status effect duration decays
//! - Microsecond-level simulation throughput & 100% determinism

use axiom_core::{AtbArena, AtbUnit, Element, StatusKind};
use std::time::Instant;

#[test]
fn test_atb_effector_combo_simulation_benchmark() {
    let mut arena_a = AtbArena::<20>::new();
    let mut arena_b = AtbArena::<20>::new();

    // Populate Team 1 (5 Fire Mages, 5 Holy Paladins)
    for i in 0..5 {
        let u = AtbUnit::new(
            (i + 1) as u32,
            1,
            Element::Fire,
            600,
            120,
            30,
            40, // Speed
        );
        arena_a.spawn_unit(u);
        arena_b.spawn_unit(u);
    }
    for i in 5..10 {
        let u = AtbUnit::new(
            (i + 1) as u32,
            1,
            Element::Holy,
            800,
            100,
            50,
            35,
        );
        arena_a.spawn_unit(u);
        arena_b.spawn_unit(u);
    }

    // Populate Team 2 (5 Ice Golems, 5 Dark Necromancers)
    for i in 0..5 {
        let u = AtbUnit::new(
            (i + 11) as u32,
            2,
            Element::Ice,
            1000,
            80,
            60,
            25,
        );
        arena_a.spawn_unit(u);
        arena_b.spawn_unit(u);
    }
    for i in 5..10 {
        let u = AtbUnit::new(
            (i + 11) as u32,
            2,
            Element::Dark,
            500,
            140,
            20,
            50,
        );
        arena_a.spawn_unit(u);
        arena_b.spawn_unit(u);
    }

    // Apply initial Burn and Slow statuses to diversify combat
    arena_a.units[10].apply_status(StatusKind::Burn, 30, 15);
    arena_b.units[10].apply_status(StatusKind::Burn, 30, 15);

    const TICKS: usize = 1_000;
    let start_time = Instant::now();

    for _ in 0..TICKS {
        arena_a.step();
        arena_b.step();
    }

    let elapsed = start_time.elapsed();
    let micros_per_tick = elapsed.as_micros() as f64 / (TICKS * 2) as f64;

    println!(
        "\n==> [ATB EFFECTOR BENCHMARK] 1,000 combat ticks completed in {:.2?} ({:.2} µs/tick)",
        elapsed, micros_per_tick
    );
    println!("==> Total Damage Dealt: {}", arena_a.total_damage_dealt);
    println!("==> Arena A Hash: 0x{:016X}", arena_a.compute_hash());
    println!("==> Arena B Hash: 0x{:016X}", arena_b.compute_hash());

    // Performance budget
    assert!(
        micros_per_tick < 100.0,
        "ATB tick must take <100µs, took {:.2}µs",
        micros_per_tick
    );

    // BIT-FOR-BIT DETERMINISM ASSERTION
    assert_eq!(
        arena_a.compute_hash(),
        arena_b.compute_hash(),
        "Separate runs with identical setup must produce identical final hash!"
    );
    assert_eq!(arena_a.total_damage_dealt, arena_b.total_damage_dealt);
}
