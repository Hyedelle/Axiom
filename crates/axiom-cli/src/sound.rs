//! Lightweight zero-float procedural audio synthesis and native Windows playback for Axiom.
//!
//! Generates 22,050 Hz 16-bit Mono PCM RIFF/WAVE waveforms in pure integer arithmetic (0 float).
//! Uses Windows native `PlaySoundW` (`winmm`) with non-blocking memory buffers (`SND_ASYNC | SND_MEMORY`).
//! On non-Windows platforms or in headless tests, playback is a safe no-op.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

/// Audio sample rate in Hz.
pub const SAMPLE_RATE: u32 = 22050;

/// Kinds of procedural sound effects available in Axiom.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SoundKind {
    /// Turret firing shot (downward frequency sweep from 880Hz to 220Hz over 100ms).
    Shoot,
    /// Monster impact hit (punchy bass thump + noise burst over 60ms).
    Hit,
    /// Structure placement blip (two-tone chime 500Hz -> 1000Hz over 50ms).
    Build,
    /// Structure demolition crunch (falling noise + rumble over 80ms).
    Demolish,
    /// Quicksave chord (ascending arpeggio C5 -> E5 -> G5 over 200ms).
    SaveChime,
    /// Wave spawn warning siren (alternating 440Hz / 880Hz siren over 300ms).
    WaveAlarm,
    /// High-pressure steam release: mechanical valve latch snap + steam hiss burst (250ms).
    SteamRelease,
    /// Heavy counter impact: sub-bass thump + resonant metallic chime (350ms).
    CounterHit,
}

static MUTED: AtomicBool = AtomicBool::new(false);

/// Returns whether audio playback is currently muted.
#[inline]
pub fn is_muted() -> bool {
    MUTED.load(Ordering::Relaxed)
}

/// Toggles the audio mute state and returns the new muted state.
pub fn toggle_mute() -> bool {
    let prev = MUTED.fetch_xor(true, Ordering::SeqCst);
    !prev
}

/// Sets the audio mute state explicitly.
pub fn set_muted(muted: bool) {
    MUTED.store(muted, Ordering::SeqCst);
}

/// Generates 16-bit mono PCM samples for a given sound kind using integer arithmetic.
pub fn generate_pcm_samples(kind: SoundKind) -> Vec<i16> {
    match kind {
        SoundKind::Shoot => generate_shoot_samples(),
        SoundKind::Hit => generate_hit_samples(),
        SoundKind::Build => generate_build_samples(),
        SoundKind::Demolish => generate_demolish_samples(),
        SoundKind::SaveChime => generate_save_chime_samples(),
        SoundKind::WaveAlarm => generate_wave_alarm_samples(),
        SoundKind::SteamRelease => generate_steam_release_samples(),
        SoundKind::CounterHit => generate_counter_hit_samples(),
    }
}

/// Turret firing sound: frequency sweep from 880Hz down to 220Hz over 100ms.
fn generate_shoot_samples() -> Vec<i16> {
    let num_samples = (SAMPLE_RATE * 100) / 1000; // 2205 samples
    let mut samples = Vec::with_capacity(num_samples as usize);
    let mut phase: u32 = 0;

    for i in 0..num_samples {
        // Linear frequency sweep: 880 -> 220 Hz
        let freq = 880 - (660 * i) / num_samples;
        let phase_step = ((freq as u64) << 32) / SAMPLE_RATE as u64;
        phase = phase.wrapping_add(phase_step as u32);

        // Square wave with linear decay envelope
        let raw = if (phase & 0x8000_0000) != 0 { 9000i32 } else { -9000i32 };
        let amp = (num_samples - i) as i32;
        let sample = (raw * amp / num_samples as i32) as i16;
        samples.push(sample);
    }
    samples
}

