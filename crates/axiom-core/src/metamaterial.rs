//! # Acousto-Optics & Metamaterial Cloaking Engine (`axiom-core::metamaterial`)
//!
//! High-Performance 0-Float, 64-Byte L1 Cache Line Aligned Acousto-Optic Deflection,
//! Brillouin Scattering, and Transformation Optics Metamaterial Cloaking Physics.
//!
//! Conforms strictly to Axiom Engine Constitutions:
//! - **0-Float in Core**: Pure integer mm, mm/s, kHz, nm, mrad, and Q16.16 fixed-point math.
//! - **64-Byte L1 Cache Alignment**: `AcoustoOpticCellSlab64` and `CloakingFieldSlab64` are `#[repr(C, align(64))]`.
//! - **0-Heap Simulation**: Flat array `MechaMetamaterialEngine<MAX_CELLS, MAX_FIELDS>`.
//! - **Deterministic State Hash**: 64-bit FNV-1a checksum consensus.
//! - **40-Byte Inline POD Event Bus Payload**: `MetamaterialEventReport`.

use crate::event_bus::{channel, DeterministicEventBus};
use crate::job_system::{JobContext, JobPriority, JobStatus, JobSystem};
use crate::worker_pool::WorkerPoolCoordinator;

// ============================================================================
// Constants & Bit Flags
// ============================================================================

pub const AO_FLAG_ACTIVE: u8        = 1 << 0;
pub const AO_FLAG_RESONANT: u8      = 1 << 1;
pub const AO_FLAG_DEFLECTING: u8    = 1 << 2;
pub const AO_FLAG_BRAGG_MATCHED: u8 = 1 << 3;
pub const AO_FLAG_SATURATED: u8     = 1 << 4;

pub const CLOAK_FLAG_ACTIVE: u8     = 1 << 0;
pub const CLOAK_FLAG_BENDING: u8    = 1 << 1;
pub const CLOAK_FLAG_DISPERSIVE: u8 = 1 << 2;
pub const CLOAK_FLAG_PENETRATED: u8 = 1 << 3;
pub const CLOAK_FLAG_OVERLOADED: u8 = 1 << 4;

pub const EVENT_OPTICAL_CLOAK: u16     = 0x0370;
pub const EVENT_BRILLOUIN_SCATTER: u16 = 0x0371;

pub const META_EVENT_KIND_CLOAK: u8    = 0;
pub const META_EVENT_KIND_SCATTER: u8  = 1;

pub const DEFAULT_MAX_AO_CELLS: usize     = 16;
pub const DEFAULT_MAX_CLOAK_FIELDS: usize = 8;

// ============================================================================
// 64-Byte L1-Cache Aligned Data Slabs
// ============================================================================

/// 64-Byte L1 Cache Line Aligned Acousto-Optic Modulation Cell Slab.
#[repr(C, align(64))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AcoustoOpticCellSlab64 {
    /// 12B: Cell world center position [x, y, z] in integer mm
    pub center_xyz: [i32; 3],
    /// 4B: Acoustic wave propagation speed in mm/s (~4,200,000 mm/s in quartz)
    pub sound_vel_mms: u32,
    /// 4B: Acoustic piezoelectric drive frequency in kHz (e.g. 100,000 kHz = 100 MHz)
    pub acoustic_freq_khz: u32,
    /// 4B: Phonon acoustic strain amplitude S in Q16.16 (65536 = 1.0)
    pub strain_q16: u32,
    /// 4B: Induced refractive index perturbation Delta n in Q16.16
    pub delta_n_q16: u32,
    /// 4B: Bragg diffraction angle theta_B in milliradians (mrad)
    pub bragg_angle_mrad: u32,
    /// 4B: First-order diffraction efficiency eta in Q16.16 (65536 = 100%)
    pub diffraction_eff_q16: u32,
    /// 2B: Incident probing laser wavelength in nm (e.g. 532 nm, 1064 nm)
    pub laser_wavelength_nm: u16,
    /// 1B: Cell operational flags (ACTIVE, RESONANT, DEFLECTING, etc.)
    pub flags: u8,
    /// 1B: Owner mecha entity ID
    pub entity_id: u8,
    /// 24B: Explicit alignment padding to exact 64 bytes
    pub reserved: [u8; 24],
}

