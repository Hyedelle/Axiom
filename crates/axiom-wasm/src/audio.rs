//! Zero-Float Procedural Audio Synthesis, 3D Spatial Panning & Web Audio AudioWorklet Engine (`axiom-wasm`)
//!
//! Generates 22,050 Hz 16-bit Mono & Stereo PCM waveforms using pure integer arithmetic (0 float).
//! Implements multi-layered hit sound design, pitch shifting, 3-tap cavernous comb reverb,
//! 3D spatial distance attenuation & stereo panning, and real-time AudioWorklet 128-frame block streaming.

#![allow(static_mut_refs)]

extern crate alloc;

use alloc::vec;
use alloc::vec::Vec;
use axiom_render::math3d::SINE_LUT;

/// Audio sample rate in Hz.
pub const SAMPLE_RATE: u32 = 22050;

/// Sound effect kinds for mecha boxing actions, layered impacts, and UI events.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum SoundKind {
    None = 0,
    /// Light / snappy jab: 120->60Hz knock + crisp attack noise (90ms).
    HitJab = 1,
    /// Mid impact cross: 110->55Hz knock + frame metal resonance (250ms).
    HitCross = 2,
    /// Heavy hook: 100->50Hz knock + frame resonance + reverb tail (450ms).
    HitHook = 3,
    /// Critical counter-punch: sub-bass knock + metal resonance + bell ring (600ms).
    HitCounter = 4,
    /// Knockout finisher: deep 95->36Hz sub-bass boom + metal crumbling rumble (1200ms).
    HitKo = 5,
    /// Metallic armor block / deflection clank (120ms).
    GuardHit = 6,
    /// Swift evasion air whoosh sweep (150ms).
    SlipWhoosh = 7,
    /// High-pressure steam release: mechanical valve latch snap + escaping steam burst (320ms).
    SteamRelease = 8,
    /// Brass round bell chime (400ms).
    BellRing = 9,
    /// Ascending major chord victory fanfare (600ms).
    KoFanfare = 10,
    /// Turret firing shot (downward sweep 880Hz -> 220Hz, 100ms).
    Shoot = 11,
    /// Structure placement blip (two-tone chime 500Hz -> 1000Hz, 50ms).
    Build = 12,
    /// Structure demolition crunch (falling noise + rumble, 80ms).
    Demolish = 13,
}

impl SoundKind {
    pub fn from_u32(val: u32) -> Self {
        match val {
            1 => SoundKind::HitJab,
            2 => SoundKind::HitCross,
            3 => SoundKind::HitHook,
            4 => SoundKind::HitCounter,
            5 => SoundKind::HitKo,
            6 => SoundKind::GuardHit,
            7 => SoundKind::SlipWhoosh,
            8 => SoundKind::SteamRelease,
            9 => SoundKind::BellRing,
            10 => SoundKind::KoFanfare,
            11 => SoundKind::Shoot,
            12 => SoundKind::Build,
            13 => SoundKind::Demolish,
            _ => SoundKind::None,
        }
    }
}

// ---------------------------------------------------------------------------
// 0-Float Multi-Layer Sound Synthesis
// ---------------------------------------------------------------------------

/// Layer 1: Sub-bass knock (lay_knock) - downward sweep + 2nd harmonic.
fn mix_knock(
    buffer: &mut [i32],
    start_sample: usize,
    duration_samples: usize,
    start_freq: u32,
    end_freq: u32,
    peak_amp: i32,
    mut lcg: u32,
) {
    let total = duration_samples;
    let mut phase: u32 = 0;
    for i in 0..total {
        let idx = start_sample + i;
        if idx >= buffer.len() {
            break;
        }
        let freq = start_freq - ((start_freq - end_freq) * (i as u32)) / (total as u32);
        let phase_step = ((freq as u64) << 32) / SAMPLE_RATE as u64;
        phase = phase.wrapping_add(phase_step as u32);

        let bam1 = (phase >> 24) as u8;
        let sin1 = SINE_LUT[bam1 as usize] as i32;
        let bam2 = (phase.wrapping_mul(2) >> 24) as u8;
        let sin2 = SINE_LUT[bam2 as usize] as i32;
        let wave = (sin1 * 3 + sin2) / 4;

        let env = ((total - i) as i64 * peak_amp as i64 / total as i64) as i32;
        let mut sample = ((wave as i64 * env as i64) / 65536) as i32;

        // Snappy initial transient attack noise (first 10ms = 220 samples)
        if i < 220 {
            lcg = lcg.wrapping_mul(1664525).wrapping_add(1013904223);
            let noise = ((lcg >> 16) as i16 as i32 * (220 - i as i32)) / 220;
            sample += (noise * peak_amp) / 65536;
        }

        buffer[idx] += sample;
    }
}

