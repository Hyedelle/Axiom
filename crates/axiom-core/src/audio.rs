//! # Deterministic 3D Spatial Audio Simulator (Phase 4 RPG Audio)
//!
//! Provides zero-float inverse-square distance attenuation, stereo panning,
//! and priority-based voice culling for thousands of simultaneous world sound events.

use crate::fixed::{Fixed, FixedVec2};
use crate::hash::deterministic_hash;

pub const DEFAULT_MAX_VOICES: usize = 64;
pub const MAX_AUDIO_QUEUE: usize = 256;

/// Sound category and playback priority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u8)]
pub enum SoundCategory {
    #[default]
    Ambient = 0,
    Footstep = 1,
    Combat = 2,
    Magic = 3,
    UI = 4,
}

/// A spatial sound event emitted in the world.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(C)]
pub struct SoundEvent {
    pub sound_id: u32,
    pub pos: FixedVec2,
    pub base_volume: u16,  // 0 ~ 1000
    pub max_distance: u16, // Maximum audible radius in tiles
    pub category: SoundCategory,
    pub priority: u8, // Higher = more important
}

impl Default for SoundEvent {
    fn default() -> Self {
        Self {
            sound_id: 0,
            pos: FixedVec2::ZERO,
            base_volume: 0,
            max_distance: 0,
            category: SoundCategory::Ambient,
            priority: 0,
        }
    }
}

/// Resolved spatial audio channel ready for hardware playback or headless verification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(C)]
pub struct SpatialVoice {
    pub sound_id: u32,
    pub volume_left: u16,  // 0 ~ 1000
    pub volume_right: u16, // 0 ~ 1000
    pub active: bool,
}

/// Frame mixing output containing top prioritized spatial voices and master stereo levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioMixFrame<const VOICES: usize = DEFAULT_MAX_VOICES> {
    pub voices: [SpatialVoice; VOICES],
    pub active_voices_count: usize,
    pub master_left: u32,
    pub master_right: u32,
}

/// Deterministic 3D spatial audio simulator.
#[derive(Debug, Clone)]
pub struct SpatialAudioSimulator<const VOICES: usize = DEFAULT_MAX_VOICES> {
    pub listener_pos: FixedVec2,
    pub event_queue: [SoundEvent; MAX_AUDIO_QUEUE],
    pub queue_len: usize,
    pub total_events_processed: u64,
}

impl<const VOICES: usize> Default for SpatialAudioSimulator<VOICES> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const VOICES: usize> SpatialAudioSimulator<VOICES> {
    /// Creates a new spatial audio simulator.
    pub const fn new() -> Self {
        Self {
            listener_pos: FixedVec2::ZERO,
            event_queue: [SoundEvent {
                sound_id: 0,
                pos: FixedVec2::ZERO,
                base_volume: 0,
                max_distance: 0,
                category: SoundCategory::Ambient,
                priority: 0,
            }; MAX_AUDIO_QUEUE],
            queue_len: 0,
            total_events_processed: 0,
        }
    }

    /// Sets listener position (typically the player coordinates).
    #[inline(always)]
    pub fn set_listener_pos(&mut self, pos: FixedVec2) {
        self.listener_pos = pos;
    }

    /// Enqueues a sound event for the current tick.
    pub fn emit_sound(&mut self, event: SoundEvent) -> bool {
        if self.queue_len < MAX_AUDIO_QUEUE {
            self.event_queue[self.queue_len] = event;
            self.queue_len += 1;
            true
        } else {
            false
        }
    }

