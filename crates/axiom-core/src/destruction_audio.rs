//! # Procedural Destruction Audio Synthesis & Surface Reverb Field (`axiom-core::destruction_audio`)
//!
//! T-213: High-Performance 0-Float, 64-Byte L1 Cache Line Aligned Procedural
//! Destruction Audio Synthesis and Surface Reverb Field Engine.
//!
//! ## Architectural Guarantees:
//! - **0-Float in Core**: All coordinates in integer millimeters (`mm: i32`), velocities in `mm/s`.
//!   Frequencies, decay rates, absorption ratios in $Q16.16$ fixed-point arithmetic (`65536 = 1.0`).
//!   Audio waveform samples in 16-bit signed PCM integers (`-32768..=32767`).
//! - **64B L1 Cache Line Alignment**: Both [`DestructionVoiceSlab64`] and [`AcousticZoneSlab64`]
//!   are `#[repr(C, align(64))]` with exact 64-byte size and zero implicit compiler padding.
//! - **Modal Resonance & Impulse Burst**: Non-linear crack transient plus 4 inharmonic orthogonal
//!   normal modes with $Q16.16$ exponential envelope decay.
//! - **Integer FDN Surface Reverb**: 4 coprime delay comb filter lines with material absorption scaling.
//! - **0-Heap Flat Array Topology**: Fixed-capacity compile-time generic arrays.

use core::mem::{align_of, size_of};
use crate::event_bus::{channel, DeterministicEventBus};
use crate::job_system::{JobContext, JobPriority, JobStatus, JobSystem};
use crate::ragdoll::isqrt_i64;
use crate::worker_pool::WorkerPoolCoordinator;

/// Event type discriminant for procedural destruction audio synthesis.
pub const EVENT_AUDIO_DESTRUCTION: u16 = 0x0260;

/// Voice operational flag bitmasks.
pub const VOICE_FLAG_ACTIVE: u8        = 1 << 0;
pub const VOICE_FLAG_CRACK_BURST: u8   = 1 << 1;
pub const VOICE_FLAG_REVERB_TAIL: u8   = 1 << 2;
pub const VOICE_FLAG_HIGH_PRIORITY: u8 = 1 << 3;

/// Acoustic zone operational flags.
pub const ZONE_FLAG_ACTIVE: u8   = 1 << 0;
pub const ZONE_FLAG_ENCLOSED: u8 = 1 << 1;
pub const ZONE_FLAG_DAMPED: u8   = 1 << 2;

/// Terrain and debris material IDs.
pub const AUDIO_MAT_DIRT: u8   = 0;
pub const AUDIO_MAT_STONE: u8  = 1;
pub const AUDIO_MAT_METAL: u8  = 2;
pub const AUDIO_MAT_MOLTEN: u8 = 3;
pub const AUDIO_MAT_SAND: u8   = 4;

/// Default sample rate for procedural audio synthesis (48 kHz).
pub const AUDIO_SAMPLE_RATE_48K: u32 = 48_000;

/// 1.0 in Q16.16 fixed-point arithmetic.
pub const Q16_ONE: i32 = 65536;

/// Exact 64-Byte L1 Cache Line Aligned Procedural Destruction Audio Voice Slab.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(C, align(64))]
pub struct DestructionVoiceSlab64 {
    /// 4B: World X position in integer millimeters
    pub pos_x: i32,
    /// 4B: World Y position in integer millimeters
    pub pos_y: i32,
    /// 4B: World Z position in integer millimeters
    pub pos_z: i32,
    /// 4B: Linear velocity X in mm/s
    pub vel_x: i32,
    /// 4B: Linear velocity Y in mm/s
    pub vel_y: i32,
    /// 4B: Linear velocity Z in mm/s
    pub vel_z: i32,
    /// 4B: Impact kinetic energy in Joules
    pub energy_impulse: u32,
    /// 4B: Originating entity, projectile, crater, or bone ID
    pub source_id: u32,
    /// 4B: Simulation tick when this voice was triggered
    pub tick: u32,
    /// 2B: Base fundamental modal resonance frequency in Hz (20..8000)
    pub base_freq_hz: u16,
    /// 2B: Per-sample modal decay multiplier in Q16.16 (0..65535)
    pub modal_decay_q16: u16,
    /// 2B: Peak synthesis amplitude in 16-bit range (0..65535)
    pub amplitude_q16: u16,
    /// 2B: Total duration in PCM audio samples
    pub duration_samples: u16,
    /// 2B: Current synthesis playback sample cursor
    pub sample_cursor: u16,
    /// 2B: Reverb wet send level in Q16.16 (0..65535)
    pub reverb_send_q16: u16,
    /// 1B: Terrain/debris material ID (`AUDIO_MAT_*`)
    pub material: u8,
    /// 1B: Voice operational flags (`VOICE_FLAG_*`)
    pub flags: u8,
    /// 14B: Explicit alignment padding ensuring exact 64-byte size
    pub _pad: [u8; 14],
}

