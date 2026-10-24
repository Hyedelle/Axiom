//! Unit tests for Radiation Multithreaded Job System & Event Bus Integration (T-262)

use axiom_core::radiation::*;
use axiom_core::event_bus::DeterministicEventBus;
use axiom_core::job_system::JobSystem;
use axiom_core::worker_pool::WorkerPoolCoordinator;

#[test]
fn test_dispatch_radiation_with_jobs() {
    let mut engine = MechaRadiationEngine::<16, 4>::new();
    let mut job_system = JobSystem::<64>::new();
    let mut coordinator = WorkerPoolCoordinator::<4, 16>::new(4);

    for i in 0..8 {
        let cell = RadiationCellSlab64::new(
            i as u8,
            [i as i32 * 500, 0, 0],
            1000 + (i as u32 * 200),
        );
        engine.register_cell(cell);
    }

    let dispatched = dispatch_radiation_with_jobs(&engine, 4, &mut job_system, &mut coordinator);
    assert_eq!(dispatched, 4, "Must dispatch 4 jobs across the worker pool");

    let executed = coordinator.drain_all(&mut job_system);
    assert_eq!(executed, 4, "Must execute all 4 radiation partition jobs");
}

#[test]
fn test_emit_radiation_burst_to_event_bus() {
    let mut bus = DeterministicEventBus::<16>::new();

    let report = RadiationEventReport {
        pos_xyz: [1200, 500, -800],
        dir_xyz: [0, 0, 32768],
        flux_or_energy_mj: 56_700,
        optical_depth_or_temp: 1000,
        penetration_depth_mm: 0,
        wavelength_or_cell_id: 1,
        emitter_id: 2,
        target_id: 0,
        flags: RAD_FLAG_ACTIVE | RAD_FLAG_INCANDESCENT,
        event_kind: RAD_EVENT_KIND_BURST,
    };

    let emitted = emit_radiation_to_event_bus(&report, 60, &mut bus);
    assert!(emitted, "Must successfully emit radiation burst to event bus");
    assert_eq!(bus.count, 1);

    let event = &bus.events[0];
    assert_eq!(event.event_type, EVENT_RADIATION_BURST);
    assert_eq!(event.source_id, 2);
    assert_eq!(event.target_id, 0);

    let decoded = RadiationEventReport::decode(&event.payload);
    assert_eq!(decoded.pos_xyz, [1200, 500, -800]);
    assert_eq!(decoded.flux_or_energy_mj, 56_700);
    assert_eq!(decoded.optical_depth_or_temp, 1000);
    assert_eq!(decoded.event_kind, RAD_EVENT_KIND_BURST);
}

#[test]
fn test_emit_beam_penetration_to_event_bus() {
    let mut bus = DeterministicEventBus::<16>::new();

    let report = RadiationEventReport {
        pos_xyz: [0, 0, 2000],
        dir_xyz: [0, 0, 32768],
        flux_or_energy_mj: 2_500_000,
        optical_depth_or_temp: 65536,
        penetration_depth_mm: 22,
        wavelength_or_cell_id: 1064,
        emitter_id: 5,
        target_id: 1,
        flags: BEAM_FLAG_ACTIVE | BEAM_FLAG_ABLATING | BEAM_FLAG_PENETRATED,
        event_kind: RAD_EVENT_KIND_BEAM,
    };

    let emitted = emit_radiation_to_event_bus(&report, 120, &mut bus);
    assert!(emitted, "Must successfully emit beam penetration to event bus");
    assert_eq!(bus.count, 1);

    let event = &bus.events[0];
    assert_eq!(event.event_type, EVENT_BEAM_PENETRATION);
    assert_eq!(event.source_id, 5);
    assert_eq!(event.target_id, 1);

    let decoded = RadiationEventReport::decode(&event.payload);
    assert_eq!(decoded.flags & BEAM_FLAG_PENETRATED, BEAM_FLAG_PENETRATED);
    assert_eq!(decoded.penetration_depth_mm, 22);
    assert_eq!(decoded.wavelength_or_cell_id, 1064);
    assert_eq!(decoded.event_kind, RAD_EVENT_KIND_BEAM);
}