/// Layer 2: Hollow frame metal resonance (lay_body) - 3 non-harmonic sines.
fn mix_body(
    buffer: &mut [i32],
    start_sample: usize,
    duration_samples: usize,
    peak_amp: i32,
) {
    let total = duration_samples;
    let mut p1: u32 = 0;
    let mut p2: u32 = 0;
    let mut p3: u32 = 0;
    let step1 = ((118u64) << 32) / SAMPLE_RATE as u64;
    let step2 = ((154u64) << 32) / SAMPLE_RATE as u64;
    let step3 = ((189u64) << 32) / SAMPLE_RATE as u64;

    for i in 0..total {
        let idx = start_sample + i;
        if idx >= buffer.len() {
            break;
        }
        p1 = p1.wrapping_add(step1 as u32);
        p2 = p2.wrapping_add(step2 as u32);
        p3 = p3.wrapping_add(step3 as u32);

        let s1 = SINE_LUT[(p1 >> 24) as u8 as usize] as i32;
        let s2 = SINE_LUT[(p2 >> 24) as u8 as usize] as i32;
        let s3 = SINE_LUT[(p3 >> 24) as u8 as usize] as i32;
        let combined = (s1 * 4 + s2 * 3 + s3 * 2) / 9;

        let rem = (total - i) as i64;
        let env = ((rem * rem * peak_amp as i64) / (total as i64 * total as i64)) as i32;
        let sample = ((combined as i64 * env as i64) / 65536) as i32;
        buffer[idx] += sample;
    }
}

/// Layer 3: Resonant reverb tail (lay_tail) - filtered noise with reflections.
fn mix_tail(
    buffer: &mut [i32],
    start_sample: usize,
    duration_samples: usize,
    peak_amp: i32,
    mut lcg: u32,
) {
    let total = duration_samples;
    let mut lowpass: i32 = 0;

    for i in 0..total {
        let idx = start_sample + i;
        if idx >= buffer.len() {
            break;
        }
        lcg = lcg.wrapping_mul(1664525).wrapping_add(1013904223);
        let raw_noise = ((lcg >> 16) as i16) as i32;
        lowpass = (lowpass * 7 + raw_noise) / 8;

        let env = ((total - i) as i64 * peak_amp as i64 / total as i64) as i32;
        let sample = ((lowpass as i64 * env as i64) / 32768) as i32;
        buffer[idx] += sample;
    }
}

/// Layer 4: Critical metallic chime (lay_ring) - bell partials.
fn mix_ring(
    buffer: &mut [i32],
    start_sample: usize,
    duration_samples: usize,
    peak_amp: i32,
) {
    let total = duration_samples;
    let mut p1: u32 = 0;
    let mut p2: u32 = 0;
    let mut p3: u32 = 0;
    let step1 = ((460u64) << 32) / SAMPLE_RATE as u64;
    let step2 = ((1270u64) << 32) / SAMPLE_RATE as u64;
    let step3 = ((2480u64) << 32) / SAMPLE_RATE as u64;

    for i in 0..total {
        let idx = start_sample + i;
        if idx >= buffer.len() {
            break;
        }
        p1 = p1.wrapping_add(step1 as u32);
        p2 = p2.wrapping_add(step2 as u32);
        p3 = p3.wrapping_add(step3 as u32);

        let s1 = SINE_LUT[(p1 >> 24) as u8 as usize] as i32;
        let s2 = SINE_LUT[(p2 >> 24) as u8 as usize] as i32;
        let s3 = SINE_LUT[(p3 >> 24) as u8 as usize] as i32;
        let combined = (s1 * 5 + s2 * 3 + s3 * 2) / 10;

        let rem = (total - i) as i64;
        let env = ((rem * rem * peak_amp as i64) / (total as i64 * total as i64)) as i32;
        let sample = ((combined as i64 * env as i64) / 65536) as i32;
        buffer[idx] += sample;
    }
}

