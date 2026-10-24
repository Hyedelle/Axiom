//! # Procedural Atmospheric Turbulence & Compressible Shock Boundary Layer Engine (`axiom-core::turbulence`)
//!
//! High-Performance 0-Float, 64-Byte L1 Cache Line Aligned Atmospheric
//! Turbulence & Compressible Shock-Induced Boundary Layer Separation (SBLI) Integrator.
//!
//! Conforms strictly to Axiom Engine Constitutions:
//! - **0-Float in Core**: Pure integer mm, mm/s, mN, and Q16.16 fixed-point math.
//! - **64-Byte L1 Cache Alignment**: `TurbulenceCellSlab64` and `ShockBoundarySlab64` are `#[repr(C, align(64))]`.
//! - **0-Heap Simulation**: Flat array `MechaTurbulenceEngine<MAX_CELLS, MAX_SHOCKS>`.
//! - **Deterministic State Hash**: 64-bit FNV-1a checksum consensus.
//! - **40-Byte Inline POD Event Bus Payload**: `TurbulenceEventReport`.

use crate::event_bus::{channel, DeterministicEventBus};
use crate::job_system::{JobContext, JobPriority, JobStatus, JobSystem};
use crate::worker_pool::WorkerPoolCoordinator;

// ============================================================================
// Constants & Bit Flags
// ============================================================================

/// Speed of sound in standard atmosphere in mm/s (340 m/s = 340,000 mm/s)
pub const SPEED_OF_SOUND_MMS: u32 = 340_000;

/// Standard air density in g/m³ (1.225 kg/m³)
pub const STANDARD_AIR_DENSITY_G_M3: u32 = 1225;

/// Turbulence cell flags
pub const TURB_FLAG_ACTIVE: u8       = 1 << 0;
pub const TURB_FLAG_VORTEX_CORE: u8  = 1 << 1;
pub const TURB_FLAG_CONFINED: u8     = 1 << 2;
pub const TURB_FLAG_WAKE_EDDY: u8    = 1 << 3;
pub const TURB_FLAG_SEPARATED: u8    = 1 << 4;

/// Shock boundary flags
pub const SHOCK_BL_ACTIVE: u8        = 1 << 0;
pub const SHOCK_BL_SUPERSONIC: u8    = 1 << 1;
pub const SHOCK_BL_SEPARATED: u8     = 1 << 2;
pub const SHOCK_BL_BUFFETING: u8     = 1 << 3;
pub const SHOCK_BL_REFLECTED: u8     = 1 << 4;

/// Event type: Atmospheric turbulence vortex shedding / confinement event
pub const EVENT_TURBULENCE_VORTEX: u16 = 0x0330;
/// Event type: Compressible shock boundary layer separation (SBLI) event
pub const EVENT_SHOCK_BOUNDARY: u16    = 0x0331;

/// Event kind discriminant
pub const TURB_EVENT_KIND_VORTEX: u8   = 0;
pub const TURB_EVENT_KIND_SHOCK_BL: u8 = 1;

/// Default capacities
pub const DEFAULT_MAX_TURB_CELLS: usize = 32;
pub const DEFAULT_MAX_SHOCK_BOUNDARIES: usize = 16;

// ============================================================================
// 64-Byte L1-Cache Aligned Data Slabs
// ============================================================================

/// 64-Byte L1 Cache Line Aligned Atmospheric Turbulence & Vorticity Cell Slab.
#[repr(C, align(64))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TurbulenceCellSlab64 {
    /// 12B: World position center [x, y, z] in integer mm
    pub center_xyz: [i32; 3],
    /// 12B: 3D Discrete vorticity vector [wx, wy, wz] in mrad/s
    pub vorticity_xyz: [i32; 3],
    /// 4B: Turbulent kinetic energy (TKE) k in Q16.16 ((m/s)^2)
    pub tke_q16: u32,
    /// 4B: Turbulence dissipation rate epsilon in Q16.16 ((m/s)^3)
    pub dissipation_q16: u32,
    /// 4B: Turbulent eddy viscosity nu_t in Q16.16 (m^2/s)
    pub eddy_viscosity_q16: u32,
    /// 12B: Velocity fluctuation perturbation [u'x, u'y, u'z] in mm/s
    pub velocity_fluc_xyz: [i32; 3],
    /// 1B: Cell identifier
    pub cell_id: u8,
    /// 1B: Status flags (ACTIVE, VORTEX_CORE, CONFINED, WAKE_EDDY, SEPARATED)
    pub flags: u8,
    /// 2B: Local atmospheric temperature in Celsius
    pub temp_c: i16,
    /// 12B: Explicit alignment padding to exact 64 bytes
    pub reserved: [u8; 12],
}

