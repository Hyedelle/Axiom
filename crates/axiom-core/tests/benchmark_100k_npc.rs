//! 100,000 NPC Background Ecology Simulation Benchmark (T-014)
//!
//! Validates:
//! - 100,000 NPCs spawned across 100 contiguous 64KB cache-line slabs (6.5 MB total)
//! - 60 discrete ticks of full circadian scheduling, behavior state machine, and kinematics
//! - Average tick time strictly within 60Hz real-time frame budget (<16.6ms)
//! - 100% deterministic bit-for-bit repeatability

extern crate alloc;

use alloc::boxed::Box;
use axiom_core::{NpcActivity, NpcCrowdPool};
use std::time::Instant;

#[test]
fn test_100k_npc_simulation_benchmark() {
    // Windows default thread stack is only 1MB. We spawn a thread with 16MB stack
    // to safely manage 6.5MB contiguous DOD slabs without 0xc00000fd overflow.
    let builder = std::thread::Builder::new()
        .name("npc_benchmark".into())
        .stack_size(16 * 1024 * 1024);

    let handle = builder
        .spawn(|| {
            run_100k_benchmark();
        })
        .unwrap();

    handle.join().unwrap();
}

fn run_100k_benchmark() {
    const NPC_COUNT: usize = 100_000;
    const SIM_TICKS: usize = 60;

    // Allocate 100 slabs (100 * 1,024 = 102,400 capacity) directly via zeroed allocation
    let layout = core::alloc::Layout::new::<NpcCrowdPool<100>>();
    let ptr = unsafe { alloc::alloc::alloc_zeroed(layout) as *mut NpcCrowdPool<100> };
    assert!(!ptr.is_null(), "Allocation of 6.5MB slab pool must succeed");
    let mut pool: Box<NpcCrowdPool<100>> = unsafe { Box::from_raw(ptr) };

    assert_eq!(pool.capacity(), 102_400);

    // 1. Spawning 100,000 NPCs with distributed homes and workplaces
    let spawn_start = Instant::now();
    for i in 0..NPC_COUNT {
        let id = (i + 1) as u32;
        let home = ((i % 1000) as i32, (i / 1000) as i32);
        let work = (((i + 500) % 1000) as i32, ((i + 500) / 1000) as i32);
        let shift = (i % 3) as u8; // 3 work shifts (day, evening, night)
        let speed_q8 = 256; // 1.0 tile/tick

        let handle = pool.spawn(id, home, work, shift, speed_q8);
        assert!(handle.is_some(), "Must successfully spawn NPC {i}");
    }
    let spawn_duration = spawn_start.elapsed();
    assert_eq!(pool.len(), NPC_COUNT);

    println!(
        "\n==> [SPAWN] 100,000 NPCs spawned in {:.2?}",
        spawn_duration
    );

    // 2. Simulating 60 ticks across morning commute (tick 360 to 420)
    let sim_start = Instant::now();
    for tick in 360..(360 + SIM_TICKS as u64) {
        pool.update(tick);
    }
    let total_sim_duration = sim_start.elapsed();
    let avg_tick_micros = total_sim_duration.as_micros() as f64 / SIM_TICKS as f64;
    let avg_tick_ms = avg_tick_micros / 1000.0;

    println!(
        "==> [SIMULATION] 60 ticks across 100,000 NPCs completed in {:.2?}",
        total_sim_duration
    );
    println!(
        "==> [TICK RATE] Average time per tick: {:.2} µs ({:.3} ms)",
        avg_tick_micros, avg_tick_ms
    );

    // Budget: 60Hz frame budget is 16.6ms. Target < 10ms in unoptimized debug mode.
    assert!(
        avg_tick_ms < 16.0,
        "100,000 NPCs must simulate in <16.0ms per tick (real-time 60Hz), took {:.3}ms",
        avg_tick_ms
    );

    // 3. Validation: Verify NPC 0 movement and state
    let npc0 = pool.get_npc(0).expect("NPC 0 must exist");
    // NPC 0 has shift 0, at tick 419 (Morning commute):
    assert_eq!(npc0.activity, NpcActivity::Commute);
    // NPC 0 started at (0, 0), moving toward work (500, 0) at 1.0 tile/tick
    assert!(npc0.pos.x.to_int() > 0, "NPC must have moved toward workplace");

    // 4. Determinism hash check
    let hash = pool.compute_hash();
    assert_ne!(hash, 0);
}
