//! # Mecha Superconductivity & Quantum Levitation Engine (`axiom-core::superconductor`)
//!
//! High-Performance 0-Float, 64-Byte L1 Cache Line Aligned Type I/II Superconductivity,
//! Meissner Effect Diamagnetism & Quantum Levitation Flux Pinning Physics Integrator.
//!
//! Conforms strictly to Axiom Engine Constitutions:
//! - **0-Float in Core**: Pure integer mm, mm/s, mN, mT, K, and Q16.16 fixed-point math.
//! - **64-Byte L1 Cache Alignment**: `SuperconductorSlab64` and `FluxPinningSlab64` are `#[repr(C, align(64))]`.
//! - **0-Heap Simulation**: Flat array `MechaSuperconductorEngine<MAX_CELLS, MAX_PINS>`.
//! - **Deterministic State Hash**: 64-bit FNV-1a checksum consensus.
//! - **40-Byte Inline POD Event Bus Payload**: `SuperconductorEventReport`.

use crate::event_bus::{channel, DeterministicEventBus};
use crate::job_system::{JobContext, JobPriority, JobStatus, JobSystem};
use crate::worker_pool::WorkerPoolCoordinator;

// ============================================================================
// Constants & Bit Flags
// ============================================================================

/// Integer divisor for magnetic pressure in kPa from mT:
/// P_B (kPa) = B_mT^2 / 2513
pub const MAGNETIC_PRESSURE_DIVISOR: u64 = 2513;

/// Conversion factor from (kPa * mm^2) to N: 1000 mN = 1 N
pub const MN_TO_N_DIVISOR: u64 = 1000;

/// Superconductor Types
pub const SC_TYPE_I: u8 = 0;  // Complete Meissner, abrupt quench at Bc
pub const SC_TYPE_II: u8 = 1; // Abrikosov vortex state & flux pinning between Bc1 and Bc2

/// Superconductor State Flags
pub const SC_FLAG_ACTIVE: u8      = 1 << 0;
pub const SC_FLAG_LEVITATING: u8  = 1 << 1;
pub const SC_FLAG_QUENCHED: u8    = 1 << 2;
pub const SC_FLAG_FLUX_PINNED: u8 = 1 << 3;
pub const SC_FLAG_MEISSNER: u8    = 1 << 4;

/// Flux Pinning Flags
pub const PIN_FLAG_ACTIVE: u8     = 1 << 0;
pub const PIN_FLAG_LOCKED: u8     = 1 << 1;
pub const PIN_FLAG_SLIPPING: u8   = 1 << 2;
pub const PIN_FLAG_OVERLOADED: u8 = 1 << 3;

/// Event Types
pub const EVENT_FLUX_PINNING: u16        = 0x0360;
pub const EVENT_SUPERCONDUCT_QUENCH: u16 = 0x0361;

/// Event Kind Discriminants
pub const SC_EVENT_KIND_PINNING: u8 = 0;
pub const SC_EVENT_KIND_QUENCH: u8  = 1;

/// Default Capacities
pub const DEFAULT_MAX_SC_CELLS: usize = 16;
pub const DEFAULT_MAX_PIN_SLABS: usize = 8;

// ============================================================================
// 64-Byte L1-Cache Aligned Data Slabs
// ============================================================================

/// 64-Byte L1 Cache Line Aligned Superconductor Element Slab.
#[repr(C, align(64))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SuperconductorSlab64 {
    /// 12B: World position center [x, y, z] in integer mm
    pub center_xyz: [i32; 3],
    /// 12B: Linear velocity [vx, vy, vz] in mm/s
    pub vel_xyz: [i32; 3],
    /// 4B: Operating temperature in Kelvin (e.g. 77 K liquid N2, 4 K liquid He)
    pub temp_k: u32,
    /// 4B: Critical temperature T_c in Kelvin
    pub crit_temp_k: u32,
    /// 4B: Upper critical magnetic field B_c2 in milliTesla (mT)
    pub crit_b_field_mt: u32,
    /// 4B: Critical current density J_c in mA/mm²
    pub crit_current_density: u32,
    /// 4B: Total current computed levitation/pinning force in Newtons (N)
    pub pinning_force_n: i32,
    /// 1B: Superconductor type (0: Type I, 1: Type II)
    pub superconductor_type: u8,
    /// 1B: Superconductor operational flags
    pub flags: u8,
    /// 1B: Owning entity ID
    pub entity_id: u8,
    /// 17B: Explicit padding ensuring exact 64 bytes
    pub reserved: [u8; 17],
}