/// Layer 5: Knockout sub-bass boom (lay_boom).
fn mix_boom(
    buffer: &mut [i32],
    start_sample: usize,
    duration_samples: usize,
    peak_amp: i32,
) {
    let total = duration_samples;
    let mut phase: u32 = 0;

    for i in 0..total {
        let idx = start_sample + i;
        if idx >= buffer.len() {
            break;
        }
        let freq = 95 - ((95 - 36) * (i as u32)) / (total as u32);
        let phase_step = ((freq as u64) << 32) / SAMPLE_RATE as u64;
        phase = phase.wrapping_add(phase_step as u32);

        let bam = (phase >> 24) as u8;
        let sin = SINE_LUT[bam as usize] as i32;
        let saturated = sin.clamp(-50000, 50000);

        let rem = (total - i) as i64;
        let env = ((rem * peak_amp as i64) / total as i64) as i32;
        let sample = ((saturated as i64 * env as i64) / 50000) as i32;
        buffer[idx] += sample;
    }
}

/// Layer 6: Mechanical valve snap & high-pressure steam burst (lay_valve_steam).
fn mix_valve_steam(
    buffer: &mut [i32],
    start_sample: usize,
    duration_samples: usize,
    peak_amp: i32,
    mut lcg: u32,
) {
    let total = duration_samples;
    let click_samples = (SAMPLE_RATE * 15 / 1000) as usize; // 15ms click
    let mut click_phase: u32 = 0;
    let mut lowpass: i32 = 0;
    let mut highpass: i32 = 0;

    for i in 0..total {
        let idx = start_sample + i;
        if idx >= buffer.len() {
            break;
        }

        let mut sample = 0i32;

        if i < click_samples {
            let click_freq = 2400 - (1200 * i as u32) / (click_samples as u32);
            let click_step = ((click_freq as u64) << 32) / SAMPLE_RATE as u64;
            click_phase = click_phase.wrapping_add(click_step as u32);
            let sq = if (click_phase & 0x8000_0000) != 0 { 20000 } else { -20000 };
            let click_env = (click_samples - i) as i32 * peak_amp / click_samples as i32;
            sample += (sq * click_env) / 32768;
        }

        lcg = lcg.wrapping_mul(1664525).wrapping_add(1013904223);
        let raw_noise = ((lcg >> 16) as i16) as i32;
        lowpass = (lowpass * 3 + raw_noise) / 4;
        highpass = (highpass * 7 + raw_noise) / 8;
        let steam_band = highpass - lowpass;

        let rem = (total - i) as i64;
        let steam_env = ((rem * rem * peak_amp as i64) / (total as i64 * total as i64)) as i32;
        sample += ((steam_band as i64 * steam_env as i64) / 32768) as i32;

        buffer[idx] += sample;
    }
}

// ---------------------------------------------------------------------------
// Procedural Waveform Generators
// ---------------------------------------------------------------------------

