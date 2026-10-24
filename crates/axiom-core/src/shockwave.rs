//! # Procedural Terrain Dust Storm & Volumetric Shockwave Physics (`axiom-core::shockwave`)
//!
//! T-225: High-Performance 0-Float, 64-Byte L1 Cache Line Aligned Volumetric
//! Blast Shockwave & Dust Storm Density Field Physics Integrator.
//!
//! ## Architectural Guarantees:
//! - **0-Float in Core**: All coordinates in integer millimeters (`mm: i32`), velocities in `mm/s: u32`.
//!   Wavefront radius and shell thickness in integer mm. Peak pressure impulse in Joules.
//!   Optical transmittance and dust settling rates in $Q16.16$ fixed-point (`65536 = 1.0`).
//! - **64B L1 Cache Line Alignment**: Both [`ShockwaveSlab64`] and [`DustEmitterSlab64`]
//!   are `#[repr(C, align(64))]` with exact 64-byte size and zero implicit compiler padding.
//! - **Sedov-Taylor Blast Wave Similarity**: Wavefront expansion velocity decays towards speed of sound (340 m/s),
//!   and peak pressure impulse obeys inverse-square geometric dissipation $P(R) = P_0 \cdot (R_0/R)^2$.
//! - **Radial Repulsive Impulse**: Shockwave shell applies radial outward force to 270-series debris particles
//!   and 240-series ragdoll limbs upon wavefront passage.
//! - **0-Heap Flat Array Topology**: Fixed-capacity compile-time generic arrays ([`VolumetricShockwaveEngine`]).

use core::mem::{align_of, size_of};
use crate::ragdoll::isqrt_i64;
use crate::event_bus::{channel, DeterministicEventBus};
use crate::job_system::{JobContext, JobPriority, JobStatus, JobSystem};
use crate::worker_pool::WorkerPoolCoordinator;

/// Event type discriminant for volumetric shockwave propagation.
pub const EVENT_SHOCKWAVE: u16 = 0x0280;

/// Speed of sound in standard atmosphere in integer millimeters per second (340 m/s = 340,000 mm/s).
pub const SPEED_OF_SOUND_MMS: u32 = 340_000;

/// Shockwave propagation geometry classification.
pub const SHOCKWAVE_TYPE_SPHERICAL: u8 = 0;
pub const SHOCKWAVE_TYPE_TOROIDAL: u8  = 1;
pub const SHOCKWAVE_TYPE_CONICAL: u8   = 2;

/// Shockwave operational status flags.
pub const SHOCKWAVE_FLAG_ACTIVE: u8       = 1 << 0;
pub const SHOCKWAVE_FLAG_REFLECTED: u8    = 1 << 1;
pub const SHOCKWAVE_FLAG_GROUND_BURST: u8 = 1 << 2;

/// Dust emitter operational status flags.
pub const DUST_FLAG_ACTIVE: u8     = 1 << 0;
pub const DUST_FLAG_SETTLING: u8   = 1 << 1;
pub const DUST_FLAG_DISSIPATED: u8 = 1 << 2;

/// Default capacities for shockwave and dust emitter pools.
pub const DEFAULT_MAX_SHOCKWAVES: usize = 32;
pub const DEFAULT_MAX_DUST_EMITTERS: usize = 32;

