//! Integration test for Mecha Plasma MHD & Electromagnetic Induction Pipeline within NetworkMatchDirector (T-268)

use axiom_core::plasma::{
    MechaPlasmaEngine, MagneticFieldSlab64, PlasmaCellSlab64, PlasmaEventReport,
    EVENT_PLASMA_DISCHARGE, EVENT_EMP_SHOCK, MAG_FLAG_ACTIVE, MAG_FLAG_PULSED_EMP,
    PLASMA_EVENT_KIND_DISCHARGE, PLASMA_EVENT_KIND_EMP, PLASMA_FLAG_ACTIVE,
    PLASMA_FLAG_PINCHED,
};
use axiom_core::event_bus::channel;
use axiom_core::job_system::JobSystem;
use axiom_core::worker_pool::WorkerPoolCoordinator;
use axiom_game::network::NetworkMatchDirector;

#[test]
fn test_network_match_director_plasma_step() {
    let mut director = NetworkMatchDirector::<64>::new(0, 0xFFFF);
    let mut engine = MechaPlasmaEngine::<16, 4>::new();

    let cell = PlasmaCellSlab64::new(0, [0, 0, 1000], [0, 100, 50], 12000);
    engine.register_cell(cell);

    let report = PlasmaEventReport {
        pos_xyz: [0, 0, 1000],
        force_or_b_xyz: [0, 100, 50],
        pressure_or_voltage: 250,
        temp_or_alfven: 12000,
        current_or_flux: 150,
        beta_or_cell_id: 0,
        emitter_id: 1,
        target_id: 0,
        flags: PLASMA_FLAG_ACTIVE | PLASMA_FLAG_PINCHED,
        event_kind: PLASMA_EVENT_KIND_DISCHARGE,
    };

    let ok = director.step_plasma(&mut engine, 101, 1200, Some(&report));
    assert!(ok, "step_plasma must return true");
    assert_eq!(engine.tick, 1);
    assert_eq!(director.event_bus.count, 1, "Must emit 1 event to event bus");

    let event = &director.event_bus.events[0];
    assert_eq!(event.event_type, EVENT_PLASMA_DISCHARGE);
    assert_eq!(
        event.channel_mask,
        channel::PHYSICS | channel::VFX | channel::AUDIO
    );
    assert_eq!(event.source_id, 1);
    assert_eq!(event.target_id, 0);

    let decoded = PlasmaEventReport::decode(event.payload);
    assert_eq!(decoded.emitter_id, 1);
    assert_eq!(decoded.pressure_or_voltage, 250);
}

#[test]
fn test_network_match_director_plasma_jobs_and_emp_shock() {
    let mut director = NetworkMatchDirector::<64>::new(0, 0xFFFF);
    let mut engine = MechaPlasmaEngine::<16, 4>::new();
    let mut job_system = JobSystem::<64>::new();
    let mut coordinator = WorkerPoolCoordinator::<4, 16>::new(4);

    for i in 0..4 {
        let cell = PlasmaCellSlab64::new(
            i as u8,
            [i as i32 * 800, 0, 500],
            [50, 0, 100],
            10000 + (i as u32 * 1000),
        );
        engine.register_cell(cell);
    }

    let field = MagneticFieldSlab64::new(
        1,
        [0, 0, 0],
        [0, 0, 500],
        1500,
    );
    engine.register_field(field);

    let report = PlasmaEventReport {
        pos_xyz: [0, 0, 500],
        force_or_b_xyz: [0, 0, 500],
        pressure_or_voltage: 45_000,
        temp_or_alfven: 407_000,
        current_or_flux: 250,
        beta_or_cell_id: 1,
        emitter_id: 1,
        target_id: 2,
        flags: MAG_FLAG_ACTIVE | MAG_FLAG_PULSED_EMP,
        event_kind: PLASMA_EVENT_KIND_EMP,
    };

    let ok = director.step_plasma_with_jobs(
        &mut engine,
        101,
        1200,
        Some(&report),
        &mut job_system,
        &mut coordinator,
    );
    assert!(ok, "step_plasma_with_jobs must return true");
    assert_eq!(engine.tick, 1);
    assert_eq!(director.event_bus.count, 1, "Must emit EMP shock event to event bus");

    let event = &director.event_bus.events[0];
    assert_eq!(event.event_type, EVENT_EMP_SHOCK);
    assert_eq!(event.source_id, 1);
    assert_eq!(event.target_id, 2);

    let decoded = PlasmaEventReport::decode(event.payload);
    assert_eq!(decoded.pressure_or_voltage, 45_000);
    assert_eq!(decoded.flags & MAG_FLAG_PULSED_EMP, MAG_FLAG_PULSED_EMP);
    assert_eq!(decoded.event_kind, PLASMA_EVENT_KIND_EMP);
}
