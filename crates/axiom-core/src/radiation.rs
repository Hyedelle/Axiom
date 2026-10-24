//! # Radiative Transfer Equation (RTE) & High-Energy Particle Beam Engine (`axiom-core::radiation`)
//!
//! High-Performance 0-Float, 64-Byte L1 Cache Line Aligned Radiative Transfer
//! & Relativistic Particle Beam Armor Ablation Physics Integrator.
//!
//! Conforms strictly to Axiom Engine Constitutions:
//! - **0-Float in Core**: Pure integer mm, mJ, W, K, and Q16.16 fixed-point math.
//! - **64-Byte L1 Cache Alignment**: `RadiationCellSlab64` and `ParticleBeamSlab64` are `#[repr(C, align(64))]`.
//! - **0-Heap Simulation**: Flat array `MechaRadiationEngine<MAX_CELLS, MAX_BEAMS>`.
//! - **Deterministic State Hash**: 64-bit FNV-1a checksum consensus.
//! - **40-Byte Inline POD Event Bus Payload**: `RadiationEventReport`.

use crate::event_bus::{channel, DeterministicEventBus};
use crate::job_system::{JobContext, JobPriority, JobStatus, JobSystem};
use crate::worker_pool::WorkerPoolCoordinator;

// ============================================================================
// Constants & Bit Flags
// ============================================================================

/// Stefan-Boltzmann constant scaling constant (sigma ~ 5.67e-8 W/(m^2 K^4))
/// In scaled integer formula: E = ((T / 100)^4 * 567) / 100 in W/m^2
pub const STEFAN_BOLTZMANN_SCALED_COEFF: u64 = 567;

/// Volumetric ablation heat for titanium/ceramic mecha armor in mJ/mm³ (~80 mJ/mm³)
pub const TITANIUM_ABLATION_HEAT_MJ_MM3: u32 = 80;

/// Default baseline spot radius for collimated particle beams in mm
pub const BEAM_BASELINE_RADIUS_MM: u32 = 5;

/// Armor penetration breakthrough threshold in mm (e.g. 20mm plate)
pub const ARMOR_BREAKTHROUGH_MM: u16 = 20;

/// Radiation cell flags
pub const RAD_FLAG_ACTIVE: u8       = 1 << 0;
pub const RAD_FLAG_IONIZED: u8      = 1 << 1;
pub const RAD_FLAG_INCANDESCENT: u8 = 1 << 2;
pub const RAD_FLAG_OPAQUE: u8       = 1 << 3;
pub const RAD_FLAG_EQUILIBRIUM: u8   = 1 << 4;

/// Particle beam flags
pub const BEAM_FLAG_ACTIVE: u8      = 1 << 0;
pub const BEAM_FLAG_COLLIMATED: u8  = 1 << 1;
pub const BEAM_FLAG_ABLATING: u8    = 1 << 2;
pub const BEAM_FLAG_PENETRATED: u8  = 1 << 3;
pub const BEAM_FLAG_DIFFUSED: u8    = 1 << 4;

/// Event type: Radiative transfer emission burst
pub const EVENT_RADIATION_BURST: u16  = 0x0340;
/// Event type: Particle beam armor ablation and penetration
pub const EVENT_BEAM_PENETRATION: u16 = 0x0341;

/// Event kind discriminant
pub const RAD_EVENT_KIND_BURST: u8 = 0;
pub const RAD_EVENT_KIND_BEAM: u8  = 1;

/// Default capacities
pub const DEFAULT_MAX_RAD_CELLS: usize = 32;
pub const DEFAULT_MAX_BEAMS: usize     = 16;

// ============================================================================
// 64-Byte L1-Cache Aligned Data Slabs
// ============================================================================

/// 64-Byte L1 Cache Line Aligned Radiative Transfer & Atmospheric Absorption Cell Slab.
#[repr(C, align(64))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RadiationCellSlab64 {
    /// 12B: World position center [x, y, z] in integer mm
    pub center_xyz: [i32; 3],
    /// 4B: Radiative flux in kW/m² in Q16.16 format
    pub radiative_flux_q16: u32,
    /// 4B: Absorption coefficient kappa_abs in Q16.16 (1/m)
    pub absorption_coeff_q16: u32,
    /// 4B: Scattering coefficient kappa_sca in Q16.16 (1/m)
    pub scattering_coeff_q16: u32,
    /// 4B: Cumulative optical depth tau in Q16.16
    pub optical_depth_q16: u32,
    /// 4B: Radiative equilibrium temperature in Kelvin
    pub temperature_k: u32,
    /// 4B: Blackbody emission power in Watts (W)
    pub emission_power_w: u32,
    /// 1B: Cell identifier
    pub cell_id: u8,
    /// 1B: Status flags (ACTIVE, IONIZED, INCANDESCENT, OPAQUE, EQUILIBRIUM)
    pub flags: u8,
    /// 26B: Explicit alignment padding to exact 64 bytes
    pub reserved: [u8; 26],
}

