//! Unit tests for Plasma MHD Multithreaded Job System & Event Bus Integration (T-268)

use axiom_core::plasma::*;
use axiom_core::event_bus::DeterministicEventBus;
use axiom_core::job_system::JobSystem;
use axiom_core::worker_pool::WorkerPoolCoordinator;

#[test]
fn test_dispatch_plasma_with_jobs() {
    let mut engine = MechaPlasmaEngine::<16, 4>::new();
    let mut job_system = JobSystem::<64>::new();
    let mut coordinator = WorkerPoolCoordinator::<4, 16>::new(4);

    for i in 0..8 {
        let cell = PlasmaCellSlab64::new(
            i as u8,
            [i as i32 * 400, 0, 0],
            [100, 200, 0],
            15000,
        );
        engine.register_cell(cell);
    }

    let dispatched = dispatch_plasma_with_jobs(&engine, 4, &mut job_system, &mut coordinator);
    assert_eq!(dispatched, 4, "Must dispatch 4 jobs across the worker pool");

    let executed = coordinator.drain_all(&mut job_system);
    assert_eq!(executed, 4, "Must execute all 4 plasma partition jobs");
}

#[test]
fn test_dispatch_plasma_empty_engine() {
    let engine = MechaPlasmaEngine::<16, 4>::new();
    let mut job_system = JobSystem::<64>::new();
    let mut coordinator = WorkerPoolCoordinator::<4, 16>::new(4);

    let dispatched = dispatch_plasma_with_jobs(&engine, 4, &mut job_system, &mut coordinator);
    assert_eq!(dispatched, 0, "Empty engine must dispatch 0 jobs");
}

#[test]
fn test_emit_plasma_discharge_to_event_bus() {
    let mut bus = DeterministicEventBus::<16>::new();

    let report = PlasmaEventReport {
        pos_xyz: [1000, 2000, -500],
        force_or_b_xyz: [50, -30, 80],
        pressure_or_voltage: 150,
        temp_or_alfven: 18000,
        current_or_flux: 250,
        beta_or_cell_id: 1,
        emitter_id: 1,
        target_id: 2,
        flags: PLASMA_FLAG_ACTIVE | PLASMA_FLAG_PINCHED,
        event_kind: PLASMA_EVENT_KIND_DISCHARGE,
    };

    let emitted = emit_plasma_to_event_bus(&report, 60, &mut bus);
    assert!(emitted, "Must successfully emit plasma discharge to event bus");
    assert_eq!(bus.count, 1);

    let event = &bus.events[0];
    assert_eq!(event.event_type, EVENT_PLASMA_DISCHARGE);
    assert_eq!(event.source_id, 1);
    assert_eq!(event.target_id, 2);

    let decoded = PlasmaEventReport::decode(event.payload);
    assert_eq!(decoded.pos_xyz, [1000, 2000, -500]);
    assert_eq!(decoded.force_or_b_xyz, [50, -30, 80]);
    assert_eq!(decoded.pressure_or_voltage, 150);
    assert_eq!(decoded.temp_or_alfven, 18000);
    assert_eq!(decoded.event_kind, PLASMA_EVENT_KIND_DISCHARGE);
}

#[test]
fn test_emit_emp_shock_to_event_bus() {
    let mut bus = DeterministicEventBus::<16>::new();

    let report = PlasmaEventReport {
        pos_xyz: [0, 0, 1500],
        force_or_b_xyz: [0, 0, 500],
        pressure_or_voltage: 125_000,
        temp_or_alfven: 800_000,
        current_or_flux: 450,
        beta_or_cell_id: 0,
        emitter_id: 3,
        target_id: 7,
        flags: MAG_FLAG_ACTIVE | MAG_FLAG_PULSED_EMP,
        event_kind: PLASMA_EVENT_KIND_EMP,
    };

    let emitted = emit_plasma_to_event_bus(&report, 120, &mut bus);
    assert!(emitted, "Must successfully emit EMP shock to event bus");
    assert_eq!(bus.count, 1);

    let event = &bus.events[0];
    assert_eq!(event.event_type, EVENT_EMP_SHOCK);
    assert_eq!(event.source_id, 3);
    assert_eq!(event.target_id, 7);

    let decoded = PlasmaEventReport::decode(event.payload);
    assert_eq!(decoded.pressure_or_voltage, 125_000);
    assert_eq!(decoded.flags & MAG_FLAG_PULSED_EMP, MAG_FLAG_PULSED_EMP);
    assert_eq!(decoded.event_kind, PLASMA_EVENT_KIND_EMP);
}
