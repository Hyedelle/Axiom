//! # Integration Test for NetworkMatchDirector Parallel Job Dispatch (T-178)

use axiom_core::job_system::JobSystem;
use axiom_core::worker_pool::WorkerPoolCoordinator;
use axiom_game::network::NetworkMatchDirector;

#[test]
fn test_network_match_job_dispatch_determinism() {
    let mut dir = Box::new(NetworkMatchDirector::<64>::new(0, 0xFFFF));
    let mut job_system = JobSystem::<128>::new();
    let mut coordinator = WorkerPoolCoordinator::<4, 64>::new(4);

    for tick in 1..=20 {
        let pkt = dir.advance_tick_with_jobs(
            axiom_game::network::INPUT_LIGHT_ATTACK,
            &mut job_system,
            &mut coordinator,
        );
        assert_eq!(pkt.current_frame, tick - 1);
    }

    assert_eq!(dir.current_world.tick, 20);
    // 20 ticks x 4 jobs per tick = 80 jobs executed
    assert_eq!(coordinator.total_jobs_executed(), 80);
    assert_eq!(dir.session.telemetry.total_desyncs, 0);
}