/// Monster hit impact: low-frequency thump combined with filtered noise burst over 60ms.
fn generate_hit_samples() -> Vec<i16> {
    let num_samples = (SAMPLE_RATE * 60) / 1000; // 1323 samples
    let mut samples = Vec::with_capacity(num_samples as usize);
    let mut lcg: u32 = 0x5A17_C0DE;
    let mut phase: u32 = 0;
    let mut filtered_noise: i32 = 0;

    for i in 0..num_samples {
        // Bass thump: 120Hz down to 40Hz
        let freq = 120 - (80 * i) / num_samples;
        let phase_step = ((freq as u64) << 32) / SAMPLE_RATE as u64;
        phase = phase.wrapping_add(phase_step as u32);

        // Triangle wave thump
        let p = (phase >> 16) as i32;
        let tri = if p < 32768 { p - 16384 } else { 49152 - p };
        let thump = (tri * 8000) >> 14;

        // Noise generator (LCG)
        lcg = lcg.wrapping_mul(1664525).wrapping_add(1013904223);
        let raw_noise = ((lcg >> 16) as i16 as i32 * 6000) >> 15;
        filtered_noise = (filtered_noise * 3 + raw_noise) / 4;

        let mixed = thump + filtered_noise;
        let amp = (num_samples - i) as i32;
        let sample = ((mixed * amp) / num_samples as i32).clamp(-32767, 32767) as i16;
        samples.push(sample);
    }
    samples
}

/// Structure placement blip: clean two-tone chime 500Hz -> 1000Hz over 50ms.
fn generate_build_samples() -> Vec<i16> {
    let num_samples = (SAMPLE_RATE * 50) / 1000; // 1102 samples
    let mut samples = Vec::with_capacity(num_samples as usize);
    let mut phase: u32 = 0;
    let half = num_samples / 2;

    for i in 0..num_samples {
        let (freq, seg_i, seg_len) = if i < half {
            (500, i, half)
        } else {
            (1000, i - half, num_samples - half)
        };
        let phase_step = ((freq as u64) << 32) / SAMPLE_RATE as u64;
        phase = phase.wrapping_add(phase_step as u32);

        let p = (phase >> 16) as i32;
        let tri = if p < 32768 { p - 16384 } else { 49152 - p };
        let raw = (tri * 10000) >> 14;

        let amp = (seg_len - seg_i) as i32;
        let sample = (raw * amp / seg_len as i32) as i16;
        samples.push(sample);
    }
    samples
}

/// Structure demolition crunch: falling noise crunch over 80ms.
fn generate_demolish_samples() -> Vec<i16> {
    let num_samples = (SAMPLE_RATE * 80) / 1000; // 1764 samples
    let mut samples = Vec::with_capacity(num_samples as usize);
    let mut lcg: u32 = 0xDEAD_BEEF;
    let mut phase: u32 = 0;

    for i in 0..num_samples {
        // Low rumble sweep: 150Hz down to 30Hz
        let freq = 150 - (120 * i) / num_samples;
        let phase_step = ((freq as u64) << 32) / SAMPLE_RATE as u64;
        phase = phase.wrapping_add(phase_step as u32);

        let sq = if (phase & 0x8000_0000) != 0 { 4000 } else { -4000 };

        lcg = lcg.wrapping_mul(1664525).wrapping_add(1013904223);
        let noise = ((lcg >> 16) as i16 as i32 * 8000) >> 15;

        let mixed = sq + noise;
        let amp = (num_samples - i) as i32;
        let sample = ((mixed * amp) / num_samples as i32).clamp(-32767, 32767) as i16;
        samples.push(sample);
    }
    samples
}

/// Quicksave chord: ascending arpeggio C5 (523Hz) -> E5 (659Hz) -> G5 (784Hz) over 200ms.
fn generate_save_chime_samples() -> Vec<i16> {
    let num_samples = (SAMPLE_RATE * 200) / 1000; // 4410 samples
    let mut samples = Vec::with_capacity(num_samples as usize);
    let mut phase_c: u32 = 0;
    let mut phase_e: u32 = 0;
    let mut phase_g: u32 = 0;

    let step_c = ((523u64) << 32) / SAMPLE_RATE as u64;
    let step_e = ((659u64) << 32) / SAMPLE_RATE as u64;
    let step_g = ((784u64) << 32) / SAMPLE_RATE as u64;

    for i in 0..num_samples {
        phase_c = phase_c.wrapping_add(step_c as u32);
        phase_e = phase_e.wrapping_add(step_e as u32);
        phase_g = phase_g.wrapping_add(step_g as u32);

        let tri = |ph: u32| -> i32 {
            let p = (ph >> 16) as i32;
            if p < 32768 { p - 16384 } else { 49152 - p }
        };

        let raw = if i < 1470 {
            (tri(phase_c) * 11000) >> 14
        } else if i < 2940 {
            (tri(phase_e) * 11000) >> 14
        } else {
            let c = (tri(phase_c) * 3500) >> 14;
            let e = (tri(phase_e) * 3500) >> 14;
            let g = (tri(phase_g) * 6500) >> 14;
            c + e + g
        };

        let amp = (num_samples - i) as i32;
        let sample = ((raw * amp) / num_samples as i32).clamp(-32767, 32767) as i16;
        samples.push(sample);
    }
    samples
}

