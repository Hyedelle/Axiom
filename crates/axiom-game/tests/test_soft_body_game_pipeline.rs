//! Integration test for Soft Body Shape Matching Pipeline within NetworkMatchDirector (T-238)

use axiom_core::event_bus::channel;
use axiom_core::job_system::JobSystem;
use axiom_core::soft_body::{
    SoftBodyClusterSlab64, SoftBodyEngine, SoftBodyEventReport, SoftBodyParticleSlab64,
    EVENT_SOFT_BODY_DEFORM, SOFT_MAT_RUBBER,
};
use axiom_core::worker_pool::WorkerPoolCoordinator;
use axiom_game::network::NetworkMatchDirector;

#[test]
fn test_network_match_director_soft_body_step() {
    let mut director = NetworkMatchDirector::<64>::new(0, 0xFFFF);
    let mut engine = SoftBodyEngine::<16, 2>::new();

    let cluster = SoftBodyClusterSlab64::new(
        (0, 1000, 0),
        0,
        4,
        32768,
        32768,
        1_000_000,
        7,
    );
    engine.add_cluster(cluster);

    let offsets = [(-50, 0, 0), (50, 0, 0), (0, -50, 0), (0, 50, 0)];
    for &off in &offsets {
        let p = SoftBodyParticleSlab64::new(
            (off.0, 1000 + off.1, off.2),
            off,
            65536,
            15,
            0,
            SOFT_MAT_RUBBER,
        );
        engine.add_particle(p);
    }

    let report = SoftBodyEventReport {
        entity_id: 7,
        cluster_id: 0,
        flags: 0,
        max_strain_q16: 4500,
        volume_ratio_q16: 65536,
        centroid_pos_xyz: [0, 1000, 0],
        deform_impulse_xyz: [0, -100, 0],
    };

    let ok = director.step_soft_body(&mut engine, Some(&report));
    assert!(ok, "step_soft_body must return true");
    assert_eq!(engine.tick, 1);
    assert_eq!(director.event_bus.count, 1, "Must emit 1 event to event bus");

    let event = &director.event_bus.events[0];
    assert_eq!(event.event_type, EVENT_SOFT_BODY_DEFORM);
    assert_eq!(event.channel_mask, channel::PHYSICS | channel::VFX | channel::AUDIO);
    assert_eq!(event.source_id, 7);

    let decoded = SoftBodyEventReport::decode(&event.payload);
    assert_eq!(decoded.entity_id, 7);
    assert_eq!(decoded.max_strain_q16, 4500);
    assert_eq!(decoded.volume_ratio_q16, 65536);
}

#[test]
fn test_network_match_director_soft_body_with_jobs() {
    let mut director = NetworkMatchDirector::<64>::new(0, 0xFFFF);
    let mut engine = SoftBodyEngine::<32, 4>::new();
    let mut job_system = JobSystem::<128>::new();
    let mut coordinator = WorkerPoolCoordinator::<4, 32>::new(4);

    for c in 0..4 {
        let cluster = SoftBodyClusterSlab64::new(
            (c as i32 * 500, 1000, 0),
            (c * 4) as u16,
            4,
            32768,
            32768,
            1_000_000,
            c as u8,
        );
        engine.add_cluster(cluster);
    }

    let report = SoftBodyEventReport {
        entity_id: 1,
        cluster_id: 0,
        flags: 1,
        max_strain_q16: 8000,
        volume_ratio_q16: 62000,
        centroid_pos_xyz: [500, 1000, 0],
        deform_impulse_xyz: [100, 0, 0],
    };

    let ok = director.step_soft_body_with_jobs(
        &mut engine,
        Some(&report),
        &mut job_system,
        &mut coordinator,
    );
    assert!(ok, "step_soft_body_with_jobs must return true");
    assert_eq!(engine.tick, 1);
    assert_eq!(director.event_bus.count, 1);
}