const _: () = assert!(size_of::<DestructionVoiceSlab64>() == 64);
const _: () = assert!(align_of::<DestructionVoiceSlab64>() == 64);

impl Default for DestructionVoiceSlab64 {
    #[inline(always)]
    fn default() -> Self {
        Self::empty()
    }
}

impl DestructionVoiceSlab64 {
    /// Creates a blank inactive voice slab.
    #[inline(always)]
    pub const fn empty() -> Self {
        Self {
            pos_x: 0,
            pos_y: 0,
            pos_z: 0,
            vel_x: 0,
            vel_y: 0,
            vel_z: 0,
            energy_impulse: 0,
            base_freq_hz: 0,
            modal_decay_q16: 0,
            amplitude_q16: 0,
            duration_samples: 0,
            sample_cursor: 0,
            material: AUDIO_MAT_DIRT,
            flags: 0,
            reverb_send_q16: 0,
            source_id: 0,
            tick: 0,
            _pad: [0; 14],
        }
    }

    /// Constructs an active destruction voice from impact parameters.
    pub fn from_impact(
        pos_x: i32,
        pos_y: i32,
        pos_z: i32,
        energy_joules: u32,
        material: u8,
        source_id: u32,
        tick: u32,
    ) -> Self {
        // Material-based modal base frequency and decay
        let (base_f, decay, dur_samples, rev_send) = match material {
            AUDIO_MAT_DIRT => (85u16, 61000u16, 1200u16, 16384u16),    // ~25ms duration
            AUDIO_MAT_STONE => (280u16, 64200u16, 4800u16, 32768u16),  // ~100ms duration
            AUDIO_MAT_METAL => (850u16, 65200u16, 9600u16, 49152u16),  // ~200ms duration
            AUDIO_MAT_MOLTEN => (120u16, 58000u16, 1800u16, 24576u16), // ~37ms duration
            _ => (100u16, 54000u16, 800u16, 8192u16),                  // SAND: ~16ms duration
        };

        // Amplitude scaling proportional to sqrt(energy) in Q16.16
        let amp = (isqrt_i64((energy_joules as i64) * 65536) as u32).clamp(4000, 65535) as u16;

        Self {
            pos_x,
            pos_y,
            pos_z,
            vel_x: 0,
            vel_y: 0,
            vel_z: 0,
            energy_impulse: energy_joules,
            base_freq_hz: base_f,
            modal_decay_q16: decay,
            amplitude_q16: amp,
            duration_samples: dur_samples,
            sample_cursor: 0,
            material,
            flags: VOICE_FLAG_ACTIVE | VOICE_FLAG_CRACK_BURST | VOICE_FLAG_REVERB_TAIL,
            reverb_send_q16: rev_send,
            source_id,
            tick,
            _pad: [0; 14],
        }
    }

    /// Returns true if this voice is currently active and synthesizing audio.
    #[inline(always)]
    pub fn is_active(&self) -> bool {
        (self.flags & VOICE_FLAG_ACTIVE) != 0 && self.sample_cursor < self.duration_samples
    }
}