/// Wave spawn warning siren: alternating 440Hz / 880Hz siren over 300ms.
fn generate_wave_alarm_samples() -> Vec<i16> {
    let num_samples = (SAMPLE_RATE * 300) / 1000; // 6615 samples
    let mut samples = Vec::with_capacity(num_samples as usize);
    let mut phase: u32 = 0;
    let period = 1102; // ~50ms per tone toggle

    for i in 0..num_samples {
        let is_high = ((i / period) % 2) == 1;
        let freq = if is_high { 880 } else { 440 };
        let phase_step = ((freq as u64) << 32) / SAMPLE_RATE as u64;
        phase = phase.wrapping_add(phase_step as u32);

        let p = (phase >> 16) as i32;
        let tri = if p < 32768 { p - 16384 } else { 49152 - p };
        let raw = (tri * 12000) >> 14;

        let fade_start = num_samples - 441;
        let amp = if i > fade_start {
            ((num_samples - i) * 1000 / 441) as i32
        } else {
            1000
        };
        let sample = ((raw * amp) / 1000).clamp(-32767, 32767) as i16;
        samples.push(sample);
    }
    samples
}

/// High-pressure steam release: mechanical latch snap + steam hiss burst over 250ms.
fn generate_steam_release_samples() -> Vec<i16> {
    let num_samples = (SAMPLE_RATE * 250) / 1000; // 5512 samples
    let mut samples = Vec::with_capacity(num_samples as usize);
    let click_samples = (SAMPLE_RATE * 12) / 1000; // 12ms click
    let mut click_phase: u32 = 0;
    let mut lcg: u32 = 0x5734_1100;
    let mut lowpass: i32 = 0;
    let mut highpass: i32 = 0;

    for i in 0..num_samples {
        let mut sample = 0i32;

        // 1. Initial mechanical valve snap
        if i < click_samples {
            let click_freq = 2400 - (1200 * i) / click_samples;
            let click_step = ((click_freq as u64) << 32) / SAMPLE_RATE as u64;
            click_phase = click_phase.wrapping_add(click_step as u32);
            let sq = if (click_phase & 0x8000_0000) != 0 { 16000 } else { -16000 };
            let click_env = (click_samples - i) as i32 * 1000 / click_samples as i32;
            sample += (sq * click_env) / 1000;
        }

        // 2. High-pressure steam burst hiss
        lcg = lcg.wrapping_mul(1664525).wrapping_add(1013904223);
        let raw_noise = ((lcg >> 16) as i16) as i32;
        lowpass = (lowpass * 3 + raw_noise) / 4;
        highpass = (highpass * 7 + raw_noise) / 8;
        let band = highpass - lowpass;

        let rem = (num_samples - i) as i64;
        let steam_env = ((rem * rem * 24000) / (num_samples as i64 * num_samples as i64)) as i32;
        sample += ((band as i64 * steam_env as i64) / 32768) as i32;

        samples.push(sample.clamp(-32767, 32767) as i16);
    }
    samples
}

