//! Integration test for XPBD Mecha Ragdoll Pipeline within NetworkMatchDirector (T-232)

use axiom_core::event_bus::channel;
use axiom_core::job_system::JobSystem;
use axiom_core::worker_pool::WorkerPoolCoordinator;
use axiom_core::xpbd_ragdoll::{
    XpbdBoneSlab64, XpbdConstraintSlab64, XpbdEventReport, XpbdRagdollEngine, EVENT_XPBD_STRAIN,
    EVENT_XPBD_TEAR,
};
use axiom_game::network::NetworkMatchDirector;

#[test]
fn test_network_match_director_xpbd_step() {
    let mut director = NetworkMatchDirector::<64>::new(0, 0xFFFF);
    let mut engine = XpbdRagdollEngine::<16, 16>::new();

    let b0 = engine.add_bone(XpbdBoneSlab64::new(0, 255, (0, 2000, 0), 32768, 150, 400)).unwrap();
    let b1 = engine.add_bone(XpbdBoneSlab64::new(1, 0, (0, 1700, 0), 65536, 100, 300)).unwrap();
    engine.add_constraint(XpbdConstraintSlab64::new_distance(b0 as u8, b1 as u8, 300, 0, 3277, 13107));

    let report = XpbdEventReport {
        entity_id: 3,
        bone_a: 0,
        bone_b: 1,
        constraint_id: 0,
        is_severed: 0,
        strain_q16: 4500,
        tension_force_n: 35000,
        rupture_pos_xyz: [0, 1850, 0],
        impulse_xyz: [0, -100, 0],
    };

    let ok = director.step_xpbd(&mut engine, Some(&report));
    assert!(ok, "step_xpbd must return true");
    assert_eq!(engine.tick, 1);
    assert_eq!(director.event_bus.count, 1, "Must emit 1 event to event bus");

    let event = &director.event_bus.events[0];
    assert_eq!(event.event_type, EVENT_XPBD_STRAIN);
    assert_eq!(event.channel_mask, channel::PHYSICS | channel::COMBAT);
    assert_eq!(event.source_id, 3);

    let decoded = XpbdEventReport::decode(&event.payload);
    assert_eq!(decoded.entity_id, 3);
    assert_eq!(decoded.strain_q16, 4500);
    assert_eq!(decoded.tension_force_n, 35000);
}

#[test]
fn test_network_match_director_xpbd_with_jobs() {
    let mut director = NetworkMatchDirector::<64>::new(0, 0xFFFF);
    let mut engine = XpbdRagdollEngine::<16, 16>::new();
    let mut job_sys = JobSystem::<64>::new();
    let mut coordinator = WorkerPoolCoordinator::<4, 64>::new(4);

    for i in 0..4 {
        engine.add_bone(XpbdBoneSlab64::new(
            i as u8,
            if i == 0 { 255 } else { (i - 1) as u8 },
            ((i as i32) * 200, 2000, 0),
            65536,
            80,
            200,
        ));
    }

    let tear_report = XpbdEventReport {
        entity_id: 7,
        bone_a: 2,
        bone_b: 3,
        constraint_id: 2,
        is_severed: 1,
        strain_q16: 18000,
        tension_force_n: 95000,
        rupture_pos_xyz: [600, 2000, 0],
        impulse_xyz: [200, 400, 0],
    };

    let ok = director.step_xpbd_with_jobs(&mut engine, Some(&tear_report), &mut job_sys, &mut coordinator);
    assert!(ok);
    assert_eq!(engine.tick, 1);
    assert_eq!(director.event_bus.count, 1);

    let event = &director.event_bus.events[0];
    assert_eq!(event.event_type, EVENT_XPBD_TEAR);
    assert_eq!(event.channel_mask, channel::PHYSICS | channel::COMBAT);
    assert_eq!(event.source_id, 7);

    let decoded = XpbdEventReport::decode(&event.payload);
    assert_eq!(decoded.is_severed, 1);
    assert_eq!(decoded.tension_force_n, 95000);
}
