//! 3D Dynamic Camera System for Combat Arena and Orbit Inspection.
//!
//! Provides 64-byte L1 cache-aligned `Camera3D` with spring-damped impact punch zoom,
//! deterministic rotational orbit, and camera shake.

use crate::math3d::{Mat4Fixed, Vec3Fixed};
use axiom_core::Fixed;

/// 64-byte L1 Cache-aligned 3D Camera with dynamic orbit and spring kinetics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C, align(64))]
pub struct Camera3D {
    pub target: Vec3Fixed,
    pub distance: Fixed,
    pub base_focal_length: i64,
    pub orbit_yaw: u8,
    pub orbit_pitch: u8,
    pub zoom_offset: Fixed,
    pub shake_x: i32,
    pub shake_y: i32,
}

impl Default for Camera3D {
    fn default() -> Self {
        Self {
            target: Vec3Fixed::new(Fixed::ZERO, Fixed::from_raw(13107), Fixed::ZERO), // Y ~0.2m
            distance: Fixed::from_raw((2.2 * 65536.0) as i64),
            base_focal_length: 520,
            orbit_yaw: 0,
            orbit_pitch: 4, // slight downward elevation angle
            zoom_offset: Fixed::ZERO,
            shake_x: 0,
            shake_y: 0,
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

    /// Triggers an immediate spring punch-zoom (e.g. upon hitting a heavy cross).
    pub fn trigger_punch_zoom(&mut self, amount: Fixed) {
        self.zoom_offset += amount;
    }

    /// Decays zoom spring and shake deterministically each tick.
    pub fn step_kinematics(&mut self) {
        // Spring return: reduce zoom_offset by 15% each tick
        if self.zoom_offset.raw() > 0 {
            self.zoom_offset = Fixed::from_raw((self.zoom_offset.raw() * 85) / 100);
            if self.zoom_offset.raw() < 500 {
                self.zoom_offset = Fixed::ZERO;
            }
        }

        // Decay shake
        self.shake_x = (self.shake_x * 70) / 100;
        self.shake_y = (self.shake_y * 70) / 100;
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
        self.base_focal_length
    }

    /// Computes view matrix rotating around target.
    pub fn compute_view_matrix(&self) -> Mat4Fixed {
        // Target translation to origin
        let t_inv = Mat4Fixed::from_translation(-self.target);

        // Orbit rotations
        let r_yaw = Mat4Fixed::from_rotation_y(self.orbit_yaw);
        let r_pitch = Mat4Fixed::from_rotation_x(self.orbit_pitch);

        r_pitch.mul_mat(&r_yaw).mul_mat(&t_inv)
    }
}
