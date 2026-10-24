//! Unit tests for Turbulence Multithreaded Job System & Event Bus Integration (T-256)

use axiom_core::turbulence::*;
use axiom_core::event_bus::DeterministicEventBus;
use axiom_core::job_system::JobSystem;
use axiom_core::worker_pool::WorkerPoolCoordinator;

#[test]
fn test_dispatch_turbulence_with_jobs() {
    let mut engine = MechaTurbulenceEngine::<16, 4>::new();
    let mut job_system = JobSystem::<64>::new();
    let mut coordinator = WorkerPoolCoordinator::<4, 16>::new(4);

    for i in 0..8 {
        let cell = TurbulenceCellSlab64::new(
            i as u8,
            [i as i32 * 500, 0, 0],
            [1000 + i as i32 * 200, 1500, 0],
        );
        engine.register_cell(cell);
    }

    let dispatched = dispatch_turbulence_with_jobs(&engine, 4, &mut job_system, &mut coordinator);
    assert_eq!(dispatched, 4, "Must dispatch 4 jobs across the worker pool");

    let executed = coordinator.drain_all(&mut job_system);
    assert_eq!(executed, 4, "Must execute all 4 turbulence partition jobs");
}

#[test]
fn test_emit_turbulence_vortex_to_event_bus() {
    let mut bus = DeterministicEventBus::<16>::new();

    let report = TurbulenceEventReport {
        pos_xyz: [1500, 2500, 750],
        vector_xyz: [500, 1200, 0], // Vorticity vector
        impulse_or_force_n: 350,
        tke_or_pressure_ratio_q16: 45000,
        mach_or_freq_q8: 3840, // 15.0 Hz
        bl_thickness_or_cell_id: 3,
        emitter_id: 2,
        target_id: 0,
        flags: TURB_FLAG_ACTIVE | TURB_FLAG_VORTEX_CORE,
        event_kind: TURB_EVENT_KIND_VORTEX,
    };

    let emitted = emit_turbulence_to_event_bus(&report, 60, &mut bus);
    assert!(emitted, "Must successfully emit vortex event to event bus");
    assert_eq!(bus.count, 1);

    let event = &bus.events[0];
    assert_eq!(event.event_type, EVENT_TURBULENCE_VORTEX);
    assert_eq!(event.source_id, 2);
    assert_eq!(event.target_id, 0);

    let decoded = TurbulenceEventReport::decode(&event.payload);
    assert_eq!(decoded.pos_xyz, [1500, 2500, 750]);
    assert_eq!(decoded.vector_xyz, [500, 1200, 0]);
    assert_eq!(decoded.impulse_or_force_n, 350);
    assert_eq!(decoded.tke_or_pressure_ratio_q16, 45000);
    assert_eq!(decoded.event_kind, TURB_EVENT_KIND_VORTEX);
}

#[test]
fn test_emit_shock_boundary_to_event_bus() {
    let mut bus = DeterministicEventBus::<16>::new();

    let report = TurbulenceEventReport {
        pos_xyz: [0, 0, 1800],
        vector_xyz: [0, 65536, 0], // Surface normal unit vector in Q16
        impulse_or_force_n: 4200,
        tke_or_pressure_ratio_q16: 98000, // P2/P1 > 1.20
        mach_or_freq_q8: 384, // 1.5 Mach in Q8.8
        bl_thickness_or_cell_id: 15,
        emitter_id: 7,
        target_id: 1,
        flags: SHOCK_BL_ACTIVE | SHOCK_BL_SUPERSONIC | SHOCK_BL_SEPARATED | SHOCK_BL_BUFFETING,
        event_kind: TURB_EVENT_KIND_SHOCK_BL,
    };

    let emitted = emit_turbulence_to_event_bus(&report, 120, &mut bus);
    assert!(emitted, "Must successfully emit SBLI event to event bus");
    assert_eq!(bus.count, 1);

    let event = &bus.events[0];
    assert_eq!(event.event_type, EVENT_SHOCK_BOUNDARY);
    assert_eq!(event.source_id, 7);
    assert_eq!(event.target_id, 1);

    let decoded = TurbulenceEventReport::decode(&event.payload);
    assert_eq!(decoded.flags & SHOCK_BL_SEPARATED, SHOCK_BL_SEPARATED);
    assert_eq!(decoded.flags & SHOCK_BL_BUFFETING, SHOCK_BL_BUFFETING);
    assert_eq!(decoded.impulse_or_force_n, 4200);
    assert_eq!(decoded.event_kind, TURB_EVENT_KIND_SHOCK_BL);
}
