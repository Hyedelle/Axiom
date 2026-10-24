//! Integration test for Mecha Actuator Pipeline within NetworkMatchDirector (T-244)

use axiom_core::actuator::{
    ActuatorEventReport, ActuatorSlab64, HydraulicChamberSlab64, MechaActuatorEngine,
    ACTUATOR_FLAG_BURST, ACTUATOR_TYPE_HYDRAULIC,
    EVENT_ACTUATOR_BURST, EVENT_ACTUATOR_PRESSURE,
};
use axiom_core::event_bus::channel;
use axiom_core::job_system::JobSystem;
use axiom_core::worker_pool::WorkerPoolCoordinator;
use axiom_game::network::NetworkMatchDirector;

#[test]
fn test_network_match_director_actuator_step() {
    let mut director = NetworkMatchDirector::<64>::new(0, 0xFFFF);
    let mut engine = MechaActuatorEngine::<16, 4>::new();

    let chamber = HydraulicChamberSlab64::new(0, 3, 200_000, 20_000, 1_500_000);
    engine.add_chamber(chamber);

    let act = ActuatorSlab64::new_hydraulic(
        1,
        2,
        (0, 0, 0),
        (0, 500, 0),
        100,
        200,
        35_000,
    );
    engine.add_actuator(act);

    let report = ActuatorEventReport {
        entity_id: 3,
        actuator_id: 0,
        chamber_id: 0,
        flags: 0,
        actuator_type: ACTUATOR_TYPE_HYDRAULIC,
        pressure_kpa: 20_000,
        thrust_n: 50_000,
        current_len_mm: 500,
        target_len_mm: 500,
        pos_x: 0,
        pos_y: 500,
        pos_z: 0,
        impulse_m_ns: 5000,
        _reserved: [0; 4],
    };

    let ok = director.step_actuator(&mut engine, Some(&report));
    assert!(ok, "step_actuator must return true");
    assert_eq!(engine.tick, 1);
    assert_eq!(director.event_bus.count, 1, "Must emit 1 event to event bus");

    let event = &director.event_bus.events[0];
    assert_eq!(event.event_type, EVENT_ACTUATOR_PRESSURE);
    assert_eq!(
        event.channel_mask,
        channel::PHYSICS | channel::VFX | channel::AUDIO
    );
    assert_eq!(event.source_id, 3);
    assert_eq!(event.target_id, 0);

    let decoded = ActuatorEventReport::decode(&event.payload);
    assert_eq!(decoded.entity_id, 3);
    assert_eq!(decoded.pressure_kpa, 20_000);
    assert_eq!(decoded.thrust_n, 50_000);
}

#[test]
fn test_network_match_director_actuator_burst_and_jobs() {
    let mut director = NetworkMatchDirector::<64>::new(0, 0xFFFF);
    let mut engine = MechaActuatorEngine::<16, 4>::new();
    let mut job_system = JobSystem::<64>::new();
    let mut coordinator = WorkerPoolCoordinator::<4, 16>::new(4);

    let chamber = HydraulicChamberSlab64::new(0, 1, 200_000, 50_000, 1_500_000);
    engine.add_chamber(chamber);

    for i in 0..4 {
        let act = if i % 2 == 0 {
            ActuatorSlab64::new_hydraulic(
                1,
                2,
                (0, 0, 0),
                (0, 600, 0),
                120,
                250,
                60_000,
            )
        } else {
            ActuatorSlab64::new_mckibben_pam(
                2,
                3,
                (0, 0, 0),
                (0, 600, 0),
                120,
                1000,
            )
        };
        engine.add_actuator(act);
    }

    let burst_report = ActuatorEventReport {
        entity_id: 1,
        actuator_id: 0,
        chamber_id: 0,
        flags: ACTUATOR_FLAG_BURST,
        actuator_type: ACTUATOR_TYPE_HYDRAULIC,
        pressure_kpa: 0,
        thrust_n: 0,
        current_len_mm: 600,
        target_len_mm: 600,
        pos_x: 0,
        pos_y: 600,
        pos_z: 0,
        impulse_m_ns: 25_000,
        _reserved: [0; 4],
    };

    let ok = director.step_actuator_with_jobs(
        &mut engine,
        Some(&burst_report),
        &mut job_system,
        &mut coordinator,
    );
    assert!(ok, "step_actuator_with_jobs must return true");
    assert_eq!(engine.tick, 1);
    assert_eq!(director.event_bus.count, 1);

    let event = &director.event_bus.events[0];
    assert_eq!(event.event_type, EVENT_ACTUATOR_BURST);
    assert_eq!(event.source_id, 1);
    assert_eq!(event.target_id, 0);

    let decoded = ActuatorEventReport::decode(&event.payload);
    assert_eq!(decoded.entity_id, 1);
    assert!((decoded.flags & ACTUATOR_FLAG_BURST) != 0);
}