/// 64-Byte L1 Cache Line Aligned Shock-Induced Boundary Layer (SBLI) Slab.
#[repr(C, align(64))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShockBoundarySlab64 {
    /// 12B: Shock impact origin [x, y, z] in integer mm
    pub origin_xyz: [i32; 3],
    /// 12B: Shockwave surface normal [nx, ny, nz] in Q1.15 fixed-point
    pub normal_xyz: [i32; 3],
    /// 4B: Mach number M in Q16.16 (65536 = 1.0 Mach = 340 m/s)
    pub mach_q16: u32,
    /// 4B: Rankine-Hugoniot pressure jump ratio P2/P1 in Q16.16
    pub pressure_jump_ratio_q16: u32,
    /// 2B: Boundary layer thickness delta in integer mm
    pub bl_thickness_mm: u16,
    /// 2B: Separation bubble detachment distance x_sep in integer mm
    pub separation_distance_mm: u16,
    /// 4B: Peak shock impulse force in Newtons (N)
    pub peak_shock_impulse_n: u32,
    /// 1B: Emitter mecha or projectile ID
    pub emitter_id: u8,
    /// 1B: Operational flags (SUPERSONIC, SEPARATED, BUFFETING, REFLECTED)
    pub flags: u8,
    /// 22B: Explicit alignment padding to exact 64 bytes
    pub reserved: [u8; 22],
}

/// 40-Byte Inline POD Turbulence & Shock Boundary Event Report (Event Bus Payload).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TurbulenceEventReport {
    /// 12B: World position [x, y, z] in integer mm
    pub pos_xyz: [i32; 3],
    /// 12B: Vorticity or surface normal vector [x, y, z]
    pub vector_xyz: [i32; 3],
    /// 4B: Transverse buffeting force or peak shock impulse in N
    pub impulse_or_force_n: i32,
    /// 4B: TKE or Rankine-Hugoniot pressure jump ratio P2/P1 in Q16.16
    pub tke_or_pressure_ratio_q16: u32,
    /// 2B: Mach number in Q8.8 or vortex shedding frequency in Q8.8 Hz
    pub mach_or_freq_q8: u16,
    /// 2B: Boundary layer thickness in mm or Cell ID
    pub bl_thickness_or_cell_id: u16,
    /// 1B: Emitter entity ID
    pub emitter_id: u8,
    /// 1B: Target entity ID (0 = environment broadcast)
    pub target_id: u8,
    /// 1B: Operational flags
    pub flags: u8,
    /// 1B: Event discriminant kind (TURB_EVENT_KIND_*)
    pub event_kind: u8,
}

// Compile-time memory layout assertions
const _: () = {
    assert!(core::mem::size_of::<TurbulenceCellSlab64>() == 64);
    assert!(core::mem::align_of::<TurbulenceCellSlab64>() == 64);
    assert!(core::mem::size_of::<ShockBoundarySlab64>() == 64);
    assert!(core::mem::align_of::<ShockBoundarySlab64>() == 64);
    assert!(core::mem::size_of::<TurbulenceEventReport>() == 40);
};

// ============================================================================
// Serialization & Math Helpers
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

impl TurbulenceCellSlab64 {
    /// Creates a new turbulence cell slab.
    pub fn new(cell_id: u8, center_xyz: [i32; 3], initial_vorticity: [i32; 3]) -> Self {
        Self {
            center_xyz,
            vorticity_xyz: initial_vorticity,
            tke_q16: 0,
            dissipation_q16: 6553, // initial ~0.1 dissipation
            eddy_viscosity_q16: 5898, // C_mu ~ 0.09
            velocity_fluc_xyz: [0; 3],
            cell_id,
            flags: TURB_FLAG_ACTIVE,
            temp_c: 15,
            reserved: [0; 12],
        }
    }
}

