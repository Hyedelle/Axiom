//! Integration test for Mecha Superconductor & Quantum Levitation Pipeline within NetworkMatchDirector (T-274)

use axiom_core::superconductor::{
    MechaSuperconductorEngine, FluxPinningSlab64, SuperconductorSlab64, SuperconductorEventReport,
    EVENT_FLUX_PINNING, EVENT_SUPERCONDUCT_QUENCH,
    SC_EVENT_KIND_PINNING, SC_EVENT_KIND_QUENCH, SC_FLAG_ACTIVE, SC_FLAG_FLUX_PINNED,
    SC_FLAG_QUENCHED, SC_TYPE_II,
};
use axiom_core::event_bus::channel;
use axiom_core::job_system::JobSystem;
use axiom_core::worker_pool::WorkerPoolCoordinator;
use axiom_game::network::NetworkMatchDirector;

#[test]
fn test_network_match_director_superconductor_step() {
    let mut director = NetworkMatchDirector::<64>::new(0, 0xFFFF);
    let mut engine = MechaSuperconductorEngine::<16, 4>::new();

    let cell = SuperconductorSlab64::new(0, [0, 0, 100], 77, 93, 30_000, SC_TYPE_II);
    engine.register_cell(cell);

    let report = SuperconductorEventReport {
        pos_xyz: [0, 0, 100],
        force_xyz: [0, 0, 1500],
        flux_or_voltage: 200,
        temp_k: 77,
        pin_displacement_mm: 10,
        crit_ratio_q8: 128,
        emitter_id: 1,
        target_id: 0,
        flags: SC_FLAG_ACTIVE | SC_FLAG_FLUX_PINNED,
        event_kind: SC_EVENT_KIND_PINNING,
    };

    let ok = director.step_superconductor(&mut engine, 1200, Some(&report));
    assert!(ok, "step_superconductor must return true");
    assert_eq!(engine.tick, 1);
    assert_eq!(director.event_bus.count, 1, "Must emit 1 event to event bus");

    let event = &director.event_bus.events[0];
    assert_eq!(event.event_type, EVENT_FLUX_PINNING);
    assert_eq!(
        event.channel_mask,
        channel::PHYSICS | channel::VFX | channel::AUDIO
    );
    assert_eq!(event.source_id, 1);
    assert_eq!(event.target_id, 0);

    let decoded = SuperconductorEventReport::decode(event.payload);
    assert_eq!(decoded.emitter_id, 1);
    assert_eq!(decoded.target_id, 0);
    assert_eq!(decoded.flux_or_voltage, 200);
}

#[test]
fn test_network_match_director_superconductor_jobs_and_quench() {
    let mut director = NetworkMatchDirector::<64>::new(0, 0xFFFF);
    let mut engine = MechaSuperconductorEngine::<16, 4>::new();
    let mut job_system = JobSystem::<64>::new();
    let mut coordinator = WorkerPoolCoordinator::<4, 16>::new(4);

    let cell = SuperconductorSlab64::new(1, [100, 200, 50], 77, 93, 30_000, SC_TYPE_II);
    let pin = FluxPinningSlab64::new(2, 1, [100, 200, 100], 150, 65536 * 100, 65536 * 10, 20_000, 50);

    engine.register_cell(cell);
    engine.register_pin(pin);

    let quench_report = SuperconductorEventReport {
        pos_xyz: [100, 200, 50],
        force_xyz: [0, 0, 0],
        flux_or_voltage: 52000,
        temp_k: 110,
        pin_displacement_mm: 0,
        crit_ratio_q8: 255,
        emitter_id: 2,
        target_id: 1,
        flags: SC_FLAG_ACTIVE | SC_FLAG_QUENCHED,
        event_kind: SC_EVENT_KIND_QUENCH,
    };

    let ok = director.step_superconductor_with_jobs(
        &mut engine,
        1500,
        Some(&quench_report),
        &mut job_system,
        &mut coordinator,
    );

    assert!(ok, "step_superconductor_with_jobs must return true");
    assert_eq!(director.event_bus.count, 1);

    let event = &director.event_bus.events[0];
    assert_eq!(event.event_type, EVENT_SUPERCONDUCT_QUENCH);
    assert_eq!(event.source_id, 2);
    assert_eq!(event.target_id, 1);

    let decoded = SuperconductorEventReport::decode(event.payload);
    assert_eq!(decoded.flux_or_voltage, 52000);
    assert_eq!(decoded.temp_k, 110);
    assert_eq!(decoded.event_kind, SC_EVENT_KIND_QUENCH);
}
