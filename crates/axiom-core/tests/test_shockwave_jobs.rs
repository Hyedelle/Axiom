//! Integration tests for Volumetric Shockwave Multithreaded Job System & Event Bus Pipeline (T-226).

use axiom_core::event_bus::{channel, DeterministicEventBus};
use axiom_core::job_system::JobSystem;
use axiom_core::shockwave::{
    dispatch_shockwave_with_jobs, emit_shockwave_to_event_bus, ShockwaveReport,
    VolumetricShockwaveEngine, EVENT_SHOCKWAVE, SHOCKWAVE_FLAG_ACTIVE,
    SHOCKWAVE_TYPE_SPHERICAL,
};
use axiom_core::worker_pool::WorkerPoolCoordinator;

#[test]
fn test_shockwave_batch_job_and_worker_pool_dispatch() {
    let mut engine = VolumetricShockwaveEngine::<16, 16>::new();

    // Spawn 4 shockwaves
    for i in 0..4 {
        engine.trigger_shockwave(
            ((i as i32) * 500, 100, 0),
            100,
            4000,
            600_000,
            200,
            50_000,
            (i + 1) as u32,
            1,
            SHOCKWAVE_TYPE_SPHERICAL,
            100,
        );
    }
    assert_eq!(engine.active_shockwaves_count(), 4);

    let mut job_system = JobSystem::<64>::new();
    let mut coordinator = WorkerPoolCoordinator::<4, 32>::new(4);

    let dispatched = dispatch_shockwave_with_jobs(&engine, 4, &mut job_system, &mut coordinator);
    assert_eq!(dispatched, 4, "Must dispatch 4 worker partition jobs");

    let mut total_enqueued = 0;
    for w in 0..4 {
        total_enqueued += coordinator.deques[w].len();
    }
    assert_eq!(total_enqueued, 4);

    // Drain all jobs
    coordinator.drain_all(&mut job_system);
    assert_eq!(job_system.active_count, 0);
}

#[test]
fn test_shockwave_event_bus_emission_and_rollback() {
    let mut bus = DeterministicEventBus::<64>::new();

    let report_1 = ShockwaveReport::from_explosion(
        (0, 100, 0),
        500,
        600_000,
        75_000,
        1,
        1,
        180,
    );

    let report_2 = ShockwaveReport::from_explosion(
        (1500, 200, -800),
        1200,
        450_000,
        50_000,
        2,
        2,
        90,
    );

    assert!(emit_shockwave_to_event_bus(&report_1, 10, &mut bus));
    assert!(emit_shockwave_to_event_bus(&report_2, 11, &mut bus));
    assert_eq!(bus.count, 2);

    // Check event 0
    let ev0 = &bus.events[0];
    assert_eq!(ev0.tick, 10);
    assert_eq!(ev0.event_type, EVENT_SHOCKWAVE);
    assert_eq!(ev0.channel_mask, channel::PHYSICS | channel::VFX);
    assert_eq!(ev0.source_id, 1);

    let dec0 = ShockwaveReport::decode(&ev0.payload);
    assert_eq!(dec0.center_x, 0);
    assert_eq!(dec0.center_y, 100);
    assert_eq!(dec0.center_z, 0);
    assert_eq!(dec0.radius_mm, 500);
    assert_eq!(dec0.speed_mms, 600_000);
    assert_eq!(dec0.peak_impulse_j, 75_000);
    assert_eq!(dec0.emitter_id, 1);
    assert_eq!(dec0.dust_density_q8, 180);
    assert_eq!(dec0.flags, SHOCKWAVE_FLAG_ACTIVE);

    // Check event 1
    let ev1 = &bus.events[1];
    assert_eq!(ev1.tick, 11);
    assert_eq!(ev1.source_id, 2);
    let dec1 = ShockwaveReport::decode(&ev1.payload);
    assert_eq!(dec1.center_x, 1500);
    assert_eq!(dec1.peak_impulse_j, 50_000);

    // Test rollback to event 0 sequence: event 1 should be rewound
    let seq0 = ev0.sequence;
    bus.rollback_to(seq0);
    assert_eq!(bus.count, 1);
    assert_eq!(bus.events[0].tick, 10);
}
