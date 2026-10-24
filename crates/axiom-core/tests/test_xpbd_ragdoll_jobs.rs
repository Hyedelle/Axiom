//! Integration tests for XPBD Mecha Ragdoll Multithreaded Job System & Event Bus Pipeline (T-232).

use axiom_core::event_bus::{channel, DeterministicEventBus};
use axiom_core::job_system::JobSystem;
use axiom_core::worker_pool::WorkerPoolCoordinator;
use axiom_core::xpbd_ragdoll::{
    dispatch_xpbd_with_jobs, emit_xpbd_to_event_bus, XpbdBoneSlab64, XpbdEventReport,
    XpbdRagdollEngine, EVENT_XPBD_STRAIN, EVENT_XPBD_TEAR,
};

#[test]
fn test_xpbd_batch_job_and_worker_pool_dispatch() {
    let mut engine = XpbdRagdollEngine::<16, 16>::new();

    // Spawn 8 bones in chain
    for i in 0..8 {
        engine.add_bone(XpbdBoneSlab64::new(
            i as u8,
            if i == 0 { 255 } else { (i - 1) as u8 },
            ((i as i32) * 200, 1500, 0),
            65536,
            80,
            200,
        ));
    }
    assert_eq!(engine.active_bones_count, 8);

    let mut job_system = JobSystem::<64>::new();
    let mut coordinator = WorkerPoolCoordinator::<4, 32>::new(4);

    let dispatched = dispatch_xpbd_with_jobs(&engine, 4, &mut job_system, &mut coordinator);
    assert_eq!(dispatched, 4, "Must dispatch 4 worker partition jobs");

    let mut total_enqueued = 0;
    for w in 0..4 {
        total_enqueued += coordinator.deques[w].len();
    }
    assert_eq!(total_enqueued, 4);

    // Drain all jobs across worker pool
    coordinator.drain_all(&mut job_system);
    assert_eq!(job_system.active_count, 0);
}

#[test]
fn test_xpbd_event_bus_strain_and_tear_emission() {
    let mut bus = DeterministicEventBus::<64>::new();

    // 1. Strain Report (Plastic deformation)
    let strain_report = XpbdEventReport {
        entity_id: 101,
        bone_a: 0,
        bone_b: 1,
        constraint_id: 0,
        is_severed: 0,
        strain_q16: 5200, // ~8% strain
        tension_force_n: 45000,
        rupture_pos_xyz: [150, 1500, 0],
        impulse_xyz: [100, 200, 0],
    };

    let ok = emit_xpbd_to_event_bus(&strain_report, 10, &mut bus);
    assert!(ok, "Must emit XPBD strain event to bus");

    // 2. Tear Report (Joint severance)
    let tear_report = XpbdEventReport {
        entity_id: 101,
        bone_a: 1,
        bone_b: 2,
        constraint_id: 1,
        is_severed: 1,
        strain_q16: 16000, // ~25% strain > break
        tension_force_n: 120000,
        rupture_pos_xyz: [350, 1500, 0],
        impulse_xyz: [500, 800, -200],
    };

    let ok = emit_xpbd_to_event_bus(&tear_report, 11, &mut bus);
    assert!(ok, "Must emit XPBD tear event to bus");

    assert_eq!(bus.count, 2);

    let ev1 = &bus.events[0];
    assert_eq!(ev1.event_type, EVENT_XPBD_STRAIN);
    assert_eq!(ev1.channel_mask, channel::PHYSICS | channel::COMBAT);
    assert_eq!(ev1.source_id, 101);

    let decoded1 = XpbdEventReport::decode(&ev1.payload);
    assert_eq!(decoded1.is_severed, 0);
    assert_eq!(decoded1.strain_q16, 5200);

    let ev2 = &bus.events[1];
    assert_eq!(ev2.event_type, EVENT_XPBD_TEAR);
    assert_eq!(ev2.channel_mask, channel::PHYSICS | channel::COMBAT);
    assert_eq!(ev2.source_id, 101);

    let decoded2 = XpbdEventReport::decode(&ev2.payload);
    assert_eq!(decoded2.is_severed, 1);
    assert_eq!(decoded2.tension_force_n, 120000);
}
