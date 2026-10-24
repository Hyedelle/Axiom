use axiom_core::actuator::{
    dispatch_actuators_with_jobs, emit_actuator_to_event_bus, ActuatorEventReport,
    ActuatorSlab64, HydraulicChamberSlab64, MechaActuatorEngine, ACTUATOR_FLAG_BURST,
    ACTUATOR_TYPE_HYDRAULIC, EVENT_ACTUATOR_BURST, EVENT_ACTUATOR_PRESSURE,
};
use axiom_core::event_bus::DeterministicEventBus;
use axiom_core::job_system::JobSystem;
use axiom_core::worker_pool::WorkerPoolCoordinator;

#[test]
fn test_dispatch_actuators_with_jobs() {
    let mut engine = MechaActuatorEngine::<16, 4>::new();
    let mut job_system = JobSystem::<64>::new();
    let mut coordinator = WorkerPoolCoordinator::<4, 16>::new(4);

    let chamber = HydraulicChamberSlab64::new(0, 1, 200_000, 15_000, 1_500_000);
    engine.add_chamber(chamber);

    for i in 0..8 {
        let act = ActuatorSlab64::new_hydraulic(
            1,
            2,
            (0, 0, 0),
            (0, 500 + i as i32 * 50, 0),
            100,
            200,
            30_000,
        );
        engine.add_actuator(act);
    }

    let dispatched = dispatch_actuators_with_jobs(&engine, 4, &mut job_system, &mut coordinator);
    assert_eq!(dispatched, 4, "Must dispatch 4 jobs across the worker pool");

    let executed = coordinator.drain_all(&mut job_system);
    assert_eq!(executed, 4, "Must execute all 4 actuator partition jobs");
}

#[test]
fn test_emit_actuator_to_event_bus() {
    let mut bus = DeterministicEventBus::<16>::new();

    let report = ActuatorEventReport {
        entity_id: 11,
        actuator_id: 2,
        chamber_id: 0,
        flags: 0,
        actuator_type: ACTUATOR_TYPE_HYDRAULIC,
        pressure_kpa: 14_500,
        thrust_n: 29_000,
        current_len_mm: 500,
        target_len_mm: 500,
        pos_x: 100,
        pos_y: 500,
        pos_z: 200,
        impulse_m_ns: 1500,
        _reserved: [0; 4],
    };

    let emitted = emit_actuator_to_event_bus(&report, 50, &mut bus);
    assert!(emitted, "Must successfully emit to event bus");
    assert_eq!(bus.count, 1);

    let event = &bus.events[0];
    assert_eq!(event.event_type, EVENT_ACTUATOR_PRESSURE);
    assert_eq!(event.source_id, 11);
    assert_eq!(event.target_id, 2);

    let decoded = ActuatorEventReport::decode(&event.payload);
    assert_eq!(decoded.entity_id, 11);
    assert_eq!(decoded.actuator_id, 2);
    assert_eq!(decoded.pressure_kpa, 14_500);
    assert_eq!(decoded.thrust_n, 29_000);
    assert_eq!(decoded.current_len_mm, 500);
    assert_eq!(decoded.impulse_m_ns, 1500);
}

#[test]
fn test_emit_actuator_burst_to_event_bus() {
    let mut bus = DeterministicEventBus::<16>::new();

    let report = ActuatorEventReport {
        entity_id: 22,
        actuator_id: 5,
        chamber_id: 1,
        flags: ACTUATOR_FLAG_BURST,
        actuator_type: ACTUATOR_TYPE_HYDRAULIC,
        pressure_kpa: 0,
        thrust_n: 0,
        current_len_mm: 600,
        target_len_mm: 600,
        pos_x: 200,
        pos_y: 600,
        pos_z: 300,
        impulse_m_ns: 20_000,
        _reserved: [0; 4],
    };

    let emitted = emit_actuator_to_event_bus(&report, 60, &mut bus);
    assert!(emitted, "Must successfully emit burst to event bus");
    assert_eq!(bus.count, 1);

    let event = &bus.events[0];
    assert_eq!(event.event_type, EVENT_ACTUATOR_BURST);
    assert_eq!(event.source_id, 22);
    assert_eq!(event.target_id, 5);

    let decoded = ActuatorEventReport::decode(&event.payload);
    assert_eq!(decoded.entity_id, 22);
    assert_eq!(decoded.actuator_id, 5);
    assert!((decoded.flags & ACTUATOR_FLAG_BURST) != 0);
}