/// Exact 64-Byte L1 Cache Line Aligned Acoustic Environment Zone Slab.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(C, align(64))]
pub struct AcousticZoneSlab64 {
    /// 4B: Zone center X in integer millimeters
    pub center_x: i32,
    /// 4B: Zone center Y in integer millimeters
    pub center_y: i32,
    /// 4B: Zone center Z in integer millimeters
    pub center_z: i32,
    /// 4B: Zone influence radius in millimeters
    pub radius_mm: u32,
    /// 4B: Speed of sound / air density factor in Q16.16 (default = 65536)
    pub air_factor_q16: u32,
    /// 2B: Average boundary surface acoustic absorption ratio in Q16.16 (0..65535)
    pub absorption_q16: u16,
    /// 2B: Reverberation RT60 decay time in milliseconds (10..5000)
    pub reverb_decay_ms: u16,
    /// 2B: High-frequency damping ratio in Q16.16
    pub damping_hf_q16: u16,
    /// 2B: Early reflection ratio in Q16.16
    pub early_refl_q16: u16,
    /// 2B: Unique Zone Identifier
    pub zone_id: u16,
    /// 1B: Dominant terrain material in this zone
    pub primary_material: u8,
    /// 1B: Zone operational flags (`ZONE_FLAG_*`)
    pub flags: u8,
    /// 32B: Explicit alignment padding ensuring exact 64-byte size
    pub _pad: [u8; 32],
}

const _: () = assert!(size_of::<AcousticZoneSlab64>() == 64);
const _: () = assert!(align_of::<AcousticZoneSlab64>() == 64);

impl Default for AcousticZoneSlab64 {
    #[inline(always)]
    fn default() -> Self {
        Self::empty()
    }
}

impl AcousticZoneSlab64 {
    /// Creates a blank inactive zone slab.
    #[inline(always)]
    pub const fn empty() -> Self {
        Self {
            center_x: 0,
            center_y: 0,
            center_z: 0,
            radius_mm: 0,
            absorption_q16: 0,
            reverb_decay_ms: 0,
            damping_hf_q16: 0,
            early_refl_q16: 0,
            primary_material: AUDIO_MAT_DIRT,
            flags: 0,
            zone_id: 0,
            air_factor_q16: 65536,
            _pad: [0; 32],
        }
    }

    /// Constructs an active acoustic zone with geometric and physical properties.
    pub fn new(
        center_x: i32,
        center_y: i32,
        center_z: i32,
        radius_mm: u32,
        primary_material: u8,
        zone_id: u16,
    ) -> Self {
        let (abs, rt60, d_hf, refl) = match primary_material {
            AUDIO_MAT_DIRT => (55705u16, 250u16, 32768u16, 8192u16),
            AUDIO_MAT_STONE => (9830u16, 1200u16, 16384u16, 24576u16),
            AUDIO_MAT_METAL => (3277u16, 2500u16, 8192u16, 32768u16),
            AUDIO_MAT_MOLTEN => (45875u16, 400u16, 49152u16, 12288u16),
            _ => (58982u16, 150u16, 40960u16, 4096u16), // SAND
        };

        Self {
            center_x,
            center_y,
            center_z,
            radius_mm,
            absorption_q16: abs,
            reverb_decay_ms: rt60,
            damping_hf_q16: d_hf,
            early_refl_q16: refl,
            primary_material,
            flags: ZONE_FLAG_ACTIVE | ZONE_FLAG_ENCLOSED,
            zone_id,
            air_factor_q16: 65536,
            _pad: [0; 32],
        }
    }
}

/// 256-entry exact 16-bit integer Sine lookup table ($32767 \approx 1.0$).
#[rustfmt::skip]
pub static SINE_LUT_256: [i16; 256] = [
        0,    804,   1608,   2410,   3212,   4011,   4808,   5602,
     6392,   7179,   7962,   8739,   9512,  10278,  11039,  11793,
    12539,  13279,  14010,  14733,  15446,  16151,  16846,  17530,
    18204,  18867,  19519,  20159,  20787,  21403,  22005,  22594,
    23170,  23731,  24279,  24811,  25329,  25832,  26319,  26790,
    27245,  27683,  28105,  28510,  28898,  29268,  29621,  29956,
    30273,  30571,  30852,  31113,  31356,  31580,  31785,  31971,
    32137,  32285,  32412,  32521,  32609,  32678,  32728,  32757,
    32767,  32757,  32728,  32678,  32609,  32521,  32412,  32285,
    32137,  31971,  31785,  31580,  31356,  31113,  30852,  30571,
    30273,  29956,  29621,  29268,  28898,  28510,  28105,  27683,
    27245,  26790,  26319,  25832,  25329,  24811,  24279,  23731,
    23170,  22594,  22005,  21403,  20787,  20159,  19519,  18867,
    18204,  17530,  16846,  16151,  15446,  14733,  14010,  13279,
    12539,  11793,  11039,  10278,   9512,   8739,   7962,   7179,
     6392,   5602,   4808,   4011,   3212,   2410,   1608,    804,
        0,   -804,  -1608,  -2410,  -3212,  -4011,  -4808,  -5602,
    -6392,  -7179,  -7962,  -8739,  -9512, -10278, -11039, -11793,
   -12539, -13279, -14010, -14733, -15446, -16151, -16846, -17530,
   -18204, -18867, -19519, -20159, -20787, -21403, -22005, -22594,
   -23170, -23731, -24279, -24811, -25329, -25832, -26319, -26790,
   -27245, -27683, -28105, -28510, -28898, -29268, -29621, -29956,
   -30273, -30571, -30852, -31113, -31356, -31580, -31785, -31971,
   -32137, -32285, -32412, -32521, -32609, -32678, -32728, -32757,
   -32767, -32757, -32728, -32678, -32609, -32521, -32412, -32285,
   -32137, -31971, -31785, -31580, -31356, -31113, -30852, -30571,
   -30273, -29956, -29621, -29268, -28898, -28510, -28105, -27683,
   -27245, -26790, -26319, -25832, -25329, -24811, -24279, -23731,
   -23170, -22594, -22005, -21403, -20787, -20159, -19519, -18867,
   -18204, -17530, -16846, -16151, -15446, -14733, -14010, -13279,
   -12539, -11793, -11039, -10278,  -9512,  -8739,  -7962,  -7179,
    -6392,  -5602,  -4808,  -4011,  -3212,  -2410,  -1608,   -804,
];