/// Generates pure integer 16-bit mono PCM samples for a given sound effect.
pub fn generate_pcm_samples(kind: SoundKind) -> Vec<i16> {
    match kind {
        SoundKind::HitJab => {
            let total = 1984usize; // ~90ms
            let mut accum = vec![0i32; total];
            mix_knock(&mut accum, 0, total, 120, 60, 24000, 0x12345679);
            accum.into_iter().map(|s| s.clamp(-32768, 32767) as i16).collect()
        }
        SoundKind::HitCross => {
            let total = 5512usize; // ~250ms
            let mut accum = vec![0i32; total];
            mix_knock(&mut accum, 0, 3969, 110, 55, 26000, 0x5A17C0DE);
            mix_body(&mut accum, 264, 5071, 19000);
            accum.into_iter().map(|s| s.clamp(-32768, 32767) as i16).collect()
        }
        SoundKind::HitHook => {
            let total = 9922usize; // ~450ms
            let mut accum = vec![0i32; total];
            mix_knock(&mut accum, 0, 5512, 100, 50, 28000, 0xDEADBEEF);
            mix_body(&mut accum, 264, 8820, 22000);
            mix_tail(&mut accum, 882, 9040, 14000, 0x98765432);
            accum.into_iter().map(|s| s.clamp(-32768, 32767) as i16).collect()
        }
        SoundKind::HitCounter => {
            let total = 13230usize; // ~600ms
            let mut accum = vec![0i32; total];
            mix_knock(&mut accum, 0, 6615, 120, 48, 29000, 0xCAFEBABE);
            mix_body(&mut accum, 220, 11025, 23000);
            mix_ring(&mut accum, 110, 12000, 22000);
            mix_tail(&mut accum, 661, 12569, 15000, 0x1337BEEF);
            accum.into_iter().map(|s| s.clamp(-32768, 32767) as i16).collect()
        }
        SoundKind::HitKo => {
            let total = 26460usize; // ~1200ms
            let mut accum = vec![0i32; total];
            mix_boom(&mut accum, 0, 22050, 31000);
            mix_knock(&mut accum, 0, 6615, 95, 40, 29000, 0xBAADF00D);
            mix_body(&mut accum, 330, 17640, 25000);
            mix_tail(&mut accum, 882, 25578, 18000, 0xFEEDFACE);
            accum.into_iter().map(|s| s.clamp(-32768, 32767) as i16).collect()
        }
        SoundKind::GuardHit => {
            let total = 2646usize; // ~120ms
            let mut accum = vec![0i32; total];
            let mut p1: u32 = 0;
            let mut p2: u32 = 0;
            let step1 = ((640u64) << 32) / SAMPLE_RATE as u64;
            let step2 = ((1480u64) << 32) / SAMPLE_RATE as u64;
            let mut lcg: u32 = 0x87654321;
            for i in 0..total {
                p1 = p1.wrapping_add(step1 as u32);
                p2 = p2.wrapping_add(step2 as u32);
                let s1 = SINE_LUT[(p1 >> 24) as u8 as usize] as i32;
                let s2 = SINE_LUT[(p2 >> 24) as u8 as usize] as i32;
                let tone = (s1 + s2) / 2;
                lcg = lcg.wrapping_mul(1664525).wrapping_add(1013904223);
                let noise = ((lcg >> 16) as i16 as i32 * 20000) >> 15;
                let mixed = (tone * 3 + noise) / 4;
                let env = (total - i) as i64 * 24000 / total as i64;
                accum[i] = ((mixed as i64 * env) / 65536) as i32;
            }
            accum.into_iter().map(|s| s.clamp(-32768, 32767) as i16).collect()
        }
        SoundKind::SlipWhoosh => {
            let total = 3307usize; // ~150ms
            let mut accum = vec![0i32; total];
            let mut lcg: u32 = 0x33445566;
            let mut band = 0i32;
            for i in 0..total {
                lcg = lcg.wrapping_mul(1664525).wrapping_add(1013904223);
                let noise = (lcg >> 16) as i16 as i32;
                band = (band * 5 + noise * 3) / 8;
                let env = if i < total / 2 {
                    (i as i64 * 22000) / (total as i64 / 2)
                } else {
                    ((total - i) as i64 * 22000) / (total as i64 / 2)
                };
                accum[i] = ((band as i64 * env) / 32768) as i32;
            }
            accum.into_iter().map(|s| s.clamp(-32768, 32767) as i16).collect()
        }
        SoundKind::SteamRelease => {
            let total = 7056usize; // ~320ms
            let mut accum = vec![0i32; total];
            mix_valve_steam(&mut accum, 0, total, 27000, 0x77889900);
            accum.into_iter().map(|s| s.clamp(-32768, 32767) as i16).collect()
        }
        SoundKind::BellRing => {
            let total = 8820usize; // ~400ms
            let mut accum = vec![0i32; total];
            mix_ring(&mut accum, 0, total, 28000);
            accum.into_iter().map(|s| s.clamp(-32768, 32767) as i16).collect()
        }
        SoundKind::KoFanfare => {
            let total = 13230usize; // ~600ms
            let mut accum = vec![0i32; total];
            let notes = [523u64, 659, 784, 1046]; // C5, E5, G5, C6
            let seg = total / 4;
            for (step_idx, &freq) in notes.iter().enumerate() {
                let start = step_idx * seg;
                let mut phase: u32 = 0;
                let phase_step = (freq << 32) / SAMPLE_RATE as u64;
                for i in 0..seg {
                    let idx = start + i;
                    phase = phase.wrapping_add(phase_step as u32);
                    let s = SINE_LUT[(phase >> 24) as u8 as usize] as i32;
                    let env = (seg - i) as i64 * 26000 / seg as i64;
                    accum[idx] = ((s as i64 * env) / 65536) as i32;
                }
            }
            accum.into_iter().map(|s| s.clamp(-32768, 32767) as i16).collect()
        }
        SoundKind::Shoot => {
            let total = 2205usize; // 100ms
            let mut accum = Vec::with_capacity(total);
            let mut phase: u32 = 0;
            for i in 0..total {
                let freq = 880 - (660 * i as u32) / total as u32;
                let phase_step = ((freq as u64) << 32) / SAMPLE_RATE as u64;
                phase = phase.wrapping_add(phase_step as u32);
                let raw = if (phase & 0x8000_0000) != 0 { 9000i32 } else { -9000i32 };
                let amp = (total - i) as i32;
                accum.push(((raw * amp) / total as i32) as i16);
            }
            accum
        }
        SoundKind::Build => {
            let total = 1102usize; // 50ms
            let mut accum = Vec::with_capacity(total);
            let mut phase: u32 = 0;
            let half = total / 2;
            for i in 0..total {
                let (freq, seg_i, seg_len) = if i < half {
                    (500u32, i, half)
                } else {
                    (1000u32, i - half, total - half)
                };
                let phase_step = ((freq as u64) << 32) / SAMPLE_RATE as u64;
                phase = phase.wrapping_add(phase_step as u32);
                let p = (phase >> 16) as i32;
                let tri = if p < 32768 { p - 16384 } else { 49152 - p };
                let raw = (tri * 10000) >> 14;
                let amp = (seg_len - seg_i) as i32;
                accum.push(((raw * amp) / seg_len as i32) as i16);
            }
            accum
        }
        SoundKind::Demolish => {
            let total = 1764usize; // 80ms
            let mut accum = Vec::with_capacity(total);
            let mut lcg: u32 = 0xDEAD_BEEF;
            let mut phase: u32 = 0;
            for i in 0..total {
                let freq = 150 - (120 * i as u32) / total as u32;
                let phase_step = ((freq as u64) << 32) / SAMPLE_RATE as u64;
                phase = phase.wrapping_add(phase_step as u32);
                let sq = if (phase & 0x8000_0000) != 0 { 4000 } else { -4000 };
                lcg = lcg.wrapping_mul(1664525).wrapping_add(1013904223);
                let noise = ((lcg >> 16) as i16 as i32 * 8000) >> 15;
                let mixed = sq + noise;
                let amp = (total - i) as i32;
                accum.push((((mixed * amp) / total as i32).clamp(-32767, 32767)) as i16);
            }
            accum
        }
        SoundKind::None => Vec::new(),
    }
}

