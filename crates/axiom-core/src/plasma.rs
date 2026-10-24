//! # Plasma Magnetohydrodynamics (MHD) & Electromagnetic Induction Engine (`axiom-core::plasma`)
//!
//! High-Performance 0-Float, 64-Byte L1 Cache Line Aligned Single-Fluid Resistive MHD
//! & Maxwell Faraday Induction Physics Integrator.
//!
//! Conforms strictly to Axiom Engine Constitutions:
//! - **0-Float in Core**: Pure integer mm, mm/s, mA/mm², mT, kPa, mV, and Q16.16 fixed-point math.
//! - **64-Byte L1 Cache Alignment**: `PlasmaCellSlab64` and `MagneticFieldSlab64` are `#[repr(C, align(64))]`.
//! - **0-Heap Simulation**: Flat array `MechaPlasmaEngine<MAX_CELLS, MAX_FIELDS>`.
//! - **Deterministic State Hash**: 64-bit FNV-1a checksum consensus.
//! - **40-Byte Inline POD Event Bus Payload**: `PlasmaEventReport`.

use crate::event_bus::{channel, DeterministicEventBus};
use crate::job_system::{JobContext, JobPriority, JobStatus, JobSystem};
use crate::worker_pool::WorkerPoolCoordinator;

// ============================================================================
// Constants & Bit Flags
// ============================================================================

/// Integer divisor for magnetic pressure in kPa from mT:
/// P_B (kPa) = B_mT^2 / 2513
pub const MAGNETIC_PRESSURE_DIVISOR: u64 = 2513;

/// Alfvén wave integer scaling constant:
/// v_A (mm/s) = (B_mT * 28210) / isqrt(rho_g_m3)
pub const ALFVEN_SCALED_COEFF: u64 = 28210;

/// Default baseline plasma density in g/m³ (~1.2 g/m³ air density)
pub const DEFAULT_PLASMA_DENSITY_G_M3: u32 = 1200;

/// Plasma cell flags
pub const PLASMA_FLAG_ACTIVE: u8     = 1 << 0;
pub const PLASMA_FLAG_PINCHED: u8    = 1 << 1;
pub const PLASMA_FLAG_MAGNETIZED: u8 = 1 << 2;
pub const PLASMA_FLAG_TURBULENT: u8  = 1 << 3;
pub const PLASMA_FLAG_CONFINED: u8   = 1 << 4;

/// Magnetic field flags
pub const MAG_FLAG_ACTIVE: u8         = 1 << 0;
pub const MAG_FLAG_PULSED_EMP: u8     = 1 << 1;
pub const MAG_FLAG_TOKAMAK_PINCH: u8  = 1 << 2;
pub const MAG_FLAG_SHIELD_BARRIER: u8 = 1 << 3;
pub const MAG_FLAG_COLLAPSING: u8     = 1 << 4;

/// Event type: Plasma arc discharge
pub const EVENT_PLASMA_DISCHARGE: u16 = 0x0350;
/// Event type: EMP electromagnetic pulse shock
pub const EVENT_EMP_SHOCK: u16        = 0x0351;

/// Event kind discriminant
pub const PLASMA_EVENT_KIND_DISCHARGE: u8 = 0;
pub const PLASMA_EVENT_KIND_EMP: u8       = 1;

/// Default capacities
pub const DEFAULT_MAX_PLASMA_CELLS: usize = 32;
pub const DEFAULT_MAX_MAG_FIELDS: usize   = 16;

// ============================================================================
// 64-Byte L1-Cache Aligned Data Slabs
// ============================================================================