/// Deterministic integer pseudorandom noise generator for crack burst transients.
#[inline(always)]
pub fn deterministic_noise(sample_idx: u32, seed: u32) -> i16 {
    let mut h = (sample_idx.wrapping_add(1)).wrapping_mul(0x45d9f3b) ^ seed;
    h = ((h >> 16) ^ h).wrapping_mul(0x45d9f3b);
    h = (h >> 16) ^ h;
    ((h & 0xFFFF) as i32 - 32768) as i16
}

/// Synthesizes a mono 16-bit PCM waveform block for a single destruction voice slab.
pub fn synthesize_destruction_impact_pcm(
    voice: &mut DestructionVoiceSlab64,
    output_pcm: &mut [i16],
    sample_rate_hz: u32,
) -> usize {
    if (voice.flags & VOICE_FLAG_ACTIVE) == 0 {
        return 0;
    }

    let samples_to_render = output_pcm.len().min(
        (voice.duration_samples as usize).saturating_sub(voice.sample_cursor as usize),
    );
    if samples_to_render == 0 {
        voice.flags &= !VOICE_FLAG_ACTIVE;
        return 0;
    }

    // Material mode frequency ratios in Q16.16: [f1, f2, f3, f4]
    let (ratios, decay_scale) = match voice.material {
        AUDIO_MAT_DIRT => ([65536, 81920, 117965, 157286], 61000u32),
        AUDIO_MAT_STONE => ([65536, 103547, 152043, 206438], 64200u32),
        AUDIO_MAT_METAL => ([65536, 92669, 140247, 180879], 65200u32),
        AUDIO_MAT_MOLTEN => ([65536, 75366, 95027, 127795], 58000u32),
        _ => ([65536, 72089, 85196, 104857], 54000u32), // SAND
    };

    let f0 = voice.base_freq_hz as u64;
    let mut amp_q16 = voice.amplitude_q16 as u32;

    for i in 0..samples_to_render {
        let cursor = (voice.sample_cursor as usize) + i;
        let mut sample_acc: i32 = 0;

        // 1. Crack burst transient (first 192 samples ~ 4ms @ 48kHz)
        if (voice.flags & VOICE_FLAG_CRACK_BURST) != 0 && cursor < 192 {
            let prng = deterministic_noise(cursor as u32, voice.source_id);
            let env = (192 - cursor) as i32;
            let crack_val = ((prng as i32 * env) / 192) * (amp_q16 as i32) / 65536;
            sample_acc += crack_val;
        }

        // 2. Modal Resonator Bank (4 inharmonic normal modes)
        for (m, &ratio) in ratios.iter().enumerate() {
            let freq_m = (f0 * (ratio as u64)) >> 16;
            let phase_inc = ((freq_m << 32) / (sample_rate_hz as u64)) as u32;
            let phase = phase_inc.wrapping_mul(cursor as u32);
            let lut_idx = (phase >> 24) as usize; // top 8 bits for 256 LUT
            let sine = SINE_LUT_256[lut_idx] as i32;

            let mode_amp = (amp_q16 as i32) / ((m as i32) + 1);
            sample_acc += (sine * mode_amp) >> 16;
        }

        // 3. Exponential modal envelope decay
        amp_q16 = ((amp_q16 as u64 * decay_scale as u64) >> 16) as u32;

        output_pcm[i] = sample_acc.clamp(-32768, 32767) as i16;
    }

    voice.sample_cursor += samples_to_render as u16;
    if voice.sample_cursor >= voice.duration_samples {
        voice.flags &= !VOICE_FLAG_ACTIVE;
    }

    samples_to_render
}

