//! # Axiom Core - Ragdoll Job System & Event Bus Pipeline Tests (T-202)
//!
//! Verifies multithreaded partition dispatch via JobSystem and WorkerPoolCoordinator,
//! and deterministic event emission of impacts and limb severances.

use axiom_core::event_bus::*;
use axiom_core::job_system::*;
use axiom_core::ragdoll::*;
use axiom_core::worker_pool::*;

#[test]
fn test_ragdoll_multithreaded_job_dispatch() {
    let mut engine = RagdollEngine::<16, 16>::new();
    for i in 0..12 {
        let b = RagdollBoneSlab64::new(
            i as u8,
            0,
            ((i as i32) * 100, 1000, 0),
            65536,
            50,
            100,
        );
        engine.add_bone(b);
    }

    let mut job_sys = JobSystem::<32>::new();
    let mut coordinator = WorkerPoolCoordinator::<4, 16>::new(4);

    let dispatched = dispatch_ragdoll_with_jobs(&engine, 4, &mut job_sys, &mut coordinator);
    assert_eq!(dispatched, 4, "Must dispatch 4 batch jobs across workers");

    // Verify jobs are ready
    for w in 0..4 {
        assert_eq!(coordinator.deques[w].len(), 1, "Worker {} must have 1 job", w);
    }

    // Drain all jobs across workers
    let executed = coordinator.drain_all(&mut job_sys);
    assert_eq!(executed, 4, "Must execute all 4 jobs");
    assert_eq!(coordinator.total_jobs_executed(), 4);
}

#[test]
fn test_ragdoll_event_bus_impact_and_severance_emission() {
    let mut bus = DeterministicEventBus::<64>::new();

    let impacts = [
        RagdollImpactReport {
            entity_id: 101,
            bone_id: 3,
            joint_id: 2,
            is_severed: 0,
            flags: 0x01,
            impact_force: 15_000,
            hit_x: 250,
            hit_y: 1100,
            hit_z: -50,
            impulse_x: 200,
            impulse_y: -100,
            impulse_z: 10,
        },
        RagdollImpactReport {
            entity_id: 102,
            bone_id: 5,
            joint_id: 4,
            is_severed: 1, // Ruptured limb severance
            flags: 0x02,
            impact_force: 85_000,
            hit_x: 600,
            hit_y: 800,
            hit_z: 120,
            impulse_x: 450,
            impulse_y: 300,
            impulse_z: -50,
        },
    ];

    let emitted = emit_ragdoll_impacts_to_event_bus(&impacts, 42, &mut bus);
    assert_eq!(emitted, 2, "Must emit 2 events");
    assert_eq!(bus.count, 2);

    let e0 = &bus.events[0];
    assert_eq!(e0.tick, 42);
    assert_eq!(e0.event_type, EVENT_RAGDOLL_IMPACT);
    assert_eq!(e0.source_id, 101);
    assert_eq!(e0.target_id, 3);

    // Verify e0 payload
    let e0_force = u32::from_le_bytes(e0.payload[8..12].try_into().unwrap());
    assert_eq!(e0_force, 15_000);

    let e1 = &bus.events[1];
    assert_eq!(e1.tick, 42);
    assert_eq!(e1.event_type, EVENT_RAGDOLL_SEVER);
    assert_eq!(e1.source_id, 102);
    assert_eq!(e1.target_id, 5);

    let e1_force = u32::from_le_bytes(e1.payload[8..12].try_into().unwrap());
    assert_eq!(e1_force, 85_000);
    assert_eq!(e1.payload[6], 1, "is_severed must be 1");
}