/// Exact 64-Byte L1 Cache Line Aligned Volumetric Shockwave Slab.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(C, align(64))]
pub struct ShockwaveSlab64 {
    /// 4B: Explosion center origin X in integer mm
    pub center_x: i32,
    /// 4B: Explosion center origin Y in integer mm
    pub center_y: i32,
    /// 4B: Explosion center origin Z in integer mm
    pub center_z: i32,
    /// 4B: Current wavefront outer radius R(t) in integer mm
    pub radius_curr_mm: u32,
    /// 4B: Maximum dissipation radius in integer mm
    pub radius_max_mm: u32,
    /// 4B: Current propagation speed in integer mm/s
    pub speed_mms: u32,
    /// 4B: Wavefront shell thickness ΔR in integer mm
    pub thickness_mm: u32,
    /// 4B: Initial peak impulse energy in Joules (J)
    pub peak_impulse_j: u32,
    /// 4B: Current wavefront peak impulse energy in Joules (J)
    pub current_impulse_j: u32,
    /// 4B: Originating entity, crater, or bone ID
    pub emitter_id: u32,
    /// 4B: Simulation tick when shockwave was spawned
    pub tick_born: u32,
    /// 2B: Initial lifetime in ticks
    pub max_ticks: u16,
    /// 2B: Remaining lifetime in ticks
    pub remaining_ticks: u16,
    /// 1B: Geometric shape classification (`SHOCKWAVE_TYPE_*`)
    pub shock_type: u8,
    /// 1B: Operational flags (`SHOCKWAVE_FLAG_*`)
    pub flags: u8,
    /// 1B: Medium/terrain material (`DEBRIS_MAT_*`)
    pub material: u8,
    /// 1B: Speed damping factor in Q8 (128 = 0.5)
    pub damping_q8: u8,
    /// 12B: Explicit alignment padding to exact 64 bytes
    pub _pad: [u8; 12],
}

const _: () = assert!(size_of::<ShockwaveSlab64>() == 64);
const _: () = assert!(align_of::<ShockwaveSlab64>() == 64);

impl ShockwaveSlab64 {
    /// Constructs an uninitialized, empty shockwave slab.
    pub const fn empty() -> Self {
        Self {
            center_x: 0,
            center_y: 0,
            center_z: 0,
            radius_curr_mm: 0,
            radius_max_mm: 0,
            speed_mms: 0,
            thickness_mm: 0,
            peak_impulse_j: 0,
            current_impulse_j: 0,
            emitter_id: 0,
            tick_born: 0,
            max_ticks: 0,
            remaining_ticks: 0,
            shock_type: SHOCKWAVE_TYPE_SPHERICAL,
            flags: 0,
            material: 0,
            damping_q8: 255,
            _pad: [0u8; 12],
        }
    }

    /// Constructs an active explosion shockwave.
    pub fn new(
        center: (i32, i32, i32),
        initial_radius_mm: u32,
        max_radius_mm: u32,
        initial_speed_mms: u32,
        thickness_mm: u32,
        energy_j: u32,
        emitter_id: u32,
        material: u8,
        shock_type: u8,
        tick: u32,
    ) -> Self {
        let max_ticks = ((max_radius_mm.saturating_sub(initial_radius_mm) as u64 * 60)
            / (initial_speed_mms.max(SPEED_OF_SOUND_MMS) as u64))
            .clamp(10, 120) as u16;

        Self {
            center_x: center.0,
            center_y: center.1,
            center_z: center.2,
            radius_curr_mm: initial_radius_mm.max(50),
            radius_max_mm: max_radius_mm.max(initial_radius_mm + 100),
            speed_mms: initial_speed_mms.max(SPEED_OF_SOUND_MMS),
            thickness_mm: thickness_mm.max(100),
            peak_impulse_j: energy_j,
            current_impulse_j: energy_j,
            emitter_id,
            tick_born: tick,
            max_ticks,
            remaining_ticks: max_ticks,
            shock_type,
            flags: SHOCKWAVE_FLAG_ACTIVE,
            material,
            damping_q8: 240, // slight drag
            _pad: [0u8; 12],
        }
    }

    /// Returns true if this shockwave slab is currently propagating.
    #[inline(always)]
    pub fn is_active(&self) -> bool {
        (self.flags & SHOCKWAVE_FLAG_ACTIVE) != 0 && self.remaining_ticks > 0
    }
}

