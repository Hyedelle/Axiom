//! 3D Procedural Particle System for Steam Vents and Kinetic Metal Sparks.
//!
//! Provides 64-byte aligned, zero-allocation `ParticleSystem3D` with Z-buffer depth testing.

use crate::framebuffer::FrameBuffer;
use crate::math3d::{Mat4Fixed, Vec3Fixed};
use axiom_core::Fixed;

/// Individual 3D particle state (24 bytes + alignment).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(C)]
pub struct Particle3D {
    pub pos: Vec3Fixed,
    pub vel: Vec3Fixed,
    pub color: u32,
    pub life: u16,
    pub max_life: u16,
    pub size: u8,
    pub is_active: bool,
}

/// 64-byte aligned, fixed-capacity 3D Particle System.
#[repr(C, align(64))]
pub struct ParticleSystem3D<const CAP: usize = 128> {
    pub particles: [Particle3D; CAP],
    pub active_count: usize,
    pub rng_state: u64,
}

impl<const CAP: usize> Default for ParticleSystem3D<CAP> {
    fn default() -> Self {
        Self::new(0xCAFE_BABE_1234_5678)
    }
}

impl<const CAP: usize> ParticleSystem3D<CAP> {
    pub fn new(seed: u64) -> Self {
        Self {
            particles: [Particle3D::default(); CAP],
            active_count: 0,
            rng_state: seed,
        }
    }

    fn next_rng(&mut self) -> u64 {
        self.rng_state = self.rng_state.wrapping_mul(6364136223846793005).wrapping_add(1);
        self.rng_state
    }

    /// Spawns a billowing exhaust steam plume (e.g. from mecha boiler pack).
    pub fn spawn_steam(&mut self, origin: Vec3Fixed, count: usize) {
        for _ in 0..count {
            if self.active_count >= CAP {
                break;
            }

            let r = self.next_rng();
            let vx = (((r & 0xFF) as i64) - 128) * 12;
            let vy = 1500 + ((r >> 8) & 0xFF) as i64 * 10; // upward lift
            let vz = (((r >> 16) & 0xFF) as i64 - 128) * 12;

            for p in &mut self.particles {
                if !p.is_active {
                    p.is_active = true;
                    p.pos = origin;
                    p.vel = Vec3Fixed::from_raw(vx, vy, vz);
                    p.color = 0xAAECEFF4; // Translucent coolant steam
                    p.life = 25;
                    p.max_life = 25;
                    p.size = 3;
                    self.active_count += 1;
                    break;
                }
            }
        }
    }

    /// Spawns high-velocity kinetic sparks on punch contact.
    pub fn spawn_sparks(&mut self, origin: Vec3Fixed, count: usize) {
        for _ in 0..count {
            if self.active_count >= CAP {
                break;
            }

            let r = self.next_rng();
            let vx = (((r & 0xFF) as i64) - 128) * 45;
            let vy = (((r >> 8) & 0xFF) as i64 - 80) * 35;
            let vz = (((r >> 16) & 0xFF) as i64 - 128) * 45;

            let col = if (r & 1) == 0 { 0xFFFBBF24 } else { 0xFFF97316 };

            for p in &mut self.particles {
                if !p.is_active {
                    p.is_active = true;
                    p.pos = origin;
                    p.vel = Vec3Fixed::from_raw(vx, vy, vz);
                    p.color = col;
                    p.life = 16;
                    p.max_life = 16;
                    p.size = 2;
                    self.active_count += 1;
                    break;
                }
            }
        }
    }

    /// Advances particle kinematics by 1 discrete tick.
    pub fn step_kinematics(&mut self) {
        let mut active = 0;
        for p in &mut self.particles {
            if !p.is_active {
                continue;
            }

            p.pos += p.vel;

            // Air drag: reduce velocity by 5%
            p.vel.x = Fixed::from_raw((p.vel.x.raw() * 95) / 100);
            p.vel.y = Fixed::from_raw((p.vel.y.raw() * 95) / 100);
            p.vel.z = Fixed::from_raw((p.vel.z.raw() * 95) / 100);

            if p.life > 0 {
                p.life -= 1;
                active += 1;
            } else {
                p.is_active = false;
            }
        }
        self.active_count = active;
    }

    /// Renders all active particles with Reversed-Z depth testing.
    pub fn render(
        &self,
        fb: &mut FrameBuffer,
        depth_buffer: &mut [i32],
        world_mat: &Mat4Fixed,
        cam_dist: Fixed,
        focal_length: i64,
    ) {
        let cx = (fb.width / 2) as i64;
        let cy = (fb.height / 2) as i64;

        for p in &self.particles {
            if !p.is_active {
                continue;
            }

            let p_world = world_mat.transform_point(p.pos);
            let vz = (p_world.z + cam_dist).raw();

            if vz <= 65536 / 10 {
                continue; // Behind near plane
            }

            let sx = cx + (p_world.x.raw() * focal_length) / vz;
            let sy = cy - (p_world.y.raw() * focal_length) / vz;
            let inv_z = ((1i64 << 30) / vz) as i32;

            let rad = p.size as i64;
            let min_px = (sx - rad).max(0).min(fb.width as i64 - 1);
            let max_px = (sx + rad).max(0).min(fb.width as i64 - 1);
            let min_py = (sy - rad).max(0).min(fb.height as i64 - 1);
            let max_py = (sy + rad).max(0).min(fb.height as i64 - 1);

            for y in min_py..=max_py {
                let row_offset = y as usize * fb.width;
                for x in min_px..=max_px {
                    let pixel_idx = row_offset + x as usize;

                    // Reversed-Z test
                    if inv_z > depth_buffer[pixel_idx] {
                        depth_buffer[pixel_idx] = inv_z;
                        fb.pixels[pixel_idx] = p.color;
                    }
                }
            }
        }
    }
}