const _: () = assert!(core::mem::size_of::<SuperconductorSlab64>() == 64);
const _: () = assert!(core::mem::align_of::<SuperconductorSlab64>() == 64);

impl Default for SuperconductorSlab64 {
    fn default() -> Self {
        Self {
            center_xyz: [0, 0, 0],
            vel_xyz: [0, 0, 0],
            temp_k: 77,
            crit_temp_k: 93,
            crit_b_field_mt: 30_000,
            crit_current_density: 10_000,
            pinning_force_n: 0,
            superconductor_type: SC_TYPE_II,
            flags: SC_FLAG_ACTIVE,
            entity_id: 0,
            reserved: [0u8; 17],
        }
    }
}

impl SuperconductorSlab64 {
    pub fn new(
        entity_id: u8,
        center_xyz: [i32; 3],
        temp_k: u32,
        crit_temp_k: u32,
        crit_b_field_mt: u32,
        superconductor_type: u8,
    ) -> Self {
        Self {
            center_xyz,
            vel_xyz: [0, 0, 0],
            temp_k,
            crit_temp_k,
            crit_b_field_mt,
            crit_current_density: 10_000,
            pinning_force_n: 0,
            superconductor_type,
            flags: SC_FLAG_ACTIVE,
            entity_id,
            reserved: [0u8; 17],
        }
    }
}

/// 64-Byte L1 Cache Line Aligned Flux Pinning & Levitation Anchor Slab.
#[repr(C, align(64))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FluxPinningSlab64 {
    /// 12B: Trapped flux magnetic anchor position [x, y, z] in integer mm
    pub anchor_xyz: [i32; 3],
    /// 4B: Pinned magnetic flux Phi in milliWeber (mWb)
    pub pinned_flux_mwb: u32,
    /// 4B: Spatial restoring spring stiffness k in Q16.16 (mN/mm)
    pub stiffness_k_q16: u32,
    /// 4B: Kinematic damping coefficient c in Q16.16 (mN / (mm/s))
    pub damping_c_q16: u32,
    /// 4B: Maximum threshold pinning force before vortex slip in Newtons (N)
    pub max_pin_force_n: u32,
    /// 4B: Target equilibrium levitation height in mm
    pub trapped_height_mm: u32,
    /// 1B: Target entity ID
    pub target_id: u8,
    /// 1B: Solenoid track / magnet emitter ID
    pub emitter_id: u8,
    /// 1B: Pinning operational flags
    pub flags: u8,
    /// 29B: Explicit padding ensuring exact 64 bytes
    pub reserved: [u8; 29],
}

const _: () = assert!(core::mem::size_of::<FluxPinningSlab64>() == 64);
const _: () = assert!(core::mem::align_of::<FluxPinningSlab64>() == 64);

impl Default for FluxPinningSlab64 {
    fn default() -> Self {
        Self {
            anchor_xyz: [0, 0, 0],
            pinned_flux_mwb: 100,
            stiffness_k_q16: 65536 * 50, // 50 N/m = 50 mN/mm
            damping_c_q16: 65536 * 5,    // damping
            max_pin_force_n: 50_000,     // 50 kN
            trapped_height_mm: 100,      // 100 mm height
            target_id: 0,
            emitter_id: 0,
            flags: PIN_FLAG_ACTIVE | PIN_FLAG_LOCKED,
            reserved: [0u8; 29],
        }
    }
}

impl FluxPinningSlab64 {
    pub fn new(
        emitter_id: u8,
        target_id: u8,
        anchor_xyz: [i32; 3],
        pinned_flux_mwb: u32,
        stiffness_k_q16: u32,
        damping_c_q16: u32,
        max_pin_force_n: u32,
        trapped_height_mm: u32,
    ) -> Self {
        Self {
            anchor_xyz,
            pinned_flux_mwb,
            stiffness_k_q16,
            damping_c_q16,
            max_pin_force_n,
            trapped_height_mm,
            target_id,
            emitter_id,
            flags: PIN_FLAG_ACTIVE | PIN_FLAG_LOCKED,
            reserved: [0u8; 29],
        }
    }
}

// ============================================================================
// 40-Byte Inline POD Event Bus Payload
// ============================================================================

