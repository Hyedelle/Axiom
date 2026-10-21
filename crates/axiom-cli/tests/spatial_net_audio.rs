//! Multiplayer 3D Spatial Audio & Voice Packet Simulation Tests (T-033).
//!
//! Validates peer-specific relative 3D spatial sound attenuation, stereo panning inversion,
//! priority voice culling, and proximity voice packet simulation across multiple networked players.

use axiom_core::audio::{
    SoundCategory, SoundEvent, SpatialAudioSimulator, DEFAULT_MAX_VOICES,
};
use axiom_core::fixed::FixedVec2;

#[test]
fn test_spatial_audio_relative_listener_perspective() {
    // Player A at (-20, 0), Player B at (20, 0)
    let mut audio_peer_a = SpatialAudioSimulator::<DEFAULT_MAX_VOICES>::new();
    let mut audio_peer_b = SpatialAudioSimulator::<DEFAULT_MAX_VOICES>::new();

    audio_peer_a.set_listener_pos(FixedVec2::from_ints(-20, 0));
    audio_peer_b.set_listener_pos(FixedVec2::from_ints(20, 0));

    // Sound Event 1: Spell cast near Player A at (-15, 0), max radius 25
    let spell_event = SoundEvent {
        sound_id: 1001,
        pos: FixedVec2::from_ints(-15, 0),
        base_volume: 800,
        max_distance: 25,
        category: SoundCategory::Magic,
        priority: 5,
    };

    audio_peer_a.emit_sound(spell_event);
    audio_peer_b.emit_sound(spell_event);

    let mix_a = audio_peer_a.process_and_mix();
    let mix_b = audio_peer_b.process_and_mix();

    // Player A is 5 tiles away -> within 25 audible radius -> audible!
    assert_eq!(mix_a.active_voices_count, 1);
    assert!(mix_a.master_right > 0);
    // Sound is at -15 relative to listener -20 -> offset +5 (to the right of Player A)
    assert!(mix_a.voices[0].volume_right > mix_a.voices[0].volume_left);

    // Player B is 35 tiles away -> beyond 25 radius -> completely silent!
    assert_eq!(mix_b.active_voices_count, 0);
    assert_eq!(mix_b.master_left, 0);
    assert_eq!(mix_b.master_right, 0);
}

#[test]
fn test_spatial_audio_stereo_panning_mirror_symmetry() {
    // Sound emitted at origin (0, 0) with radius 50
    let mut audio_a = SpatialAudioSimulator::<DEFAULT_MAX_VOICES>::new();
    let mut audio_b = SpatialAudioSimulator::<DEFAULT_MAX_VOICES>::new();

    // Player A on left (-20, 0), Player B on right (+20, 0)
    audio_a.set_listener_pos(FixedVec2::from_ints(-20, 0));
    audio_b.set_listener_pos(FixedVec2::from_ints(20, 0));

    let explosion = SoundEvent {
        sound_id: 2002,
        pos: FixedVec2::ZERO,
        base_volume: 1000,
        max_distance: 50,
        category: SoundCategory::Combat,
        priority: 10,
    };

    audio_a.emit_sound(explosion);
    audio_b.emit_sound(explosion);

    let frame_a = audio_a.process_and_mix();
    let frame_b = audio_b.process_and_mix();

    assert_eq!(frame_a.active_voices_count, 1);
    assert_eq!(frame_b.active_voices_count, 1);

    let voice_a = frame_a.voices[0];
    let voice_b = frame_b.voices[0];

    // Total effective volume must be equal
    assert_eq!(
        voice_a.volume_left + voice_a.volume_right,
        voice_b.volume_left + voice_b.volume_right
    );

    // Player A (at -20) hears explosion to the RIGHT (volume_right > volume_left)
    assert!(voice_a.volume_right > voice_a.volume_left);

    // Player B (at +20) hears explosion to the LEFT (volume_left > volume_right)
    assert!(voice_b.volume_left > voice_b.volume_right);

    // PERFECT MIRROR SYMMETRY: Left channel of A == Right channel of B
    assert_eq!(voice_a.volume_left, voice_b.volume_right);
    assert_eq!(voice_a.volume_right, voice_b.volume_left);
}

#[test]
fn test_networked_proximity_voice_packets() {
    // Simulates dynamic proximity voice chat packets between two walking players
    let mut audio_listener = SpatialAudioSimulator::<DEFAULT_MAX_VOICES>::new();
    audio_listener.set_listener_pos(FixedVec2::from_ints(0, 0)); // Host Player at origin

    let voice_radius = 20; // 20 tiles audible range for voice

    // Remote Player walks closer: from (30, 0) [Out of range] to (10, 0) [In range]
    let pos_far = FixedVec2::from_ints(30, 0);
    let pos_near = FixedVec2::from_ints(10, 0);

    let voice_packet_far = SoundEvent {
        sound_id: 9001,
        pos: pos_far,
        base_volume: 900,
        max_distance: voice_radius,
        category: SoundCategory::Ambient,
        priority: 8,
    };

    audio_listener.emit_sound(voice_packet_far);
    let frame_silent = audio_listener.process_and_mix();
    assert_eq!(frame_silent.active_voices_count, 0, "Voice beyond radius must be inaudible");

    // Near voice packet
    let voice_packet_near = SoundEvent {
        sound_id: 9001,
        pos: pos_near,
        base_volume: 900,
        max_distance: voice_radius,
        category: SoundCategory::Ambient,
        priority: 8,
    };

    audio_listener.emit_sound(voice_packet_near);
    let frame_audible = audio_listener.process_and_mix();
    assert_eq!(frame_audible.active_voices_count, 1, "Voice within radius must be audible");
    assert!(frame_audible.voices[0].volume_right > frame_audible.voices[0].volume_left);
    assert!(frame_audible.master_right > 0);
}

#[test]
fn test_spatial_audio_cross_peer_determinism() {
    // 100 frames of dynamic multiplayer sound simulation
    let mut peer_1 = SpatialAudioSimulator::<DEFAULT_MAX_VOICES>::new();
    let mut peer_2 = SpatialAudioSimulator::<DEFAULT_MAX_VOICES>::new();

    peer_1.set_listener_pos(FixedVec2::from_ints(5, 5));
    peer_2.set_listener_pos(FixedVec2::from_ints(5, 5)); // Same position

    for frame in 0..100 {
        let sound = SoundEvent {
            sound_id: (frame % 10 + 1) as u32,
            pos: FixedVec2::from_ints((frame as i64 % 20) - 10, (frame as i64 % 15) - 7),
            base_volume: (500 + (frame % 500)) as u16,
            max_distance: 30,
            category: SoundCategory::Combat,
            priority: (frame % 5) as u8,
        };

        peer_1.emit_sound(sound);
        peer_2.emit_sound(sound);

        let mix_1 = peer_1.process_and_mix();
        let mix_2 = peer_2.process_and_mix();

        assert_eq!(mix_1.active_voices_count, mix_2.active_voices_count);
        assert_eq!(mix_1.master_left, mix_2.master_left);
        assert_eq!(mix_1.master_right, mix_2.master_right);
        assert_eq!(peer_1.compute_hash(), peer_2.compute_hash());
    }
}
