//! Unit tests for Destruction Audio Job System & Event Bus Pipeline (T-214)

use axiom_core::destruction_audio::{
    dispatch_destruction_audio_with_jobs, emit_destruction_audio_to_event_bus,
    DestructionAudioEngine, DestructionAudioReport, DestructionVoiceSlab64, AUDIO_MAT_METAL,
    AUDIO_MAT_STONE, EVENT_AUDIO_DESTRUCTION, VOICE_FLAG_ACTIVE, VOICE_FLAG_CRACK_BURST,
};
use axiom_core::event_bus::{channel, DeterministicEventBus};
use axiom_core::job_system::JobSystem;
use axiom_core::worker_pool::WorkerPoolCoordinator;

#[test]
fn test_destruction_audio_job_dispatch_and_drain() {
    let mut engine = DestructionAudioEngine::<16, 4>::default();
    let v0 = DestructionVoiceSlab64::from_impact(
        0, 0, 0,
        50_000,
        AUDIO_MAT_METAL,
        1,
        100,
    );
    let v1 = DestructionVoiceSlab64::from_impact(
        500, 0, 0,
        35_000,
        AUDIO_MAT_STONE,
        2,
        100,
    );
    assert!(engine.spawn_destruction_voice(v0));
    assert!(engine.spawn_destruction_voice(v1));
    assert_eq!(engine.active_voices_count(), 2);

    let mut job_sys = JobSystem::<64>::new();
    let mut coordinator = WorkerPoolCoordinator::<4, 64>::new(4);

    let dispatched = dispatch_destruction_audio_with_jobs(&engine, 2, &mut job_sys, &mut coordinator);
    assert_eq!(dispatched, 2, "Must dispatch 2 worker partition jobs");

    let executed = coordinator.drain_all(&mut job_sys);
    assert_eq!(executed, 2, "Must drain all 2 jobs cooperatively");
    assert_eq!(job_sys.active_count, 0, "No dangling active jobs should remain");
}

#[test]
fn test_destruction_audio_event_bus_emission_and_payload_roundtrip() {
    let mut bus = DeterministicEventBus::<64>::new();

    let report = DestructionAudioReport::from_impact(
        (-1500, 200, 750),
        420,
        29000,
        AUDIO_MAT_STONE,
        88,
        50_000,
    );

    let success = emit_destruction_audio_to_event_bus(&report, 128, &mut bus);
    assert!(success, "Event emission must succeed");
    assert_eq!(bus.count, 1, "Bus must hold exactly 1 event");

    let event = &bus.events[0];
    assert_eq!(event.tick, 128);
    assert_eq!(event.event_type, EVENT_AUDIO_DESTRUCTION);
    assert_eq!(event.channel_mask, channel::PHYSICS | channel::AUDIO);
    assert_eq!(event.source_id, 88);

    // Decode 40-byte inline POD payload
    let decoded = DestructionAudioReport::decode(&event.payload);
    assert_eq!(decoded.pos_x, -1500);
    assert_eq!(decoded.pos_y, 200);
    assert_eq!(decoded.pos_z, 750);
    assert_eq!(decoded.base_freq_hz, 420);
    assert_eq!(decoded.amplitude_q16, 29000);
    assert_eq!(decoded.material, AUDIO_MAT_STONE);
    assert_eq!(decoded.source_id, 88);
    assert_eq!(decoded.energy_impulse, 50_000);
    assert_eq!(decoded.voice_flags, VOICE_FLAG_ACTIVE | VOICE_FLAG_CRACK_BURST);
    assert_eq!(decoded.reverb_send_q16, 16384);
}