/// Heavy counter impact: sub-bass thump + resonant metallic chime over 350ms.
fn generate_counter_hit_samples() -> Vec<i16> {
    let num_samples = (SAMPLE_RATE * 350) / 1000; // 7717 samples
    let mut samples = Vec::with_capacity(num_samples as usize);
    let mut phase_thump: u32 = 0;
    let mut phase_bell: u32 = 0;

    for i in 0..num_samples {
        // Bass thump: 130Hz -> 45Hz
        let freq_thump = 130 - (85 * i) / num_samples;
        let step_thump = ((freq_thump as u64) << 32) / SAMPLE_RATE as u64;
        phase_thump = phase_thump.wrapping_add(step_thump as u32);
        let p_thump = (phase_thump >> 16) as i32;
        let tri_thump = if p_thump < 32768 { p_thump - 16384 } else { 49152 - p_thump };
        let thump = (tri_thump * 10000) >> 14;

        // Metallic bell harmonic: 440Hz + 880Hz + 1320Hz
        let step_bell = ((440u64) << 32) / SAMPLE_RATE as u64;
        phase_bell = phase_bell.wrapping_add(step_bell as u32);
        let p_bell1 = (phase_bell >> 16) as i32;
        let p_bell2 = ((phase_bell.wrapping_mul(2)) >> 16) as i32;
        let p_bell3 = ((phase_bell.wrapping_mul(3)) >> 16) as i32;
        let tri1 = if p_bell1 < 32768 { p_bell1 - 16384 } else { 49152 - p_bell1 };
        let tri2 = if p_bell2 < 32768 { p_bell2 - 16384 } else { 49152 - p_bell2 };
        let tri3 = if p_bell3 < 32768 { p_bell3 - 16384 } else { 49152 - p_bell3 };
        let bell = ((tri1 * 6 + tri2 * 4 + tri3 * 2) * 5000) >> 16;

        let rem = (num_samples - i) as i64;
        let env_thump = (rem * 1000 / num_samples as i64) as i32;
        let env_bell = ((rem * rem * 1000) / (num_samples as i64 * num_samples as i64)) as i32;

        let mixed = (thump * env_thump / 1000) + (bell * env_bell / 1000);
        samples.push(mixed.clamp(-32767, 32767) as i16);
    }
    samples
}

/// Packs 16-bit mono PCM samples into a valid 44-byte RIFF/WAVE header and data block.
pub fn generate_wav_bytes(samples: &[i16]) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let chunk_size = 36 + data_len;
    let byte_rate = SAMPLE_RATE * 2;
    let block_align: u16 = 2;
    let bits_per_sample: u16 = 16;
    let num_channels: u16 = 1;
    let audio_format: u16 = 1; // PCM

    let mut buf = Vec::with_capacity(44 + data_len as usize);

    // RIFF Chunk
    buf.extend_from_slice(b"RIFF");
    buf.extend_from_slice(&chunk_size.to_le_bytes());
    buf.extend_from_slice(b"WAVE");

    // "fmt " Subchunk
    buf.extend_from_slice(b"fmt ");
    buf.extend_from_slice(&16u32.to_le_bytes()); // Subchunk1Size (16 for PCM)
    buf.extend_from_slice(&audio_format.to_le_bytes());
    buf.extend_from_slice(&num_channels.to_le_bytes());
    buf.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    buf.extend_from_slice(&byte_rate.to_le_bytes());
    buf.extend_from_slice(&block_align.to_le_bytes());
    buf.extend_from_slice(&bits_per_sample.to_le_bytes());

    // "data" Subchunk
    buf.extend_from_slice(b"data");
    buf.extend_from_slice(&data_len.to_le_bytes());

    for &s in samples {
        buf.extend_from_slice(&s.to_le_bytes());
    }

    buf
}

/// Computes (vol_left_permill, vol_right_permill) using pure integer arithmetic.
/// - `x`: lateral offset from listener (-1000..1000, negative = left, positive = right)
/// - `z`: depth distance from listener (permille of reference unit, 1000 = standard distance)
pub fn compute_spatial_stereo(x: i32, z: i32, base_vol_permill: u16) -> (u16, u16) {
    let z_safe = z.max(200) as u32;
    let atten = ((base_vol_permill as u32) * 1000) / z_safe;
    let atten_clamped = atten.min(1000);

    let pan_right = ((x.clamp(-1000, 1000) + 1000) as u32 * 1000) / 2000;
    let pan_left = 1000 - pan_right;

    let left = ((atten_clamped * pan_left) / 1000) as u16;
    let right = ((atten_clamped * pan_right) / 1000) as u16;
    (left, right)
}

