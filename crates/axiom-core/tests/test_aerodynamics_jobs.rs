//! Unit tests for Aerodynamics Multithreaded Job System & Event Bus Integration (T-250)

use axiom_core::aerodynamics::*;
use axiom_core::event_bus::DeterministicEventBus;
use axiom_core::job_system::JobSystem;
use axiom_core::worker_pool::WorkerPoolCoordinator;

#[test]
fn test_dispatch_aerodynamics_with_jobs() {
    let mut engine = MechaAerodynamicsEngine::<16, 4>::new();
    let mut job_system = JobSystem::<64>::new();
    let mut coordinator = WorkerPoolCoordinator::<4, 16>::new(4);

    let cell = AirflowGridSlab64::new(0, [2000, 0, 0]);
    engine.register_cell(cell);

    for i in 0..8 {
        let mut body = AeroBodySlab64::new(
            1,
            AERO_BODY_CYLINDER,
            40_000 + i as u32 * 2000,
            30_000,
            100,
            52428,
        );
        body.vel_xyz = [15_000 + i as i32 * 2000, 0, 0];
        engine.register_body(body);
    }

    let dispatched = dispatch_aerodynamics_with_jobs(&engine, 4, &mut job_system, &mut coordinator);
    assert_eq!(dispatched, 4, "Must dispatch 4 jobs across the worker pool");

    let executed = coordinator.drain_all(&mut job_system);
    assert_eq!(executed, 4, "Must execute all 4 aerodynamics partition jobs");
}

#[test]
fn test_emit_aero_drag_to_event_bus() {
    let mut bus = DeterministicEventBus::<16>::new();

    let report = AeroEventReport {
        pos_xyz: [1000, 2000, 500],
        rel_vel_xyz: [25000, 0, 0],
        drag_force_n: 1850,
        viv_force_n: 240,
        shedding_freq_q8: 4800, // 18.75 Hz (outside lock-in)
        entity_id: 5,
        body_id: 1,
        flags: AERO_FLAG_ACTIVE | AERO_FLAG_HIGH_SPEED,
        body_type: AERO_BODY_CYLINDER,
        reserved: [0; 2],
    };

    let emitted = emit_aero_to_event_bus(&report, 42, &mut bus);
    assert!(emitted, "Must successfully emit to event bus");
    assert_eq!(bus.count, 1);

    let event = &bus.events[0];
    assert_eq!(event.event_type, EVENT_AERO_DRAG);
    assert_eq!(event.source_id, 5);
    assert_eq!(event.target_id, 1);

    let decoded = AeroEventReport::decode(&event.payload);
    assert_eq!(decoded.pos_xyz, [1000, 2000, 500]);
    assert_eq!(decoded.rel_vel_xyz, [25000, 0, 0]);
    assert_eq!(decoded.drag_force_n, 1850);
    assert_eq!(decoded.viv_force_n, 240);
    assert_eq!(decoded.shedding_freq_q8, 4800);
}

#[test]
fn test_emit_aero_vortex_to_event_bus() {
    let mut bus = DeterministicEventBus::<16>::new();

    let report = AeroEventReport {
        pos_xyz: [0, 0, 1500],
        rel_vel_xyz: [20000, 0, 0],
        drag_force_n: 1200,
        viv_force_n: 950,
        shedding_freq_q8: 10240, // 40.0 Hz (in lock-in)
        entity_id: 8,
        body_id: 2,
        flags: AERO_FLAG_ACTIVE | AERO_FLAG_LOCK_IN,
        body_type: AERO_BODY_CYLINDER,
        reserved: [0; 2],
    };

    let emitted = emit_aero_to_event_bus(&report, 88, &mut bus);
    assert!(emitted, "Must successfully emit lock-in vortex event");
    assert_eq!(bus.count, 1);

    let event = &bus.events[0];
    assert_eq!(event.event_type, EVENT_AERO_VORTEX);
    assert_eq!(event.source_id, 8);
    assert_eq!(event.target_id, 2);

    let decoded = AeroEventReport::decode(&event.payload);
    assert_eq!(decoded.flags & AERO_FLAG_LOCK_IN, AERO_FLAG_LOCK_IN);
    assert_eq!(decoded.viv_force_n, 950);
}