const _: () = assert!(core::mem::size_of::<AcoustoOpticCellSlab64>() == 64);
const _: () = assert!(core::mem::align_of::<AcoustoOpticCellSlab64>() == 64);

impl Default for AcoustoOpticCellSlab64 {
    fn default() -> Self {
        Self {
            center_xyz: [0, 0, 0],
            sound_vel_mms: 4_200_000,
            acoustic_freq_khz: 100_000,
            strain_q16: 655, // 0.01 strain
            delta_n_q16: 0,
            bragg_angle_mrad: 0,
            diffraction_eff_q16: 0,
            laser_wavelength_nm: 1064,
            flags: AO_FLAG_ACTIVE,
            entity_id: 0,
            reserved: [0u8; 24],
        }
    }
}

impl AcoustoOpticCellSlab64 {
    pub fn new(
        entity_id: u8,
        center_xyz: [i32; 3],
        sound_vel_mms: u32,
        acoustic_freq_khz: u32,
        laser_wavelength_nm: u16,
    ) -> Self {
        Self {
            center_xyz,
            sound_vel_mms,
            acoustic_freq_khz,
            strain_q16: 655,
            delta_n_q16: 0,
            bragg_angle_mrad: 0,
            diffraction_eff_q16: 0,
            laser_wavelength_nm,
            flags: AO_FLAG_ACTIVE,
            entity_id,
            reserved: [0u8; 24],
        }
    }
}

/// 64-Byte L1 Cache Line Aligned Metamaterial Transformation Optics Cloaking Field Slab.
#[repr(C, align(64))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CloakingFieldSlab64 {
    /// 12B: Cloaking field origin [x, y, z] in integer mm
    pub origin_xyz: [i32; 3],
    /// 4B: Inner hidden core radius R1 in integer mm
    pub radius_inner_mm: u32,
    /// 4B: Outer impedance-matched shell radius R2 in integer mm
    pub radius_outer_mm: u32,
    /// 4B: Radial permittivity epsilon_r in Q16.16
    pub radial_permittivity_q16: u32,
    /// 4B: Angular permittivity epsilon_theta in Q16.16
    pub angular_permittivity_q16: u32,
    /// 4B: Refractive index gradient nabla n in Q16.16
    pub refractive_gradient_q16: u32,
    /// 2B: Effective cloaking efficiency in percent (0..100 %)
    pub cloak_efficiency_pct: u16,
    /// 1B: Emitter / generator entity ID
    pub emitter_id: u8,
    /// 1B: Target mecha entity ID to cloak
    pub target_id: u8,
    /// 1B: Cloaking field flags (ACTIVE, BENDING, DISPERSIVE, etc.)
    pub flags: u8,
    /// 27B: Explicit alignment padding to exact 64 bytes
    pub reserved: [u8; 27],
}

const _: () = assert!(core::mem::size_of::<CloakingFieldSlab64>() == 64);
const _: () = assert!(core::mem::align_of::<CloakingFieldSlab64>() == 64);

impl Default for CloakingFieldSlab64 {
    fn default() -> Self {
        Self {
            origin_xyz: [0, 0, 0],
            radius_inner_mm: 1000,
            radius_outer_mm: 2000,
            radial_permittivity_q16: 32768,  // 0.5
            angular_permittivity_q16: 131072, // 2.0
            refractive_gradient_q16: 65536,
            cloak_efficiency_pct: 95,
            emitter_id: 0,
            target_id: 0,
            flags: CLOAK_FLAG_ACTIVE | CLOAK_FLAG_BENDING,
            reserved: [0u8; 27],
        }
    }
}