// ---------------------------------------------------------------------------
// 0-Float DSP Filters: Pitch Shift, Reverb, Spatial Panning
// ---------------------------------------------------------------------------

/// Applies a 0-Float linear interpolation pitch shift to PCM samples.
/// - `pitch_permill`: 1000 = original pitch, >1000 = higher pitch, <1000 = lower pitch.
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

    for (i, &s) in samples.iter().enumerate() {
        output[i] = s as i32;
    }

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

// ---------------------------------------------------------------------------
// Static Synthesizer & AudioWorklet Multi-Voice Real-Time Mixer
// ---------------------------------------------------------------------------

pub const MAX_STEREO_SAMPLES: usize = 131072; // 65536 frames * 2 channels
static mut STEREO_BUFFER: [i16; MAX_STEREO_SAMPLES] = [0; MAX_STEREO_SAMPLES];
static mut STEREO_FRAME_COUNT: usize = 0;

/// Synthesizes a sound into the static stereo buffer with pitch, reverb, and 3D spatial panning.
/// Returns number of stereo frames written (each frame has 2 samples: Left, Right).
pub fn synthesize_to_static_buffer(
    kind: SoundKind,
    pitch_permill: u16,
    reverb_decay_permill: u16,
    pan_x: i32,
    pan_z: i32,
) -> usize {
    let raw = generate_pcm_samples(kind);
    if raw.is_empty() {
        unsafe {
            STEREO_FRAME_COUNT = 0;
        }
        return 0;
    }

    let pitched = apply_pitch_shift(&raw, pitch_permill);
    let reverbed = apply_arena_reverb(&pitched, reverb_decay_permill);
    let (vol_l, vol_r) = compute_spatial_stereo(pan_x, pan_z, 1000);

    let frames = reverbed.len().min(MAX_STEREO_SAMPLES / 2);
    unsafe {
        for i in 0..frames {
            let s = reverbed[i] as i32;
            let l = ((s * vol_l as i32) / 1000).clamp(-32768, 32767) as i16;
            let r = ((s * vol_r as i32) / 1000).clamp(-32768, 32767) as i16;
            STEREO_BUFFER[i * 2] = l;
            STEREO_BUFFER[i * 2 + 1] = r;
        }
        STEREO_FRAME_COUNT = frames;
    }
    frames
}

