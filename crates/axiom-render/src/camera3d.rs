//! 3D Dynamic Camera System for Combat Arena and Orbit Inspection.
//!
//! Provides 64-byte L1 cache-aligned `Camera3D` with spring-damped impact punch zoom,
//! deterministic rotational orbit, corner camera shots, and camera shake.

use crate::math3d::{Mat4Fixed, Vec3Fixed};
use axiom_core::Fixed;

/// Cinematic camera shot presets for ringside combat.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum CameraShot {
    #[default]
    BroadcastCenter = 0,
    PlayerCorner = 1,
    OpponentCorner = 2,
    ImpactCloseUp = 3,
    KnockdownLowAngle = 4,
}

/// 64-byte L1 Cache-aligned 3D Camera with dynamic orbit, corner shots, and spring kinetics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C, align(64))]
pub struct Camera3D {
    pub target: Vec3Fixed,       // 24 bytes (3 x Fixed i64)
    pub distance: Fixed,         // 8 bytes
    pub base_focal_length: i64,  // 8 bytes
    pub zoom_offset: Fixed,      // 8 bytes
    pub shake_x: i32,            // 4 bytes
    pub shake_y: i32,            // 4 bytes
    pub orbit_yaw: u8,           // 1 byte
    pub orbit_pitch: u8,         // 1 byte
    pub target_yaw: u8,          // 1 byte
    pub target_pitch: u8,        // 1 byte
    pub shot_mode: CameraShot,   // 1 byte
    pub shake_rot: i8,           // 1 byte
    pub _pad: [u8; 2],           // 2 bytes -> exactly 64 bytes
}

impl Default for Camera3D {
    fn default() -> Self {
        Self {
            target: Vec3Fixed::new(Fixed::ZERO, Fixed::from_raw(13107), Fixed::ZERO), // Y ~0.2m
            distance: Fixed::from_raw((2.2 * 65536.0) as i64),
            base_focal_length: 520,
            zoom_offset: Fixed::ZERO,
            shake_x: 0,
            shake_y: 0,
            orbit_yaw: 0,
            orbit_pitch: 4, // slight downward elevation angle
            target_yaw: 0,
            target_pitch: 4,
            shot_mode: CameraShot::BroadcastCenter,
            shake_rot: 0,
            _pad: [0; 2],
        }
    }
}

impl Camera3D {
    pub fn new(target: Vec3Fixed, distance: Fixed, focal_length: i64) -> Self {
        Self {
            target,
            distance,
            base_focal_length: focal_length,
            ..Default::default()
        }
    }

    /// Sets the cinematic camera shot mode and targets.
    pub fn set_shot_mode(&mut self, shot: CameraShot) {
        self.shot_mode = shot;
        match shot {
            CameraShot::BroadcastCenter => {
                self.target_yaw = 0;
                self.target_pitch = 4;
            }
            CameraShot::PlayerCorner => {
                // Over-the-shoulder looking into opponent
                self.target_yaw = 18; // ~+25 deg BAM
                self.target_pitch = 6;
            }
            CameraShot::OpponentCorner => {
                // Looking back at player
                self.target_yaw = 238; // ~-25 deg BAM
                self.target_pitch = 6;
            }
            CameraShot::ImpactCloseUp => {
                self.target_yaw = 0;
                self.target_pitch = 2;
                self.trigger_punch_zoom(Fixed::from_raw(22000)); // ~0.33m zoom
            }
            CameraShot::KnockdownLowAngle => {
                // Low-angle worm's-eye hero shot looking up
                self.target_yaw = 0;
                self.target_pitch = 250; // -6 BAM upward tilt
            }
        }
    }

    /// Triggers an immediate spring punch-zoom (e.g. upon hitting a heavy cross).
    pub fn trigger_punch_zoom(&mut self, amount: Fixed) {
        self.zoom_offset += amount;
    }

    /// Triggers impact screen shake and camera rotational shudder.
    pub fn trigger_impact_shake(&mut self, intensity: i32) {
        self.shake_x = intensity;
        self.shake_y = -(intensity * 3) / 4;
        self.shake_rot = if intensity > 6 { 2 } else { -2 };
    }

    /// Decays zoom spring, interpolates yaw/pitch towards targets, and decays shake deterministically.
    pub fn step_kinematics(&mut self) {
        // Spring return: reduce zoom_offset by 15% each tick
        if self.zoom_offset.raw() > 0 {
            self.zoom_offset = Fixed::from_raw((self.zoom_offset.raw() * 85) / 100);
            if self.zoom_offset.raw() < 500 {
                self.zoom_offset = Fixed::ZERO;
            }
        }

        // Smoothly interpolate orbit yaw towards target
        if self.orbit_yaw != self.target_yaw {
            let diff = (self.target_yaw as i16) - (self.orbit_yaw as i16);
            let step = (diff / 4).clamp(-3, 3);
            if step == 0 {
                self.orbit_yaw = self.target_yaw;
            } else {
                self.orbit_yaw = (self.orbit_yaw as i16 + step) as u8;
            }
        }

        // Smoothly interpolate orbit pitch towards target
        if self.orbit_pitch != self.target_pitch {
            let diff = (self.target_pitch as i16) - (self.orbit_pitch as i16);
            let step = (diff / 4).clamp(-2, 2);
            if step == 0 {
                self.orbit_pitch = self.target_pitch;
            } else {
                self.orbit_pitch = (self.orbit_pitch as i16 + step) as u8;
            }
        }

        // Decay shake
        self.shake_x = (self.shake_x * 70) / 100;
        self.shake_y = (self.shake_y * 70) / 100;
        if self.shake_x.abs() < 1 { self.shake_x = 0; }
        if self.shake_y.abs() < 1 { self.shake_y = 0; }
        self.shake_rot = 0;
    }

    /// Computes the effective camera distance including punch-zoom.
    #[inline(always)]
    pub fn effective_distance(&self) -> Fixed {
        let d = self.distance - self.zoom_offset;
        if d.raw() < 32768 {
            Fixed::from_raw(32768) // Min 0.5m clamp
        } else {
            d
        }
    }

    /// Computes effective focal length in pixels.
    #[inline(always)]
    pub fn effective_focal_length(&self) -> i64 {
        if self.shot_mode == CameraShot::ImpactCloseUp {
            self.base_focal_length + 60
        } else {
            self.base_focal_length
        }
    }

    /// Computes view matrix rotating around target.
    pub fn compute_view_matrix(&self) -> Mat4Fixed {
        // Target translation to origin
        let t_inv = Mat4Fixed::from_translation(-self.target);

        // Orbit rotations
        let r_yaw = Mat4Fixed::from_rotation_y(self.orbit_yaw);
        let r_pitch = Mat4Fixed::from_rotation_x(self.orbit_pitch);
        let t_fwd = Mat4Fixed::from_translation(self.target);

        t_fwd.mul_mat(&r_pitch).mul_mat(&r_yaw).mul_mat(&t_inv)
    }
}