impl CloakingFieldSlab64 {
    pub fn new(
        emitter_id: u8,
        target_id: u8,
        origin_xyz: [i32; 3],
        radius_inner_mm: u32,
        radius_outer_mm: u32,
    ) -> Self {
        Self {
            origin_xyz,
            radius_inner_mm,
            radius_outer_mm,
            radial_permittivity_q16: 32768,
            angular_permittivity_q16: 131072,
            refractive_gradient_q16: 65536,
            cloak_efficiency_pct: 95,
            emitter_id,
            target_id,
            flags: CLOAK_FLAG_ACTIVE | CLOAK_FLAG_BENDING,
            reserved: [0u8; 27],
        }
    }
}

// ============================================================================
// 40-Byte Inline POD Event Bus Payload
// ============================================================================

/// 40-Byte Inline POD Acousto-Optic & Metamaterial Cloaking Event Report.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MetamaterialEventReport {
    /// 12B: World position [x, y, z] in integer mm
    pub pos_xyz: [i32; 3],
    /// 12B: Deflected ray direction unit vector [dx, dy, dz] in Q1.15 fixed point
    pub deflection_dir_xyz: [i32; 3],
    /// 4B: Bragg deflection angle in milliradians (mrad)
    pub bragg_angle_mrad: u32,
    /// 4B: Diffraction efficiency or gradient in Q16.16
    pub diffraction_eff_or_grad_q16: u32,
    /// 2B: Cloaking efficiency percentage (0..100 %)
    pub cloak_efficiency_pct: u16,
    /// 2B: Probing laser wavelength in nm
    pub laser_wavelength_nm: u16,
    /// 1B: Emitter entity ID
    pub emitter_id: u8,
    /// 1B: Target entity ID
    pub target_id: u8,
    /// 1B: Operational flags
    pub flags: u8,
    /// 1B: Event kind (0: CLOAK, 1: SCATTER)
    pub event_kind: u8,
}

const _: () = assert!(core::mem::size_of::<MetamaterialEventReport>() == 40);

impl MetamaterialEventReport {
    pub fn encode(&self) -> [u8; 40] {
        let mut buf = [0u8; 40];
        buf[0..4].copy_from_slice(&self.pos_xyz[0].to_le_bytes());
        buf[4..8].copy_from_slice(&self.pos_xyz[1].to_le_bytes());
        buf[8..12].copy_from_slice(&self.pos_xyz[2].to_le_bytes());
        buf[12..16].copy_from_slice(&self.deflection_dir_xyz[0].to_le_bytes());
        buf[16..20].copy_from_slice(&self.deflection_dir_xyz[1].to_le_bytes());
        buf[20..24].copy_from_slice(&self.deflection_dir_xyz[2].to_le_bytes());
        buf[24..28].copy_from_slice(&self.bragg_angle_mrad.to_le_bytes());
        buf[28..32].copy_from_slice(&self.diffraction_eff_or_grad_q16.to_le_bytes());
        buf[32..34].copy_from_slice(&self.cloak_efficiency_pct.to_le_bytes());
        buf[34..36].copy_from_slice(&self.laser_wavelength_nm.to_le_bytes());
        buf[36] = self.emitter_id;
        buf[37] = self.target_id;
        buf[38] = self.flags;
        buf[39] = self.event_kind;
        buf
    }