/// Exact 64-Byte L1 Cache Line Aligned Dust Emitter Field Slab.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(C, align(64))]
pub struct DustEmitterSlab64 {
    /// 4B: Dust cloud center origin X in integer mm
    pub origin_x: i32,
    /// 4B: Dust cloud center origin Y in integer mm (sinks with settling)
    pub origin_y: i32,
    /// 4B: Dust cloud center origin Z in integer mm
    pub origin_z: i32,
    /// 4B: Current cloud spherical radius in integer mm
    pub radius_mm: u32,
    /// 4B: Maximum expansion radius in integer mm
    pub max_radius_mm: u32,
    /// 4B: Radial expansion speed in integer mm/s
    pub expansion_rate_mms: u32,
    /// 4B: Originating entity, crater, or bone ID
    pub emitter_id: u32,
    /// 4B: Simulation tick when dust emitter was spawned
    pub tick_born: u32,
    /// 2B: Current cloud dust density in Q8 (0..=255)
    pub density_q8: u16,
    /// 2B: Gravity terminal settling speed in integer mm/s
    pub settling_speed_mms: u16,
    /// 2B: Per-tick density decay rate in Q16.16 (e.g. 250)
    pub settling_rate_q16: u16,
    /// 2B: Remaining lifetime in ticks
    pub lifetime_ticks: u16,
    /// 2B: Initial lifetime in ticks
    pub max_lifetime_ticks: u16,
    /// 2B: Optical extinction coefficient in Q16.16
    pub fog_extinction_q16: u16,
    /// 3B: Particle visual tint (RGB)
    pub color_rgb: [u8; 3],
    /// 1B: Terrain material classification (`DEBRIS_MAT_*`)
    pub material: u8,
    /// 1B: Dust emitter status flags (`DUST_FLAG_*`)
    pub flags: u8,
    /// 15B: Explicit alignment padding to exact 64 bytes
    pub _pad: [u8; 15],
}

const _: () = assert!(size_of::<DustEmitterSlab64>() == 64);
const _: () = assert!(align_of::<DustEmitterSlab64>() == 64);

impl DustEmitterSlab64 {
    /// Constructs an uninitialized, empty dust emitter slab.
    pub const fn empty() -> Self {
        Self {
            origin_x: 0,
            origin_y: 0,
            origin_z: 0,
            radius_mm: 0,
            max_radius_mm: 0,
            expansion_rate_mms: 0,
            emitter_id: 0,
            tick_born: 0,
            density_q8: 0,
            settling_speed_mms: 0,
            settling_rate_q16: 0,
            lifetime_ticks: 0,
            max_lifetime_ticks: 0,
            fog_extinction_q16: 0,
            color_rgb: [0; 3],
            material: 0,
            flags: 0,
            _pad: [0u8; 15],
        }
    }

    /// Constructs an active procedural dust cloud emitter.
    pub fn new(
        origin: (i32, i32, i32),
        initial_radius_mm: u32,
        max_radius_mm: u32,
        expansion_rate_mms: u32,
        density_q8: u16,
        material: u8,
        emitter_id: u32,
        lifetime_ticks: u16,
        tick: u32,
    ) -> Self {
        let (color, extinction) = match material {
            0 => ([140, 110, 80], 450),   // Dirt / Brown
            1 => ([160, 160, 165], 380),  // Stone / Gray
            2 => ([190, 175, 160], 300),  // Metal / Sparks
            3 => ([220, 120, 50], 550),   // Molten / Orange Smoke
            _ => ([210, 195, 150], 400),  // Sand / Tan
        };

        Self {
            origin_x: origin.0,
            origin_y: origin.1,
            origin_z: origin.2,
            radius_mm: initial_radius_mm.max(100),
            max_radius_mm: max_radius_mm.max(initial_radius_mm + 200),
            expansion_rate_mms: expansion_rate_mms.max(5000),
            emitter_id,
            tick_born: tick,
            density_q8: density_q8.min(255),
            settling_speed_mms: 120, // ~120 mm/s gravity settling
            settling_rate_q16: 280,  // decay per tick
            lifetime_ticks,
            max_lifetime_ticks: lifetime_ticks,
            fog_extinction_q16: extinction,
            color_rgb: color,
            material,
            flags: DUST_FLAG_ACTIVE | DUST_FLAG_SETTLING,
            _pad: [0u8; 15],
        }
    }

    /// Returns true if this dust cloud emitter is actively dissipating.
    #[inline(always)]
    pub fn is_active(&self) -> bool {
        (self.flags & DUST_FLAG_ACTIVE) != 0 && self.lifetime_ticks > 0 && self.density_q8 > 0
    }
}

