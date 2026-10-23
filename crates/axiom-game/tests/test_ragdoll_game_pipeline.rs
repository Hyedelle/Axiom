//! # Axiom Game - Ragdoll Pipeline Integration Tests (T-202)
//!
//! Verifies NetworkMatchDirector ragdoll step and worker pool parallel job execution.

use axiom_core::job_system::JobSystem;
use axiom_core::worker_pool::WorkerPoolCoordinator;
use axiom_game::network::*;

#[test]
fn test_match_director_ragdoll_step_and_jobs() {
    let mut director = NetworkMatchDirector::<16>::new(0, 0x0003); // 2 peers

    // Advance 1 tick
    director.advance_tick(0);

    // Simulate peer 1 knockdown
    director.current_world.players[1].action_state = STATE_KNOCKDOWN;
    director.current_world.players[1].health = 100;

    // Step ragdoll synchronously
    let impacts = director.step_ragdoll();
    assert_eq!(impacts, 1, "Must generate 1 ragdoll impact for knocked down player");
    assert_eq!(director.event_bus.count, 1);

    // Step ragdoll with JobSystem
    let mut job_sys = JobSystem::<32>::new();
    let mut coordinator = WorkerPoolCoordinator::<4, 16>::new(4);

    let async_impacts = director.step_ragdoll_with_jobs(&mut job_sys, &mut coordinator);
    assert_eq!(async_impacts, 1);
    assert_eq!(director.event_bus.count, 2);
}