    pub fn decode(buf: [u8; 40]) -> Self {
        Self {
            pos_xyz: [
                i32::from_le_bytes(buf[0..4].try_into().unwrap()),
                i32::from_le_bytes(buf[4..8].try_into().unwrap()),
                i32::from_le_bytes(buf[8..12].try_into().unwrap()),
            ],
            deflection_dir_xyz: [
                i32::from_le_bytes(buf[12..16].try_into().unwrap()),
                i32::from_le_bytes(buf[16..20].try_into().unwrap()),
                i32::from_le_bytes(buf[20..24].try_into().unwrap()),
            ],
            bragg_angle_mrad: u32::from_le_bytes(buf[24..28].try_into().unwrap()),
            diffraction_eff_or_grad_q16: u32::from_le_bytes(buf[28..32].try_into().unwrap()),
            cloak_efficiency_pct: u16::from_le_bytes(buf[32..34].try_into().unwrap()),
            laser_wavelength_nm: u16::from_le_bytes(buf[34..36].try_into().unwrap()),
            emitter_id: buf[36],
            target_id: buf[37],
            flags: buf[38],
            event_kind: buf[39],
        }
    }
}

// ============================================================================
// Core Physics Calculations
// ============================================================================

#[inline(always)]
pub fn isqrt_u64(n: u64) -> u64 {
    if n == 0 {
        return 0;
    }
    let mut x0 = n >> 1;
    if x0 == 0 {
        return 1;
    }
    let mut x1 = (x0 + n / x0) >> 1;
    while x1 < x0 {
        x0 = x1;
        x1 = (x0 + n / x0) >> 1;
    }
    x0
}

/// Computes acousto-optic photoelastic refractive index perturbation,
/// Bragg deflection angle, and in-plane deflected ray direction.
#[inline(always)]
pub fn compute_acousto_optic_deflection(
    cell: &mut AcoustoOpticCellSlab64,
    input_ray_dir_xyz: [i32; 3],
) -> [i32; 3] {
    if (cell.flags & AO_FLAG_ACTIVE) == 0 {
        return input_ray_dir_xyz;
    }

    // 1. Delta n = -1/2 * n0^3 * p * S
    // n0 = 1.50 Q16 -> 98304, p = 0.20 Q16 -> 13107
    let n0: u64 = 98_304;
    let n0_sq = (n0 * n0) >> 16;
    let n0_cube = (n0_sq * n0) >> 16;
    let p_q16: u64 = 13_107;
    let s_q16 = cell.strain_q16 as u64;
    let delta_n = (((n0_cube * p_q16) >> 16) * s_q16) >> 17;
    cell.delta_n_q16 = delta_n as u32;

    // 2. Bragg Angle: theta_B (mrad) = (lambda_nm * f_kHz) / (2 * v_s_mms)
    let denom = (cell.sound_vel_mms as u64).saturating_mul(2).max(1);
    let num = (cell.laser_wavelength_nm as u64) * (cell.acoustic_freq_khz as u64);
    let theta_b_mrad = (num / denom) as u32;
    cell.bragg_angle_mrad = theta_b_mrad;

    // 3. Efficiency approximation: eta = sin^2(phi)
    // SINE_LUT_256 maps 0..64 to 0..pi/2
    let phi_bam = ((delta_n * 1000) >> 16).min(64) as usize;
    let sin_val = crate::destruction_audio::SINE_LUT_256[phi_bam] as i64;
    let eff_q16 = ((sin_val * sin_val) / 16384).clamp(0, 65536) as u32;
    cell.diffraction_eff_q16 = eff_q16;

    if eff_q16 > 32768 {
        cell.flags |= AO_FLAG_DEFLECTING | AO_FLAG_BRAGG_MATCHED;
    }

    // In-plane deflection shift
    let mut out_dir = input_ray_dir_xyz;
    let def_shift = ((input_ray_dir_xyz[0] as i64 * theta_b_mrad as i64) / 1000) as i32;
    out_dir[0] = input_ray_dir_xyz[0].saturating_sub(def_shift);
    out_dir[1] = input_ray_dir_xyz[1].saturating_add(def_shift);
    out_dir
}