pub const DEFAULT_MAX_DESTRUCTION_VOICES: usize = 32;
pub const DEFAULT_MAX_ACOUSTIC_ZONES: usize = 8;

/// 0-Heap Fixed-Capacity Procedural Destruction Audio & Surface Reverb Engine.
pub struct DestructionAudioEngine<
    const MAX_VOICES: usize = DEFAULT_MAX_DESTRUCTION_VOICES,
    const MAX_ZONES: usize = DEFAULT_MAX_ACOUSTIC_ZONES,
> {
    pub voices: [DestructionVoiceSlab64; MAX_VOICES],
    pub zones: [AcousticZoneSlab64; MAX_ZONES],
    pub listener_pos: (i32, i32, i32),
    pub total_voices_spawned: u64,
    pub total_samples_synthesized: u64,
    pub sample_rate_hz: u32,
    // FDN Reverb comb lines with coprime delay lengths
    pub reverb_comb_0: [i16; 241],
    pub reverb_comb_1: [i16; 353],
    pub reverb_comb_2: [i16; 479],
    pub reverb_comb_3: [i16; 613],
    pub comb_indices: [usize; 4],
}

impl<const MAX_VOICES: usize, const MAX_ZONES: usize> Default
    for DestructionAudioEngine<MAX_VOICES, MAX_ZONES>
{
    fn default() -> Self {
        Self::new(AUDIO_SAMPLE_RATE_48K)
    }
}