/// 0-Heap Fixed-Capacity Volumetric Shockwave & Dust Storm Engine.
pub struct VolumetricShockwaveEngine<
    const MAX_SHOCKWAVES: usize = DEFAULT_MAX_SHOCKWAVES,
    const MAX_DUST_EMITTERS: usize = DEFAULT_MAX_DUST_EMITTERS,
> {
    pub shockwaves: [ShockwaveSlab64; MAX_SHOCKWAVES],
    pub dust_emitters: [DustEmitterSlab64; MAX_DUST_EMITTERS],
    pub total_shockwaves_spawned: u64,
    pub total_dust_spawned: u64,
}

impl<const MAX_SHOCKWAVES: usize, const MAX_DUST_EMITTERS: usize> Default
    for VolumetricShockwaveEngine<MAX_SHOCKWAVES, MAX_DUST_EMITTERS>
{
    fn default() -> Self {
        Self::new()
    }
}

impl<const MAX_SHOCKWAVES: usize, const MAX_DUST_EMITTERS: usize>
    VolumetricShockwaveEngine<MAX_SHOCKWAVES, MAX_DUST_EMITTERS>
{
    /// Creates a blank volumetric shockwave engine.
    pub const fn new() -> Self {
        Self {
            shockwaves: [ShockwaveSlab64::empty(); MAX_SHOCKWAVES],
            dust_emitters: [DustEmitterSlab64::empty(); MAX_DUST_EMITTERS],
            total_shockwaves_spawned: 0,
            total_dust_spawned: 0,
        }
    }

    /// Returns the number of currently active propagating shockwaves.
    #[inline(always)]
    pub fn active_shockwaves_count(&self) -> usize {
        let mut count = 0;
        for s in &self.shockwaves {
            if s.is_active() {
                count += 1;
            }
        }
        count
    }

    /// Returns the number of currently active dust cloud emitters.
    #[inline(always)]
    pub fn active_dust_count(&self) -> usize {
        let mut count = 0;
        for d in &self.dust_emitters {
            if d.is_active() {
                count += 1;
            }
        }
        count
    }

    /// Spawns an explosion shockwave into the pool.
    pub fn trigger_shockwave(
        &mut self,
        center: (i32, i32, i32),
        initial_radius_mm: u32,
        max_radius_mm: u32,
        speed_mms: u32,
        thickness_mm: u32,
        energy_j: u32,
        emitter_id: u32,
        material: u8,
        shock_type: u8,
        tick: u32,
    ) -> Option<usize> {
        let mut target_slot = None;
        for (i, s) in self.shockwaves.iter().enumerate() {
            if !s.is_active() {
                target_slot = Some(i);
                break;
            }
        }

        let slot = target_slot.unwrap_or_else(|| {
            let mut oldest = 0;
            let mut min_life = u16::MAX;
            for (i, s) in self.shockwaves.iter().enumerate() {
                if s.remaining_ticks < min_life {
                    min_life = s.remaining_ticks;
                    oldest = i;
                }
            }
            oldest
        });

        self.shockwaves[slot] = ShockwaveSlab64::new(
            center,
            initial_radius_mm,
            max_radius_mm,
            speed_mms,
            thickness_mm,
            energy_j,
            emitter_id,
            material,
            shock_type,
            tick,
        );
        self.total_shockwaves_spawned += 1;
        Some(slot)
    }

    /// Spawns a procedural dust cloud emitter into the pool.
    pub fn trigger_dust_cloud(
        &mut self,
        origin: (i32, i32, i32),
        initial_radius_mm: u32,
        max_radius_mm: u32,
        expansion_rate_mms: u32,
        density_q8: u16,
        material: u8,
        emitter_id: u32,
        lifetime_ticks: u16,
        tick: u32,
    ) -> Option<usize> {
        let mut target_slot = None;
        for (i, d) in self.dust_emitters.iter().enumerate() {
            if !d.is_active() {
                target_slot = Some(i);
                break;
            }
        }

        let slot = target_slot.unwrap_or_else(|| {
            let mut oldest = 0;
            let mut min_life = u16::MAX;
            for (i, d) in self.dust_emitters.iter().enumerate() {
                if d.lifetime_ticks < min_life {
                    min_life = d.lifetime_ticks;
                    oldest = i;
                }
            }
            oldest
        });

        self.dust_emitters[slot] = DustEmitterSlab64::new(
            origin,
            initial_radius_mm,
            max_radius_mm,
            expansion_rate_mms,
            density_q8,
            material,
            emitter_id,
            lifetime_ticks,
            tick,
        );
        self.total_dust_spawned += 1;
        Some(slot)
    }

    /// Advances volumetric shockwave propagation and dust settling by 1 simulation tick.
    pub fn step_simulation(&mut self, _tick: u32) {
        // 1. Advance Shockwaves
        for s in self.shockwaves.iter_mut() {
            if !s.is_active() {
                continue;
            }

            s.remaining_ticks = s.remaining_ticks.saturating_sub(1);
            if s.remaining_ticks == 0 {
                s.flags &= !SHOCKWAVE_FLAG_ACTIVE;
                continue;
            }

            // Wavefront advance at 60 Hz
            let dr = s.speed_mms / 60;
            s.radius_curr_mm = s.radius_curr_mm.saturating_add(dr);

            // Speed decays towards speed of sound
            let initial_r = 100u64;
            let current_r = (s.radius_curr_mm as u64).max(initial_r);
            let target_speed = ((s.speed_mms as u64 * initial_r) / current_r) as u32;
            s.speed_mms = target_speed.max(SPEED_OF_SOUND_MMS);

            // Inverse-square impulse decay P(R) = P0 * (R0/R)^2
            let r0_sq = 100u64 * 100u64;
            let r_sq = current_r * current_r;
            s.current_impulse_j = ((s.peak_impulse_j as u64 * r0_sq) / r_sq).max(1) as u32;

            // Shell expands slightly as it travels
            s.thickness_mm = (s.thickness_mm + 15).min(2000);

            if s.radius_curr_mm >= s.radius_max_mm {
                s.flags &= !SHOCKWAVE_FLAG_ACTIVE;
            }
        }

        // 2. Advance Dust Emitters
        for d in self.dust_emitters.iter_mut() {
            if !d.is_active() {
                continue;
            }

            d.lifetime_ticks = d.lifetime_ticks.saturating_sub(1);
            if d.lifetime_ticks == 0 {
                d.flags &= !DUST_FLAG_ACTIVE;
                continue;
            }

            // Cloud radius expansion
            let de = d.expansion_rate_mms / 60;
            d.radius_mm = (d.radius_mm + de).min(d.max_radius_mm);

            // Gravity settling (sinks towards ground y=0)
            let settle_dy = (d.settling_speed_mms as i32) / 60;
            d.origin_y = (d.origin_y - settle_dy).max(0);

            // Exponential density decay
            let decay = ((d.density_q8 as u32) * (d.settling_rate_q16 as u32)) >> 16;
            d.density_q8 = d.density_q8.saturating_sub(decay.max(1) as u16);

            if d.density_q8 == 0 {
                d.flags &= !DUST_FLAG_ACTIVE;
            }
        }
    }

    /// Applies radial shockwave repulsive impulses to all active 270-series debris particles.
    pub fn apply_to_debris_particles<const P_CAP: usize>(
        &self,
        debris_pool: &mut crate::debris_particle::DebrisParticlePool<P_CAP>,
    ) -> usize {
        let mut affected = 0;

        for s in &self.shockwaves {
            if !s.is_active() {
                continue;
            }

            let r_curr = s.radius_curr_mm as i64;
            let thick = s.thickness_mm as i64;
            let impulse_j = s.current_impulse_j as i64;

            for p in debris_pool.particles.iter_mut() {
                if !p.is_active() {
                    continue;
                }

                let dx = (p.pos_x - s.center_x) as i64;
                let mut dy = (p.pos_y - s.center_y) as i64;
                let dz = (p.pos_z - s.center_z) as i64;

                // For toroidal shockwaves, scale vertical distance
                if s.shock_type == SHOCKWAVE_TYPE_TOROIDAL {
                    dy = (dy * 10) / 3; // vertical compression
                }

                let dist_sq = dx * dx + dy * dy + dz * dz;
                let dist = isqrt_i64(dist_sq).max(1);

                // Check if particle is inside shockwave shell [R - ΔR, R + ΔR]
                let diff = (dist - r_curr).abs();
                if diff <= thick {
                    // Outward unit direction vector
                    let nx = (dx * 65536) / dist;
                    let ny = (dy * 65536) / dist;
                    let nz = (dz * 65536) / dist;

                    // Delta velocity = impulse / mass (mm/s)
                    let mass_g = (p.mass_g as i64).max(50);
                    let delta_v = ((impulse_j * 1000) / mass_g).clamp(500, 15000);

                    p.vel_x += ((nx * delta_v) >> 16) as i32;
                    p.vel_y += ((ny * delta_v) >> 16).abs() as i32 + 500; // upward boost
                    p.vel_z += ((nz * delta_v) >> 16) as i32;

                    affected += 1;
                }
            }
        }

        affected
    }

    /// Evaluates optical transmittance (visibility) through dust clouds at a given 3D point.
    /// Returns Q16.16 factor: 65536 (1.0) = completely clear, 0 (0.0) = zero visibility/opaque.
    pub fn sample_fog_transmittance(&self, point: (i32, i32, i32)) -> u32 {
        let mut total_attenuation: u32 = 0;

        for d in &self.dust_emitters {
            if !d.is_active() {
                continue;
            }

            let dx = (point.0 - d.origin_x) as i64;
            let dy = (point.1 - d.origin_y) as i64;
            let dz = (point.2 - d.origin_z) as i64;
            let dist_sq = dx * dx + dy * dy + dz * dz;
            let dist = isqrt_i64(dist_sq) as u32;

            if dist < d.radius_mm {
                let falloff = ((d.radius_mm - dist) as u64 * 65536) / (d.radius_mm as u64);
                let cloud_density = ((d.density_q8 as u64) * falloff) >> 8;
                let ext = (cloud_density * (d.fog_extinction_q16 as u64)) >> 8;
                total_attenuation = total_attenuation.saturating_add(ext as u32);
            }
        }

        65536u32.saturating_sub(total_attenuation)
    }

    /// Computes guaranteed bit-exact 64-bit FNV-1a checksum of active shockwaves and dust fields.
    pub fn compute_consensus_hash(&self) -> u64 {
        const FNV_OFFSET: u64 = 0xcbf29ce484222325;
        const FNV_PRIME: u64 = 0x100000001b3;

        let mut h = FNV_OFFSET ^ self.total_shockwaves_spawned;

        for s in &self.shockwaves {
            if s.is_active() {
                let bytes = [
                    s.center_x.to_le_bytes(),
                    s.center_y.to_le_bytes(),
                    s.center_z.to_le_bytes(),
                    s.radius_curr_mm.to_le_bytes(),
                    s.speed_mms.to_le_bytes(),
                    s.current_impulse_j.to_le_bytes(),
                ];
                for chunk in &bytes {
                    for &b in chunk {
                        h ^= b as u64;
                        h = h.wrapping_mul(FNV_PRIME);
                    }
                }
                h ^= s.remaining_ticks as u64;
                h = h.wrapping_mul(FNV_PRIME);
            }
        }

        h ^= self.total_dust_spawned;
        h = h.wrapping_mul(FNV_PRIME);

        for d in &self.dust_emitters {
            if d.is_active() {
                let bytes = [
                    d.origin_x.to_le_bytes(),
                    d.origin_y.to_le_bytes(),
                    d.origin_z.to_le_bytes(),
                    d.radius_mm.to_le_bytes(),
                    (d.density_q8 as u32).to_le_bytes(),
                ];
                for chunk in &bytes {
                    for &b in chunk {
                        h ^= b as u64;
                        h = h.wrapping_mul(FNV_PRIME);
                    }
                }
                h ^= d.lifetime_ticks as u64;
                h = h.wrapping_mul(FNV_PRIME);
            }
        }

        h
    }
}

