//! # Physics-Based Debris Particle Scattering & Collision Impulse (`axiom-core::debris_particle`)
//!
//! T-219: High-Performance 0-Float, 64-Byte L1 Cache Line Aligned Physics-Based
//! Debris Particle System and Collision Impulse Integrator.
//!
//! ## Architectural Guarantees:
//! - **0-Float in Core**: All coordinates in integer millimeters (`mm: i32`), velocities in `mm/s`.
//!   Gravity $g = -9810\text{ mm/s}^2$ discretely integrated at 60 Hz ($-163\text{ mm/s/tick}$).
//!   Angles in 16-bit BAMS (`0..=65535`), angular velocities in BAMS/tick.
//!   Restitution $e$, friction $\mu$, and air drag in $Q16.16$ fixed-point (`65536 = 1.0`).
//! - **64B L1 Cache Line Alignment**: Both [`DebrisParticleSlab64`] and [`ParticleContactSlab64`]
//!   are `#[repr(C, align(64))]` with exact 64-byte size and zero implicit compiler padding.
//! - **Symplectic Semi-Implicit Euler**: Position updated with new velocity for physical energy conservation.
//! - **Restitution Bounce & Surface Friction**: Normal velocity reflection and tangential friction damping.
//! - **0-Heap Flat Array Topology**: Fixed-capacity compile-time generic arrays ([`DebrisParticlePool<MAX_PARTICLES>`]).

use core::mem::{align_of, size_of};
use crate::ragdoll::isqrt_i64;

/// Event type discriminant for debris burst scattering.
pub const EVENT_DEBRIS_BURST: u16 = 0x0270;

/// Standard Earth gravity in integer millimeters per second squared (g = -9810 mm/s^2).
pub const GRAVITY_MMS2: i32 = -9810;

/// Gravitational velocity increment per tick at 60 Hz (-9810 / 60 = -163.5 mm/s -> -163 mm/s).
pub const GRAVITY_PER_TICK_MMS: i32 = -163;

/// Debris particle operational flag bitmasks.
pub const DEBRIS_FLAG_ACTIVE: u8    = 1 << 0;
pub const DEBRIS_FLAG_GROUNDED: u8  = 1 << 1;
pub const DEBRIS_FLAG_SLEEPING: u8  = 1 << 2;
pub const DEBRIS_FLAG_BOUNCED: u8   = 1 << 3;
pub const DEBRIS_FLAG_CRITICAL: u8  = 1 << 4;

/// Debris material classification.
pub const DEBRIS_MAT_DIRT: u8   = 0;
pub const DEBRIS_MAT_STONE: u8  = 1;
pub const DEBRIS_MAT_METAL: u8  = 2;
pub const DEBRIS_MAT_MOLTEN: u8 = 3;
pub const DEBRIS_MAT_SAND: u8   = 4;
pub const DEBRIS_MAT_SPARK: u8  = 5;

/// Contact geometry type discriminant.
pub const CONTACT_TYPE_FLOOR: u8   = 0;
pub const CONTACT_TYPE_TERRAIN: u8 = 1;
pub const CONTACT_TYPE_WALL: u8    = 2;
pub const CONTACT_TYPE_ENTITY: u8  = 3;

/// Default capacity for debris particle pool.
pub const DEFAULT_MAX_DEBRIS_PARTICLES: usize = 256;