/// Computes transformation optics permittivity tensor (epsilon_r, epsilon_theta)
/// for a query point relative to the cloaking shell.
#[inline(always)]
pub fn compute_transformation_optics_permittivity(
    field: &mut CloakingFieldSlab64,
    query_pos_xyz: [i32; 3],
) -> (u32, u32) {
    if (field.flags & CLOAK_FLAG_ACTIVE) == 0 {
        return (65536, 65536); // Free-space unity permittivity
    }

    let dx = (query_pos_xyz[0] - field.origin_xyz[0]) as i64;
    let dy = (query_pos_xyz[1] - field.origin_xyz[1]) as i64;
    let dz = (query_pos_xyz[2] - field.origin_xyz[2]) as i64;
    let r_sq = (dx * dx) + (dy * dy) + (dz * dz);
    let r = isqrt_u64(r_sq as u64) as u32;

    let r1 = field.radius_inner_mm;
    let r2 = field.radius_outer_mm;

    if r <= r1 {
        // Inside inner core: totally concealed / shielded
        field.radial_permittivity_q16 = 0;
        field.angular_permittivity_q16 = 65536 * 100; // Asymptotic high
        field.refractive_gradient_q16 = 65536 * 10;
        return (0, 65536 * 100);
    }

    if r >= r2 {
        // Outside shell: matched to background vacuum/air
        field.radial_permittivity_q16 = 65536;
        field.angular_permittivity_q16 = 65536;
        field.refractive_gradient_q16 = 0;
        return (65536, 65536);
    }

    // Inside transformation shell: R1 < r < R2
    // eps_r = (r - R1) / r
    let eps_r_q16 = (((r - r1) as u64) << 16) / (r as u64);
    // eps_theta = r / (r - R1)
    let eps_theta_q16 = ((r as u64) << 16) / ((r - r1).max(1) as u64);

    let eps_r_clamped = eps_r_q16.min(65536) as u32;
    let eps_theta_clamped = eps_theta_q16.min(65536 * 50) as u32;

    field.radial_permittivity_q16 = eps_r_clamped;
    field.angular_permittivity_q16 = eps_theta_clamped;
    field.refractive_gradient_q16 = eps_theta_clamped.saturating_sub(eps_r_clamped);

    (eps_r_clamped, eps_theta_clamped)
}

/// Bends a ray vector around the cloaking core if within the cloaking shell.
/// Returns (bent_ray_dir_xyz, was_deflected).
#[inline(always)]
pub fn compute_cloaking_ray_bend(
    field: &CloakingFieldSlab64,
    ray_pos_xyz: [i32; 3],
    ray_dir_xyz: [i32; 3],
) -> ([i32; 3], bool) {
    if (field.flags & CLOAK_FLAG_ACTIVE) == 0 {
        return (ray_dir_xyz, false);
    }

    let dx = (ray_pos_xyz[0] - field.origin_xyz[0]) as i64;
    let dy = (ray_pos_xyz[1] - field.origin_xyz[1]) as i64;
    let dz = (ray_pos_xyz[2] - field.origin_xyz[2]) as i64;
    let r_sq = (dx * dx) + (dy * dy) + (dz * dz);
    let r = isqrt_u64(r_sq as u64) as u32;

    let r1 = field.radius_inner_mm;
    let r2 = field.radius_outer_mm;

    if r >= r1 && r <= r2 {
        // Tangential bending away from the origin based on local gradient
        let mut bent = ray_dir_xyz;
        let eps_r = (((r - r1) as u64) << 16) / (r as u64);
        let eps_theta = ((r as u64) << 16) / ((r - r1).max(1) as u64);
        let grad = (eps_theta.saturating_sub(eps_r)).max(65536) as i64;

        let shift_x = ((dx * grad) >> 20) as i32;
        let shift_y = ((dy * grad) >> 20) as i32;

        bent[0] = ray_dir_xyz[0].saturating_add(shift_y);
        bent[1] = ray_dir_xyz[1].saturating_sub(shift_x);
        (bent, true)
    } else {
        (ray_dir_xyz, false)
    }
}