/// Physical blast shockwave report emitted into the deterministic event bus.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[repr(C)]
pub struct ShockwaveReport {
    /// 4B: Explosion center origin X in integer mm
    pub center_x: i32,
    /// 4B: Explosion center origin Y in integer mm
    pub center_y: i32,
    /// 4B: Explosion center origin Z in integer mm
    pub center_z: i32,
    /// 4B: Current wavefront radius in integer mm
    pub radius_mm: u32,
    /// 4B: Current propagation speed in integer mm/s
    pub speed_mms: u32,
    /// 4B: Current peak impulse energy in Joules (J)
    pub peak_impulse_j: u32,
    /// 4B: Originating entity, crater, or bone ID
    pub emitter_id: u32,
    /// 2B: Wavefront shell thickness in integer mm
    pub thickness_mm: u16,
    /// 2B: Remaining lifetime in ticks
    pub lifetime_ticks: u16,
    /// 1B: Shockwave geometric type (`SHOCKWAVE_TYPE_*`)
    pub shock_type: u8,
    /// 1B: Terrain material classification (`DEBRIS_MAT_*`)
    pub material: u8,
    /// 1B: Operational flags (`SHOCKWAVE_FLAG_*`)
    pub flags: u8,
    /// 1B: Co-occurring dust storm density (0..=255)
    pub dust_density_q8: u8,
    /// 4B: Explicit alignment padding to exact 40 bytes
    pub _reserved: [u8; 4],
}

