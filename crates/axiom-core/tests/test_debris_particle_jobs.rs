//! Integration tests for Debris Particle Multithreaded Job System & Event Bus Pipeline.

use axiom_core::debris_particle::{
    dispatch_debris_with_jobs, emit_debris_burst_to_event_bus, DebrisBurstReport,
    DebrisParticlePool, DEBRIS_FLAG_ACTIVE, DEBRIS_MAT_STONE, EVENT_DEBRIS_BURST,
};
use axiom_core::event_bus::{channel, DeterministicEventBus};
use axiom_core::job_system::JobSystem;
use axiom_core::worker_pool::WorkerPoolCoordinator;

#[test]
fn test_debris_batch_job_and_worker_pool_dispatch() {
    let mut pool = DebrisParticlePool::<128>::new(12345, 0, 3000);
    let spawned = pool.scatter_debris_burst(
        0, 500, 0,
        48,
        3000, 10000,
        DEBRIS_MAT_STONE,
        1,
        12345,
    );
    assert_eq!(spawned, 48);
    assert_eq!(pool.active_count(), 48);

    let mut job_system = JobSystem::<64>::new();
    let mut coordinator = WorkerPoolCoordinator::<4, 32>::new(4);

    let dispatched = dispatch_debris_with_jobs(&pool, 4, &mut job_system, &mut coordinator);
    assert_eq!(dispatched, 4, "Should successfully dispatch 4 batch jobs across workers");

    // Verify all jobs are placed in worker deques
    let mut total_enqueued = 0;
    for w in 0..4 {
        total_enqueued += coordinator.deques[w].len();
    }
    assert_eq!(total_enqueued, 4);

    // Drain jobs through coordinator
    coordinator.drain_all(&mut job_system);
}

#[test]
fn test_debris_burst_event_bus_emission_and_rollback() {
    let mut bus = DeterministicEventBus::<64>::new();

    let report_1 = DebrisBurstReport::from_burst(
        (-500, 0, 500),
        24,
        2000,
        6000,
        8192,
        DEBRIS_MAT_STONE,
        101,
        25000,
    );

    let report_2 = DebrisBurstReport::from_burst(
        (1000, 200, -1000),
        36,
        4000,
        9000,
        16384,
        DEBRIS_MAT_STONE,
        102,
        50000,
    );

    assert!(emit_debris_burst_to_event_bus(&report_1, 10, &mut bus));
    assert!(emit_debris_burst_to_event_bus(&report_2, 11, &mut bus));
    assert_eq!(bus.count, 2);

    // Check event 0
    let ev0 = &bus.events[0];
    assert_eq!(ev0.tick, 10);
    assert_eq!(ev0.event_type, EVENT_DEBRIS_BURST);
    assert_eq!(ev0.channel_mask, channel::PHYSICS | channel::VFX);
    assert_eq!(ev0.source_id, 101);

    let dec0 = DebrisBurstReport::decode(&ev0.payload);
    assert_eq!(dec0.origin_x, -500);
    assert_eq!(dec0.origin_y, 0);
    assert_eq!(dec0.origin_z, 500);
    assert_eq!(dec0.particle_count, 24);
    assert_eq!(dec0.impulse_energy_j, 25000);
    assert_eq!(dec0.material, DEBRIS_MAT_STONE);
    assert_eq!(dec0.flags, DEBRIS_FLAG_ACTIVE);

    // Check event 1
    let ev1 = &bus.events[1];
    assert_eq!(ev1.tick, 11);
    assert_eq!(ev1.source_id, 102);
    let dec1 = DebrisBurstReport::decode(&ev1.payload);
    assert_eq!(dec1.origin_x, 1000);
    assert_eq!(dec1.particle_count, 36);
    assert_eq!(dec1.impulse_energy_j, 50000);

    // Test rollback to event 0 sequence: event 1 should be removed
    let seq0 = ev0.sequence;
    bus.rollback_to(seq0);
    assert_eq!(bus.count, 1);
    assert_eq!(bus.events[0].tick, 10);
}