pub fn get_stereo_buffer_ptr() -> *const i16 {
    unsafe { STEREO_BUFFER.as_ptr() }
}

pub fn get_stereo_frame_count() -> usize {
    unsafe { STEREO_FRAME_COUNT }
}

// ---------------------------------------------------------------------------
// Real-time AudioWorklet 8-Voice Mixer & Streaming
// ---------------------------------------------------------------------------

pub const MAX_WORKLET_VOICES: usize = 8;
pub const WORKLET_BLOCK_FRAMES: usize = 128; // Web Audio standard block size

#[derive(Clone)]
pub struct WorkletVoice {
    pub samples: Vec<i16>,
    pub cursor: usize,
    pub vol_left: u16,
    pub vol_right: u16,
    pub active: bool,
}

impl Default for WorkletVoice {
    fn default() -> Self {
        Self {
            samples: Vec::new(),
            cursor: 0,
            vol_left: 1000,
            vol_right: 1000,
            active: false,
        }
    }
}

pub struct WorkletMixer {
    pub voices: [WorkletVoice; MAX_WORKLET_VOICES],
    // 3-Tap Comb Filters for real-time cavernous streaming reverb
    pub comb0: [i32; 727],
    pub comb1: [i32; 1036],
    pub comb2: [i32; 1367],
    pub c0_idx: usize,
    pub c1_idx: usize,
    pub c2_idx: usize,
    pub reverb_decay_permill: u16,
}

impl Default for WorkletMixer {
    fn default() -> Self {
        Self::new()
    }
}

impl WorkletMixer {
    pub fn new() -> Self {
        Self {
            voices: [
                WorkletVoice::default(),
                WorkletVoice::default(),
                WorkletVoice::default(),
                WorkletVoice::default(),
                WorkletVoice::default(),
                WorkletVoice::default(),
                WorkletVoice::default(),
                WorkletVoice::default(),
            ],
            comb0: [0; 727],
            comb1: [0; 1036],
            comb2: [0; 1367],
            c0_idx: 0,
            c1_idx: 0,
            c2_idx: 0,
            reverb_decay_permill: 0,
        }
    }