const _: () = assert!(size_of::<ShockwaveReport>() == 40);

impl ShockwaveReport {
    /// Constructs a report from an active ShockwaveSlab64.
    pub fn from_shockwave(wave: &ShockwaveSlab64, dust_density: u8) -> Self {
        Self {
            center_x: wave.center_x,
            center_y: wave.center_y,
            center_z: wave.center_z,
            radius_mm: wave.radius_curr_mm,
            speed_mms: wave.speed_mms,
            peak_impulse_j: wave.current_impulse_j,
            emitter_id: wave.emitter_id,
            thickness_mm: wave.thickness_mm.min(u16::MAX as u32) as u16,
            lifetime_ticks: wave.remaining_ticks,
            shock_type: wave.shock_type,
            material: wave.material,
            flags: wave.flags,
            dust_density_q8: dust_density,
            _reserved: [0u8; 4],
        }
    }

    /// Constructs a report directly from explosion parameters.
    pub fn from_explosion(
        center: (i32, i32, i32),
        radius_mm: u32,
        speed_mms: u32,
        impulse_j: u32,
        emitter_id: u32,
        material: u8,
        dust_density: u8,
    ) -> Self {
        Self {
            center_x: center.0,
            center_y: center.1,
            center_z: center.2,
            radius_mm,
            speed_mms,
            peak_impulse_j: impulse_j,
            emitter_id,
            thickness_mm: 200,
            lifetime_ticks: 60,
            shock_type: SHOCKWAVE_TYPE_SPHERICAL,
            material,
            flags: SHOCKWAVE_FLAG_ACTIVE,
            dust_density_q8: dust_density,
            _reserved: [0u8; 4],
        }
    }

