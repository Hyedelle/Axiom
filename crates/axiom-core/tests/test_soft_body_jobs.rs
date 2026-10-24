use axiom_core::event_bus::DeterministicEventBus;
use axiom_core::job_system::JobSystem;
use axiom_core::soft_body::{
    dispatch_soft_body_with_jobs, emit_soft_body_to_event_bus, SoftBodyClusterSlab64,
    SoftBodyEngine, SoftBodyEventReport, EVENT_SOFT_BODY_DEFORM,
};
use axiom_core::worker_pool::WorkerPoolCoordinator;

#[test]
fn test_dispatch_soft_body_with_jobs() {
    let mut engine = SoftBodyEngine::<32, 4>::new();
    let mut job_system = JobSystem::<64>::new();
    let mut coordinator = WorkerPoolCoordinator::<4, 16>::new(4);

    for c in 0..4 {
        let cluster = SoftBodyClusterSlab64::new(
            (c as i32 * 500, 1000, 0),
            (c * 4) as u16,
            4,
            32768,
            32768,
            1_000_000,
            c as u8,
        );
        engine.add_cluster(cluster);
    }

    let dispatched = dispatch_soft_body_with_jobs(&engine, 4, &mut job_system, &mut coordinator);
    assert_eq!(dispatched, 4, "Must dispatch 4 jobs across the worker pool");

    let executed = coordinator.drain_all(&mut job_system);
    assert_eq!(executed, 4, "Must execute all 4 soft body partition jobs");
}

#[test]
fn test_emit_soft_body_to_event_bus() {
    let mut bus = DeterministicEventBus::<16>::new();

    let report = SoftBodyEventReport {
        entity_id: 42,
        cluster_id: 1,
        flags: 0,
        max_strain_q16: 9876,
        volume_ratio_q16: 65000,
        centroid_pos_xyz: [1000, 2000, 3000],
        deform_impulse_xyz: [100, -200, 50],
    };

    let emitted = emit_soft_body_to_event_bus(&report, 100, &mut bus);
    assert!(emitted, "Must successfully emit to event bus");
    assert_eq!(bus.count, 1);

    let event = &bus.events[0];
    assert_eq!(event.event_type, EVENT_SOFT_BODY_DEFORM);
    assert_eq!(event.source_id, 42);

    let decoded = SoftBodyEventReport::decode(&event.payload);
    assert_eq!(decoded.entity_id, 42);
    assert_eq!(decoded.max_strain_q16, 9876);
    assert_eq!(decoded.volume_ratio_q16, 65000);
}