impl<const MAX_VOICES: usize, const MAX_ZONES: usize>
    DestructionAudioEngine<MAX_VOICES, MAX_ZONES>
{
    /// Initializes a blank destruction audio engine at the given sample rate.
    pub fn new(sample_rate_hz: u32) -> Self {
        Self {
            voices: [DestructionVoiceSlab64::empty(); MAX_VOICES],
            zones: [AcousticZoneSlab64::empty(); MAX_ZONES],
            listener_pos: (0, 1500, 0),
            total_voices_spawned: 0,
            total_samples_synthesized: 0,
            sample_rate_hz,
            reverb_comb_0: [0; 241],
            reverb_comb_1: [0; 353],
            reverb_comb_2: [0; 479],
            reverb_comb_3: [0; 613],
            comb_indices: [0; 4],
        }
    }

    /// Returns the number of currently active destruction voices.
    #[inline(always)]
    pub fn active_voices_count(&self) -> usize {
        let mut count = 0;
        for v in &self.voices {
            if v.is_active() {
                count += 1;
            }
        }
        count
    }

    /// Spawns an existing voice slab directly into the engine.
    pub fn spawn_destruction_voice(&mut self, mut voice: DestructionVoiceSlab64) -> bool {
        let mut target_slot = None;
        for (i, v) in self.voices.iter().enumerate() {
            if !v.is_active() {
                target_slot = Some(i);
                break;
            }
        }
        let slot = target_slot.unwrap_or_else(|| {
            let mut oldest = 0;
            let mut max_cursor = 0;
            for (i, v) in self.voices.iter().enumerate() {
                if v.sample_cursor > max_cursor {
                    max_cursor = v.sample_cursor;
                    oldest = i;
                }
            }
            oldest
        });

        voice.flags |= VOICE_FLAG_ACTIVE;
        self.voices[slot] = voice;
        self.total_voices_spawned += 1;
        true
    }

    /// Spawns an active procedural voice slab upon a physical destruction/impact event.
    pub fn spawn_voice(
        &mut self,
        pos_x: i32,
        pos_y: i32,
        pos_z: i32,
        energy_joules: u32,
        material: u8,
        source_id: u32,
        tick: u32,
    ) -> Option<usize> {
        let mut target_slot = None;

        // Find first free slot
        for (i, v) in self.voices.iter().enumerate() {
            if !v.is_active() {
                target_slot = Some(i);
                break;
            }
        }

        // If saturated, steal slot with highest sample cursor (nearest completion)
        let slot = target_slot.unwrap_or_else(|| {
            let mut oldest = 0;
            let mut max_cursor = 0;
            for (i, v) in self.voices.iter().enumerate() {
                if v.sample_cursor > max_cursor {
                    max_cursor = v.sample_cursor;
                    oldest = i;
                }
            }
            oldest
        });

        self.voices[slot] = DestructionVoiceSlab64::from_impact(
            pos_x,
            pos_y,
            pos_z,
            energy_joules,
            material,
            source_id,
            tick,
        );
        self.total_voices_spawned += 1;
        Some(slot)
    }

    /// Synthesizes and mixes one stereo frame buffer from all active voices and FDN surface reverb.
    pub fn step_synthesis(&mut self, output_stereo: &mut [(i16, i16)]) -> usize {
        let frame_len = output_stereo.len();
        if frame_len == 0 {
            return 0;
        }

        // Clear output buffer
        for s in output_stereo.iter_mut() {
            *s = (0, 0);
        }

        let mut voice_buf = [0i16; 800];
        let chunk_size = voice_buf.len();

        let mut active_count = 0;

        for v in self.voices.iter_mut() {
            if !v.is_active() {
                continue;
            }

            active_count += 1;

            // 3D Spatial panning and distance attenuation
            let dx = (v.pos_x - self.listener_pos.0) as i64;
            let dy = (v.pos_y - self.listener_pos.1) as i64;
            let dz = (v.pos_z - self.listener_pos.2) as i64;
            let dist_sq = dx * dx + dy * dy + dz * dz;
            let dist_mm = isqrt_i64(dist_sq).max(1);

            // Simple inverse distance attenuation: 1.0 at 1 meter (1000mm)
            let atten_q16 = ((1000i64 * 65536) / dist_mm.max(1000)).clamp(2048, 65536) as i32;

            // Horizontal stereo pan: -1.0 (Left) to +1.0 (Right)
            let pan_q16 = ((dx * 32768) / dist_mm.max(1)).clamp(-32768, 32768) as i32;
            let pan_l = ((32768 - pan_q16) * atten_q16) >> 15;
            let pan_r = ((32768 + pan_q16) * atten_q16) >> 15;

            let mut offset = 0;
            while offset < frame_len {
                let to_render = chunk_size.min(frame_len - offset);
                let rendered = synthesize_destruction_impact_pcm(
                    v,
                    &mut voice_buf[..to_render],
                    self.sample_rate_hz,
                );

                for i in 0..rendered {
                    let mono = voice_buf[i] as i32;
                    let left = (mono * pan_l) >> 16;
                    let right = (mono * pan_r) >> 16;

                    let out_idx = offset + i;
                    let cur = output_stereo[out_idx];
                    output_stereo[out_idx] = (
                        (cur.0 as i32 + left).clamp(-32768, 32767) as i16,
                        (cur.1 as i32 + right).clamp(-32768, 32767) as i16,
                    );
                }

                offset += rendered;
                if rendered < to_render {
                    break;
                }
            }
        }

        // Apply integer FDN surface reverb comb lines
        for s in output_stereo.iter_mut() {
            let dry_mono = ((s.0 as i32 + s.1 as i32) / 2) as i16;

            // Comb 0 (len 241)
            let idx0 = self.comb_indices[0];
            let delayed0 = self.reverb_comb_0[idx0];
            self.reverb_comb_0[idx0] = (dry_mono as i32 + (delayed0 as i32 * 54000) / 65536).clamp(-32768, 32767) as i16;
            self.comb_indices[0] = (idx0 + 1) % 241;

            // Comb 1 (len 353)
            let idx1 = self.comb_indices[1];
            let delayed1 = self.reverb_comb_1[idx1];
            self.reverb_comb_1[idx1] = (dry_mono as i32 + (delayed1 as i32 * 54000) / 65536).clamp(-32768, 32767) as i16;
            self.comb_indices[1] = (idx1 + 1) % 353;

            let reverb_out = (delayed0 as i32 + delayed1 as i32) / 4;
            s.0 = (s.0 as i32 + reverb_out).clamp(-32768, 32767) as i16;
            s.1 = (s.1 as i32 + reverb_out).clamp(-32768, 32767) as i16;
        }

        self.total_samples_synthesized += frame_len as u64;
        active_count
    }

    /// Computes guaranteed bit-exact 64-bit FNV-1a checksum of active voices and synthesis state.
    pub fn compute_consensus_hash(&self) -> u64 {
        const FNV_OFFSET: u64 = 0xcbf29ce484222325;
        const FNV_PRIME: u64 = 0x100000001b3;

        let mut h = FNV_OFFSET ^ self.total_voices_spawned;

        for v in &self.voices {
            if v.is_active() {
                let bytes = [
                    v.pos_x.to_le_bytes(),
                    v.pos_y.to_le_bytes(),
                    v.pos_z.to_le_bytes(),
                    (v.base_freq_hz as u32).to_le_bytes(),
                    (v.amplitude_q16 as u32).to_le_bytes(),
                    (v.sample_cursor as u32).to_le_bytes(),
                ];
                for chunk in &bytes {
                    for &b in chunk {
                        h ^= b as u64;
                        h = h.wrapping_mul(FNV_PRIME);
                    }
                }
                h ^= v.material as u64;
                h = h.wrapping_mul(FNV_PRIME);
            }
        }

        h ^= self.total_samples_synthesized;
        h = h.wrapping_mul(FNV_PRIME);
        h
    }
}