/// 64-Byte L1 Cache Line Aligned High-Energy Particle Beam Slab.
#[repr(C, align(64))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParticleBeamSlab64 {
    /// 12B: Beam emitter origin [x, y, z] in integer mm
    pub origin_xyz: [i32; 3],
    /// 12B: Beam direction unit vector [dx, dy, dz] in Q1.15 fixed-point (32768 = 1.0)
    pub dir_xyz: [i32; 3],
    /// 4B: Beam total pulse energy in mJ
    pub beam_energy_mj: u32,
    /// 2B: Beam wavelength in nm (e.g. 1064nm Nd:YAG, 532nm Green, 248nm KrF)
    pub wavelength_nm: u16,
    /// 2B: Beam angular divergence in milliradians (mrad)
    pub divergence_mrad: u16,
    /// 2B: Armor ablation penetration depth in integer mm
    pub penetration_depth_mm: u16,
    /// 1B: Target entity ID (0 = environment)
    pub target_id: u8,
    /// 1B: Emitter entity ID
    pub emitter_id: u8,
    /// 1B: Operational flags (ACTIVE, COLLIMATED, ABLATING, PENETRATED, DIFFUSED)
    pub flags: u8,
    /// 27B: Explicit alignment padding to exact 64 bytes
    pub reserved: [u8; 27],
}

/// 40-Byte Inline POD Radiative Transfer & Particle Beam Event Report (Event Bus Payload).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RadiationEventReport {
    /// 12B: World position [x, y, z] in integer mm
    pub pos_xyz: [i32; 3],
    /// 12B: Beam direction unit vector or emission gradient [x, y, z] in Q1.15
    pub dir_xyz: [i32; 3],
    /// 4B: Absorbed flux (W/m²) or beam pulse energy (mJ)
    pub flux_or_energy_mj: u32,
    /// 4B: Optical depth tau in Q16.16 or temperature in K
    pub optical_depth_or_temp: u32,
    /// 2B: Armor ablation penetration depth in mm
    pub penetration_depth_mm: u16,
    /// 2B: Laser wavelength in nm or Cell ID
    pub wavelength_or_cell_id: u16,
    /// 1B: Emitter entity ID
    pub emitter_id: u8,
    /// 1B: Target entity ID
    pub target_id: u8,
    /// 1B: Status flags
    pub flags: u8,
    /// 1B: Event discriminant kind (RAD_EVENT_KIND_BURST, RAD_EVENT_KIND_BEAM)
    pub event_kind: u8,
}

// Compile-time memory layout assertions
const _: () = {
    assert!(core::mem::size_of::<RadiationCellSlab64>() == 64);
    assert!(core::mem::align_of::<RadiationCellSlab64>() == 64);
    assert!(core::mem::size_of::<ParticleBeamSlab64>() == 64);
    assert!(core::mem::align_of::<ParticleBeamSlab64>() == 64);
    assert!(core::mem::size_of::<RadiationEventReport>() == 40);
};

// ============================================================================
// Math & Serialization Helpers
// ============================================================================

/// Fast integer square root using bitwise digit-by-digit algorithm.
pub fn isqrt_u64(n: u64) -> u64 {
    if n == 0 { return 0; }
    let mut x0 = n >> 1;
    if x0 == 0 { return 1; }
    let mut x1 = (x0 + n / x0) >> 1;
    while x1 < x0 {
        x0 = x1;
        x1 = (x0 + n / x0) >> 1;
    }
    x0
}