    pub fn play_sound(
        &mut self,
        kind: SoundKind,
        pitch_permill: u16,
        reverb_decay: u16,
        pan_x: i32,
        pan_z: i32,
    ) -> bool {
        let raw = generate_pcm_samples(kind);
        if raw.is_empty() {
            return false;
        }
        let samples = apply_pitch_shift(&raw, pitch_permill);
        let (vol_l, vol_r) = compute_spatial_stereo(pan_x, pan_z, 1000);

        // Find inactive voice or steal oldest / furthest progressed
        let mut target_idx = None;
        for i in 0..MAX_WORKLET_VOICES {
            if !self.voices[i].active {
                target_idx = Some(i);
                break;
            }
        }
        let idx = target_idx.unwrap_or(0);

        self.voices[idx] = WorkletVoice {
            samples,
            cursor: 0,
            vol_left: vol_l,
            vol_right: vol_r,
            active: true,
        };

        if reverb_decay > self.reverb_decay_permill {
            self.reverb_decay_permill = reverb_decay;
        }

        true
    }

    /// Renders `frames` (e.g. 128) into interleaved `[L0, R0, L1, R1, ...]`.
    pub fn render_block(&mut self, out_interleaved: &mut [i16], frames: usize) {
        let g0 = (420u32 * self.reverb_decay_permill.min(1000) as u32) / 1000;
        let g1 = (340u32 * self.reverb_decay_permill.min(1000) as u32) / 1000;
        let g2 = (260u32 * self.reverb_decay_permill.min(1000) as u32) / 1000;

        for f in 0..frames {
            let mut sum_l: i32 = 0;
            let mut sum_r: i32 = 0;

            for v in self.voices.iter_mut() {
                if !v.active {
                    continue;
                }
                if v.cursor < v.samples.len() {
                    let s = v.samples[v.cursor] as i32;
                    sum_l += (s * v.vol_left as i32) / 1000;
                    sum_r += (s * v.vol_right as i32) / 1000;
                    v.cursor += 1;
                } else {
                    v.active = false;
                }
            }

            // Real-time comb filter feedback for arena spatial reverb
            if self.reverb_decay_permill > 0 {
                let dry_mono = (sum_l + sum_r) / 2;

                let fb0 = self.comb0[self.c0_idx];
                self.comb0[self.c0_idx] = dry_mono + ((fb0 * g0 as i32) / 1000);
                self.c0_idx = (self.c0_idx + 1) % 727;

                let fb1 = self.comb1[self.c1_idx];
                self.comb1[self.c1_idx] = dry_mono + ((fb1 * g1 as i32) / 1000);
                self.c1_idx = (self.c1_idx + 1) % 1036;

                let fb2 = self.comb2[self.c2_idx];
                self.comb2[self.c2_idx] = dry_mono + ((fb2 * g2 as i32) / 1000);
                self.c2_idx = (self.c2_idx + 1) % 1367;

                let reverb_sum = (fb0 + fb1 + fb2) / 3;
                sum_l += reverb_sum;
                sum_r += reverb_sum;
            }

            let idx = f * 2;
            if idx + 1 < out_interleaved.len() {
                out_interleaved[idx] = sum_l.clamp(-32768, 32767) as i16;
                out_interleaved[idx + 1] = sum_r.clamp(-32768, 32767) as i16;
            }
        }

        // Slowly decay reverb parameter if no active voices
        let any_active = self.voices.iter().any(|v| v.active);
        if !any_active && self.reverb_decay_permill > 0 {
            self.reverb_decay_permill = self.reverb_decay_permill.saturating_sub(10);
        }
    }

    pub fn active_voice_count(&self) -> usize {
        self.voices.iter().filter(|v| v.active).count()
    }
}

static mut WORKLET_MIXER: Option<WorkletMixer> = None;
static mut BLOCK_BUFFER: [i16; WORKLET_BLOCK_FRAMES * 2] = [0; WORKLET_BLOCK_FRAMES * 2];

pub fn get_worklet_mixer() -> &'static mut WorkletMixer {
    unsafe {
        if WORKLET_MIXER.is_none() {
            WORKLET_MIXER = Some(WorkletMixer::new());
        }
        WORKLET_MIXER.as_mut().unwrap()
    }
}

pub fn render_worklet_block() -> *const i16 {
    let mixer = get_worklet_mixer();
    unsafe {
        mixer.render_block(&mut BLOCK_BUFFER, WORKLET_BLOCK_FRAMES);
        BLOCK_BUFFER.as_ptr()
    }
}
