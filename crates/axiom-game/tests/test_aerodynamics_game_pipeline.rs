//! Integration test for Mecha Aerodynamics Pipeline within NetworkMatchDirector (T-250)

use axiom_core::aerodynamics::{
    AeroBodySlab64, AeroEventReport, AirflowGridSlab64, MechaAerodynamicsEngine,
    AERO_BODY_CYLINDER, AERO_FLAG_LOCK_IN, EVENT_AERO_DRAG, EVENT_AERO_VORTEX,
};
use axiom_core::event_bus::channel;
use axiom_core::job_system::JobSystem;
use axiom_core::worker_pool::WorkerPoolCoordinator;
use axiom_game::network::NetworkMatchDirector;

#[test]
fn test_network_match_director_aero_step() {
    let mut director = NetworkMatchDirector::<64>::new(0, 0xFFFF);
    let mut engine = MechaAerodynamicsEngine::<16, 4>::new();

    let cell = AirflowGridSlab64::new(0, [0, 0, 0]);
    engine.register_cell(cell);

    let mut body = AeroBodySlab64::new(
        1,
        AERO_BODY_CYLINDER,
        50_000,
        30_000,
        100,
        52428,
    );
    body.vel_xyz = [25_000, 0, 0];
    engine.register_body(body);

    let report = AeroEventReport {
        pos_xyz: [0, 0, 1000],
        rel_vel_xyz: [25_000, 0, 0],
        drag_force_n: 1500,
        viv_force_n: 200,
        shedding_freq_q8: 5000,
        entity_id: 1,
        body_id: 0,
        flags: 0,
        body_type: AERO_BODY_CYLINDER,
        reserved: [0; 2],
    };

    let ok = director.step_aerodynamics(&mut engine, Some(&report));
    assert!(ok, "step_aerodynamics must return true");
    assert_eq!(engine.tick, 1);
    assert_eq!(director.event_bus.count, 1, "Must emit 1 event to event bus");

    let event = &director.event_bus.events[0];
    assert_eq!(event.event_type, EVENT_AERO_DRAG);
    assert_eq!(
        event.channel_mask,
        channel::PHYSICS | channel::VFX | channel::AUDIO
    );
    assert_eq!(event.source_id, 1);
    assert_eq!(event.target_id, 0);

    let decoded = AeroEventReport::decode(&event.payload);
    assert_eq!(decoded.entity_id, 1);
    assert_eq!(decoded.drag_force_n, 1500);
}

#[test]
fn test_network_match_director_aero_jobs_and_lock_in() {
    let mut director = NetworkMatchDirector::<64>::new(0, 0xFFFF);
    let mut engine = MechaAerodynamicsEngine::<16, 4>::new();
    let mut job_system = JobSystem::<64>::new();
    let mut coordinator = WorkerPoolCoordinator::<4, 16>::new(4);

    let cell = AirflowGridSlab64::new(0, [1000, 0, 0]);
    engine.register_cell(cell);

    for i in 0..4 {
        let mut body = AeroBodySlab64::new(
            (i + 1) as u8,
            AERO_BODY_CYLINDER,
            40_000,
            25_000,
            100,
            52428,
        );
        body.vel_xyz = [20_000, 0, 0]; // 20 m/s -> ~40 Hz (lock-in)
        engine.register_body(body);
    }

    let report = AeroEventReport {
        pos_xyz: [1000, 500, 1200],
        rel_vel_xyz: [19_000, 0, 0],
        drag_force_n: 1100,
        viv_force_n: 850,
        shedding_freq_q8: 9800, // in lock-in
        entity_id: 2,
        body_id: 1,
        flags: AERO_FLAG_LOCK_IN,
        body_type: AERO_BODY_CYLINDER,
        reserved: [0; 2],
    };

    let ok = director.step_aerodynamics_with_jobs(
        &mut engine,
        Some(&report),
        &mut job_system,
        &mut coordinator,
    );
    assert!(ok, "step_aerodynamics_with_jobs must return true");
    assert_eq!(engine.tick, 1);
    assert_eq!(director.event_bus.count, 1);

    let event = &director.event_bus.events[0];
    assert_eq!(event.event_type, EVENT_AERO_VORTEX);
    assert_eq!(event.source_id, 2);

    let decoded = AeroEventReport::decode(&event.payload);
    assert_eq!(decoded.flags & AERO_FLAG_LOCK_IN, AERO_FLAG_LOCK_IN);
    assert_eq!(decoded.viv_force_n, 850);
}