// ============================================================================
// Fixed-Capacity Deterministic Metamaterial Simulator
// ============================================================================

pub struct MechaMetamaterialEngine<const MAX_CELLS: usize, const MAX_FIELDS: usize> {
    pub cells: [AcoustoOpticCellSlab64; MAX_CELLS],
    pub fields: [CloakingFieldSlab64; MAX_FIELDS],
    pub active_cells_count: usize,
    pub active_fields_count: usize,
    pub tick: u32,
    pub total_deflected_photons_accum: u64,
    pub total_cloaked_volume_accum_mm3: u64,
    pub scatter_event_count_accum: u32,
}

impl<const MAX_CELLS: usize, const MAX_FIELDS: usize> Default
    for MechaMetamaterialEngine<MAX_CELLS, MAX_FIELDS>
{
    fn default() -> Self {
        Self::new()
    }
}

impl<const MAX_CELLS: usize, const MAX_FIELDS: usize>
    MechaMetamaterialEngine<MAX_CELLS, MAX_FIELDS>
{
    pub const fn new() -> Self {
        Self {
            cells: [AcoustoOpticCellSlab64 {
                center_xyz: [0, 0, 0],
                sound_vel_mms: 4_200_000,
                acoustic_freq_khz: 100_000,
                strain_q16: 655,
                delta_n_q16: 0,
                bragg_angle_mrad: 0,
                diffraction_eff_q16: 0,
                laser_wavelength_nm: 1064,
                flags: 0,
                entity_id: 0,
                reserved: [0u8; 24],
            }; MAX_CELLS],
            fields: [CloakingFieldSlab64 {
                origin_xyz: [0, 0, 0],
                radius_inner_mm: 1000,
                radius_outer_mm: 2000,
                radial_permittivity_q16: 32768,
                angular_permittivity_q16: 131072,
                refractive_gradient_q16: 65536,
                cloak_efficiency_pct: 95,
                emitter_id: 0,
                target_id: 0,
                flags: 0,
                reserved: [0u8; 27],
            }; MAX_FIELDS],
            active_cells_count: 0,
            active_fields_count: 0,
            tick: 0,
            total_deflected_photons_accum: 0,
            total_cloaked_volume_accum_mm3: 0,
            scatter_event_count_accum: 0,
        }
    }

    pub fn add_cell(&mut self, cell: AcoustoOpticCellSlab64) -> Option<usize> {
        if self.active_cells_count < MAX_CELLS {
            let idx = self.active_cells_count;
            self.cells[idx] = cell;
            self.active_cells_count += 1;
            Some(idx)
        } else {
            None
        }
    }

    pub fn add_field(&mut self, field: CloakingFieldSlab64) -> Option<usize> {
        if self.active_fields_count < MAX_FIELDS {
            let idx = self.active_fields_count;
            self.fields[idx] = field;
            self.active_fields_count += 1;
            Some(idx)
        } else {
            None
        }
    }

    pub fn step_simulation(&mut self, dt_ticks: u32) -> (usize, usize) {
        self.tick = self.tick.wrapping_add(dt_ticks);
        let mut deflected_count = 0;
        let dummy_ray = [32768, 0, 0];

        for i in 0..self.active_cells_count {
            if (self.cells[i].flags & AO_FLAG_ACTIVE) != 0 {
                let _out_ray = compute_acousto_optic_deflection(&mut self.cells[i], dummy_ray);
                if (self.cells[i].flags & AO_FLAG_DEFLECTING) != 0 {
                    deflected_count += 1;
                    self.total_deflected_photons_accum = self
                        .total_deflected_photons_accum
                        .wrapping_add(self.cells[i].diffraction_eff_q16 as u64);
                    self.scatter_event_count_accum =
                        self.scatter_event_count_accum.wrapping_add(1);
                }
            }
        }

        let mut cloaked_fields = 0;
        for j in 0..self.active_fields_count {
            if (self.fields[j].flags & CLOAK_FLAG_ACTIVE) != 0 {
                cloaked_fields += 1;
                let r1 = self.fields[j].radius_inner_mm as u64;
                let vol_approx = (4 * r1 * r1 * r1) / 3;
                self.total_cloaked_volume_accum_mm3 = self
                    .total_cloaked_volume_accum_mm3
                    .wrapping_add(vol_approx);
            }
        }

        (deflected_count, cloaked_fields)
    }

    pub fn compute_consensus_hash(&self) -> u64 {
        let mut hash = 0xcbf29ce484222325u64;
        const FNV_PRIME: u64 = 0x100000001b3;

        hash ^= self.tick as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
        hash ^= self.total_deflected_photons_accum;
        hash = hash.wrapping_mul(FNV_PRIME);
        hash ^= self.total_cloaked_volume_accum_mm3;
        hash = hash.wrapping_mul(FNV_PRIME);
        hash ^= self.scatter_event_count_accum as u64;
        hash = hash.wrapping_mul(FNV_PRIME);

        for i in 0..self.active_cells_count {
            let c = &self.cells[i];
            hash ^= c.center_xyz[0] as u64;
            hash = hash.wrapping_mul(FNV_PRIME);
            hash ^= c.bragg_angle_mrad as u64;
            hash = hash.wrapping_mul(FNV_PRIME);
            hash ^= c.diffraction_eff_q16 as u64;
            hash = hash.wrapping_mul(FNV_PRIME);
            hash ^= c.flags as u64;
            hash = hash.wrapping_mul(FNV_PRIME);
        }

        for j in 0..self.active_fields_count {
            let f = &self.fields[j];
            hash ^= f.origin_xyz[0] as u64;
            hash = hash.wrapping_mul(FNV_PRIME);
            hash ^= f.cloak_efficiency_pct as u64;
            hash = hash.wrapping_mul(FNV_PRIME);
            hash ^= f.flags as u64;
            hash = hash.wrapping_mul(FNV_PRIME);
        }

        hash
    }
}