/// Applies a 0-Float linear interpolation pitch shift to PCM samples.
/// - `pitch_permill`: 1000 = original pitch, >1000 = higher pitch/snappier, <1000 = lower pitch/heavier.
pub fn apply_pitch_shift(samples: &[i16], pitch_permill: u16) -> Vec<i16> {
    if pitch_permill == 1000 || samples.is_empty() {
        return samples.to_vec();
    }
    let p = pitch_permill.max(200) as u64;
    let orig_len = samples.len();
    let new_len = ((orig_len as u64 * 1000) / p) as usize;
    let mut output = Vec::with_capacity(new_len);

    for i in 0..new_len {
        let src_fixed = (i as u64 * p) / 1000;
        let src_idx = src_fixed as usize;
        if src_idx + 1 < orig_len {
            let frac = (src_fixed % 1000) as i32;
            let s0 = samples[src_idx] as i32;
            let s1 = samples[src_idx + 1] as i32;
            let interp = s0 + ((s1 - s0) * frac) / 1000;
            output.push(interp.clamp(-32768, 32767) as i16);
        } else if src_idx < orig_len {
            output.push(samples[src_idx]);
        } else {
            break;
        }
    }
    output
}

/// Applies a 0-Float 3-tap metallic cavern reverb to PCM samples.
/// - `decay_permill`: 0 = dry signal, 1000 = maximum cavernous decay.
pub fn apply_arena_reverb(samples: &[i16], decay_permill: u16) -> Vec<i16> {
    if decay_permill == 0 || samples.is_empty() {
        return samples.to_vec();
    }
    let delays = [727usize, 1036, 1367];
    let gains = [420u32, 340, 260];
    let extra_tail = 4410; // ~200ms tail
    let total_len = samples.len() + extra_tail;
    let mut output = vec![0i32; total_len];

    // Copy dry signal
    for (i, &s) in samples.iter().enumerate() {
        output[i] = s as i32;
    }

    // Mix early reflections and cavern comb reverb
    for (tap, &d) in delays.iter().enumerate() {
        let g = (gains[tap] * (decay_permill.min(1000) as u32)) / 1000;
        for i in d..total_len {
            let src_idx = i - d;
            let feed = (output[src_idx] * g as i32) / 1000;
            output[i] += feed;
        }
    }

    output.into_iter().map(|s| s.clamp(-32768, 32767) as i16).collect()
}

/// Packs 16-bit mono PCM samples into a spatialized 2-channel 16-bit Stereo RIFF/WAVE buffer.
pub fn generate_stereo_wav_bytes(
    samples: &[i16],
    pan_left_permill: u16,
    pan_right_permill: u16,
) -> Vec<u8> {
    let num_samples = samples.len();
    let data_len = (num_samples * 4) as u32;
    let chunk_size = 36 + data_len;
    let byte_rate = SAMPLE_RATE * 4;
    let block_align: u16 = 4;
    let bits_per_sample: u16 = 16;
    let num_channels: u16 = 2; // Stereo
    let audio_format: u16 = 1; // PCM

    let mut buf = Vec::with_capacity(44 + data_len as usize);

    buf.extend_from_slice(b"RIFF");
    buf.extend_from_slice(&chunk_size.to_le_bytes());
    buf.extend_from_slice(b"WAVE");

    buf.extend_from_slice(b"fmt ");
    buf.extend_from_slice(&16u32.to_le_bytes());
    buf.extend_from_slice(&audio_format.to_le_bytes());
    buf.extend_from_slice(&num_channels.to_le_bytes());
    buf.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    buf.extend_from_slice(&byte_rate.to_le_bytes());
    buf.extend_from_slice(&block_align.to_le_bytes());
    buf.extend_from_slice(&bits_per_sample.to_le_bytes());

    buf.extend_from_slice(b"data");
    buf.extend_from_slice(&data_len.to_le_bytes());

    for &s in samples {
        let left = ((s as i32 * pan_left_permill as i32) / 1000).clamp(-32767, 32767) as i16;
        let right = ((s as i32 * pan_right_permill as i32) / 1000).clamp(-32767, 32767) as i16;
        buf.extend_from_slice(&left.to_le_bytes());
        buf.extend_from_slice(&right.to_le_bytes());
    }

    buf
}

