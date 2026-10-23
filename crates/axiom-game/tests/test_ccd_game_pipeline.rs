//! Integration verification of CCD Hitbox Physics, Job System & Event Bus Pipeline (T-196)

use axiom_core::ccd::EVENT_CCD_HIT;
use axiom_core::event_bus::channel;
use axiom_core::job_system::JobSystem;
use axiom_core::worker_pool::WorkerPoolCoordinator;
use axiom_game::network::{
    NetworkMatchDirector, STATE_HEAVY_ATTACK, STATE_LIGHT_ATTACK,
};

#[test]
fn test_match_director_step_ccd_synchronous() {
    let mut dir = NetworkMatchDirector::<32>::new(0, 0x0003); // 2 active peers

    // Setup player 0 attacking with heavy punch
    dir.current_world.players[0].pos_x = 0;
    dir.current_world.players[0].pos_y = 0;
    dir.current_world.players[0].pos_z = 0;
    dir.current_world.players[0].action_state = STATE_HEAVY_ATTACK;
    dir.current_world.players[0].vel_x = 600; // 600 mm displacement punch

    // Setup player 1 defender at [400, 0, 0]
    dir.current_world.players[1].pos_x = 400;
    dir.current_world.players[1].pos_y = 0;
    dir.current_world.players[1].pos_z = 0;
    dir.current_world.players[1].health = 1000;

    let hits = dir.step_ccd();
    assert!(hits >= 1, "Heavy punch at 400mm must register CCD hit");
    assert!(dir.event_bus.count >= 1, "Event bus must contain emitted CCD hit event");

    let mut found_ccd_event = false;
    dir.event_bus.for_each_matching(channel::COMBAT, |ev| {
        if ev.event_type == EVENT_CCD_HIT {
            found_ccd_event = true;
            assert_eq!(ev.source_id, 1); // attacker is player 0 + 1 = 1
            assert_eq!(ev.target_id, 2); // defender is player 1 + 1 = 2
        }
    });
    assert!(found_ccd_event, "Must find matching EVENT_CCD_HIT in event bus");
}

#[test]
fn test_match_director_step_ccd_with_jobs() {
    let mut dir = NetworkMatchDirector::<32>::new(0, 0x0007); // 3 active peers
    let mut job_system = JobSystem::<128>::new();
    let mut coordinator = WorkerPoolCoordinator::<4, 32>::new(4);

    // Setup player 0 attacking with light punch
    dir.current_world.players[0].pos_x = 0;
    dir.current_world.players[0].pos_y = 0;
    dir.current_world.players[0].pos_z = 0;
    dir.current_world.players[0].action_state = STATE_LIGHT_ATTACK;
    dir.current_world.players[0].vel_x = 500;

    // Setup player 1 defender at [350, 0, 0]
    dir.current_world.players[1].pos_x = 350;
    dir.current_world.players[1].pos_y = 0;
    dir.current_world.players[1].pos_z = 0;
    dir.current_world.players[1].health = 1000;

    // Setup player 2 far away at [4000, 4000, 0]
    dir.current_world.players[2].pos_x = 4000;
    dir.current_world.players[2].pos_y = 4000;
    dir.current_world.players[2].pos_z = 0;
    dir.current_world.players[2].health = 1000;

    let hits = dir.step_ccd_with_jobs(&mut job_system, &mut coordinator);
    assert_eq!(hits, 1, "Light punch should hit player 1 and miss player 2");
    assert_eq!(dir.event_bus.count, 1);

    let mut found = false;
    dir.event_bus.for_each_matching(channel::PHYSICS, |ev| {
        if ev.event_type == EVENT_CCD_HIT {
            found = true;
            assert_eq!(ev.source_id, 1);
            assert_eq!(ev.target_id, 2);
        }
    });
    assert!(found, "Event bus must contain EVENT_CCD_HIT under channel::PHYSICS");
}
