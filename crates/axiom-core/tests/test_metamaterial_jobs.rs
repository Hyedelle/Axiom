//! Unit tests for Metamaterial Acousto-Optics Multithreaded Job System & Event Bus Integration (T-280)

use axiom_core::event_bus::{channel, DeterministicEventBus};
use axiom_core::job_system::JobSystem;
use axiom_core::metamaterial::*;
use axiom_core::worker_pool::WorkerPoolCoordinator;

#[test]
fn test_dispatch_metamaterial_with_jobs() {
    let mut engine = MechaMetamaterialEngine::<16, 8>::new();
    let mut job_system = JobSystem::<64>::new();
    let mut coordinator = WorkerPoolCoordinator::<4, 16>::new(4);

    for i in 0..8 {
        let cell = AcoustoOpticCellSlab64::new(
            i as u8,
            [i as i32 * 500, 0, 100],
            4_200_000,
            100_000,
            1064,
        );
        engine.register_cell(cell);
    }

    let dispatched =
        dispatch_metamaterial_with_jobs(&engine, 4, &mut job_system, &mut coordinator);
    assert_eq!(dispatched, 4, "Must dispatch 4 jobs across the worker pool");

    let executed = coordinator.drain_all(&mut job_system);
    assert_eq!(executed, 4, "Must execute all 4 metamaterial partition jobs");
}

#[test]
fn test_dispatch_metamaterial_empty_engine() {
    let engine = MechaMetamaterialEngine::<16, 8>::new();
    let mut job_system = JobSystem::<64>::new();
    let mut coordinator = WorkerPoolCoordinator::<4, 16>::new(4);

    let dispatched =
        dispatch_metamaterial_with_jobs(&engine, 4, &mut job_system, &mut coordinator);
    assert_eq!(dispatched, 0, "Empty engine must dispatch 0 jobs");
}

#[test]
fn test_emit_metamaterial_events_to_event_bus() {
    let mut bus = DeterministicEventBus::<16>::new();

    // 1. Emit Optical Cloaking Event
    let cloak_report = MetamaterialEventReport {
        pos_xyz: [1000, 2000, 100],
        deflection_dir_xyz: [32000, 1500, 0],
        bragg_angle_mrad: 15,
        diffraction_eff_or_grad_q16: 45000,
        cloak_efficiency_pct: 96,
        laser_wavelength_nm: 1064,
        emitter_id: 1,
        target_id: 2,
        flags: CLOAK_FLAG_ACTIVE | CLOAK_FLAG_BENDING,
        event_kind: META_EVENT_KIND_CLOAK,
    };

    let emitted = emit_metamaterial_to_event_bus(&cloak_report, 100, &mut bus);
    assert!(emitted, "Must successfully emit optical cloak event");
    assert_eq!(bus.count, 1);

    let event = &bus.events[0];
    assert_eq!(event.event_type, EVENT_OPTICAL_CLOAK);
    assert_eq!(event.tick, 100);
    assert_eq!(
        event.channel_mask & (channel::PHYSICS | channel::VFX | channel::AUDIO),
        channel::PHYSICS | channel::VFX | channel::AUDIO
    );

    // 2. Emit Brillouin Scatter Event
    let scatter_report = MetamaterialEventReport {
        pos_xyz: [500, 0, 0],
        deflection_dir_xyz: [31500, 2500, 0],
        bragg_angle_mrad: 22,
        diffraction_eff_or_grad_q16: 58000,
        cloak_efficiency_pct: 0,
        laser_wavelength_nm: 532,
        emitter_id: 3,
        target_id: 4,
        flags: AO_FLAG_ACTIVE | AO_FLAG_DEFLECTING,
        event_kind: META_EVENT_KIND_SCATTER,
    };

    let emitted2 = emit_metamaterial_to_event_bus(&scatter_report, 101, &mut bus);
    assert!(emitted2, "Must successfully emit brillouin scatter event");
    assert_eq!(bus.count, 2);

    let event2 = &bus.events[1];
    assert_eq!(event2.event_type, EVENT_BRILLOUIN_SCATTER);
    assert_eq!(event2.tick, 101);

    // Decode and verify roundtrip from bus slab
    let decoded = MetamaterialEventReport::decode(event2.payload);
    assert_eq!(decoded.bragg_angle_mrad, 22);
    assert_eq!(decoded.laser_wavelength_nm, 532);
    assert_eq!(decoded.event_kind, META_EVENT_KIND_SCATTER);
}

#[test]
fn test_metamaterial_multithreaded_consensus_consistency() {
    let mut engine1 = MechaMetamaterialEngine::<16, 8>::new();
    let mut engine2 = MechaMetamaterialEngine::<16, 8>::new();

    let mut job_sys = JobSystem::<64>::new();
    let mut coord = WorkerPoolCoordinator::<4, 16>::new(4);

    for i in 0..6 {
        let cell = AcoustoOpticCellSlab64::new(
            i as u8,
            [i as i32 * 200, 0, 50],
            4_200_000,
            100_000,
            1064,
        );
        engine1.register_cell(cell);
        engine2.register_cell(cell);
    }

    let field = CloakingFieldSlab64::new(10, 0, [0, 0, 100], 1000, 2000);
    engine1.register_field(field);
    engine2.register_field(field);

    // Step engine 1 single-threaded
    for _ in 0..5 {
        engine1.step_simulation(10);
    }

    // Step engine 2 with multithreaded dispatch
    for _ in 0..5 {
        dispatch_metamaterial_with_jobs(&engine2, 4, &mut job_sys, &mut coord);
        coord.drain_all(&mut job_sys);
        engine2.step_simulation(10);
    }

    assert_eq!(
        engine1.compute_consensus_hash(),
        engine2.compute_consensus_hash(),
        "Consensus hash must match identically regardless of worker pool dispatch"
    );
}