/// Sound bank pre-generating and storing all procedural WAV byte buffers.
struct SoundBank {
    shoot: Vec<u8>,
    hit: Vec<u8>,
    build: Vec<u8>,
    demolish: Vec<u8>,
    save_chime: Vec<u8>,
    wave_alarm: Vec<u8>,
    steam_release: Vec<u8>,
    counter_hit: Vec<u8>,
}

static SOUND_BANK: OnceLock<SoundBank> = OnceLock::new();

fn get_sound_bank() -> &'static SoundBank {
    SOUND_BANK.get_or_init(|| SoundBank {
        shoot: generate_wav_bytes(&generate_pcm_samples(SoundKind::Shoot)),
        hit: generate_wav_bytes(&generate_pcm_samples(SoundKind::Hit)),
        build: generate_wav_bytes(&generate_pcm_samples(SoundKind::Build)),
        demolish: generate_wav_bytes(&generate_pcm_samples(SoundKind::Demolish)),
        save_chime: generate_wav_bytes(&generate_pcm_samples(SoundKind::SaveChime)),
        wave_alarm: generate_wav_bytes(&generate_pcm_samples(SoundKind::WaveAlarm)),
        steam_release: generate_wav_bytes(&generate_pcm_samples(SoundKind::SteamRelease)),
        counter_hit: generate_wav_bytes(&generate_pcm_samples(SoundKind::CounterHit)),
    })
}

/// Retrieves static reference to pre-synthesized WAV bytes for a sound kind.
pub fn get_sound_bytes(kind: SoundKind) -> &'static [u8] {
    let bank = get_sound_bank();
    match kind {
        SoundKind::Shoot => &bank.shoot,
        SoundKind::Hit => &bank.hit,
        SoundKind::Build => &bank.build,
        SoundKind::Demolish => &bank.demolish,
        SoundKind::SaveChime => &bank.save_chime,
        SoundKind::WaveAlarm => &bank.wave_alarm,
        SoundKind::SteamRelease => &bank.steam_release,
        SoundKind::CounterHit => &bank.counter_hit,
    }
}

#[cfg(target_os = "windows")]
#[link(name = "winmm")]
extern "system" {
    fn PlaySoundW(pszSound: *const u16, hmod: *mut core::ffi::c_void, fdwSound: u32) -> i32;
}

#[cfg(target_os = "windows")]
const SND_ASYNC: u32 = 0x0001;
#[cfg(target_os = "windows")]
const SND_NODEFAULT: u32 = 0x0002;
#[cfg(target_os = "windows")]
const SND_MEMORY: u32 = 0x0004;