/// 40-Byte Packed POD Event Descriptor for Deterministic Event Bus & GGPO Rollback.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SuperconductorEventReport {
    /// 12B: World position center [x, y, z] in integer mm
    pub pos_xyz: [i32; 3],
    /// 12B: Pinning or quench force vector [fx, fy, fz] in mN
    pub force_xyz: [i32; 3],
    /// 4B: Trapped magnetic flux (mWb) or induced quench voltage (mV)
    pub flux_or_voltage: u32,
    /// 4B: Operating temperature in Kelvin (K)
    pub temp_k: u32,
    /// 2B: Spatial displacement magnitude |x - x_pin| in mm
    pub pin_displacement_mm: u16,
    /// 2B: Critical parameter ratio (T/Tc or B/Bc2) in Q8.8 format
    pub crit_ratio_q8: u16,
    /// 1B: Emitter entity ID
    pub emitter_id: u8,
    /// 1B: Target entity ID
    pub target_id: u8,
    /// 1B: Operating status flags
    pub flags: u8,
    /// 1B: Event kind discriminant (0: Pinning, 1: Quench)
    pub event_kind: u8,
}

const _: () = assert!(core::mem::size_of::<SuperconductorEventReport>() == 40);

impl Default for SuperconductorEventReport {
    fn default() -> Self {
        Self {
            pos_xyz: [0, 0, 0],
            force_xyz: [0, 0, 0],
            flux_or_voltage: 0,
            temp_k: 0,
            pin_displacement_mm: 0,
            crit_ratio_q8: 0,
            emitter_id: 0,
            target_id: 0,
            flags: 0,
            event_kind: SC_EVENT_KIND_PINNING,
        }
    }
}

impl SuperconductorEventReport {
    #[inline(always)]
    pub fn encode(&self) -> [u8; 40] {
        let mut buf = [0u8; 40];
        buf[0..4].copy_from_slice(&self.pos_xyz[0].to_le_bytes());
        buf[4..8].copy_from_slice(&self.pos_xyz[1].to_le_bytes());
        buf[8..12].copy_from_slice(&self.pos_xyz[2].to_le_bytes());

        buf[12..16].copy_from_slice(&self.force_xyz[0].to_le_bytes());
        buf[16..20].copy_from_slice(&self.force_xyz[1].to_le_bytes());
        buf[20..24].copy_from_slice(&self.force_xyz[2].to_le_bytes());

        buf[24..28].copy_from_slice(&self.flux_or_voltage.to_le_bytes());
        buf[28..32].copy_from_slice(&self.temp_k.to_le_bytes());
        buf[32..34].copy_from_slice(&self.pin_displacement_mm.to_le_bytes());
        buf[34..36].copy_from_slice(&self.crit_ratio_q8.to_le_bytes());

        buf[36] = self.emitter_id;
        buf[37] = self.target_id;
        buf[38] = self.flags;
        buf[39] = self.event_kind;
        buf
    }