/// Computes blackbody radiative emission flux in W/m² via 0-Float Stefan-Boltzmann approximation.
/// E_flux = ((T / 100)^4 * 567) / 100 for T >= 100K, or (T^4 * 567) / 100_000_000 for T < 100K.
pub fn compute_stefan_boltzmann_emission_flux(temp_k: u32) -> u64 {
    if temp_k == 0 {
        return 0;
    }
    if temp_k >= 100 {
        let t_div100 = (temp_k / 100) as u64;
        let t4 = t_div100 * t_div100 * t_div100 * t_div100;
        (t4 * STEFAN_BOLTZMANN_SCALED_COEFF) / 100
    } else {
        let t = temp_k as u64;
        let t4 = t * t * t * t;
        (t4 * STEFAN_BOLTZMANN_SCALED_COEFF) / 100_000_000
    }
}

/// Computes Beer-Lambert transmittance T = exp(-tau) in Q16.16 format
/// using rational Padé approximation: T = 65536^2 / (65536 + tau + tau^2 / 131072).
pub fn compute_beer_lambert_transmittance_q16(optical_depth_q16: u32) -> u32 {
    let tau = optical_depth_q16 as u64;
    let tau_sq_half = (tau * tau) / 131072;
    let denom = 65536u64.saturating_add(tau).saturating_add(tau_sq_half);
    if denom == 0 {
        return 65536;
    }
    let num = 65536u64 * 65536u64;
    let trans = num / denom;
    trans.min(65536) as u32
}

/// Solves particle beam propagation, atmospheric extinction, and armor ablation penetration.
/// Returns calculated penetration depth in mm.
pub fn compute_beam_ablation_penetration(
    beam: &mut ParticleBeamSlab64,
    distance_mm: u32,
    target_extinction_q16: u32,
    armor_ablation_heat_mj_mm3: u32,
) -> u16 {
    if (beam.flags & BEAM_FLAG_ACTIVE) == 0 {
        return 0;
    }

    // 1. Calculate optical depth: tau = (kappa * distance) / 1000
    let opt_depth_q16 = ((target_extinction_q16 as u64 * distance_mm as u64) / 1000) as u32;

    // 2. Transmittance via Beer-Lambert
    let trans_q16 = compute_beer_lambert_transmittance_q16(opt_depth_q16);

    // 3. Effective energy delivered after atmospheric extinction
    let eff_energy_mj = ((beam.beam_energy_mj as u64 * trans_q16 as u64) / 65536) as u64;

    // 4. Beam spot radius with angular divergence: r(s) = r0 + (s * divergence) / 1000
    let div_expansion = (distance_mm as u64 * beam.divergence_mrad as u64) / 1000;
    let spot_radius_mm = (BEAM_BASELINE_RADIUS_MM as u64 + div_expansion).max(1);

    // 5. Spot Area: A = pi * r^2 (pi ~ 31416 / 10000)
    let spot_area_mm2 = (31416u64 * spot_radius_mm * spot_radius_mm) / 10000;
    let q_vol = armor_ablation_heat_mj_mm3.max(1) as u64;

    // 6. Penetration depth: d = E_eff / (A * Q_vol)
    let denom = (spot_area_mm2 * q_vol).max(1);
    let penetration_mm = (eff_energy_mj / denom).min(u16::MAX as u64) as u16;

    beam.penetration_depth_mm = penetration_mm;

    if penetration_mm > 0 {
        beam.flags |= BEAM_FLAG_ABLATING;
        if penetration_mm >= ARMOR_BREAKTHROUGH_MM {
            beam.flags |= BEAM_FLAG_PENETRATED;
        } else {
            beam.flags &= !BEAM_FLAG_PENETRATED;
        }
    } else {
        beam.flags &= !(BEAM_FLAG_ABLATING | BEAM_FLAG_PENETRATED);
    }

    if beam.divergence_mrad > 50 {
        beam.flags |= BEAM_FLAG_DIFFUSED;
    }

    penetration_mm
}

impl RadiationCellSlab64 {
    /// Creates a new radiation cell slab.
    pub fn new(cell_id: u8, center_xyz: [i32; 3], initial_temp_k: u32) -> Self {
        let emission_power = compute_stefan_boltzmann_emission_flux(initial_temp_k).min(u32::MAX as u64) as u32;
        let mut flags = RAD_FLAG_ACTIVE;
        if initial_temp_k >= 1000 {
            flags |= RAD_FLAG_INCANDESCENT;
        }
        if initial_temp_k >= 3000 {
            flags |= RAD_FLAG_IONIZED;
        }
        Self {
            center_xyz,
            radiative_flux_q16: 0,
            absorption_coeff_q16: 6554, // ~0.1 m^-1 baseline
            scattering_coeff_q16: 3277, // ~0.05 m^-1 baseline
            optical_depth_q16: 0,
            temperature_k: initial_temp_k,
            emission_power_w: emission_power,
            cell_id,
            flags,
            reserved: [0; 26],
        }
    }
}

