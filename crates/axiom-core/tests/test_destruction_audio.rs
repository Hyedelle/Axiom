//! Unit tests for Procedural Destruction Audio Synthesis & Surface Reverb Field (T-213)

use core::mem::{align_of, size_of};
use axiom_core::destruction_audio::{
    synthesize_destruction_impact_pcm, AcousticZoneSlab64, DestructionAudioEngine,
    DestructionVoiceSlab64, AUDIO_MAT_DIRT, AUDIO_MAT_METAL, AUDIO_MAT_STONE,
    AUDIO_SAMPLE_RATE_48K, SINE_LUT_256,
};

#[test]
fn test_audio_slabs_64b_l1_alignment() {
    assert_eq!(
        size_of::<DestructionVoiceSlab64>(),
        64,
        "DestructionVoiceSlab64 must be exactly 64 bytes"
    );
    assert_eq!(
        align_of::<DestructionVoiceSlab64>(),
        64,
        "DestructionVoiceSlab64 must align to 64-byte L1 cache line"
    );

    assert_eq!(
        size_of::<AcousticZoneSlab64>(),
        64,
        "AcousticZoneSlab64 must be exactly 64 bytes"
    );
    assert_eq!(
        align_of::<AcousticZoneSlab64>(),
        64,
        "AcousticZoneSlab64 must align to 64-byte L1 cache line"
    );
}

#[test]
fn test_sine_lut_256_properties() {
    assert_eq!(SINE_LUT_256.len(), 256);
    assert_eq!(SINE_LUT_256[0], 0, "sin(0) must be 0");
    assert_eq!(SINE_LUT_256[64], 32767, "sin(pi/2) must be peak 32767");
    assert_eq!(SINE_LUT_256[128], 0, "sin(pi) must be 0");
    assert_eq!(SINE_LUT_256[192], -32767, "sin(3pi/2) must be trough -32767");

    // All values must be valid 16-bit PCM amplitude
    for &val in &SINE_LUT_256 {
        assert!(val >= -32768);
    }
}

#[test]
fn test_modal_synthesis_material_variations() {
    let mut voice_dirt = DestructionVoiceSlab64::from_impact(
        0, 0, 0,
        50_000,
        AUDIO_MAT_DIRT,
        1,
        100,
    );
    assert!(voice_dirt.is_active());
    assert_eq!(voice_dirt.base_freq_hz, 85);

    let mut buf_dirt = [0i16; 480]; // 10ms at 48kHz
    let rendered_dirt = synthesize_destruction_impact_pcm(
        &mut voice_dirt,
        &mut buf_dirt,
        AUDIO_SAMPLE_RATE_48K,
    );
    assert_eq!(rendered_dirt, 480);
    assert!(buf_dirt.iter().any(|&s| s != 0), "Must synthesize non-zero PCM");

    // Metal voice: higher fundamental and slower decay
    let mut voice_metal = DestructionVoiceSlab64::from_impact(
        0, 0, 0,
        50_000,
        AUDIO_MAT_METAL,
        2,
        100,
    );
    assert_eq!(voice_metal.base_freq_hz, 850);
    let mut buf_metal = [0i16; 480];
    synthesize_destruction_impact_pcm(
        &mut voice_metal,
        &mut buf_metal,
        AUDIO_SAMPLE_RATE_48K,
    );
    assert!(buf_metal.iter().any(|&s| s != 0));
}

#[test]
fn test_destruction_audio_engine_lifecycle_and_spatial_panning() {
    let mut engine = DestructionAudioEngine::<16, 4>::new(AUDIO_SAMPLE_RATE_48K);
    engine.listener_pos = (0, 1500, 0);

    // Spawn voice to the far right (+3000mm X)
    let slot = engine.spawn_voice(
        3000, 1500, 0,
        75_000,
        AUDIO_MAT_STONE,
        101,
        200,
    );
    assert!(slot.is_some());
    assert_eq!(engine.total_voices_spawned, 1);

    // Step synthesis
    let mut stereo_frame = [(0i16, 0i16); 800]; // 1 tick (~16.6ms)
    let active_voices = engine.step_synthesis(&mut stereo_frame);
    assert_eq!(active_voices, 1);

    // Verify right channel energy is higher due to positive X position (+3000mm)
    let mut left_energy: i64 = 0;
    let mut right_energy: i64 = 0;
    for (l, r) in stereo_frame {
        left_energy += (l as i64).abs();
        right_energy += (r as i64).abs();
    }

    assert!(right_energy > 0, "Right channel must receive energy");
    assert!(
        right_energy > left_energy,
        "Sound to the right must have higher right-channel energy (got L: {}, R: {})",
        left_energy, right_energy
    );
}

#[test]
fn test_destruction_audio_consensus_hash_determinism() {
    let mut engine1 = DestructionAudioEngine::<16, 4>::new(AUDIO_SAMPLE_RATE_48K);
    let mut engine2 = DestructionAudioEngine::<16, 4>::new(AUDIO_SAMPLE_RATE_48K);

    engine1.spawn_voice(-1500, 0, 500, 60_000, AUDIO_MAT_METAL, 42, 50);
    engine2.spawn_voice(-1500, 0, 500, 60_000, AUDIO_MAT_METAL, 42, 50);

    let mut frame1 = [(0i16, 0i16); 400];
    let mut frame2 = [(0i16, 0i16); 400];

    engine1.step_synthesis(&mut frame1);
    engine2.step_synthesis(&mut frame2);

    assert_eq!(frame1, frame2, "Synthesized waveforms must be bit-identical");

    let hash1 = engine1.compute_consensus_hash();
    let hash2 = engine2.compute_consensus_hash();

    assert_eq!(
        hash1, hash2,
        "Consensus hash must match bit-for-bit across identical engines"
    );
}
