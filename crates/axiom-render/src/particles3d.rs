//! 3D Procedural Particle System for Steam Vents and Kinetic Metal Sparks.
//!
//! Provides 64-byte aligned, zero-allocation `ParticleSystem3D` with Z-buffer depth testing.

use crate::framebuffer::FrameBuffer;
use crate::math3d::{Mat4Fixed, Vec3Fixed};
use axiom_core::Fixed;

/// Classification of 3D particle kinematics and visual behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum ParticleKind {
    #[default]
    Steam = 0,
    Spark = 1,
    Debris = 2,
}

/// Individual 3D particle state (32 bytes).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(C)]
pub struct Particle3D {
    pub pos: Vec3Fixed,
    pub vel: Vec3Fixed,
    pub color: u32,
    pub life: u16,
    pub max_life: u16,
    pub kind: ParticleKind,
    pub size: u8,
    pub restitution: u8,
    pub is_active: bool,
}

/// 64-byte aligned, fixed-capacity 3D Particle System.
#[repr(C, align(64))]
pub struct ParticleSystem3D<const CAP: usize = 128> {
    pub particles: [Particle3D; CAP],
    pub active_count: usize,
    pub rng_state: u64,
    pub floor_y: Fixed,
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
            floor_y: Fixed::from_raw(-42598), // Default canvas floor ~ -0.65m
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
            let vy = 1200 + ((r >> 8) & 0xFF) as i64 * 12; // upward thermal lift
            let vz = (((r >> 16) & 0xFF) as i64 - 128) * 12;

            for p in &mut self.particles {
                if !p.is_active {
                    p.is_active = true;
                    p.pos = origin;
                    p.vel = Vec3Fixed::from_raw(vx, vy, vz);
                    p.color = 0xAAECEFF4; // Translucent coolant steam
                    p.kind = ParticleKind::Steam;
                    p.life = 28;
                    p.max_life = 28;
                    p.size = 2;
                    p.restitution = 0;
                    self.active_count += 1;
                    break;
                }
            }
        }
    }

    /// Spawns high-velocity kinetic sparks on punch contact with ground bounce.
    pub fn spawn_sparks(&mut self, origin: Vec3Fixed, count: usize) {
        for _ in 0..count {
            if self.active_count >= CAP {
                break;
            }

            let r = self.next_rng();
            let vx = (((r & 0xFF) as i64) - 128) * 55;
            let vy = (((r >> 8) & 0xFF) as i64 - 40) * 45; // outward/upward spray
            let vz = (((r >> 16) & 0xFF) as i64 - 128) * 55;

            for p in &mut self.particles {
                if !p.is_active {
                    p.is_active = true;
                    p.pos = origin;
                    p.vel = Vec3Fixed::from_raw(vx, vy, vz);
                    p.color = 0xFFFBBF24; // Incandescent Gold
                    p.kind = ParticleKind::Spark;
                    p.life = 22;
                    p.max_life = 22;
                    p.size = 2;
                    p.restitution = 65; // 65% elastic bounce off canvas floor
                    self.active_count += 1;
                    break;
                }
            }
        }
    }

    /// Spawns jagged armor metal debris chunks on heavy punch impact.
    pub fn spawn_debris(&mut self, origin: Vec3Fixed, count: usize, color: u32) {
        for _ in 0..count {
            if self.active_count >= CAP {
                break;
            }

            let r = self.next_rng();
            let vx = (((r & 0xFF) as i64) - 128) * 35;
            let vy = (((r >> 8) & 0xFF) as i64 + 40) * 30; // upward trajectory arc
            let vz = (((r >> 16) & 0xFF) as i64 - 128) * 35;

            for p in &mut self.particles {
                if !p.is_active {
                    p.is_active = true;
                    p.pos = origin;
                    p.vel = Vec3Fixed::from_raw(vx, vy, vz);
                    p.color = color;
                    p.kind = ParticleKind::Debris;
                    p.life = 26;
                    p.max_life = 26;
                    p.size = 3;
                    p.restitution = 35; // 35% bounce
                    self.active_count += 1;
                    break;
                }
            }
        }
    }

    /// Advances particle kinematics using the default canvas floor height.
    pub fn step_kinematics(&mut self) {
        self.step_kinematics_with_floor(self.floor_y);
    }

    /// Advances particle kinematics with gravity, floor bounce, air drag, and cooling decay.
    pub fn step_kinematics_with_floor(&mut self, floor_y: Fixed) {
        let mut active = 0;
        let floor_raw = floor_y.raw();

        for p in &mut self.particles {
            if !p.is_active {
                continue;
            }

            p.pos += p.vel;

            match p.kind {
                ParticleKind::Steam => {
                    // Upward thermal lift + air resistance drag
                    p.vel.y = Fixed::from_raw(p.vel.y.raw() + 30);
                    p.vel.x = Fixed::from_raw((p.vel.x.raw() * 92) / 100);
                    p.vel.z = Fixed::from_raw((p.vel.z.raw() * 92) / 100);

                    // Expand size as steam cools
                    if p.life < p.max_life / 2 && p.size < 5 {
                        p.size = (p.size + 1).min(5);
                    }
                }
                ParticleKind::Spark => {
                    // Gravity acceleration down
                    p.vel.y = Fixed::from_raw(p.vel.y.raw() - 140);
                    // Light air drag
                    p.vel.x = Fixed::from_raw((p.vel.x.raw() * 98) / 100);
                    p.vel.z = Fixed::from_raw((p.vel.z.raw() * 98) / 100);

                    // Ground elastic bounce
                    if p.pos.y.raw() <= floor_raw {
                        p.pos.y = Fixed::from_raw(floor_raw);
                        if p.vel.y.raw() < 0 {
                            p.vel.y = Fixed::from_raw((-p.vel.y.raw() * p.restitution as i64) / 100);
                            // Surface friction on bounce
                            p.vel.x = Fixed::from_raw((p.vel.x.raw() * 75) / 100);
                            p.vel.z = Fixed::from_raw((p.vel.z.raw() * 75) / 100);
                        }
                    }

                    // Spark incandescent color cooling: Gold -> Orange -> Ember Red
                    let progress = (p.life as u32 * 100) / p.max_life as u32;
                    p.color = if progress > 60 {
                        0xFFFBBF24 // Brilliant Gold
                    } else if progress > 25 {
                        0xFFF97316 // Incandescent Orange
                    } else {
                        0xFFDC2626 // Dying Red Ember
                    };
                }
                ParticleKind::Debris => {
                    // Heavy gravity acceleration down
                    p.vel.y = Fixed::from_raw(p.vel.y.raw() - 220);
                    // Ground inelastic collision & friction
                    if p.pos.y.raw() <= floor_raw {
                        p.pos.y = Fixed::from_raw(floor_raw);
                        if p.vel.y.raw() < 0 {
                            p.vel.y = Fixed::from_raw((-p.vel.y.raw() * p.restitution as i64) / 100);
                            p.vel.x = Fixed::from_raw((p.vel.x.raw() * 60) / 100);
                            p.vel.z = Fixed::from_raw((p.vel.z.raw() * 60) / 100);
                        }
                    }
                }
            }

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
            let vz = (cam_dist - p_world.z).raw();

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