impl ParticleBeamSlab64 {
    /// Creates a new high-energy particle beam slab.
    pub fn new(
        emitter_id: u8,
        target_id: u8,
        origin_xyz: [i32; 3],
        dir_xyz: [i32; 3],
        beam_energy_mj: u32,
        wavelength_nm: u16,
        divergence_mrad: u16,
    ) -> Self {
        let mut flags = BEAM_FLAG_ACTIVE;
        if divergence_mrad <= 5 {
            flags |= BEAM_FLAG_COLLIMATED;
        }
        Self {
            origin_xyz,
            dir_xyz,
            beam_energy_mj,
            wavelength_nm,
            divergence_mrad,
            penetration_depth_mm: 0,
            target_id,
            emitter_id,
            flags,
            reserved: [0; 27],
        }
    }
}

impl RadiationEventReport {
    /// Serializes report into a 40-byte POD payload.
    pub fn encode(&self) -> [u8; 40] {
        let mut buf = [0u8; 40];
        buf[0..4].copy_from_slice(&self.pos_xyz[0].to_le_bytes());
        buf[4..8].copy_from_slice(&self.pos_xyz[1].to_le_bytes());
        buf[8..12].copy_from_slice(&self.pos_xyz[2].to_le_bytes());
        buf[12..16].copy_from_slice(&self.dir_xyz[0].to_le_bytes());
        buf[16..20].copy_from_slice(&self.dir_xyz[1].to_le_bytes());
        buf[20..24].copy_from_slice(&self.dir_xyz[2].to_le_bytes());
        buf[24..28].copy_from_slice(&self.flux_or_energy_mj.to_le_bytes());
        buf[28..32].copy_from_slice(&self.optical_depth_or_temp.to_le_bytes());
        buf[32..34].copy_from_slice(&self.penetration_depth_mm.to_le_bytes());
        buf[34..36].copy_from_slice(&self.wavelength_or_cell_id.to_le_bytes());
        buf[36] = self.emitter_id;
        buf[37] = self.target_id;
        buf[38] = self.flags;
        buf[39] = self.event_kind;
        buf
    }

    /// Reconstructs report from a 40-byte POD payload.
    pub fn decode(payload: &[u8; 40]) -> Self {
        let px = i32::from_le_bytes(payload[0..4].try_into().unwrap_or([0; 4]));
        let py = i32::from_le_bytes(payload[4..8].try_into().unwrap_or([0; 4]));
        let pz = i32::from_le_bytes(payload[8..12].try_into().unwrap_or([0; 4]));
        let dx = i32::from_le_bytes(payload[12..16].try_into().unwrap_or([0; 4]));
        let dy = i32::from_le_bytes(payload[16..20].try_into().unwrap_or([0; 4]));
        let dz = i32::from_le_bytes(payload[20..24].try_into().unwrap_or([0; 4]));
        let flux_energy = u32::from_le_bytes(payload[24..28].try_into().unwrap_or([0; 4]));
        let opt_temp = u32::from_le_bytes(payload[28..32].try_into().unwrap_or([0; 4]));
        let pen = u16::from_le_bytes(payload[32..34].try_into().unwrap_or([0; 2]));
        let wave_id = u16::from_le_bytes(payload[34..36].try_into().unwrap_or([0; 2]));
        let emitter_id = payload[36];
        let target_id = payload[37];
        let flags = payload[38];
        let event_kind = payload[39];

        Self {
            pos_xyz: [px, py, pz],
            dir_xyz: [dx, dy, dz],
            flux_or_energy_mj: flux_energy,
            optical_depth_or_temp: opt_temp,
            penetration_depth_mm: pen,
            wavelength_or_cell_id: wave_id,
            emitter_id,
            target_id,
            flags,
            event_kind,
        }
    }
}

// ============================================================================
// Fixed-Capacity Deterministic Mecha Radiation Engine
// ============================================================================

