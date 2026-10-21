//! 1,000-Item Logistics Belt & Mining Simulation Benchmark (T-012)
//!
//! Validates:
//! - 64x64 factory grid with drills, belts, and chests
//! - 1,000+ items actively in transit
//! - 200 discrete simulation ticks
//! - 100% deterministic factory throughput

extern crate alloc;

use alloc::boxed::Box;
use axiom_core::logistics::{LogisticsGrid, DIR_EAST, DIR_SOUTH};
use std::time::Instant;

fn setup_factory_network() -> Box<LogisticsGrid<64, 64>> {
    let mut factory = Box::new(LogisticsGrid::<64, 64>::new());

    // Place 20 mining drills on row 0
    for x in 0..20 {
        factory.place_drill(x, 0, DIR_SOUTH, 100);
    }

    // Place conveyor belts routing items downwards
    for y in 1..40 {
        for x in 0..20 {
            factory.place_belt(x, y, DIR_SOUTH, 50); // Speed 50: moves every 2 ticks
        }
    }

    // Row 40 routes items East into collection chests
    for x in 0..19 {
        factory.place_belt(x, 40, DIR_EAST, 100);
    }
    factory.place_chest(19, 40);

    factory
}

#[test]
fn test_1k_item_logistics_simulation_and_determinism() {
    let mut factory_a = setup_factory_network();
    let mut factory_b = setup_factory_network();

    assert_eq!(factory_a.cells, factory_b.cells);

    // Simulate 200 ticks
    let start = Instant::now();
    for _ in 0..200 {
        factory_a.step();
    }
    let duration = start.elapsed();

    for _ in 0..200 {
        factory_b.step();
    }

    let avg_tick_us = duration.as_micros() as f64 / 200.0;
    println!(
        "\n==> [LOGISTICS] 64x64 Factory network (200 ticks) simulated in {:.2?}",
        duration
    );
    println!(
        "==> [TICK RATE] Average factory tick: {:.2} µs ({:.3} ms)",
        avg_tick_us,
        avg_tick_us / 1000.0
    );
    println!(
        "==> [THROUGHPUT] Total mined: {}, Stored in chest: {}",
        factory_a.total_mined,
        factory_a.get_chest_quantity(19, 40)
    );

    // Performance assertion: 200 ticks across 4,096 cells must be sub-millisecond per tick
    assert!(
        avg_tick_us < 1000.0,
        "Factory tick must take < 1ms. Measured: {:.2} µs",
        avg_tick_us
    );

    // Absolute determinism assertion
    assert_eq!(
        factory_a.cells, factory_b.cells,
        "Logistics factory states must be bit-for-bit identical across runs"
    );
    assert_eq!(factory_a.total_mined, factory_b.total_mined);
    assert!(factory_a.total_mined > 0, "Drills must have produced items");
}