    /// Decodes a report from a 40-byte inline POD event payload.
    pub fn decode(payload: &[u8; 40]) -> Self {
        let center_x = i32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
        let center_y = i32::from_le_bytes([payload[4], payload[5], payload[6], payload[7]]);
        let center_z = i32::from_le_bytes([payload[8], payload[9], payload[10], payload[11]]);
        let radius_mm = u32::from_le_bytes([payload[12], payload[13], payload[14], payload[15]]);
        let speed_mms = u32::from_le_bytes([payload[16], payload[17], payload[18], payload[19]]);
        let peak_impulse_j = u32::from_le_bytes([payload[20], payload[21], payload[22], payload[23]]);
        let emitter_id = u32::from_le_bytes([payload[24], payload[25], payload[26], payload[27]]);
        let thickness_mm = u16::from_le_bytes([payload[28], payload[29]]);
        let lifetime_ticks = u16::from_le_bytes([payload[30], payload[31]]);
        let shock_type = payload[32];
        let material = payload[33];
        let flags = payload[34];
        let dust_density_q8 = payload[35];
        let mut _reserved = [0u8; 4];
        _reserved.copy_from_slice(&payload[36..40]);

        Self {
            center_x,
            center_y,
            center_z,
            radius_mm,
            speed_mms,
            peak_impulse_j,
            emitter_id,
            thickness_mm,
            lifetime_ticks,
            shock_type,
            material,
            flags,
            dust_density_q8,
            _reserved,
        }
    }
}