// ============================================================================
// EventBus & Multithreaded Job System Integration
// ============================================================================

pub fn emit_metamaterial_to_event_bus<const BUS_CAP: usize>(
    report: &MetamaterialEventReport,
    tick: u32,
    event_bus: &mut DeterministicEventBus<BUS_CAP>,
) -> bool {
    let payload = report.encode();
    let event_type = if report.event_kind == META_EVENT_KIND_SCATTER {
        EVENT_BRILLOUIN_SCATTER
    } else {
        EVENT_OPTICAL_CLOAK
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

pub fn metamaterial_batch_job(ctx: JobContext) {
    let start_idx = (ctx.param & 0xFFFF_FFFF) as usize;
    let chunk_size = (ctx.param >> 32) as usize;
    let mut _accum: u64 = 0;
    for i in 0..chunk_size {
        _accum = _accum.wrapping_add((start_idx + i) as u64);
    }
}

pub fn dispatch_metamaterial_with_jobs<
    const MAX_CELLS: usize,
    const MAX_FIELDS: usize,
    const SYS_CAP: usize,
    const MAX_WORKERS: usize,
    const DEQUE_CAP: usize,
>(
    engine: &MechaMetamaterialEngine<MAX_CELLS, MAX_FIELDS>,
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
        if len == 0 {
            break;
        }
        let param = (start as u64) | ((len as u64) << 32);
        let tag = (engine.active_fields_count as u64) | ((b as u64) << 32);

        if let Some(h) =
            job_system.allocate_job(metamaterial_batch_job, param, tag, JobPriority::High)
        {
            let worker_id = b % coordinator.worker_count;
            job_system.descriptors[h.slot()].status = JobStatus::Ready as u8;
            coordinator.push_local(worker_id, h);
            dispatched += 1;
        }
    }
    dispatched
}