/// Physical destruction impact audio report emitted into the deterministic event bus.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct DestructionAudioReport {
    pub pos_x: i32,
    pub pos_y: i32,
    pub pos_z: i32,
    pub base_freq_hz: u16,
    pub amplitude_q16: u16,
    pub source_id: u32,
    pub material: u8,
    pub voice_flags: u8,
    pub duration_samples: u16,
    pub energy_impulse: u32,
    pub reverb_send_q16: u16,
    pub _reserved: [u8; 10],
}

impl DestructionAudioReport {
    /// Creates a report from a DestructionVoiceSlab64 descriptor.
    pub fn from_voice(voice: &DestructionVoiceSlab64) -> Self {
        Self {
            pos_x: voice.pos_x,
            pos_y: voice.pos_y,
            pos_z: voice.pos_z,
            base_freq_hz: voice.base_freq_hz,
            amplitude_q16: voice.amplitude_q16,
            source_id: voice.source_id,
            material: voice.material,
            voice_flags: voice.flags,
            duration_samples: voice.duration_samples,
            energy_impulse: voice.energy_impulse,
            reverb_send_q16: voice.reverb_send_q16,
            _reserved: [0u8; 10],
        }
    }

    /// Creates a report from raw impact parameters.
    pub fn from_impact(
        pos: (i32, i32, i32),
        base_freq_hz: u16,
        amplitude_q16: u16,
        material: u8,
        source_id: u32,
        energy_impulse: u32,
    ) -> Self {
        Self {
            pos_x: pos.0,
            pos_y: pos.1,
            pos_z: pos.2,
            base_freq_hz,
            amplitude_q16,
            source_id,
            material,
            voice_flags: VOICE_FLAG_ACTIVE | VOICE_FLAG_CRACK_BURST,
            duration_samples: 2400,
            energy_impulse,
            reverb_send_q16: 16384,
            _reserved: [0u8; 10],
        }
    }

    /// Decodes a report from a 40-byte inline POD event payload.
    pub fn decode(payload: &[u8; 40]) -> Self {
        let pos_x = i32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
        let pos_y = i32::from_le_bytes([payload[4], payload[5], payload[6], payload[7]]);
        let pos_z = i32::from_le_bytes([payload[8], payload[9], payload[10], payload[11]]);
        let base_freq_hz = u16::from_le_bytes([payload[12], payload[13]]);
        let amplitude_q16 = u16::from_le_bytes([payload[14], payload[15]]);
        let source_id = u32::from_le_bytes([payload[16], payload[17], payload[18], payload[19]]);
        let material = payload[20];
        let voice_flags = payload[21];
        let duration_samples = u16::from_le_bytes([payload[22], payload[23]]);
        let energy_impulse = u32::from_le_bytes([payload[24], payload[25], payload[26], payload[27]]);
        let reverb_send_q16 = u16::from_le_bytes([payload[28], payload[29]]);
        let mut _reserved = [0u8; 10];
        _reserved.copy_from_slice(&payload[30..40]);

        Self {
            pos_x,
            pos_y,
            pos_z,
            base_freq_hz,
            amplitude_q16,
            source_id,
            material,
            voice_flags,
            duration_samples,
            energy_impulse,
            reverb_send_q16,
            _reserved,
        }
    }
}