/// Exact 64-Byte L1 Cache Line Aligned Debris Particle Slab.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(C, align(64))]
pub struct DebrisParticleSlab64 {
    /// 4B: World X position in integer millimeters
    pub pos_x: i32,
    /// 4B: World Y position in integer millimeters (elevation)
    pub pos_y: i32,
    /// 4B: World Z position in integer millimeters
    pub pos_z: i32,
    /// 4B: Linear velocity X in integer mm/s
    pub vel_x: i32,
    /// 4B: Linear velocity Y in integer mm/s
    pub vel_y: i32,
    /// 4B: Linear velocity Z in integer mm/s
    pub vel_z: i32,
    /// 4B: Particle mass in grams (g)
    pub mass_g: u32,
    /// 4B: Originating entity, crater, or bone ID
    pub emitter_id: u32,
    /// 2B: Yaw orientation in 16-bit BAMS (0..=65535)
    pub yaw_bams: u16,
    /// 2B: Pitch orientation in 16-bit BAMS
    pub pitch_bams: u16,
    /// 2B: Roll orientation in 16-bit BAMS
    pub roll_bams: u16,
    /// 2B: Angular velocity Yaw in BAMS/tick
    pub ang_vel_yaw: i16,
    /// 2B: Angular velocity Pitch in BAMS/tick
    pub ang_vel_pitch: i16,
    /// 2B: Angular velocity Roll in BAMS/tick
    pub ang_vel_roll: i16,
    /// 2B: Coefficient of restitution e in Q16.16 (0..=65535)
    pub restitution_q16: u16,
    /// 2B: Surface friction coefficient mu in Q16.16 (0..=65535)
    pub friction_q16: u16,
    /// 2B: Visual and collision radius in millimeters
    pub size_mm: u16,
    /// 2B: Remaining lifetime in ticks
    pub lifetime_ticks: u16,
    /// 2B: Initial lifetime in ticks
    pub max_lifetime_ticks: u16,
    /// 1B: Material classification (`DEBRIS_MAT_*`)
    pub material: u8,
    /// 1B: Operational flags (`DEBRIS_FLAG_*`)
    pub flags: u8,
    /// 1B: Accumulated ground/obstacle bounce count
    pub bounce_count: u8,
    /// 7B: Explicit alignment padding ensuring exact 64-byte size
    pub _pad: [u8; 7],
}

const _: () = assert!(size_of::<DebrisParticleSlab64>() == 64);
const _: () = assert!(align_of::<DebrisParticleSlab64>() == 64);

impl Default for DebrisParticleSlab64 {
    #[inline(always)]
    fn default() -> Self {
        Self::empty()
    }
}

impl DebrisParticleSlab64 {
    /// Creates a blank inactive particle slab.
    #[inline(always)]
    pub const fn empty() -> Self {
        Self {
            pos_x: 0,
            pos_y: 0,
            pos_z: 0,
            vel_x: 0,
            vel_y: 0,
            vel_z: 0,
            mass_g: 0,
            emitter_id: 0,
            yaw_bams: 0,
            pitch_bams: 0,
            roll_bams: 0,
            ang_vel_yaw: 0,
            ang_vel_pitch: 0,
            ang_vel_roll: 0,
            restitution_q16: 0,
            friction_q16: 0,
            size_mm: 0,
            lifetime_ticks: 0,
            max_lifetime_ticks: 0,
            material: DEBRIS_MAT_DIRT,
            flags: 0,
            bounce_count: 0,
            _pad: [0; 7],
        }
    }

    /// Constructs an active debris particle with given position, velocity, and physical traits.
    pub fn new(
        pos: (i32, i32, i32),
        vel: (i32, i32, i32),
        mass_g: u32,
        emitter_id: u32,
        material: u8,
        size_mm: u16,
        lifetime_ticks: u16,
    ) -> Self {
        let (rest, fric) = match material {
            DEBRIS_MAT_DIRT => (13107u16, 52428u16),    // e = 0.20, mu = 0.80
            DEBRIS_MAT_STONE => (32768u16, 39321u16),   // e = 0.50, mu = 0.60
            DEBRIS_MAT_METAL => (42598u16, 26214u16),   // e = 0.65, mu = 0.40
            DEBRIS_MAT_MOLTEN => (6553u16, 58982u16),   // e = 0.10, mu = 0.90
            DEBRIS_MAT_SPARK => (52428u16, 13107u16),   // e = 0.80, mu = 0.20
            _ => (16384u16, 45875u16),                  // SAND: e = 0.25, mu = 0.70
        };

        Self {
            pos_x: pos.0,
            pos_y: pos.1,
            pos_z: pos.2,
            vel_x: vel.0,
            vel_y: vel.1,
            vel_z: vel.2,
            mass_g,
            emitter_id,
            yaw_bams: 0,
            pitch_bams: 0,
            roll_bams: 0,
            ang_vel_yaw: 1200,
            ang_vel_pitch: -800,
            ang_vel_roll: 600,
            restitution_q16: rest,
            friction_q16: fric,
            size_mm,
            lifetime_ticks,
            max_lifetime_ticks: lifetime_ticks,
            material,
            flags: DEBRIS_FLAG_ACTIVE,
            bounce_count: 0,
            _pad: [0; 7],
        }
    }

