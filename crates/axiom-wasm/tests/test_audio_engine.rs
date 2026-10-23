//! Comprehensive test suite for Axiom WASM procedural audio, reverb, and spatial panning.

use axiom_wasm::audio::{
    apply_arena_reverb, apply_pitch_shift, compute_spatial_stereo, generate_pcm_samples,
    get_stereo_buffer_ptr, get_stereo_frame_count, get_worklet_mixer, render_worklet_block,
    synthesize_to_static_buffer, SoundKind, WorkletMixer, SAMPLE_RATE,
};

#[test]
fn test_all_sound_kinds_synthesis() {
    assert_eq!(SAMPLE_RATE, 22050);

    let kinds = [
        SoundKind::HitJab,
        SoundKind::HitCross,
        SoundKind::HitHook,
        SoundKind::HitCounter,
        SoundKind::HitKo,
        SoundKind::GuardHit,
        SoundKind::SlipWhoosh,
        SoundKind::SteamRelease,
        SoundKind::BellRing,
        SoundKind::KoFanfare,
        SoundKind::Shoot,
        SoundKind::Build,
        SoundKind::Demolish,
        SoundKind::VoxelBlast,
        SoundKind::DebrisScatter,
    ];

    for &k in &kinds {
        let pcm = generate_pcm_samples(k);
        assert!(!pcm.is_empty(), "Sound {:?} generated 0 samples", k);
        let max_val = pcm.iter().map(|&s| s.unsigned_abs() as u32).max().unwrap_or(0);
        assert!(max_val > 1000, "Sound {:?} is silent (max: {})", k, max_val);
    }
}

#[test]
fn test_pitch_shift_scaling() {
    let raw = generate_pcm_samples(SoundKind::HitCross);
    let orig_len = raw.len();

    let pitched_up = apply_pitch_shift(&raw, 1500); // 1.5x pitch -> shorter length
    assert!(pitched_up.len() < orig_len);

    let pitched_down = apply_pitch_shift(&raw, 800); // 0.8x pitch -> longer length
    assert!(pitched_down.len() > orig_len);
}

#[test]
fn test_arena_reverb_decay() {
    let raw = generate_pcm_samples(SoundKind::HitJab);
    let dry_len = raw.len();

    let reverbed = apply_arena_reverb(&raw, 500);
    assert_eq!(reverbed.len(), dry_len + 4410); // ~200ms tail

    // Tail should have non-zero reverberations
    let tail_max = reverbed[dry_len..].iter().map(|&s| s.unsigned_abs() as u32).max().unwrap_or(0);
    assert!(tail_max > 0, "Reverb tail is silent");
}

#[test]
fn test_3d_spatial_stereo_panning() {
    // Center, standard distance
    let (c_l, c_r) = compute_spatial_stereo(0, 1000, 1000);
    assert_eq!(c_l, c_r);
    assert_eq!(c_l, 500);

    // Hard Left
    let (l_l, l_r) = compute_spatial_stereo(-1000, 1000, 1000);
    assert!(l_l > 900);
    assert_eq!(l_r, 0);

    // Hard Right
    let (r_l, r_r) = compute_spatial_stereo(1000, 1000, 1000);
    assert_eq!(r_l, 0);
    assert!(r_r > 900);

    // Distance attenuation
    let (close_l, close_r) = compute_spatial_stereo(0, 500, 1000);
    let (far_l, far_r) = compute_spatial_stereo(0, 2000, 1000);
    assert!(close_l > far_l);
    assert!(close_r > far_r);
}

#[test]
fn test_static_buffer_synthesizer_api() {
    let frames = synthesize_to_static_buffer(SoundKind::HitCross, 1000, 250, -400, 1000);
    assert!(frames > 0);
    assert_eq!(get_stereo_frame_count(), frames);

    let ptr = get_stereo_buffer_ptr();
    assert!(!ptr.is_null());

    unsafe {
        let first_left = *ptr;
        let first_right = *ptr.add(1);
        // Pan was -400 (left-biased), so left should be louder than right
        assert!(first_left.unsigned_abs() >= first_right.unsigned_abs());
    }
}

#[test]
fn test_worklet_multi_voice_streaming() {
    let mixer = get_worklet_mixer();
    assert!(mixer.play_sound(SoundKind::HitJab, 1000, 200, 0, 1000));
    assert!(mixer.play_sound(SoundKind::HitHook, 900, 400, 300, 900));
    assert_eq!(mixer.active_voice_count(), 2);

    let block_ptr = render_worklet_block();
    assert!(!block_ptr.is_null());
}

#[test]
fn test_voxel_blast_and_debris_spatial_audio() {
    let pcm_blast = generate_pcm_samples(SoundKind::VoxelBlast);
    assert!(!pcm_blast.is_empty());
    assert!(pcm_blast.len() >= 17640);

    let pcm_debris = generate_pcm_samples(SoundKind::DebrisScatter);
    assert!(!pcm_debris.is_empty());
    assert!(pcm_debris.len() >= 7700);

    let mut mixer = WorkletMixer::new();
    assert!(mixer.play_sound(SoundKind::VoxelBlast, 800, 850, 0, 1000));
    assert!(mixer.play_sound(SoundKind::DebrisScatter, 1150, 400, 200, 1000));
    assert_eq!(mixer.active_voice_count(), 2);
}