impl ShockBoundarySlab64 {
    /// Creates a new shock-induced boundary layer slab.
    pub fn new(
        emitter_id: u8,
        origin_xyz: [i32; 3],
        normal_xyz: [i32; 3],
        bl_thickness_mm: u16,
        peak_shock_impulse_n: u32,
    ) -> Self {
        Self {
            origin_xyz,
            normal_xyz,
            mach_q16: 65536, // 1.0 Mach
            pressure_jump_ratio_q16: 65536, // 1.0 (no jump)
            bl_thickness_mm: bl_thickness_mm.max(1),
            separation_distance_mm: 0,
            peak_shock_impulse_n,
            emitter_id,
            flags: SHOCK_BL_ACTIVE,
            reserved: [0; 22],
        }
    }
}

impl TurbulenceEventReport {
    /// Serializes report into a 40-byte POD payload.
    pub fn encode(&self) -> [u8; 40] {
        let mut buf = [0u8; 40];
        buf[0..4].copy_from_slice(&self.pos_xyz[0].to_le_bytes());
        buf[4..8].copy_from_slice(&self.pos_xyz[1].to_le_bytes());
        buf[8..12].copy_from_slice(&self.pos_xyz[2].to_le_bytes());
        buf[12..16].copy_from_slice(&self.vector_xyz[0].to_le_bytes());
        buf[16..20].copy_from_slice(&self.vector_xyz[1].to_le_bytes());
        buf[20..24].copy_from_slice(&self.vector_xyz[2].to_le_bytes());
        buf[24..28].copy_from_slice(&self.impulse_or_force_n.to_le_bytes());
        buf[28..32].copy_from_slice(&self.tke_or_pressure_ratio_q16.to_le_bytes());
        buf[32..34].copy_from_slice(&self.mach_or_freq_q8.to_le_bytes());
        buf[34..36].copy_from_slice(&self.bl_thickness_or_cell_id.to_le_bytes());
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
        let vx = i32::from_le_bytes(payload[12..16].try_into().unwrap_or([0; 4]));
        let vy = i32::from_le_bytes(payload[16..20].try_into().unwrap_or([0; 4]));
        let vz = i32::from_le_bytes(payload[20..24].try_into().unwrap_or([0; 4]));
        let impulse = i32::from_le_bytes(payload[24..28].try_into().unwrap_or([0; 4]));
        let tke = u32::from_le_bytes(payload[28..32].try_into().unwrap_or([0; 4]));
        let mach = u16::from_le_bytes(payload[32..34].try_into().unwrap_or([0; 2]));
        let bl_or_id = u16::from_le_bytes(payload[34..36].try_into().unwrap_or([0; 2]));
        let emitter_id = payload[36];
        let target_id = payload[37];
        let flags = payload[38];
        let event_kind = payload[39];

        Self {
            pos_xyz: [px, py, pz],
            vector_xyz: [vx, vy, vz],
            impulse_or_force_n: impulse,
            tke_or_pressure_ratio_q16: tke,
            mach_or_freq_q8: mach,
            bl_thickness_or_cell_id: bl_or_id,
            emitter_id,
            target_id,
            flags,
            event_kind,
        }
    }
}

// ============================================================================
// Core Physics Solvers
// ============================================================================