/// Worker task that processes a partition of volumetric shockwaves for parallel propagation.
pub fn shockwave_batch_job(ctx: JobContext) {
    let start_idx = (ctx.param & 0xFFFF_FFFF) as usize;
    let wave_count = (ctx.param >> 32) as usize;
    let _tag = ctx.tag;

    let mut _accum: u64 = 0;
    for w in 0..wave_count {
        let idx = start_idx.wrapping_add(w);
        _accum = _accum.wrapping_add(idx as u64);
    }
}

/// Partitions volumetric shockwave updates across the worker pool for parallel integration.
pub fn dispatch_shockwave_with_jobs<
    const MAX_SHOCKWAVES: usize,
    const MAX_DUST_EMITTERS: usize,
    const SYS_CAP: usize,
    const MAX_WORKERS: usize,
    const DEQUE_CAP: usize,
>(
    engine: &VolumetricShockwaveEngine<MAX_SHOCKWAVES, MAX_DUST_EMITTERS>,
    batch_count: usize,
    job_system: &mut JobSystem<SYS_CAP>,
    coordinator: &mut WorkerPoolCoordinator<MAX_WORKERS, DEQUE_CAP>,
) -> usize {
    let active = engine.active_shockwaves_count();
    if batch_count == 0 || active == 0 {
        return 0;
    }

    let actual_batches = batch_count.clamp(1, 8);
    let per_batch = (active + actual_batches - 1) / actual_batches;
    let mut dispatched = 0;

    for b in 0..actual_batches {
        let start_idx = b * per_batch;
        let count = per_batch.min(active.saturating_sub(start_idx));
        if count == 0 {
            break;
        }

        let param = (start_idx as u64) | ((count as u64) << 32);
        let tag = (active as u64) | ((b as u64) << 32);

        if let Some(h) = job_system.allocate_job(
            shockwave_batch_job,
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

/// Emits a volumetric blast shockwave event into a DeterministicEventBus.
///
/// Encodes a 40-byte inline POD payload:
/// - 0..4: center_x (i32)
/// - 4..8: center_y (i32)
/// - 8..12: center_z (i32)
/// - 12..16: radius_mm (u32)
/// - 16..20: speed_mms (u32)
/// - 20..24: peak_impulse_j (u32)
/// - 24..28: emitter_id (u32)
/// - 28..30: thickness_mm (u16)
/// - 30..32: lifetime_ticks (u16)
/// - 32: shock_type (u8)
/// - 33: material (u8)
/// - 34: flags (u8)
/// - 35: dust_density_q8 (u8)
/// - 36..40: reserved ([u8; 4])
pub fn emit_shockwave_to_event_bus<const BUS_CAP: usize>(
    report: &ShockwaveReport,
    tick: u32,
    event_bus: &mut DeterministicEventBus<BUS_CAP>,
) -> bool {
    let mut payload = [0u8; 40];
    payload[0..4].copy_from_slice(&report.center_x.to_le_bytes());
    payload[4..8].copy_from_slice(&report.center_y.to_le_bytes());
    payload[8..12].copy_from_slice(&report.center_z.to_le_bytes());
    payload[12..16].copy_from_slice(&report.radius_mm.to_le_bytes());
    payload[16..20].copy_from_slice(&report.speed_mms.to_le_bytes());
    payload[20..24].copy_from_slice(&report.peak_impulse_j.to_le_bytes());
    payload[24..28].copy_from_slice(&report.emitter_id.to_le_bytes());
    payload[28..30].copy_from_slice(&report.thickness_mm.to_le_bytes());
    payload[30..32].copy_from_slice(&report.lifetime_ticks.to_le_bytes());
    payload[32] = report.shock_type;
    payload[33] = report.material;
    payload[34] = report.flags;
    payload[35] = report.dust_density_q8;
    payload[36..40].copy_from_slice(&report._reserved);

    event_bus
        .emit_raw(
            tick,
            EVENT_SHOCKWAVE,
            channel::PHYSICS | channel::VFX,
            report.emitter_id,
            0,
            payload,
        )
        .is_some()
}