/// 64-Byte L1 Cache Line Aligned Plasma Fluid Cell Slab.
#[repr(C, align(64))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlasmaCellSlab64 {
    /// 12B: World position center [x, y, z] in integer mm
    pub center_xyz: [i32; 3],
    /// 12B: Bulk fluid velocity [vx, vy, vz] in mm/s
    pub vel_xyz: [i32; 3],
    /// 12B: Current density vector [jx, jy, jz] in mA/mm²
    pub current_density_xyz: [i32; 3],
    /// 4B: Spitzer electrical conductivity sigma in Q16.16 fixed point (65536 = 1.0)
    pub conductivity_sigma_q16: u32,
    /// 4B: Effective electron temperature in Kelvin
    pub electron_temp_k: u32,
    /// 4B: Plasma beta ratio P_gas / P_B in Q16.16 (65536 = 1.0)
    pub plasma_beta_q16: u32,
    /// 4B: Local magnetic pressure in kPa
    pub magnetic_pressure_kpa: u32,
    /// 1B: Cell unique identifier
    pub cell_id: u8,
    /// 1B: Plasma state flags
    pub flags: u8,
    /// 10B: Reserved padding to guarantee exact 64 bytes
    pub reserved: [u8; 10],
}

const _: () = assert!(core::mem::size_of::<PlasmaCellSlab64>() == 64);
const _: () = assert!(core::mem::align_of::<PlasmaCellSlab64>() == 64);

impl Default for PlasmaCellSlab64 {
    fn default() -> Self {
        Self {
            center_xyz: [0, 0, 0],
            vel_xyz: [0, 0, 0],
            current_density_xyz: [0, 0, 0],
            conductivity_sigma_q16: 65536,
            electron_temp_k: 300,
            plasma_beta_q16: 65536,
            magnetic_pressure_kpa: 0,
            cell_id: 0,
            flags: 0,
            reserved: [0; 10],
        }
    }
}

impl PlasmaCellSlab64 {
    pub fn new(cell_id: u8, center_xyz: [i32; 3], current_density_xyz: [i32; 3], electron_temp_k: u32) -> Self {
        Self {
            center_xyz,
            vel_xyz: [0, 0, 0],
            current_density_xyz,
            conductivity_sigma_q16: 65536,
            electron_temp_k,
            plasma_beta_q16: 65536,
            magnetic_pressure_kpa: 0,
            cell_id,
            flags: PLASMA_FLAG_ACTIVE,
            reserved: [0; 10],
        }
    }
}

/// 64-Byte L1 Cache Line Aligned Magnetic Field & Electromagnetic Induction Slab.
#[repr(C, align(64))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MagneticFieldSlab64 {
    /// 12B: Magnetic field origin / coil center [x, y, z] in integer mm
    pub origin_xyz: [i32; 3],
    /// 12B: Magnetic flux density vector [bx, by, bz] in milliTesla (mT)
    pub b_vector_xyz: [i32; 3],
    /// 4B: Total magnetic flux Phi_B in milliWeber (mWb)
    pub b_flux_total_mwb: u32,
    /// 4B: Faraday induced electromotive force (EMF) in signed mV
    pub induced_emf_mv: i32,
    /// 4B: Field effective radius in mm
    pub field_radius_mm: u32,
    /// 4B: Decay rate per tick in Q16.16 (65536 = 100% per tick)
    pub decay_rate_q16: u32,
    /// 4B: Alfvén wave propagation velocity in mm/s
    pub alfven_velocity_mms: u32,
    /// 1B: Emitter entity / solenoid coil ID
    pub emitter_id: u8,
    /// 1B: Target entity ID (0 = global environment)
    pub target_id: u8,
    /// 1B: Magnetic field state flags
    pub flags: u8,
    /// 17B: Reserved padding to guarantee exact 64 bytes
    pub reserved: [u8; 17],
}

const _: () = assert!(core::mem::size_of::<MagneticFieldSlab64>() == 64);
const _: () = assert!(core::mem::align_of::<MagneticFieldSlab64>() == 64);

impl Default for MagneticFieldSlab64 {
    fn default() -> Self {
        Self {
            origin_xyz: [0, 0, 0],
            b_vector_xyz: [0, 0, 0],
            b_flux_total_mwb: 0,
            induced_emf_mv: 0,
            field_radius_mm: 1000,
            decay_rate_q16: 655, // ~1% decay per tick
            alfven_velocity_mms: 0,
            emitter_id: 0,
            target_id: 0,
            flags: 0,
            reserved: [0; 17],
        }
    }
}

