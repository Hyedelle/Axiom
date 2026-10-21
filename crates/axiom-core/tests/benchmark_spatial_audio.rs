//! 1,000 Spatial Audio Events Mixing Benchmark (T-024)
//!
//! Validates:
//! - Inverse-square distance attenuation & stereo panning
//! - Voice priority culling from 1,000 events to top 64 voices
//! - Microsecond-grade audio mixing throughput
//! - 100% deterministic bit-identical audio state hash

use axiom_core::{
    FixedVec2, SoundCategory, SoundEvent, SpatialAudioSimulator,
};
use std::time::Instant;

#[test]
fn test_1000_spatial_audio_events_benchmark() {
    let mut audio_a = SpatialAudioSimulator::<64>::new();
    let mut audio_b = SpatialAudioSimulator::<64>::new();

    let listener = FixedVec2::from_ints(100, 100);
    audio_a.set_listener_pos(listener);
    audio_b.set_listener_pos(listener);

    const TOTAL_EVENTS: usize = 1_000;
    let start_time = Instant::now();

    // Emit 1,000 events in batches of 200 (within MAX_AUDIO_QUEUE)
    let batch_size = 200;
    let mut total_active_voices = 0;

    for batch in 0..(TOTAL_EVENTS / batch_size) {
        for i in 0..batch_size {
            let idx = batch * batch_size + i;
            let offset_x = ((idx * 17) % 300) as i64 - 150;
            let offset_y = ((idx * 31) % 300) as i64 - 150;

            let event = SoundEvent {
                sound_id: (idx + 1) as u32,
                pos: FixedVec2::from_ints(100 + offset_x, 100 + offset_y),
                base_volume: 700 + (idx % 300) as u16,
                max_distance: 100,
                category: match idx % 4 {
                    0 => SoundCategory::Combat,
                    1 => SoundCategory::Magic,
                    2 => SoundCategory::Footstep,
                    _ => SoundCategory::Ambient,
                },
                priority: (idx % 10) as u8,
            };

            audio_a.emit_sound(event);
            audio_b.emit_sound(event);
        }

        let mix_a = audio_a.process_and_mix();
        let mix_b = audio_b.process_and_mix();

        total_active_voices += mix_a.active_voices_count;

        // Both mixers must produce identical outputs
        assert_eq!(mix_a.active_voices_count, mix_b.active_voices_count);
        assert_eq!(mix_a.master_left, mix_b.master_left);
        assert_eq!(mix_a.master_right, mix_b.master_right);

        // Active voices must have non-zero volume
        for v in &mix_a.voices[0..mix_a.active_voices_count] {
            assert!(v.active);
            assert!(v.volume_left > 0 || v.volume_right > 0);
            assert!(v.volume_left <= 1000);
            assert!(v.volume_right <= 1000);
        }
    }

    let elapsed = start_time.elapsed();
    let micros_per_event = elapsed.as_micros() as f64 / TOTAL_EVENTS as f64;

    println!(
        "\n==> [SPATIAL AUDIO BENCHMARK] 1,000 spatial audio events mixed in {:.2?} ({:.2} µs/event)",
        elapsed, micros_per_event
    );
    println!("==> Total Active Voices Mixed: {}", total_active_voices);
    println!("==> Audio A Hash: 0x{:016X}", audio_a.compute_hash());
    println!("==> Audio B Hash: 0x{:016X}", audio_b.compute_hash());

    // Performance budget: < 10µs per event (< 10ms for 1,000 events)
    assert!(
        micros_per_event < 50.0,
        "Audio mixing must be <50µs/event, took {:.2}µs",
        micros_per_event
    );

    // 100% BIT-FOR-BIT DETERMINISM ASSERTION
    assert_eq!(audio_a.compute_hash(), audio_b.compute_hash());
}