    #[inline(always)]
    pub fn decode(buf: [u8; 40]) -> Self {
        let px = i32::from_le_bytes(buf[0..4].try_into().unwrap());
        let py = i32::from_le_bytes(buf[4..8].try_into().unwrap());
        let pz = i32::from_le_bytes(buf[8..12].try_into().unwrap());

        let fx = i32::from_le_bytes(buf[12..16].try_into().unwrap());
        let fy = i32::from_le_bytes(buf[16..20].try_into().unwrap());
        let fz = i32::from_le_bytes(buf[20..24].try_into().unwrap());

        let flux_or_voltage = u32::from_le_bytes(buf[24..28].try_into().unwrap());
        let temp_k = u32::from_le_bytes(buf[28..32].try_into().unwrap());
        let pin_displacement_mm = u16::from_le_bytes(buf[32..34].try_into().unwrap());
        let crit_ratio_q8 = u16::from_le_bytes(buf[34..36].try_into().unwrap());

        Self {
            pos_xyz: [px, py, pz],
            force_xyz: [fx, fy, fz],
            flux_or_voltage,
            temp_k,
            pin_displacement_mm,
            crit_ratio_q8,
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

/// Computes Meissner diamagnetic repulsive force in Newtons:
/// F_rep (N) = (B_mT^2 * area_mm2) / 2_513_000
#[inline(always)]
pub fn compute_meissner_repulsion_n(b_field_mt: u32, area_mm2: u32) -> u32 {
    let b = b_field_mt as u64;
    let a = area_mm2 as u64;
    let num = b * b * a;
    (num / 2_513_000) as u32
}

/// Computes 3D flux pinning restoring force in mN:
/// F_pin = -k * delta_x - c * v
#[inline(always)]
pub fn compute_superconductor_forces(
    cell: &SuperconductorSlab64,
    pin: &FluxPinningSlab64,
) -> [i32; 3] {
    if (cell.flags & SC_FLAG_ACTIVE) == 0 || (pin.flags & PIN_FLAG_ACTIVE) == 0 {
        return [0, 0, 0];
    }
    if (cell.flags & SC_FLAG_QUENCHED) != 0 {
        return [0, 0, 0];
    }

    let mut force_xyz = [0i32; 3];
    let k = pin.stiffness_k_q16 as i64;
    let c = pin.damping_c_q16 as i64;
    let max_n = pin.max_pin_force_n as i64;
    let max_mn = max_n * 1000;

    for i in 0..3 {
        let dx = (cell.center_xyz[i] - pin.anchor_xyz[i]) as i64;
        let v = cell.vel_xyz[i] as i64;

        let spring_mn = -((k * dx) >> 16);
        let damp_mn = -((c * v) >> 16);
        let f_mn = spring_mn + damp_mn;

        force_xyz[i] = f_mn.clamp(-max_mn, max_mn) as i32;
    }

    force_xyz
}

// ============================================================================
// Fixed-Capacity Deterministic Superconductor Simulator
// ============================================================================

pub struct MechaSuperconductorEngine<const MAX_CELLS: usize, const MAX_PINS: usize> {
    pub cells: [SuperconductorSlab64; MAX_CELLS],
    pub pins: [FluxPinningSlab64; MAX_PINS],
    pub active_cells_count: usize,
    pub active_pins_count: usize,
    pub tick: u32,
    pub total_pinning_force_accum_n: u64,
    pub total_meissner_repulsion_accum_n: u64,
    pub quench_count_accum: u32,
}

impl<const MAX_CELLS: usize, const MAX_PINS: usize> Default for MechaSuperconductorEngine<MAX_CELLS, MAX_PINS> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const MAX_CELLS: usize, const MAX_PINS: usize> MechaSuperconductorEngine<MAX_CELLS, MAX_PINS> {
    pub fn new() -> Self {
        Self {
            cells: [SuperconductorSlab64::default(); MAX_CELLS],
            pins: [FluxPinningSlab64::default(); MAX_PINS],
            active_cells_count: 0,
            active_pins_count: 0,
            tick: 0,
            total_pinning_force_accum_n: 0,
            total_meissner_repulsion_accum_n: 0,
            quench_count_accum: 0,
        }
    }

    pub fn register_cell(&mut self, cell: SuperconductorSlab64) -> Option<usize> {
        if self.active_cells_count < MAX_CELLS {
            let idx = self.active_cells_count;
            self.cells[idx] = cell;
            self.active_cells_count += 1;
            Some(idx)
        } else {
            None
        }
    }

    pub fn register_pin(&mut self, pin: FluxPinningSlab64) -> Option<usize> {
        if self.active_pins_count < MAX_PINS {
            let idx = self.active_pins_count;
            self.pins[idx] = pin;
            self.active_pins_count += 1;
            Some(idx)
        } else {
            None
        }
    }

    pub fn step_simulation(&mut self, ambient_b_field_mt: u32) -> (usize, usize) {
        self.tick = self.tick.wrapping_add(1);

        for i in 0..self.active_cells_count {
            if (self.cells[i].flags & SC_FLAG_ACTIVE) == 0 {
                continue;
            }

            // Quench condition check: T >= Tc OR B >= Bc2
            let is_quenched = self.cells[i].temp_k >= self.cells[i].crit_temp_k
                || ambient_b_field_mt >= self.cells[i].crit_b_field_mt;

            if is_quenched {
                self.cells[i].flags |= SC_FLAG_QUENCHED;
                self.cells[i].flags &= !(SC_FLAG_LEVITATING | SC_FLAG_FLUX_PINNED | SC_FLAG_MEISSNER);
                self.cells[i].pinning_force_n = 0;
                self.quench_count_accum = self.quench_count_accum.wrapping_add(1);
                continue;
            }

            self.cells[i].flags &= !SC_FLAG_QUENCHED;

            // Type I: Pure Meissner repulsion
            if self.cells[i].superconductor_type == SC_TYPE_I {
                self.cells[i].flags |= SC_FLAG_MEISSNER;
                let f_rep_n = compute_meissner_repulsion_n(ambient_b_field_mt, 10_000); // 100x100mm area
                self.cells[i].pinning_force_n = f_rep_n as i32;
                self.total_meissner_repulsion_accum_n = self.total_meissner_repulsion_accum_n.wrapping_add(f_rep_n as u64);
                continue;
            }

            // Type II: Evaluate closest active pinning anchor
            let mut best_pin_idx = None;
            let mut min_dist_sq = u64::MAX;
            let cpos = self.cells[i].center_xyz;

            for j in 0..self.active_pins_count {
                if (self.pins[j].flags & PIN_FLAG_ACTIVE) == 0 {
                    continue;
                }
                let apos = self.pins[j].anchor_xyz;
                let dx = (cpos[0] - apos[0]) as i64;
                let dy = (cpos[1] - apos[1]) as i64;
                let dz = (cpos[2] - apos[2]) as i64;
                let d_sq = (dx * dx + dy * dy + dz * dz) as u64;
                if d_sq < min_dist_sq {
                    min_dist_sq = d_sq;
                    best_pin_idx = Some(j);
                }
            }

            if let Some(p_idx) = best_pin_idx {
                let pin = &self.pins[p_idx];
                let f_mn = compute_superconductor_forces(&self.cells[i], pin);
                let fx = f_mn[0] as i64;
                let fy = f_mn[1] as i64;
                let fz = f_mn[2] as i64;
                let f_mag_n = (isqrt_u64((fx * fx + fy * fy + fz * fz) as u64) / 1000) as i32;

                self.cells[i].pinning_force_n = f_mag_n;
                self.cells[i].flags |= SC_FLAG_FLUX_PINNED | SC_FLAG_LEVITATING;
                self.total_pinning_force_accum_n = self.total_pinning_force_accum_n.wrapping_add(f_mag_n.unsigned_abs() as u64);
            }
        }

        (self.active_cells_count, self.active_pins_count)
    }

    pub fn compute_consensus_hash(&self) -> u64 {
        let mut hash = 0xcbf29ce484222325u64;
        const FNV_PRIME: u64 = 0x100000001b3;

        hash ^= self.tick as u64;
        hash = hash.wrapping_mul(FNV_PRIME);

        hash ^= self.total_pinning_force_accum_n;
        hash = hash.wrapping_mul(FNV_PRIME);

        hash ^= self.total_meissner_repulsion_accum_n;
        hash = hash.wrapping_mul(FNV_PRIME);

        hash ^= self.quench_count_accum as u64;
        hash = hash.wrapping_mul(FNV_PRIME);

        for i in 0..self.active_cells_count {
            let c = &self.cells[i];
            hash ^= c.center_xyz[0] as u64;
            hash = hash.wrapping_mul(FNV_PRIME);
            hash ^= c.temp_k as u64;
            hash = hash.wrapping_mul(FNV_PRIME);
            hash ^= c.pinning_force_n as u64;
            hash = hash.wrapping_mul(FNV_PRIME);
            hash ^= c.flags as u64;
            hash = hash.wrapping_mul(FNV_PRIME);
        }

        for j in 0..self.active_pins_count {
            let p = &self.pins[j];
            hash ^= p.anchor_xyz[0] as u64;
            hash = hash.wrapping_mul(FNV_PRIME);
            hash ^= p.pinned_flux_mwb as u64;
            hash = hash.wrapping_mul(FNV_PRIME);
            hash ^= p.flags as u64;
            hash = hash.wrapping_mul(FNV_PRIME);
        }

        hash
    }
}

// ============================================================================
// EventBus & Multithreaded Job System Integration
// ============================================================================

pub fn emit_superconductor_to_event_bus<const BUS_CAP: usize>(
    report: &SuperconductorEventReport,
    tick: u32,
    event_bus: &mut DeterministicEventBus<BUS_CAP>,
) -> bool {
    let payload = report.encode();
    let event_type = if report.event_kind == SC_EVENT_KIND_QUENCH {
        EVENT_SUPERCONDUCT_QUENCH
    } else {
        EVENT_FLUX_PINNING
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

pub fn superconductor_batch_job(ctx: JobContext) {
    let start_idx = (ctx.param & 0xFFFF_FFFF) as usize;
    let chunk_size = (ctx.param >> 32) as usize;
    let mut _accum: u64 = 0;
    for i in 0..chunk_size {
        let idx = start_idx.wrapping_add(i);
        _accum = _accum.wrapping_add(idx as u64);
    }
}

pub fn dispatch_superconductor_with_jobs<
    const MAX_CELLS: usize,
    const MAX_PINS: usize,
    const SYS_CAP: usize,
    const MAX_WORKERS: usize,
    const DEQUE_CAP: usize,
>(
    engine: &MechaSuperconductorEngine<MAX_CELLS, MAX_PINS>,
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
        let tag = (engine.active_pins_count as u64) | ((b as u64) << 32);

        if let Some(h) = job_system.allocate_job(
            superconductor_batch_job,
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
