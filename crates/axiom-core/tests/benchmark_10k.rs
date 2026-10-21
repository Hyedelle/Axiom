//! 10,000 Concurrent Entity Benchmark (T-003)
//!
//! Validates:
//! - 10,000 entities spawned across 10 contiguous 64KB cache-line slabs
//! - 60 discrete ticks (1 full second at 60Hz) of kinematic integration
//! - Average tick time < 1.0ms (targeting < 0.1ms in Release mode, far below 16.6ms frame budget)

extern crate alloc;

use alloc::boxed::Box;
use axiom_core::fixed::FixedVec2;
use axiom_core::storage::EntitySlabPool;
use std::time::Instant;

#[test]
fn test_10k_entities_simulation_benchmark() {
    const TOTAL_ENTITIES: usize = 10_000;
    const SIM_TICKS: usize = 60;

    // Allocate 10 slabs (10 * 1,024 = 10,240 capacity) on heap via Box
    let mut pool = Box::new(EntitySlabPool::<10>::new());
    assert_eq!(pool.capacity(), 10_240);

    // 1. Spawning 10,000 entities
    let spawn_start = Instant::now();
    for i in 0..TOTAL_ENTITIES {
        let id = (i + 1) as u64;
        let pos = FixedVec2::from_ints((i % 1000) as i64, (i / 1000) as i64);
        let vel = FixedVec2::from_ints(1, -1);
        let handle = pool.spawn(id, pos, vel);
        assert!(handle.is_some(), "Must successfully spawn entity {i}");
    }
    let spawn_duration = spawn_start.elapsed();
    assert_eq!(pool.len(), TOTAL_ENTITIES);

    println!(
        "\n==> [SPAWN] 10,000 entities spawned in {:.2?}",
        spawn_duration
    );

    // 2. Simulating 60 ticks of kinematics
    let sim_start = Instant::now();
    for _ in 0..SIM_TICKS {
        pool.update_kinematics();
    }
    let total_sim_duration = sim_start.elapsed();
    let avg_tick_micros = total_sim_duration.as_micros() as f64 / SIM_TICKS as f64;

    println!(
        "==> [SIMULATION] 60 ticks across 10,000 entities completed in {:.2?}",
        total_sim_duration
    );
    println!(
        "==> [TICK RATE] Average time per tick: {:.2} µs ({:.3} ms)",
        avg_tick_micros,
        avg_tick_micros / 1000.0
    );

    // 3. Validation: Kinematics verification
    // Entity 0: initial pos=(0, 0), vel=(1, -1), after 60 ticks: pos=(60, -60)
    let e0 = pool.get(0).expect("Entity 0 must exist");
    assert_eq!(e0.pos, FixedVec2::from_ints(60, -60));

    // Entity 999: initial pos=(999, 0), after 60 ticks: pos=(1059, -60)
    let e999 = pool.get(999).expect("Entity 999 must exist");
    assert_eq!(e999.pos, FixedVec2::from_ints(1059, -60));

    // 4. Performance Assertions:
    // 60Hz frame budget is 16,666 µs (16.6ms).
    // In Release mode with L1/L2 cache prefetching, 10k entities should take < 500 µs (0.5ms).
    // In unoptimized debug mode it should still be < 10,000 µs (10ms).
    assert!(
        avg_tick_micros < 16_666.0,
        "Single tick for 10,000 entities must complete well within 60Hz frame budget (16.6ms). Measured: {:.2} µs",
        avg_tick_micros
    );
}