    #[inline(always)]
    pub fn is_active(&self) -> bool {
        (self.flags & DEBRIS_FLAG_ACTIVE) != 0 && self.lifetime_ticks > 0
    }

    #[inline(always)]
    pub fn is_sleeping(&self) -> bool {
        (self.flags & DEBRIS_FLAG_SLEEPING) != 0
    }
}

/// Exact 64-Byte L1 Cache Line Aligned Particle Collision Impulse Descriptor.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(C, align(64))]
pub struct ParticleContactSlab64 {
    /// 4B: Contact world X in integer millimeters
    pub contact_x: i32,
    /// 4B: Contact world Y in integer millimeters
    pub contact_y: i32,
    /// 4B: Contact world Z in integer millimeters
    pub contact_z: i32,
    /// 4B: Surface normal X in Q16.16 (-65536..=65536)
    pub normal_x_q16: i32,
    /// 4B: Surface normal Y in Q16.16 (upward default = 65536)
    pub normal_y_q16: i32,
    /// 4B: Surface normal Z in Q16.16
    pub normal_z_q16: i32,
    /// 8B: Impact kinetic energy in Joules (natural 8-byte alignment)
    pub impulse_energy_j: u64,
    /// 4B: Momentum impulse in millinewton-seconds (mN*s = g * mm/s)
    pub impulse_m_ns: u32,
    /// 4B: Particle slot index in pool
    pub particle_id: u32,
    /// 4B: Emitter/source entity ID
    pub emitter_id: u32,
    /// 4B: Simulation tick when impact occurred
    pub tick: u32,
    /// 2B: Rebound separation speed in mm/s
    pub rebound_speed_mms: u16,
    /// 2B: Transient penetration depth in mm
    pub penetration_depth_mm: u16,
    /// 1B: Impact surface material
    pub surface_material: u8,
    /// 1B: Particle material
    pub particle_material: u8,
    /// 1B: Contact event flags
    pub flags: u8,
    /// 1B: Contact geometry type (`CONTACT_TYPE_*`)
    pub contact_type: u8,
    /// 8B: Explicit alignment padding ensuring exact 64-byte size
    pub _pad: [u8; 8],
}

const _: () = assert!(size_of::<ParticleContactSlab64>() == 64);
const _: () = assert!(align_of::<ParticleContactSlab64>() == 64);

impl Default for ParticleContactSlab64 {
    #[inline(always)]
    fn default() -> Self {
        Self::empty()
    }
}

impl ParticleContactSlab64 {
    /// Creates an empty contact slab.
    #[inline(always)]
    pub const fn empty() -> Self {
        Self {
            contact_x: 0,
            contact_y: 0,
            contact_z: 0,
            normal_x_q16: 0,
            normal_y_q16: 65536,
            normal_z_q16: 0,
            impulse_energy_j: 0,
            impulse_m_ns: 0,
            particle_id: 0,
            emitter_id: 0,
            tick: 0,
            rebound_speed_mms: 0,
            penetration_depth_mm: 0,
            surface_material: 0,
            particle_material: 0,
            flags: 0,
            contact_type: CONTACT_TYPE_FLOOR,
            _pad: [0; 8],
        }
    }
}

/// Simple deterministic xorshift64 PRNG generator.
#[inline(always)]
fn next_prng(state: &mut u64) -> u64 {
    let mut x = *state;
    if x == 0 {
        x = 0xDA3E_5A1B_8812_4567;
    }
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    *state = x;
    x
}

/// Zero-Heap Fixed-Capacity Debris Particle Simulation Pool.
pub struct DebrisParticlePool<const MAX_PARTICLES: usize = DEFAULT_MAX_DEBRIS_PARTICLES> {
    /// Flat array of 64B L1 cache-resident particle slabs
    pub particles: [DebrisParticleSlab64; MAX_PARTICLES],
    /// Count of currently active particles
    pub active_count: usize,
    /// Monotonic total particles spawned counter
    pub total_spawned: u64,
    /// Monotonic total ground bounces recorded
    pub total_bounces: u64,
    /// PRNG state for deterministic particle scattering
    pub rng_state: u64,
    /// Arena floor Y elevation in integer millimeters
    pub floor_y_mm: i32,
    /// Arena circular boundary radius in integer millimeters
    pub arena_radius_mm: i32,
    /// Air drag damping factor in Q16.16 (65208 = 0.995)
    pub drag_q16: u16,
    /// Angular drag damping factor in Q16.16 (64225 = 0.980)
    pub ang_drag_q16: u16,
    /// Rest/Sleep linear velocity threshold in mm/s (30 mm/s)
    pub sleep_threshold_mms: u16,
}

