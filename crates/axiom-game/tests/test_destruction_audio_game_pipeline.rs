//! Integration test for Destruction Audio Pipeline within NetworkMatchDirector (T-214)

use axiom_core::destruction_audio::{
    DestructionAudioEngine, DestructionVoiceSlab64, AUDIO_MAT_METAL, AUDIO_MAT_STONE,
    EVENT_AUDIO_DESTRUCTION,
};
use axiom_core::job_system::JobSystem;
use axiom_core::worker_pool::WorkerPoolCoordinator;
use axiom_game::network::NetworkMatchDirector;

#[test]
fn test_network_match_director_destruction_audio_step() {
    let mut director = NetworkMatchDirector::<64>::new(0, 0xFFFF);
    let mut engine = DestructionAudioEngine::<8, 2>::default();

    let voice = DestructionVoiceSlab64::from_impact(
        1000, 200, 500,
        60_000,
        AUDIO_MAT_METAL,
        1,
        director.current_world.tick,
    );

    let spawned = director.step_destruction_audio(&mut engine, &voice);
    assert!(spawned, "Destruction audio voice must spawn successfully");
    assert_eq!(director.event_bus.count, 1, "Must record 1 audio event to event bus");

    let event = &director.event_bus.events[0];
    assert_eq!(event.event_type, EVENT_AUDIO_DESTRUCTION);
    assert_eq!(event.source_id, 1);
}

#[test]
fn test_network_match_director_destruction_audio_with_jobs() {
    let mut director = NetworkMatchDirector::<64>::new(0, 0xFFFF);
    let mut engine = DestructionAudioEngine::<8, 2>::default();
    let mut job_sys = JobSystem::<64>::new();
    let mut coordinator = WorkerPoolCoordinator::<4, 64>::new(4);

    let voice = DestructionVoiceSlab64::from_impact(
        -500, 0, 200,
        45_000,
        AUDIO_MAT_STONE,
        2,
        director.current_world.tick,
    );

    let spawned = director.step_destruction_audio_with_jobs(
        &mut engine,
        &voice,
        &mut job_sys,
        &mut coordinator,
    );
    assert!(spawned, "Parallel audio step must spawn voice");
    assert_eq!(director.event_bus.count, 1);
    assert_eq!(director.event_bus.events[0].event_type, EVENT_AUDIO_DESTRUCTION);
    assert_eq!(director.event_bus.events[0].source_id, 2);
}