/// Computes Vorticity Confinement Force (F_vc = eps_vc * (N x w)) and updates TKE.
pub fn compute_turbulence_forces(
    cell: &mut TurbulenceCellSlab64,
    confinement_eps_q16: u16,
) -> [i32; 3] {
    if (cell.flags & TURB_FLAG_ACTIVE) == 0 {
        return [0; 3];
    }

    let wx = cell.vorticity_xyz[0] as i64;
    let wy = cell.vorticity_xyz[1] as i64;
    let wz = cell.vorticity_xyz[2] as i64;
    let w_sq = wx * wx + wy * wy + wz * wz;

    if w_sq == 0 {
        return [0; 3];
    }

    let w_mag = isqrt_u64(w_sq as u64) as i64;
    if w_mag > 500 {
        cell.flags |= TURB_FLAG_VORTEX_CORE;
    } else {
        cell.flags &= !TURB_FLAG_VORTEX_CORE;
    }

    // Direction N pointing towards vortex core (approximated from perturbation)
    let nx = -wy;
    let ny = wx;
    let nz = 0i64;
    let n_mag = isqrt_u64((nx * nx + ny * ny + nz * nz) as u64) as i64;
    let (norm_nx, norm_ny, norm_nz) = if n_mag > 0 {
        ((nx * 32768) / n_mag, (ny * 32768) / n_mag, (nz * 32768) / n_mag)
    } else {
        (0, 32768, 0)
    };

    // Cross product: N x w
    let cross_x = (norm_ny * wz - norm_nz * wy) / 32768;
    let cross_y = (norm_nz * wx - norm_nx * wz) / 32768;
    let cross_z = (norm_nx * wy - norm_ny * wx) / 32768;

    // F_vc in mN
    let eps = confinement_eps_q16 as i64;
    let f_vc_x = ((cross_x * eps) / 65536) as i32;
    let f_vc_y = ((cross_y * eps) / 65536) as i32;
    let f_vc_z = ((cross_z * eps) / 65536) as i32;

    if eps > 0 {
        cell.flags |= TURB_FLAG_CONFINED;
    }

    // Discrete TKE k-epsilon integration (60Hz)
    // Production P_k ~ nu_t * w^2 / scale (w_mag in mrad/s, so div by 1,000,000)
    let nu_t = (cell.eddy_viscosity_q16 as u64).max(5898);
    let p_k = (nu_t * (w_mag as u64) * (w_mag as u64)) / 1_000_000;
    let eps_decay = cell.dissipation_q16 as u64;

    let k_next = (cell.tke_q16 as u64)
        .saturating_add(p_k / 60)
        .saturating_sub(eps_decay / 60);
    cell.tke_q16 = k_next.min(u32::MAX as u64) as u32;

    // Dissipation decay
    cell.dissipation_q16 = (cell.dissipation_q16 as u64)
        .saturating_sub((eps_decay / 120).max(1)) as u32;

    // Update eddy viscosity: nu_t = C_mu * k^2 / eps (C_mu ~ 0.09 = 5898)
    if cell.dissipation_q16 > 0 && cell.tke_q16 > 0 {
        let k = cell.tke_q16 as u64;
        let eps = cell.dissipation_q16 as u64;
        let k_sq = (k * k) / 65536;
        let nu = (5898 * k_sq) / eps;
        cell.eddy_viscosity_q16 = nu.clamp(5898, 655360) as u32;
    }

    // Update velocity fluctuations from TKE
    let v_rms = isqrt_u64((cell.tke_q16 as u64) * 2000) as i32;
    cell.velocity_fluc_xyz[0] = (f_vc_x.signum()) * (v_rms / 3);
    cell.velocity_fluc_xyz[1] = (f_vc_y.signum()) * (v_rms / 3);
    cell.velocity_fluc_xyz[2] = (f_vc_z.signum()) * (v_rms / 3);

    [f_vc_x, f_vc_y, f_vc_z]
}