impl<const MAX_PARTICLES: usize> Default for DebrisParticlePool<MAX_PARTICLES> {
    fn default() -> Self {
        Self::new(0xCAFE_BABE_1234_5678, 0, 6000)
    }
}

impl<const MAX_PARTICLES: usize> DebrisParticlePool<MAX_PARTICLES> {
    /// Initializes a new debris particle pool.
    pub const fn new(seed: u64, floor_y_mm: i32, arena_radius_mm: i32) -> Self {
        Self {
            particles: [DebrisParticleSlab64::empty(); MAX_PARTICLES],
            active_count: 0,
            total_spawned: 0,
            total_bounces: 0,
            rng_state: if seed == 0 { 0x5EED_0001_DEAD_BEEF } else { seed },
            floor_y_mm,
            arena_radius_mm,
            drag_q16: 65208, // 0.995 in Q16.16
            ang_drag_q16: 64225, // 0.980 in Q16.16
            sleep_threshold_mms: 30,
        }
    }

    /// Spawns a particle slab into the pool, reusing inactive slots or oldest sleeping slot.
    pub fn spawn_particle(&mut self, particle: DebrisParticleSlab64) -> Option<usize> {
        let mut target_slot = None;

        // 1. Search for first inactive slot
        for (i, p) in self.particles.iter().enumerate() {
            if !p.is_active() {
                target_slot = Some(i);
                break;
            }
        }

        // 2. If saturated, steal sleeping or lowest lifetime slot
        let slot = target_slot.unwrap_or_else(|| {
            let mut best_slot = 0;
            let mut min_life = u16::MAX;
            for (i, p) in self.particles.iter().enumerate() {
                if p.is_sleeping() {
                    return i;
                }
                if p.lifetime_ticks < min_life {
                    min_life = p.lifetime_ticks;
                    best_slot = i;
                }
            }
            best_slot
        });

        self.particles[slot] = particle;
        self.particles[slot].flags |= DEBRIS_FLAG_ACTIVE;
        self.particles[slot].flags &= !DEBRIS_FLAG_SLEEPING;
        self.total_spawned += 1;
        Some(slot)
    }

    /// Scatters a burst of debris particles from an impact origin.
    pub fn scatter_debris_burst(
        &mut self,
        center_x: i32,
        center_y: i32,
        center_z: i32,
        count: usize,
        speed_min_mms: i32,
        speed_max_mms: i32,
        material: u8,
        emitter_id: u32,
        energy_joules: u32,
    ) -> usize {
        let spawn_count = count.min(MAX_PARTICLES);
        let speed_range = (speed_max_mms - speed_min_mms).max(1) as u64;
        let mut spawned = 0;

        for _ in 0..spawn_count {
            let r0 = next_prng(&mut self.rng_state);
            let r1 = next_prng(&mut self.rng_state);
            let r2 = next_prng(&mut self.rng_state);

            // Polar coordinates conical distribution
            let speed = speed_min_mms + ((r0 % speed_range) as i32);
            let yaw_bams = (r1 & 0xFFFF) as u16;
            let elevation_bams = 4096 + ((r2 % 12288) as u16); // 22.5 to 90 degrees upward

            // 16-bit BAMS trigonometry approx using integer lookup
            let yaw_idx = ((yaw_bams >> 8) & 0xFF) as usize;
            let cos_y = crate::destruction_audio::SINE_LUT_256[(yaw_idx + 64) % 256] as i64;
            let sin_y = crate::destruction_audio::SINE_LUT_256[yaw_idx] as i64;

            let elev_idx = ((elevation_bams >> 8) & 0xFF) as usize;
            let cos_el = crate::destruction_audio::SINE_LUT_256[(elev_idx + 64) % 256] as i64;
            let sin_el = crate::destruction_audio::SINE_LUT_256[elev_idx] as i64;

            let horiz_speed = (speed as i64 * cos_el) / 32767;
            let vx = ((horiz_speed * cos_y) / 32767) as i32;
            let vz = ((horiz_speed * sin_y) / 32767) as i32;
            let vy = ((speed as i64 * sin_el) / 32767).abs() as i32; // upward bias

            let mass = 50 + ((r0 % 450) as u32); // 50g to 500g
            let size = 20 + ((r1 % 80) as u16);  // 20mm to 100mm
            let life = 120 + ((r2 % 180) as u16); // 2 to 5 seconds

            let particle = DebrisParticleSlab64::new(
                (center_x, center_y, center_z),
                (vx, vy, vz),
                mass,
                emitter_id,
                material,
                size,
                life,
            );

            if self.spawn_particle(particle).is_some() {
                spawned += 1;
            }
        }

        let _ = energy_joules;
        spawned
    }

