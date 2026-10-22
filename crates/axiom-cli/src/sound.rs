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

/// Sound bank pre-generating and storing all procedural WAV byte buffers.
struct SoundBank {
    shoot: Vec<u8>,
    hit: Vec<u8>,
    build: Vec<u8>,
    demolish: Vec<u8>,
    save_chime: Vec<u8>,
    wave_alarm: Vec<u8>,
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
