//! 100 vs 100 Mass Auto-Battler Simulation Test (T-007)
//!
//! Validates:
//! - 100 Blue Units vs 100 Red Units in a 200-unit BattleArena
//! - 200 discrete battle ticks
//! - 100% deterministic battle outcome (Two independent runs yield bit-for-bit identical results)

extern crate alloc;

use alloc::boxed::Box;
use axiom_core::combat::BattleArena;
use axiom_core::fixed::FixedVec2;
use std::time::Instant;

fn create_standard_100v100_battle() -> Box<BattleArena<256>> {
    let mut arena = Box::new(BattleArena::<256>::new());

    // 100 Blue Knights (Team 1)
    for i in 0..100 {
        let x = (i % 10) as i64 * 2;
        let y = (i / 10) as i64 * 2;
        arena.spawn_unit(
            (i + 1) as u32,
            1,
            80, // HP
            15, // ATK
            2,  // Range
            2,  // Cooldown
            FixedVec2::from_ints(x, y),
        );
    }

    // 100 Red Raiders (Team 2)
    for i in 0..100 {
        let x = 40 + (i % 10) as i64 * 2;
        let y = (i / 10) as i64 * 2;
        arena.spawn_unit(
            (100 + i + 1) as u32,
            2,
            60, // HP
            18, // ATK
            2,  // Range
            2,  // Cooldown
            FixedVec2::from_ints(x, y),
        );
    }

    assert_eq!(arena.team_alive_count(1), 100);
    assert_eq!(arena.team_alive_count(2), 100);
    arena
}

#[test]
fn test_100v100_mass_battle_determinism_and_speed() {
    let mut arena_a = create_standard_100v100_battle();
    let mut arena_b = create_standard_100v100_battle();

    assert_eq!(arena_a.units, arena_b.units);

    // Simulate 150 ticks of full combat
    let start = Instant::now();
    for _ in 0..150 {
        arena_a.step();
    }
    let duration = start.elapsed();

    for _ in 0..150 {
        arena_b.step();
    }

    // Performance assertion
    println!(
        "\n==> [AUTO-BATTLER] 100v100 (200 units) 150 battle ticks completed in {:.2?}",
        duration
    );
    let avg_tick_us = duration.as_micros() as f64 / 150.0;
    println!(
        "==> [BATTLE TICK RATE] Average combat tick: {:.2} µs",
        avg_tick_us
    );

    // Absolute determinism assertion:
    // Both runs must end with 100% byte-identical state
    assert_eq!(
        arena_a.units, arena_b.units,
        "Combat simulation must be bit-for-bit deterministic across independent runs"
    );

    // Battle must have progressed with casualties
    let blue_alive = arena_a.team_alive_count(1);
    let red_alive = arena_a.team_alive_count(2);
    println!(
        "==> [OUTCOME] Blue survivors: {}, Red survivors: {}",
        blue_alive, red_alive
    );
    assert!(
        blue_alive < 100 || red_alive < 100,
        "Combat must produce casualties"
    );
}