    /// Processes all queued sound events, performs spatial attenuation, and outputs mixed voices.
    pub fn process_and_mix(&mut self) -> AudioMixFrame<VOICES> {
        let mut frame = AudioMixFrame {
            voices: [SpatialVoice::default(); VOICES],
            active_voices_count: 0,
            master_left: 0,
            master_right: 0,
        };

        if self.queue_len == 0 {
            return frame;
        }

        // Temporary evaluation buffer: (effective_volume, voice)
        let mut candidate_voices = [(0u32, SpatialVoice::default()); MAX_AUDIO_QUEUE];
        let mut candidate_count = 0;

        for i in 0..self.queue_len {
            let event = self.event_queue[i];
            let dx = event.pos.x - self.listener_pos.x;
            let dy = event.pos.y - self.listener_pos.y;
            let dist_sq = dx * dx + dy * dy;

            let max_r = Fixed::from_int(event.max_distance as i64);
            let max_r_sq = max_r * max_r;

            // Inaudible check
            if dist_sq >= max_r_sq || event.base_volume == 0 {
                continue;
            }

            // Fixed-point attenuation: 1.0 - (dist_sq / max_r_sq)
            let atten_ratio = Fixed::ONE - (dist_sq / max_r_sq);
            let base_vol = Fixed::from_int(event.base_volume as i64);
            let effective_vol = (base_vol * atten_ratio).to_int().max(0) as u32;

            if effective_vol == 0 {
                continue;
            }

            // Stereo Panning: relative X offset determines Left/Right split
            // pan in [-1.0, 1.0] -> Q32.16 clamped
            let clamped_dx = if dx > max_r {
                max_r
            } else if dx < -max_r {
                -max_r
            } else {
                dx
            };
            let pan_ratio = clamped_dx / max_r; // -1.0 = full left, 1.0 = full right

            // Left multiplier = (1.0 - pan) / 2
            // Right multiplier = (1.0 + pan) / 2
            let half = Fixed::from_raw(32_768); // 0.5
            let mul_l = (Fixed::ONE - pan_ratio) * half;
            let mul_r = (Fixed::ONE + pan_ratio) * half;

            let vol_l = ((Fixed::from_int(effective_vol as i64) * mul_l).to_int().max(0)).min(1000) as u16;
            let vol_r = ((Fixed::from_int(effective_vol as i64) * mul_r).to_int().max(0)).min(1000) as u16;

            let priority_weighted_vol = effective_vol.saturating_mul(event.priority as u32 + 1);

            candidate_voices[candidate_count] = (
                priority_weighted_vol,
                SpatialVoice {
                    sound_id: event.sound_id,
                    volume_left: vol_l,
                    volume_right: vol_r,
                    active: true,
                },
            );
            candidate_count += 1;
        }

        self.total_events_processed += self.queue_len as u64;
        self.queue_len = 0; // Clear queue for next tick

        // Sort candidates by audibility weight descending (insertion sort for small arrays)
        for i in 1..candidate_count {
            let mut j = i;
            while j > 0 && candidate_voices[j - 1].0 < candidate_voices[j].0 {
                candidate_voices.swap(j - 1, j);
                j -= 1;
            }
        }

        // Fill top VOICES
        let active = candidate_count.min(VOICES);
        for i in 0..active {
            let voice = candidate_voices[i].1;
            frame.voices[i] = voice;
            frame.master_left = frame.master_left.saturating_add(voice.volume_left as u32);
            frame.master_right = frame.master_right.saturating_add(voice.volume_right as u32);
        }
        frame.active_voices_count = active;

        frame
    }

    /// Computes deterministic state hash of current audio queue and statistics.
    pub fn compute_hash(&self) -> u64 {
        let mut h = self.total_events_processed.wrapping_mul(0x9E3779B97F4A7C15);
        h ^= deterministic_hash(&self.listener_pos);
        for i in 0..self.queue_len {
            h ^= deterministic_hash(&self.event_queue[i]);
            h = h.rotate_left(5);
        }
        h
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spatial_audio_attenuation_and_panning() {
        let mut audio = SpatialAudioSimulator::<16>::new();
        audio.set_listener_pos(FixedVec2::ZERO);

        // Sound 1: 10 tiles directly East (Pan Right)
        audio.emit_sound(SoundEvent {
            sound_id: 101,
            pos: FixedVec2::from_ints(10, 0),
            base_volume: 800,
            max_distance: 20,
            category: SoundCategory::Combat,
            priority: 5,
        });

        // Sound 2: 10 tiles directly West (Pan Left)
        audio.emit_sound(SoundEvent {
            sound_id: 102,
            pos: FixedVec2::from_ints(-10, 0),
            base_volume: 800,
            max_distance: 20,
            category: SoundCategory::Combat,
            priority: 5,
        });

        let mix = audio.process_and_mix();
        assert_eq!(mix.active_voices_count, 2);

        // Sound at East should be louder in Right channel than Left channel
        let east_voice = mix.voices.iter().find(|v| v.sound_id == 101).unwrap();
        assert!(
            east_voice.volume_right > east_voice.volume_left,
            "East sound must pan to right: Left={}, Right={}",
            east_voice.volume_left,
            east_voice.volume_right
        );

        // Sound at West should be louder in Left channel than Right channel
        let west_voice = mix.voices.iter().find(|v| v.sound_id == 102).unwrap();
        assert!(
            west_voice.volume_left > west_voice.volume_right,
            "West sound must pan to left: Left={}, Right={}",
            west_voice.volume_left,
            west_voice.volume_right
        );
    }
}