/// Solves Rankine-Hugoniot pressure jump and SBLI boundary layer separation.
pub fn compute_shock_boundary_interaction(
    shock: &mut ShockBoundarySlab64,
    flow_speed_mms: u32,
) -> (u32, i32) {
    if (shock.flags & SHOCK_BL_ACTIVE) == 0 {
        return (65536, 0);
    }

    // Mach number M = v / c_sound
    let mach_q16 = ((flow_speed_mms as u64 * 65536) / SPEED_OF_SOUND_MMS as u64) as u32;
    shock.mach_q16 = mach_q16;

    if mach_q16 < 65536 {
        shock.flags &= !(SHOCK_BL_SUPERSONIC | SHOCK_BL_SEPARATED | SHOCK_BL_BUFFETING);
        shock.pressure_jump_ratio_q16 = 65536;
        shock.separation_distance_mm = 0;
        return (65536, 0);
    }

    shock.flags |= SHOCK_BL_SUPERSONIC;

    // Rankine-Hugoniot Pressure Jump: P2/P1 = 1 + (7/6)*(M^2 - 1)
    let m_sq_q16 = ((mach_q16 as u64 * mach_q16 as u64) / 65536) as u64;
    let delta_m_sq = m_sq_q16.saturating_sub(65536);
    let p_jump_ratio = 65536 + ((76458 * delta_m_sq) / 65536);
    shock.pressure_jump_ratio_q16 = p_jump_ratio.min(u32::MAX as u64) as u32;

    // SBLI Criterion: Detachment when P2/P1 >= 1.20 (78643 in Q16.16)
    let mut buffet_force_n = 0i32;
    if shock.pressure_jump_ratio_q16 >= 78643 {
        shock.flags |= SHOCK_BL_SEPARATED | SHOCK_BL_BUFFETING;
        let delta_m = (mach_q16.saturating_sub(65536)) as u64;
        shock.separation_distance_mm = ((shock.bl_thickness_mm as u64 * delta_m) / 32768) as u16;

        // High-frequency buffeting force in N
        let dp_ratio = (shock.pressure_jump_ratio_q16 - 65536) as u64;
        buffet_force_n = ((shock.peak_shock_impulse_n as u64 * dp_ratio) / 65536) as i32;
    } else {
        shock.flags &= !(SHOCK_BL_SEPARATED | SHOCK_BL_BUFFETING);
        shock.separation_distance_mm = 0;
    }

    (shock.pressure_jump_ratio_q16, buffet_force_n)
}

// ============================================================================
// Fixed-Capacity Deterministic Mecha Turbulence Engine
// ============================================================================

/// 0-Heap Fixed-Capacity Mecha Turbulence & Shock Boundary Engine.
pub struct MechaTurbulenceEngine<const MAX_CELLS: usize, const MAX_SHOCKS: usize> {
    pub cells: [TurbulenceCellSlab64; MAX_CELLS],
    pub shocks: [ShockBoundarySlab64; MAX_SHOCKS],
    pub active_cells_count: usize,
    pub active_shocks_count: usize,
    pub tick: u32,
    pub total_dissipated_energy_j: u64,
    pub total_shock_impulse_n: u64,
}

