//! Integration test for Volumetric Shockwave Pipeline within NetworkMatchDirector (T-226)

use axiom_core::event_bus::channel;
use axiom_core::job_system::JobSystem;
use axiom_core::shockwave::{
    ShockwaveReport, VolumetricShockwaveEngine, EVENT_SHOCKWAVE,
};
use axiom_core::worker_pool::WorkerPoolCoordinator;
use axiom_game::network::NetworkMatchDirector;

#[test]
fn test_network_match_director_shockwave_step() {
    let mut director = NetworkMatchDirector::<64>::new(0, 0xFFFF);
    let mut engine = VolumetricShockwaveEngine::<16, 16>::new();

    let report = ShockwaveReport::from_explosion(
        (500, 100, -200),
        300,
        700_000,
        90_000,
        5,
        1,
        220,
    );

    let spawned = director.step_shockwave(&mut engine, &report);
    assert!(spawned, "Must spawn shockwave and dust cloud");
    assert_eq!(engine.active_shockwaves_count(), 1);
    assert_eq!(engine.active_dust_count(), 1);
    assert_eq!(director.event_bus.count, 1, "Must record 1 shockwave event to event bus");

    let event = &director.event_bus.events[0];
    assert_eq!(event.event_type, EVENT_SHOCKWAVE);
    assert_eq!(event.channel_mask, channel::PHYSICS | channel::VFX);
    assert_eq!(event.source_id, 5);

    let decoded = ShockwaveReport::decode(&event.payload);
    assert_eq!(decoded.center_x, 500);
    assert_eq!(decoded.center_y, 100);
    assert_eq!(decoded.center_z, -200);
    assert_eq!(decoded.radius_mm, 300);
    assert_eq!(decoded.peak_impulse_j, 90_000);
    assert_eq!(decoded.dust_density_q8, 220);
}

#[test]
fn test_network_match_director_shockwave_with_jobs() {
    let mut director = NetworkMatchDirector::<64>::new(0, 0xFFFF);
    let mut engine = VolumetricShockwaveEngine::<16, 16>::new();
    let mut job_sys = JobSystem::<64>::new();
    let mut coordinator = WorkerPoolCoordinator::<4, 64>::new(4);

    let report = ShockwaveReport::from_explosion(
        (-800, 0, 400),
        250,
        650_000,
        70_000,
        12,
        2,
        150,
    );

    let spawned = director.step_shockwave_with_jobs(
        &mut engine,
        &report,
        &mut job_sys,
        &mut coordinator,
    );
    assert!(spawned, "Must spawn shockwave via parallel worker jobs");
    assert_eq!(engine.active_shockwaves_count(), 1);
    assert_eq!(engine.active_dust_count(), 1);
    assert_eq!(director.event_bus.count, 1);
    assert_eq!(director.event_bus.events[0].event_type, EVENT_SHOCKWAVE);
    assert_eq!(director.event_bus.events[0].source_id, 12);
}
