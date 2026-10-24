//! Unit tests for Superconductor Multithreaded Job System & Event Bus Integration (T-274)

use axiom_core::superconductor::*;
use axiom_core::event_bus::{channel, DeterministicEventBus};
use axiom_core::job_system::JobSystem;
use axiom_core::worker_pool::WorkerPoolCoordinator;

#[test]
fn test_dispatch_superconductor_with_jobs() {
    let mut engine = MechaSuperconductorEngine::<16, 4>::new();
    let mut job_system = JobSystem::<64>::new();
    let mut coordinator = WorkerPoolCoordinator::<4, 16>::new(4);

    for i in 0..8 {
        let cell = SuperconductorSlab64::new(
            i as u8,
            [i as i32 * 500, 0, 100],
            77,
            93,
            30_000,
            SC_TYPE_II,
        );
        engine.register_cell(cell);
    }

    let dispatched = dispatch_superconductor_with_jobs(&engine, 4, &mut job_system, &mut coordinator);
    assert_eq!(dispatched, 4, "Must dispatch 4 jobs across the worker pool");

    let executed = coordinator.drain_all(&mut job_system);
    assert_eq!(executed, 4, "Must execute all 4 superconductor partition jobs");
}

#[test]
fn test_dispatch_superconductor_empty_engine() {
    let engine = MechaSuperconductorEngine::<16, 4>::new();
    let mut job_system = JobSystem::<64>::new();
    let mut coordinator = WorkerPoolCoordinator::<4, 16>::new(4);

    let dispatched = dispatch_superconductor_with_jobs(&engine, 4, &mut job_system, &mut coordinator);
    assert_eq!(dispatched, 0, "Empty engine must dispatch 0 jobs");
}

#[test]
fn test_emit_superconductor_events_to_event_bus() {
    let mut bus = DeterministicEventBus::<16>::new();

    // 1. Emit flux pinning event
    let pin_report = SuperconductorEventReport {
        pos_xyz: [1000, 2000, 100],
        force_xyz: [500, -250, 1200],
        flux_or_voltage: 150,
        temp_k: 77,
        pin_displacement_mm: 25,
        crit_ratio_q8: 180,
        emitter_id: 1,
        target_id: 2,
        flags: SC_FLAG_ACTIVE | SC_FLAG_FLUX_PINNED,
        event_kind: SC_EVENT_KIND_PINNING,
    };

    let emitted = emit_superconductor_to_event_bus(&pin_report, 100, &mut bus);
    assert!(emitted, "Must successfully emit flux pinning event");
    assert_eq!(bus.count, 1);

    let event = &bus.events[0];
    assert_eq!(event.event_type, EVENT_FLUX_PINNING);
    assert_eq!(event.tick, 100);
    assert_eq!(
        event.channel_mask & (channel::PHYSICS | channel::VFX | channel::AUDIO),
        channel::PHYSICS | channel::VFX | channel::AUDIO
    );

    // 2. Emit quench event
    let quench_report = SuperconductorEventReport {
        pos_xyz: [1000, 2000, 100],
        force_xyz: [0, 0, 0],
        flux_or_voltage: 48000, // 48V quench voltage spike
        temp_k: 105,            // Above Tc
        pin_displacement_mm: 0,
        crit_ratio_q8: 255,
        emitter_id: 1,
        target_id: 2,
        flags: SC_FLAG_ACTIVE | SC_FLAG_QUENCHED,
        event_kind: SC_EVENT_KIND_QUENCH,
    };

    let emitted2 = emit_superconductor_to_event_bus(&quench_report, 101, &mut bus);
    assert!(emitted2, "Must successfully emit superconductor quench event");
    assert_eq!(bus.count, 2);

    let event2 = &bus.events[1];
    assert_eq!(event2.event_type, EVENT_SUPERCONDUCT_QUENCH);
    assert_eq!(event2.tick, 101);

    // Decode and verify roundtrip from bus slab
    let decoded = SuperconductorEventReport::decode(event2.payload);
    assert_eq!(decoded.flux_or_voltage, 48000);
    assert_eq!(decoded.temp_k, 105);
    assert_eq!(decoded.event_kind, SC_EVENT_KIND_QUENCH);
}

#[test]
fn test_superconductor_multithreaded_consensus_consistency() {
    let mut engine1 = MechaSuperconductorEngine::<16, 4>::new();
    let mut engine2 = MechaSuperconductorEngine::<16, 4>::new();

    let mut job_sys = JobSystem::<64>::new();
    let mut coord = WorkerPoolCoordinator::<4, 16>::new(4);

    for i in 0..6 {
        let cell = SuperconductorSlab64::new(
            i as u8,
            [i as i32 * 200, 0, 50],
            77,
            93,
            30_000,
            SC_TYPE_II,
        );
        engine1.register_cell(cell);
        engine2.register_cell(cell);
    }

    let pin = FluxPinningSlab64::new(
        10,
        0,
        [0, 0, 100],
        200,
        65536 * 60,
        65536 * 6,
        15_000,
        50,
    );
    engine1.register_pin(pin);
    engine2.register_pin(pin);

    // Step engine 1 single-threaded
    for _ in 0..5 {
        engine1.step_simulation(1200);
    }

    // Step engine 2 with multithreaded dispatch
    for _ in 0..5 {
        dispatch_superconductor_with_jobs(&engine2, 4, &mut job_sys, &mut coord);
        coord.drain_all(&mut job_sys);
        engine2.step_simulation(1200);
    }

    assert_eq!(
        engine1.compute_consensus_hash(),
        engine2.compute_consensus_hash(),
        "Consensus hash must match identically regardless of worker pool dispatch"
    );
}