/// Plays the requested sound asynchronously without blocking engine simulation.
pub fn play_sound(kind: SoundKind) {
    if is_muted() {
        return;
    }

    #[cfg(target_os = "windows")]
    {
        let wav_bytes = get_sound_bytes(kind);
        unsafe {
            PlaySoundW(
                wav_bytes.as_ptr() as *const u16,
                core::ptr::null_mut(),
                SND_MEMORY | SND_ASYNC | SND_NODEFAULT,
            );
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = kind;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wav_header_and_pcm_generation() {
        let kinds = [
            SoundKind::Shoot,
            SoundKind::Hit,
            SoundKind::Build,
            SoundKind::Demolish,
            SoundKind::SaveChime,
            SoundKind::WaveAlarm,
            SoundKind::SteamRelease,
            SoundKind::CounterHit,
        ];

        for &kind in &kinds {
            let samples = generate_pcm_samples(kind);
            assert!(!samples.is_empty(), "Samples for {:?} must not be empty", kind);

            // Check non-silent (at least some non-zero samples)
            let non_zero_count = samples.iter().filter(|&&s| s != 0).count();
            assert!(
                non_zero_count > samples.len() / 2,
                "Sound {:?} should contain non-silent samples (found {} / {})",
                kind,
                non_zero_count,
                samples.len()
            );

            let wav = generate_wav_bytes(&samples);
            assert!(wav.len() >= 44, "WAV length must be at least 44 bytes");
            let data_len = (samples.len() * 2) as u32;
            assert_eq!(wav.len(), 44 + data_len as usize);

            // Check RIFF header
            assert_eq!(&wav[0..4], b"RIFF");
            let chunk_size = u32::from_le_bytes(wav[4..8].try_into().unwrap());
            assert_eq!(chunk_size, 36 + data_len);
            assert_eq!(&wav[8..12], b"WAVE");

            // Check fmt subchunk
            assert_eq!(&wav[12..16], b"fmt ");
            let subchunk1_size = u32::from_le_bytes(wav[16..20].try_into().unwrap());
            assert_eq!(subchunk1_size, 16);
            let audio_format = u16::from_le_bytes(wav[20..22].try_into().unwrap());
            assert_eq!(audio_format, 1, "Must be PCM (1)");
            let num_channels = u16::from_le_bytes(wav[22..24].try_into().unwrap());
            assert_eq!(num_channels, 1, "Must be Mono (1)");
            let sample_rate = u32::from_le_bytes(wav[24..28].try_into().unwrap());
            assert_eq!(sample_rate, 22050);
            let byte_rate = u32::from_le_bytes(wav[28..32].try_into().unwrap());
            assert_eq!(byte_rate, 44100);
            let block_align = u16::from_le_bytes(wav[32..34].try_into().unwrap());
            assert_eq!(block_align, 2);
            let bits_per_sample = u16::from_le_bytes(wav[34..36].try_into().unwrap());
            assert_eq!(bits_per_sample, 16);

            // Check data subchunk
            assert_eq!(&wav[36..40], b"data");
            let subchunk2_size = u32::from_le_bytes(wav[40..44].try_into().unwrap());
            assert_eq!(subchunk2_size, data_len);

            // Test get_sound_bytes returns valid static slice
            let cached_bytes = get_sound_bytes(kind);
            assert_eq!(cached_bytes.len(), wav.len());
            assert_eq!(cached_bytes, &wav[..]);
        }
    }

    #[test]
    fn test_spatial_stereo_attenuation() {
        let (l_mid, r_mid) = compute_spatial_stereo(0, 1000, 1000);
        assert_eq!(l_mid, 500);
        assert_eq!(r_mid, 500);

        let (l_opp, r_opp) = compute_spatial_stereo(400, 1000, 1000);
        assert!(r_opp > l_opp);
        assert_eq!(l_opp + r_opp, 1000);

        let (l_ply, r_ply) = compute_spatial_stereo(-400, 1000, 1000);
        assert!(l_ply > r_ply);
        assert_eq!(l_ply + r_ply, 1000);

        let (l_far, r_far) = compute_spatial_stereo(0, 2000, 1000);
        assert_eq!(l_far, 250);
        assert_eq!(r_far, 250);
    }

    #[test]
    fn test_stereo_wav_bytes() {
        let samples = generate_pcm_samples(SoundKind::CounterHit);
        let wav = generate_stereo_wav_bytes(&samples, 400, 600);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(&wav[12..16], b"fmt ");
        let channels = u16::from_le_bytes(wav[22..24].try_into().unwrap());
        assert_eq!(channels, 2);
        let byte_rate = u32::from_le_bytes(wav[28..32].try_into().unwrap());
        assert_eq!(byte_rate, 88200);
        assert_eq!(wav.len(), 44 + samples.len() * 4);
    }

    #[test]
    fn test_pitch_shift_and_reverb() {
        let dry = generate_pcm_samples(SoundKind::CounterHit);
        assert!(!dry.is_empty());

        // Higher pitch -> shorter duration
        let high = apply_pitch_shift(&dry, 1200);
        assert!(high.len() < dry.len(), "Higher pitch must result in fewer samples");

        // Lower pitch -> longer duration
        let low = apply_pitch_shift(&dry, 800);
        assert!(low.len() > dry.len(), "Lower pitch must result in more samples");

        // Reverb -> extended tail
        let rev = apply_arena_reverb(&dry, 500);
        assert!(rev.len() > dry.len(), "Reverb must extend tail duration");
        // Verify tail contains decaying reverberations
        let tail_energy: i64 = rev[dry.len()..].iter().map(|&s| s.abs() as i64).sum();
        assert!(tail_energy > 0, "Reverb tail must contain non-zero acoustic reflections");
    }

    #[test]
    fn test_mute_toggle() {
        set_muted(false);
        assert!(!is_muted());
        let muted = toggle_mute();
        assert!(muted);
        assert!(is_muted());
        let unmuted = toggle_mute();
        assert!(!unmuted);
        assert!(!is_muted());
    }
}
