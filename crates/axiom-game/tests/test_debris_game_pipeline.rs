//! Integration test for Debris Particle Pipeline within NetworkMatchDirector (T-220)

use axiom_core::debris_particle::{
    DebrisBurstReport, DebrisParticlePool, DEBRIS_MAT_STONE, DEBRIS_MAT_METAL,
    EVENT_DEBRIS_BURST,
};
use axiom_core::event_bus::channel;
use axiom_core::job_system::JobSystem;
use axiom_core::worker_pool::WorkerPoolCoordinator;
use axiom_game::network::NetworkMatchDirector;

#[test]
fn test_network_match_director_debris_burst_step() {
    let mut director = NetworkMatchDirector::<64>::new(0, 0xFFFF);
    let mut pool = DebrisParticlePool::<128>::new(12345, 0, 4000);

    let report = DebrisBurstReport::from_burst(
        (800, 100, -300),
        32,
        2500,
        7500,
        16384,
        DEBRIS_MAT_STONE,
        7,
        40000,
    );

    let spawned = director.step_debris_burst(&mut pool, &report);
    assert_eq!(spawned, 32, "Must spawn exactly 32 debris particles");
    assert_eq!(pool.active_count(), 32);
    assert_eq!(director.event_bus.count, 1, "Must emit 1 debris burst event to bus");

    let event = &director.event_bus.events[0];
    assert_eq!(event.event_type, EVENT_DEBRIS_BURST);
    assert_eq!(event.channel_mask, channel::PHYSICS | channel::VFX);
    assert_eq!(event.source_id, 7);

    let decoded = DebrisBurstReport::decode(&event.payload);
    assert_eq!(decoded.origin_x, 800);
    assert_eq!(decoded.origin_y, 100);
    assert_eq!(decoded.origin_z, -300);
    assert_eq!(decoded.particle_count, 32);
    assert_eq!(decoded.emitter_id, 7);
}

#[test]
fn test_network_match_director_debris_burst_with_jobs() {
    let mut director = NetworkMatchDirector::<64>::new(0, 0xFFFF);
    let mut pool = DebrisParticlePool::<128>::new(54321, 0, 4000);
    let mut job_sys = JobSystem::<64>::new();
    let mut coordinator = WorkerPoolCoordinator::<4, 64>::new(4);

    let report = DebrisBurstReport::from_burst(
        (-1200, 50, 600),
        40,
        3000,
        9000,
        12288,
        DEBRIS_MAT_METAL,
        15,
        65000,
    );

    let spawned = director.step_debris_burst_with_jobs(
        &mut pool,
        &report,
        &mut job_sys,
        &mut coordinator,
    );
    assert_eq!(spawned, 40, "Must spawn 40 debris particles through worker jobs");
    assert_eq!(pool.active_count(), 40);
    assert_eq!(director.event_bus.count, 1);
    assert_eq!(director.event_bus.events[0].event_type, EVENT_DEBRIS_BURST);
    assert_eq!(director.event_bus.events[0].source_id, 15);
}