    /// Advances debris particle physics by 1 simulation tick using Symplectic Semi-Implicit Euler.
    /// Records collision contacts into `contacts_out` up to `MAX_CONTACTS`.
    pub fn step_debris_simulation<const MAX_CONTACTS: usize>(
        &mut self,
        contacts_out: &mut [ParticleContactSlab64; MAX_CONTACTS],
        tick: u32,
    ) -> usize {
        let mut contact_count = 0;
        let mut active_count = 0;

        let drag = self.drag_q16 as i64;
        let ang_drag = self.ang_drag_q16 as i64;
        let floor_y = self.floor_y_mm;
        let arena_r_sq = (self.arena_radius_mm as i64) * (self.arena_radius_mm as i64);
        let sleep_thresh = self.sleep_threshold_mms as i32;

        for (idx, p) in self.particles.iter_mut().enumerate() {
            if !p.is_active() {
                continue;
            }

            // Decrement lifetime
            p.lifetime_ticks = p.lifetime_ticks.saturating_sub(1);
            if p.lifetime_ticks == 0 {
                p.flags = 0;
                continue;
            }

            if p.is_sleeping() {
                active_count += 1;
                continue;
            }

            // 1. Semi-Implicit Euler: apply discrete gravity and air drag
            let mut vx = ((p.vel_x as i64 * drag) >> 16) as i32;
            let mut vy = (((p.vel_y + GRAVITY_PER_TICK_MMS) as i64 * drag) >> 16) as i32;
            let mut vz = ((p.vel_z as i64 * drag) >> 16) as i32;

            // 2. Angular orientation integration and damping
            p.yaw_bams = p.yaw_bams.wrapping_add(p.ang_vel_yaw as u16);
            p.pitch_bams = p.pitch_bams.wrapping_add(p.ang_vel_pitch as u16);
            p.roll_bams = p.roll_bams.wrapping_add(p.ang_vel_roll as u16);
            p.ang_vel_yaw = ((p.ang_vel_yaw as i64 * ang_drag) >> 16) as i16;
            p.ang_vel_pitch = ((p.ang_vel_pitch as i64 * ang_drag) >> 16) as i16;
            p.ang_vel_roll = ((p.ang_vel_roll as i64 * ang_drag) >> 16) as i16;

            // 3. Position update (1 tick = 1/60s)
            let mut px = p.pos_x + vx / 60;
            let mut py = p.pos_y + vy / 60;
            let mut pz = p.pos_z + vz / 60;

            let radius = p.size_mm as i32;

            // 4. Ground Collision Detection and Restitution / Friction Response
            let min_y = floor_y + radius;
            if py <= min_y {
                let penetration = (min_y - py).max(0) as u16;
                py = min_y;

                let vin_sq = (vx as i64) * (vx as i64) + (vy as i64) * (vy as i64) + (vz as i64) * (vz as i64);

                // Normal rebound
                let e = p.restitution_q16 as i64;
                vy = (-((vy as i64 * e) >> 16) as i32).max(0);

                // Tangential friction damping
                let mu = p.friction_q16 as i64;
                vx = ((vx as i64 * mu) >> 16) as i32;
                vz = ((vz as i64 * mu) >> 16) as i32;

                let vout_sq = (vx as i64) * (vx as i64) + (vy as i64) * (vy as i64) + (vz as i64) * (vz as i64);
                let loss_sq = (vin_sq - vout_sq).max(0);
                let impulse_energy = ((loss_sq as u64) * (p.mass_g as u64)) / 2_000_000;

                p.bounce_count = p.bounce_count.saturating_add(1);
                p.flags |= DEBRIS_FLAG_GROUNDED | DEBRIS_FLAG_BOUNCED;
                self.total_bounces += 1;

                // Check sleep transition
                if vx.abs() < sleep_thresh && vy.abs() < sleep_thresh && vz.abs() < sleep_thresh {
                    p.flags |= DEBRIS_FLAG_SLEEPING;
                    vx = 0;
                    vy = 0;
                    vz = 0;
                }

                // Record contact descriptor
                if contact_count < MAX_CONTACTS {
                    contacts_out[contact_count] = ParticleContactSlab64 {
                        contact_x: px,
                        contact_y: py - radius,
                        contact_z: pz,
                        normal_x_q16: 0,
                        normal_y_q16: 65536, // +Y upward normal
                        normal_z_q16: 0,
                        impulse_energy_j: impulse_energy,
                        impulse_m_ns: (isqrt_i64(vin_sq) as u32) * p.mass_g / 1000,
                        particle_id: idx as u32,
                        emitter_id: p.emitter_id,
                        tick,
                        rebound_speed_mms: isqrt_i64(vout_sq).min(u16::MAX as i64) as u16,
                        penetration_depth_mm: penetration,
                        surface_material: DEBRIS_MAT_STONE,
                        particle_material: p.material,
                        flags: p.flags,
                        contact_type: CONTACT_TYPE_FLOOR,
                        _pad: [0; 8],
                    };
                    contact_count += 1;
                }
            } else {
                p.flags &= !DEBRIS_FLAG_GROUNDED;
            }

            // 5. Arena circular boundary bounce
            let dist_horiz_sq = (px as i64) * (px as i64) + (pz as i64) * (pz as i64);
            if dist_horiz_sq > arena_r_sq {
                let dist_h = isqrt_i64(dist_horiz_sq).max(1);
                let norm_x = ((px as i64 * 65536) / dist_h) as i32;
                let norm_z = ((pz as i64 * 65536) / dist_h) as i32;

                // Clamp to perimeter
                px = ((norm_x as i64 * (self.arena_radius_mm as i64)) >> 16) as i32;
                pz = ((norm_z as i64 * (self.arena_radius_mm as i64)) >> 16) as i32;

                // Reflect horizontal velocity
                let dot = ((vx as i64 * norm_x as i64 + vz as i64 * norm_z as i64) >> 16) as i32;
                if dot > 0 {
                    vx -= ((2 * dot as i64 * norm_x as i64) >> 16) as i32;
                    vz -= ((2 * dot as i64 * norm_z as i64) >> 16) as i32;
                    vx = (vx * 3) / 4; // slight energy loss on boundary ropes
                    vz = (vz * 3) / 4;
                }
            }

            p.pos_x = px;
            p.pos_y = py;
            p.pos_z = pz;
            p.vel_x = vx;
            p.vel_y = vy;
            p.vel_z = vz;

            active_count += 1;
        }

        self.active_count = active_count;
        contact_count
    }

    /// Computes guaranteed bit-exact 64-bit FNV-1a checksum of active particles and pool state.
    pub fn compute_consensus_hash(&self) -> u64 {
        const FNV_OFFSET: u64 = 0xcbf29ce484222325;
        const FNV_PRIME: u64 = 0x100000001b3;

        let mut h = FNV_OFFSET ^ self.total_spawned;

        for p in &self.particles {
            if p.is_active() {
                let bytes = [
                    p.pos_x.to_le_bytes(),
                    p.pos_y.to_le_bytes(),
                    p.pos_z.to_le_bytes(),
                    p.vel_x.to_le_bytes(),
                    p.vel_y.to_le_bytes(),
                    p.vel_z.to_le_bytes(),
                ];
                for chunk in &bytes {
                    for &b in chunk {
                        h ^= b as u64;
                        h = h.wrapping_mul(FNV_PRIME);
                    }
                }
                h ^= p.lifetime_ticks as u64;
                h = h.wrapping_mul(FNV_PRIME);
                h ^= p.flags as u64;
                h = h.wrapping_mul(FNV_PRIME);
            }
        }

        h ^= self.total_bounces;
        h = h.wrapping_mul(FNV_PRIME);
        h
    }
}