/// 0-Heap Fixed-Capacity Mecha Radiative Transfer & Particle Beam Engine.
pub struct MechaRadiationEngine<const MAX_CELLS: usize, const MAX_BEAMS: usize> {
    pub cells: [RadiationCellSlab64; MAX_CELLS],
    pub beams: [ParticleBeamSlab64; MAX_BEAMS],
    pub active_cells_count: usize,
    pub active_beams_count: usize,
    pub tick: u32,
    pub total_emitted_energy_w: u64,
    pub total_penetration_depth_mm: u64,
}

impl<const MAX_CELLS: usize, const MAX_BEAMS: usize>
    MechaRadiationEngine<MAX_CELLS, MAX_BEAMS>
{
    pub fn new() -> Self {
        let dummy_cell = RadiationCellSlab64 {
            center_xyz: [0; 3],
            radiative_flux_q16: 0,
            absorption_coeff_q16: 6554,
            scattering_coeff_q16: 3277,
            optical_depth_q16: 0,
            temperature_k: 300,
            emission_power_w: 459,
            cell_id: 0,
            flags: 0,
            reserved: [0; 26],
        };
        let dummy_beam = ParticleBeamSlab64 {
            origin_xyz: [0; 3],
            dir_xyz: [0, 0, 32768],
            beam_energy_mj: 0,
            wavelength_nm: 1064,
            divergence_mrad: 2,
            penetration_depth_mm: 0,
            target_id: 0,
            emitter_id: 0,
            flags: 0,
            reserved: [0; 27],
        };

        Self {
            cells: [dummy_cell; MAX_CELLS],
            beams: [dummy_beam; MAX_BEAMS],
            active_cells_count: 0,
            active_beams_count: 0,
            tick: 0,
            total_emitted_energy_w: 0,
            total_penetration_depth_mm: 0,
        }
    }

    pub fn register_cell(&mut self, cell: RadiationCellSlab64) -> Option<usize> {
        if self.active_cells_count >= MAX_CELLS { return None; }
        let idx = self.active_cells_count;
        self.cells[idx] = cell;
        self.active_cells_count += 1;
        Some(idx)
    }

    pub fn register_beam(&mut self, beam: ParticleBeamSlab64) -> Option<usize> {
        if self.active_beams_count >= MAX_BEAMS { return None; }
        let idx = self.active_beams_count;
        self.beams[idx] = beam;
        self.active_beams_count += 1;
        Some(idx)
    }

    /// Advances simulation by 1 deterministic tick.
    /// ambient_extinction_q16: background atmospheric extinction coefficient in Q16.16 (1/m).
    pub fn step_simulation(&mut self, ambient_extinction_q16: u32) -> (usize, usize) {
        self.tick = self.tick.wrapping_add(1);
        let mut evaluated_cells = 0;
        let mut penetrating_beams = 0;

        // 1. Process active radiation cells
        for i in 0..self.active_cells_count {
            if (self.cells[i].flags & RAD_FLAG_ACTIVE) == 0 { continue; }

            // Update emission power via Stefan-Boltzmann
            let emission_w = compute_stefan_boltzmann_emission_flux(self.cells[i].temperature_k);
            self.cells[i].emission_power_w = emission_w.min(u32::MAX as u64) as u32;

            // Optical depth integration across 1m cell size: tau = (kappa_abs + kappa_sca)
            let ext_q16 = self.cells[i].absorption_coeff_q16.saturating_add(self.cells[i].scattering_coeff_q16);
            self.cells[i].optical_depth_q16 = ext_q16;

            // Flags
            if self.cells[i].temperature_k >= 1000 {
                self.cells[i].flags |= RAD_FLAG_INCANDESCENT;
            }
            if self.cells[i].temperature_k >= 3000 {
                self.cells[i].flags |= RAD_FLAG_IONIZED;
            }
            if self.cells[i].optical_depth_q16 >= 65536 * 3 { // tau >= 3 -> opaque
                self.cells[i].flags |= RAD_FLAG_OPAQUE;
            }

            self.total_emitted_energy_w = self.total_emitted_energy_w.wrapping_add(emission_w);
            evaluated_cells += 1;
        }

        // 2. Process active particle beams
        for i in 0..self.active_beams_count {
            if (self.beams[i].flags & BEAM_FLAG_ACTIVE) == 0 { continue; }

            // Standard target engagement range ~ 2000 mm (2 meters)
            let distance_mm = 2000u32;
            let pen = compute_beam_ablation_penetration(
                &mut self.beams[i],
                distance_mm,
                ambient_extinction_q16,
                TITANIUM_ABLATION_HEAT_MJ_MM3,
            );

            if (self.beams[i].flags & BEAM_FLAG_PENETRATED) != 0 {
                penetrating_beams += 1;
            }

            self.total_penetration_depth_mm = self.total_penetration_depth_mm.wrapping_add(pen as u64);
        }

        (evaluated_cells, penetrating_beams)
    }

    /// Computes deterministic consensus checksum using 64-bit FNV-1a.
    pub fn compute_consensus_hash(&self) -> u64 {
        let mut h = 0xcbf29ce484222325u64;
        for b in self.tick.to_le_bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        for b in self.total_emitted_energy_w.to_le_bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        for b in self.total_penetration_depth_mm.to_le_bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        for b in (self.active_cells_count as u32).to_le_bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        for b in (self.active_beams_count as u32).to_le_bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }

        for i in 0..self.active_cells_count {
            let c = &self.cells[i];
            for b in c.emission_power_w.to_le_bytes() {
                h ^= b as u64;
                h = h.wrapping_mul(0x100000001b3);
            }
            for b in c.temperature_k.to_le_bytes() {
                h ^= b as u64;
                h = h.wrapping_mul(0x100000001b3);
            }
            h ^= c.flags as u64;
            h = h.wrapping_mul(0x100000001b3);
        }

        for i in 0..self.active_beams_count {
            let bm = &self.beams[i];
            for b in bm.penetration_depth_mm.to_le_bytes() {
                h ^= b as u64;
                h = h.wrapping_mul(0x100000001b3);
            }
            for b in bm.beam_energy_mj.to_le_bytes() {
                h ^= b as u64;
                h = h.wrapping_mul(0x100000001b3);
            }
            h ^= bm.flags as u64;
            h = h.wrapping_mul(0x100000001b3);
        }

        h
    }
}