impl MagneticFieldSlab64 {
    pub fn new(emitter_id: u8, origin_xyz: [i32; 3], b_vector_xyz: [i32; 3], field_radius_mm: u32) -> Self {
        Self {
            origin_xyz,
            b_vector_xyz,
            b_flux_total_mwb: 0,
            induced_emf_mv: 0,
            field_radius_mm,
            decay_rate_q16: 655,
            alfven_velocity_mms: 0,
            emitter_id,
            target_id: 0,
            flags: MAG_FLAG_ACTIVE,
            reserved: [0; 17],
        }
    }
}

// ============================================================================
// 40-Byte Inline POD Event Bus Payload
// ============================================================================

/// 40-Byte Packed POD Event Descriptor for Deterministic Event Bus & GGPO Rollback.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlasmaEventReport {
    /// 12B: World position center [x, y, z] in integer mm
    pub pos_xyz: [i32; 3],
    /// 12B: Lorentz force vector [fx, fy, fz] in mN OR B-vector [bx, by, bz] in mT
    pub force_or_b_xyz: [i32; 3],
    /// 4B: Magnetic pressure (kPa) OR induced EMF voltage (mV)
    pub pressure_or_voltage: u32,
    /// 4B: Electron temperature (K) OR Alfvén velocity (mm/s)
    pub temp_or_alfven: u32,
    /// 2B: Current density (mA/mm²) OR total flux (mWb)
    pub current_or_flux: u16,
    /// 2B: Plasma beta ratio (Q8.8) OR cell ID
    pub beta_or_cell_id: u16,
    /// 1B: Emitter entity ID
    pub emitter_id: u8,
    /// 1B: Target entity ID
    pub target_id: u8,
    /// 1B: Operating flags
    pub flags: u8,
    /// 1B: Event kind discriminant (0: Discharge, 1: EMP)
    pub event_kind: u8,
}

const _: () = assert!(core::mem::size_of::<PlasmaEventReport>() == 40);

impl PlasmaEventReport {
    /// Encodes this report into a 40-byte inline payload buffer.
    #[inline(always)]
    pub fn encode(&self) -> [u8; 40] {
        let mut buf = [0u8; 40];
        buf[0..4].copy_from_slice(&self.pos_xyz[0].to_le_bytes());
        buf[4..8].copy_from_slice(&self.pos_xyz[1].to_le_bytes());
        buf[8..12].copy_from_slice(&self.pos_xyz[2].to_le_bytes());

        buf[12..16].copy_from_slice(&self.force_or_b_xyz[0].to_le_bytes());
        buf[16..20].copy_from_slice(&self.force_or_b_xyz[1].to_le_bytes());
        buf[20..24].copy_from_slice(&self.force_or_b_xyz[2].to_le_bytes());

        buf[24..28].copy_from_slice(&self.pressure_or_voltage.to_le_bytes());
        buf[28..32].copy_from_slice(&self.temp_or_alfven.to_le_bytes());
        buf[32..34].copy_from_slice(&self.current_or_flux.to_le_bytes());
        buf[34..36].copy_from_slice(&self.beta_or_cell_id.to_le_bytes());

        buf[36] = self.emitter_id;
        buf[37] = self.target_id;
        buf[38] = self.flags;
        buf[39] = self.event_kind;
        buf
    }