impl<const MAX_CELLS: usize, const MAX_SHOCKS: usize>
    MechaTurbulenceEngine<MAX_CELLS, MAX_SHOCKS>
{
    pub fn new() -> Self {
        let dummy_cell = TurbulenceCellSlab64 {
            center_xyz: [0; 3],
            vorticity_xyz: [0; 3],
            tke_q16: 0,
            dissipation_q16: 0,
            eddy_viscosity_q16: 5898,
            velocity_fluc_xyz: [0; 3],
            cell_id: 0,
            flags: 0,
            temp_c: 15,
            reserved: [0; 12],
        };
        let dummy_shock = ShockBoundarySlab64 {
            origin_xyz: [0; 3],
            normal_xyz: [0, 32768, 0],
            mach_q16: 65536,
            pressure_jump_ratio_q16: 65536,
            bl_thickness_mm: 10,
            separation_distance_mm: 0,
            peak_shock_impulse_n: 0,
            emitter_id: 0,
            flags: 0,
            reserved: [0; 22],
        };

        Self {
            cells: [dummy_cell; MAX_CELLS],
            shocks: [dummy_shock; MAX_SHOCKS],
            active_cells_count: 0,
            active_shocks_count: 0,
            tick: 0,
            total_dissipated_energy_j: 0,
            total_shock_impulse_n: 0,
        }
    }

    pub fn register_cell(&mut self, cell: TurbulenceCellSlab64) -> Option<usize> {
        if self.active_cells_count >= MAX_CELLS { return None; }
        let idx = self.active_cells_count;
        self.cells[idx] = cell;
        self.active_cells_count += 1;
        Some(idx)
    }

    pub fn register_shock(&mut self, shock: ShockBoundarySlab64) -> Option<usize> {
        if self.active_shocks_count >= MAX_SHOCKS { return None; }
        let idx = self.active_shocks_count;
        self.shocks[idx] = shock;
        self.active_shocks_count += 1;
        Some(idx)
    }

    pub fn step_simulation(&mut self, confinement_eps_q16: u16) -> (usize, usize) {
        self.tick = self.tick.wrapping_add(1);
        let mut evaluated_cells = 0;
        let mut separated_shocks = 0;

        for i in 0..self.active_cells_count {
            if (self.cells[i].flags & TURB_FLAG_ACTIVE) == 0 { continue; }
            compute_turbulence_forces(&mut self.cells[i], confinement_eps_q16);
            evaluated_cells += 1;
            let diss_j = (self.cells[i].dissipation_q16 as u64) / 65536;
            self.total_dissipated_energy_j = self.total_dissipated_energy_j.wrapping_add(diss_j);
        }

        for i in 0..self.active_shocks_count {
            if (self.shocks[i].flags & SHOCK_BL_ACTIVE) == 0 { continue; }
            let local_speed = SPEED_OF_SOUND_MMS + (self.shocks[i].mach_q16.saturating_sub(65536) * 340 / 65536);
            let (_p_ratio, buffet_n) = compute_shock_boundary_interaction(
                &mut self.shocks[i],
                local_speed,
            );
            if (self.shocks[i].flags & SHOCK_BL_SEPARATED) != 0 {
                separated_shocks += 1;
            }
            self.total_shock_impulse_n = self.total_shock_impulse_n.wrapping_add(buffet_n.unsigned_abs() as u64);
        }

        (evaluated_cells, separated_shocks)
    }

    pub fn compute_consensus_hash(&self) -> u64 {
        let mut h = 0xcbf29ce484222325u64;
        for b in self.tick.to_le_bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        for b in self.total_dissipated_energy_j.to_le_bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        for b in self.total_shock_impulse_n.to_le_bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }

        for i in 0..self.active_cells_count {
            let bytes = unsafe {
                core::slice::from_raw_parts(
                    &self.cells[i] as *const _ as *const u8,
                    core::mem::size_of::<TurbulenceCellSlab64>(),
                )
            };
            for &b in bytes {
                h ^= b as u64;
                h = h.wrapping_mul(0x100000001b3);
            }
        }

        for i in 0..self.active_shocks_count {
            let bytes = unsafe {
                core::slice::from_raw_parts(
                    &self.shocks[i] as *const _ as *const u8,
                    core::mem::size_of::<ShockBoundarySlab64>(),
                )
            };
            for &b in bytes {
                h ^= b as u64;
                h = h.wrapping_mul(0x100000001b3);
            }
        }

        h
    }
}

// ============================================================================
// Event Bus & Job System Integration
// ============================================================================

/// Emits a turbulence or shock boundary event to the deterministic event bus.
pub fn emit_turbulence_to_event_bus<const BUS_CAP: usize>(
    report: &TurbulenceEventReport,
    tick: u32,
    event_bus: &mut DeterministicEventBus<BUS_CAP>,
) -> bool {
    let payload = report.encode();
    let event_type = if report.event_kind == TURB_EVENT_KIND_SHOCK_BL {
        EVENT_SHOCK_BOUNDARY
    } else {
        EVENT_TURBULENCE_VORTEX
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

pub fn turbulence_batch_job(ctx: JobContext) {
    let start_idx = (ctx.param & 0xFFFF_FFFF) as usize;
    let chunk_size = (ctx.param >> 32) as usize;
    let mut _accum: u64 = 0;
    for i in 0..chunk_size {
        let idx = start_idx.wrapping_add(i);
        _accum = _accum.wrapping_add(idx as u64);
    }
}

pub fn dispatch_turbulence_with_jobs<
    const MAX_CELLS: usize,
    const MAX_SHOCKS: usize,
    const SYS_CAP: usize,
    const MAX_WORKERS: usize,
    const DEQUE_CAP: usize,
>(
    engine: &MechaTurbulenceEngine<MAX_CELLS, MAX_SHOCKS>,
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
        let tag = (engine.active_shocks_count as u64) | ((b as u64) << 32);

        if let Some(h) = job_system.allocate_job(
            turbulence_batch_job,
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