impl<const MAX_CELLS: usize, const MAX_BEAMS: usize> Default
    for MechaRadiationEngine<MAX_CELLS, MAX_BEAMS>
{
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Event Bus & Job System Integration
// ============================================================================

/// Emits a radiative transfer or particle beam penetration event to the deterministic event bus.
pub fn emit_radiation_to_event_bus<const BUS_CAP: usize>(
    report: &RadiationEventReport,
    tick: u32,
    event_bus: &mut DeterministicEventBus<BUS_CAP>,
) -> bool {
    let payload = report.encode();
    let event_type = if report.event_kind == RAD_EVENT_KIND_BEAM {
        EVENT_BEAM_PENETRATION
    } else {
        EVENT_RADIATION_BURST
    };

    event_bus
        .emit_raw(
            tick,
            event_type,
            channel::PHYSICS | channel::VFX | channel::AUDIO,
            report.emitter_id as u32,
            report.target_id as u32,
            payload,
        )
        .is_some()
}

pub fn radiation_batch_job(ctx: JobContext) {
    let start_idx = (ctx.param & 0xFFFF_FFFF) as usize;
    let chunk_size = (ctx.param >> 32) as usize;
    let mut _accum: u64 = 0;
    for i in 0..chunk_size {
        let idx = start_idx.wrapping_add(i);
        _accum = _accum.wrapping_add(idx as u64);
    }
}

pub fn dispatch_radiation_with_jobs<
    const MAX_CELLS: usize,
    const MAX_BEAMS: usize,
    const SYS_CAP: usize,
    const MAX_WORKERS: usize,
    const DEQUE_CAP: usize,
>(
    engine: &MechaRadiationEngine<MAX_CELLS, MAX_BEAMS>,
    batch_count: usize,
    job_system: &mut JobSystem<SYS_CAP>,
    coordinator: &mut WorkerPoolCoordinator<MAX_WORKERS, DEQUE_CAP>,
) -> usize {
    if engine.active_cells_count == 0 || batch_count == 0 {
        return 0;
    }

    let actual_batches = batch_count.clamp(1, 8);
    let chunk_size = (engine.active_cells_count + actual_batches - 1) / actual_batches;
    let mut dispatched = 0;

    for b in 0..actual_batches {
        let start = b * chunk_size;
        let len = chunk_size.min(engine.active_cells_count.saturating_sub(start));
        if len == 0 { break; }

        let param = (start as u64) | ((len as u64) << 32);
        let tag = (engine.active_beams_count as u64) | ((b as u64) << 32);

        if let Some(h) = job_system.allocate_job(
            radiation_batch_job,
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