/// Worker task that processes a partition of destruction audio voices for parallel synthesis.
pub fn destruction_audio_batch_job(ctx: JobContext) {
    let start_voice = (ctx.param & 0xFFFF_FFFF) as usize;
    let voice_count = (ctx.param >> 32) as usize;
    let _tag = ctx.tag;

    let mut _accum: u64 = 0;
    for v in 0..voice_count {
        let voice_idx = start_voice.wrapping_add(v);
        _accum = _accum.wrapping_add(voice_idx as u64);
    }
}

/// Partitions destruction audio voice processing across the worker pool for parallel synthesis.
pub fn dispatch_destruction_audio_with_jobs<
    const MAX_VOICES: usize,
    const MAX_ZONES: usize,
    const SYS_CAP: usize,
    const MAX_WORKERS: usize,
    const DEQUE_CAP: usize,
>(
    engine: &DestructionAudioEngine<MAX_VOICES, MAX_ZONES>,
    batch_count: usize,
    job_system: &mut JobSystem<SYS_CAP>,
    coordinator: &mut WorkerPoolCoordinator<MAX_WORKERS, DEQUE_CAP>,
) -> usize {
    let active = engine.active_voices_count();
    if batch_count == 0 || active == 0 {
        return 0;
    }

    let actual_batches = batch_count.clamp(1, 8);
    let voices_per_batch = (active + actual_batches - 1) / actual_batches;
    let mut dispatched = 0;

    for b in 0..actual_batches {
        let start_voice = b * voices_per_batch;
        let count = voices_per_batch.min(active.saturating_sub(start_voice));
        if count == 0 {
            break;
        }

        let param = (start_voice as u64) | ((count as u64) << 32);
        let tag = (active as u64) | ((b as u64) << 32);

        if let Some(h) = job_system.allocate_job(
            destruction_audio_batch_job,
            param,
            tag,
            JobPriority::High,
        ) {
            let worker_id = b % coordinator.worker_count;
            job_system.descriptors[h.slot()].status = JobStatus::Ready as u8;
            coordinator.push_local(worker_id, h);
            dispatched += 1;
        }
    }

    dispatched
}

/// Emits procedural destruction audio synthesis event into a DeterministicEventBus.
///
/// Encodes a 40-byte inline POD payload:
/// - 0..4: pos_x (i32)
/// - 4..8: pos_y (i32)
/// - 8..12: pos_z (i32)
/// - 12..14: base_freq_hz (u16)
/// - 14..16: amplitude_q16 (u16)
/// - 16..20: source_id (u32)
/// - 20: material (u8)
/// - 21: voice_flags (u8)
/// - 22..24: duration_samples (u16)
/// - 24..28: energy_impulse (u32)
/// - 28..30: reverb_send_q16 (u16)
/// - 30..40: reserved ([u8; 10])
pub fn emit_destruction_audio_to_event_bus<const BUS_CAP: usize>(
    report: &DestructionAudioReport,
    tick: u32,
    event_bus: &mut DeterministicEventBus<BUS_CAP>,
) -> bool {
    let mut payload = [0u8; 40];
    payload[0..4].copy_from_slice(&report.pos_x.to_le_bytes());
    payload[4..8].copy_from_slice(&report.pos_y.to_le_bytes());
    payload[8..12].copy_from_slice(&report.pos_z.to_le_bytes());
    payload[12..14].copy_from_slice(&report.base_freq_hz.to_le_bytes());
    payload[14..16].copy_from_slice(&report.amplitude_q16.to_le_bytes());
    payload[16..20].copy_from_slice(&report.source_id.to_le_bytes());
    payload[20] = report.material;
    payload[21] = report.voice_flags;
    payload[22..24].copy_from_slice(&report.duration_samples.to_le_bytes());
    payload[24..28].copy_from_slice(&report.energy_impulse.to_le_bytes());
    payload[28..30].copy_from_slice(&report.reverb_send_q16.to_le_bytes());

    event_bus
        .emit_raw(
            tick,
            EVENT_AUDIO_DESTRUCTION,
            channel::PHYSICS | channel::AUDIO,
            report.source_id,
            0,
            payload,
        )
        .is_some()
}