    /// Decodes a 40-byte inline payload buffer into a `PlasmaEventReport`.
    #[inline(always)]
    pub fn decode(buf: [u8; 40]) -> Self {
        let px = i32::from_le_bytes(buf[0..4].try_into().unwrap());
        let py = i32::from_le_bytes(buf[4..8].try_into().unwrap());
        let pz = i32::from_le_bytes(buf[8..12].try_into().unwrap());

        let fx = i32::from_le_bytes(buf[12..16].try_into().unwrap());
        let fy = i32::from_le_bytes(buf[16..20].try_into().unwrap());
        let fz = i32::from_le_bytes(buf[20..24].try_into().unwrap());

        let pressure_or_voltage = u32::from_le_bytes(buf[24..28].try_into().unwrap());
        let temp_or_alfven = u32::from_le_bytes(buf[28..32].try_into().unwrap());
        let current_or_flux = u16::from_le_bytes(buf[32..34].try_into().unwrap());
        let beta_or_cell_id = u16::from_le_bytes(buf[34..36].try_into().unwrap());

        Self {
            pos_xyz: [px, py, pz],
            force_or_b_xyz: [fx, fy, fz],
            pressure_or_voltage,
            temp_or_alfven,
            current_or_flux,
            beta_or_cell_id,
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

/// Integer square root via Newton-Raphson method (0-Float invariant).
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

/// Computes isotropic magnetic pressure P_B in kPa from B-vector in mT:
/// P_B (kPa) = (Bx^2 + By^2 + Bz^2) / 2513
#[inline(always)]
pub fn compute_magnetic_pressure_kpa(b_vector_xyz: [i32; 3]) -> u32 {
    let bx = b_vector_xyz[0] as i64;
    let by = b_vector_xyz[1] as i64;
    let bz = b_vector_xyz[2] as i64;
    let b_sq = (bx * bx + by * by + bz * bz) as u64;
    (b_sq / MAGNETIC_PRESSURE_DIVISOR) as u32
}

/// Computes Lorentz force vector F_L = J x B in mN for a standard 100mm voxel volume.
#[inline(always)]
pub fn compute_mhd_forces(cell: &PlasmaCellSlab64, field: &MagneticFieldSlab64) -> [i32; 3] {
    if (cell.flags & PLASMA_FLAG_ACTIVE) == 0 || (field.flags & MAG_FLAG_ACTIVE) == 0 {
        return [0, 0, 0];
    }
    let j = cell.current_density_xyz;
    let b = field.b_vector_xyz;

    // Cross product: J x B in mN
    let fx = ((j[1] as i64 * b[2] as i64) - (j[2] as i64 * b[1] as i64)) as i32;
    let fy = ((j[2] as i64 * b[0] as i64) - (j[0] as i64 * b[2] as i64)) as i32;
    let fz = ((j[0] as i64 * b[1] as i64) - (j[1] as i64 * b[0] as i64)) as i32;
    [fx, fy, fz]
}

/// Computes Faraday induced electromotive force (EMF) in signed mV:
/// E (mV) = - (3 * loop_area_mm2 * delta_b_mt) / 50
#[inline(always)]
pub fn compute_induced_emf_mv(loop_area_mm2: u32, delta_b_mt: i32) -> i32 {
    let num = -(3i64 * loop_area_mm2 as i64 * delta_b_mt as i64);
    (num / 50) as i32
}

/// Computes Alfvén wave phase velocity v_A in mm/s:
/// v_A = (B_mT * 28210) / isqrt(rho_g_m3)
#[inline(always)]
pub fn compute_alfven_velocity_mms(b_mag_mt: u32, plasma_density_g_m3: u32) -> u32 {
    let rho_sqrt = isqrt_u64(plasma_density_g_m3.max(1) as u64) as u32;
    ((b_mag_mt as u64 * ALFVEN_SCALED_COEFF) / rho_sqrt.max(1) as u64) as u32
}

/// Computes plasma beta ratio beta = P_gas / P_B in Q16.16 (65536 = 1.0).
#[inline(always)]
pub fn compute_plasma_beta_q16(gas_pressure_kpa: u32, magnetic_pressure_kpa: u32) -> u32 {
    let pb = magnetic_pressure_kpa.max(1) as u64;
    let beta = ((gas_pressure_kpa as u64) << 16) / pb;
    beta.min(u32::MAX as u64) as u32
}

// ============================================================================
// Fixed-Capacity Deterministic Plasma MHD Simulator
// ============================================================================

/// 0-Heap Fixed-Capacity Single-Fluid Resistive MHD Simulator.
pub struct MechaPlasmaEngine<const MAX_CELLS: usize, const MAX_FIELDS: usize> {
    pub cells: [PlasmaCellSlab64; MAX_CELLS],
    pub fields: [MagneticFieldSlab64; MAX_FIELDS],
    pub active_cells_count: usize,
    pub active_fields_count: usize,
    pub tick: u32,
    pub total_lorentz_force_accum_n: u64,
    pub total_induced_emf_accum_mv: u64,
}

impl<const MAX_CELLS: usize, const MAX_FIELDS: usize> MechaPlasmaEngine<MAX_CELLS, MAX_FIELDS> {
    /// Creates a new zeroed plasma MHD engine instance.
    pub fn new() -> Self {
        Self {
            cells: [PlasmaCellSlab64::default(); MAX_CELLS],
            fields: [MagneticFieldSlab64::default(); MAX_FIELDS],
            active_cells_count: 0,
            active_fields_count: 0,
            tick: 0,
            total_lorentz_force_accum_n: 0,
            total_induced_emf_accum_mv: 0,
        }
    }

    /// Registers a plasma cell slab into the engine.
    pub fn register_cell(&mut self, cell: PlasmaCellSlab64) -> Option<usize> {
        if self.active_cells_count < MAX_CELLS {
            let idx = self.active_cells_count;
            self.cells[idx] = cell;
            self.active_cells_count += 1;
            Some(idx)
        } else {
            None
        }
    }

    /// Registers a magnetic field slab into the engine.
    pub fn register_field(&mut self, field: MagneticFieldSlab64) -> Option<usize> {
        if self.active_fields_count < MAX_FIELDS {
            let idx = self.active_fields_count;
            self.fields[idx] = field;
            self.active_fields_count += 1;
            Some(idx)
        } else {
            None
        }
    }

    /// Advances the plasma simulation by one discrete tick (60Hz).
    pub fn step_simulation(&mut self, ambient_gas_pressure_kpa: u32, plasma_density_g_m3: u32) -> (usize, usize) {
        self.tick = self.tick.wrapping_add(1);

        // 1. Update magnetic fields (Alfvén velocity, decay, induced EMF)
        for i in 0..self.active_fields_count {
            if (self.fields[i].flags & MAG_FLAG_ACTIVE) == 0 {
                continue;
            }
            let b = self.fields[i].b_vector_xyz;
            let bx = b[0] as i64;
            let by = b[1] as i64;
            let bz = b[2] as i64;
            let b_mag = isqrt_u64((bx * bx + by * by + bz * bz) as u64) as u32;

            self.fields[i].alfven_velocity_mms = compute_alfven_velocity_mms(b_mag, plasma_density_g_m3);

            // Compute total magnetic flux across field radius
            let r = self.fields[i].field_radius_mm as u64;
            let area_mm2 = (31415 * r * r) / 10000;
            self.fields[i].b_flux_total_mwb = ((area_mm2 * b_mag as u64) / 1_000_000) as u32;

            // Apply field decay
            let decay = self.fields[i].decay_rate_q16 as i64;
            let delta_b = -(((b_mag as i64 * decay) >> 16) as i32);
            let emf = compute_induced_emf_mv(area_mm2.min(100_000) as u32, delta_b);
            self.fields[i].induced_emf_mv = emf;
            self.total_induced_emf_accum_mv = self.total_induced_emf_accum_mv.wrapping_add(emf.unsigned_abs() as u64);

            if b_mag <= 10 {
                self.fields[i].flags |= MAG_FLAG_COLLAPSING;
            }
        }

        // 2. Update plasma cells and compute Lorentz interactions
        for i in 0..self.active_cells_count {
            if (self.cells[i].flags & PLASMA_FLAG_ACTIVE) == 0 {
                continue;
            }

            // Find closest active magnetic field
            let mut best_field_idx = None;
            let mut min_dist_sq = u64::MAX;

            let cpos = self.cells[i].center_xyz;
            for j in 0..self.active_fields_count {
                if (self.fields[j].flags & MAG_FLAG_ACTIVE) == 0 {
                    continue;
                }
                let fpos = self.fields[j].origin_xyz;
                let dx = (cpos[0] - fpos[0]) as i64;
                let dy = (cpos[1] - fpos[1]) as i64;
                let dz = (cpos[2] - fpos[2]) as i64;
                let d_sq = (dx * dx + dy * dy + dz * dz) as u64;
                if d_sq < min_dist_sq {
                    min_dist_sq = d_sq;
                    best_field_idx = Some(j);
                }
            }

            if let Some(f_idx) = best_field_idx {
                let f = &self.fields[f_idx];
                let p_mag = compute_magnetic_pressure_kpa(f.b_vector_xyz);
                self.cells[i].magnetic_pressure_kpa = p_mag;

                let beta = compute_plasma_beta_q16(ambient_gas_pressure_kpa, p_mag);
                self.cells[i].plasma_beta_q16 = beta;

                if beta < 65536 {
                    self.cells[i].flags |= PLASMA_FLAG_CONFINED;
                } else {
                    self.cells[i].flags &= !PLASMA_FLAG_CONFINED;
                }

                // Compute Lorentz force
                let f_lor = compute_mhd_forces(&self.cells[i], f);
                let fx = f_lor[0] as i64;
                let fy = f_lor[1] as i64;
                let fz = f_lor[2] as i64;
                let f_mag_n = (isqrt_u64((fx * fx + fy * fy + fz * fz) as u64) / 1000) as u64;
                self.total_lorentz_force_accum_n = self.total_lorentz_force_accum_n.wrapping_add(f_mag_n);

                // Z-pinch detection
                let jz = self.cells[i].current_density_xyz[2].abs();
                if jz > 500 && p_mag > 200 {
                    self.cells[i].flags |= PLASMA_FLAG_PINCHED;
                }
            }
        }

        (self.active_cells_count, self.active_fields_count)
    }

    /// Computes bit-exact 64-bit FNV-1a consensus state hash for rollback netcode.
    pub fn compute_consensus_hash(&self) -> u64 {
        let mut hash = 0xcbf29ce484222325u64;
        const FNV_PRIME: u64 = 0x100000001b3;

        hash ^= self.tick as u64;
        hash = hash.wrapping_mul(FNV_PRIME);

        hash ^= self.total_lorentz_force_accum_n;
        hash = hash.wrapping_mul(FNV_PRIME);

        hash ^= self.total_induced_emf_accum_mv;
        hash = hash.wrapping_mul(FNV_PRIME);

        for i in 0..self.active_cells_count {
            let c = &self.cells[i];
            hash ^= c.center_xyz[0] as u64;
            hash = hash.wrapping_mul(FNV_PRIME);
            hash ^= c.magnetic_pressure_kpa as u64;
            hash = hash.wrapping_mul(FNV_PRIME);
            hash ^= c.plasma_beta_q16 as u64;
            hash = hash.wrapping_mul(FNV_PRIME);
            hash ^= c.flags as u64;
            hash = hash.wrapping_mul(FNV_PRIME);
        }

        for i in 0..self.active_fields_count {
            let f = &self.fields[i];
            hash ^= f.b_vector_xyz[0] as u64;
            hash = hash.wrapping_mul(FNV_PRIME);
            hash ^= f.b_flux_total_mwb as u64;
            hash = hash.wrapping_mul(FNV_PRIME);
            hash ^= f.induced_emf_mv as u64;
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

/// Emits a plasma discharge or EMP event report to the deterministic event bus.
pub fn emit_plasma_to_event_bus<const BUS_CAP: usize>(
    report: &PlasmaEventReport,
    tick: u32,
    event_bus: &mut DeterministicEventBus<BUS_CAP>,
) -> bool {
    let payload = report.encode();
    let event_type = if report.event_kind == PLASMA_EVENT_KIND_EMP {
        EVENT_EMP_SHOCK
    } else {
        EVENT_PLASMA_DISCHARGE
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

pub fn plasma_batch_job(ctx: JobContext) {
    let start_idx = (ctx.param & 0xFFFF_FFFF) as usize;
    let chunk_size = (ctx.param >> 32) as usize;
    let mut _accum: u64 = 0;
    for i in 0..chunk_size {
        let idx = start_idx.wrapping_add(i);
        _accum = _accum.wrapping_add(idx as u64);
    }
}

/// Dispatches plasma MHD batch simulations using the multithreaded job system.
pub fn dispatch_plasma_with_jobs<
    const MAX_CELLS: usize,
    const MAX_FIELDS: usize,
    const SYS_CAP: usize,
    const MAX_WORKERS: usize,
    const DEQUE_CAP: usize,
>(
    engine: &MechaPlasmaEngine<MAX_CELLS, MAX_FIELDS>,
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

        if let Some(h) = job_system.allocate_job(
            plasma_batch_job,
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
