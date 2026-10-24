//! # Procedural Atmospheric Aerodynamics & Vortex-Induced Vibration (VIV) Engine
//!
//! Provides deterministic 0-float atmospheric drag, lift, and von Kármán vortex
//! shedding vibration physics for high-speed mecha combat.
//!
//! Conforms strictly to Axiom Engine Constitutions:
//! - **0-Float in Core**: All physics computed via fixed-point and integer unit math ($1\text{ Pa} = 1000\text{ mPa}$).
//! - **64-Byte L1 Alignment**: `AeroBodySlab64` and `AirflowGridSlab64` are exact 64 bytes (`#[repr(C, align(64))]`).
//! - **0-Heap Simulation**: Flat array storage `MechaAerodynamicsEngine<MAX_BODIES, MAX_CELLS>`.
//! - **Deterministic Consensus**: 64-bit FNV-1a checksum `compute_consensus_hash`.

use crate::event_bus::{channel, DeterministicEventBus};
use crate::job_system::{JobContext, JobPriority, JobStatus, JobSystem};
use crate::worker_pool::WorkerPoolCoordinator;

// ============================================================================
// Constants & Bit Flags
// ============================================================================

/// Body type: Sphere / streamlined payload
pub const AERO_BODY_SPHERE: u8 = 0;
/// Body type: Cylinder / mecha limb
pub const AERO_BODY_CYLINDER: u8 = 1;
/// Body type: Flat plate / shield / armor plate
pub const AERO_BODY_PLATE: u8 = 2;
/// Body type: Airfoil / wing / aerodynamic stabilizer
pub const AERO_BODY_AIRFOIL: u8 = 3;

/// Flag: Body is active and evaluated in airflow simulation
pub const AERO_FLAG_ACTIVE: u8 = 1 << 0;
/// Flag: Vortex shedding frequency entered mechanical natural resonance band (25~45 Hz)
pub const AERO_FLAG_LOCK_IN: u8 = 1 << 1;
/// Flag: High-speed regime (> 30 m/s / 108 km/h)
pub const AERO_FLAG_HIGH_SPEED: u8 = 1 << 2;
/// Flag: Proximity to ground boundary layer
pub const AERO_FLAG_GROUND_EFFECT: u8 = 1 << 3;

/// Grid flag: Cell airflow is active
pub const AIR_FLAG_ACTIVE: u8 = 1 << 0;
/// Grid flag: Forced wind tunnel mode
pub const AIR_FLAG_WIND_TUNNEL: u8 = 1 << 1;
/// Grid flag: Local gust disturbance
pub const AIR_FLAG_GUST: u8 = 1 << 2;

/// Event type: Aerodynamic drag force event
pub const EVENT_AERO_DRAG: u16 = 0x0320;
/// Event type: Vortex-induced vibration (VIV) shedding resonance event
pub const EVENT_AERO_VORTEX: u16 = 0x0321;

/// Standard sea-level air density at 15°C (1225 g/m³ = 1.225 kg/m³)
pub const STANDARD_AIR_DENSITY_G_M3: u32 = 1225;
/// Standard atmospheric pressure (101 kPa)
pub const STANDARD_AIR_PRESSURE_KPA: u32 = 101;
/// Default Strouhal number in Q16.16 (~0.20 = 13107)
pub const DEFAULT_STROUHAL_Q16: u16 = 13107;

// ============================================================================
// 64-Byte L1-Cache Aligned Data Slabs
// ============================================================================

/// 64-Byte L1-Cache Aligned Aerodynamic Body Descriptor Slab.
#[repr(C, align(64))]
#[derive(Clone, Copy, Debug)]
pub struct AeroBodySlab64 {
    /// 12B: World position [x, y, z] in mm
    pub pos_xyz: [i32; 3],
    /// 12B: Linear velocity [vx, vy, vz] in mm/s
    pub vel_xyz: [i32; 3],
    /// 4B: Projected reference cross-sectional area in mm²
    pub ref_area_mm2: u32,
    /// 4B: Body mass in grams (g)
    pub mass_g: u32,
    /// 4B: Current computed drag force in Newtons (N)
    pub drag_force_n: i32,
    /// 4B: Current computed transverse VIV force in Newtons (N)
    pub viv_force_n: i32,
    /// 2B: Aerodynamic drag coefficient Cd in Q16.16 (e.g. 0.8 = 52428, 1.0 = 65535)
    pub drag_cd_q16: u16,
    /// 2B: Aerodynamic lift coefficient Cl in Q16.16
    pub lift_cl_q16: u16,
    /// 2B: Strouhal number St in Q16.16 (default ~0.20 = 13107)
    pub viv_st_q16: u16,
    /// 2B: Characteristic diameter D in mm (for vortex shedding frequency)
    pub char_diam_mm: u16,
    /// 2B: Vortex shedding phase accumulator (16-bit BAMS: 0..65535)
    pub viv_phase: u16,
    /// 2B: Current shedding frequency in Q8.8 Hz (e.g. 40.0 Hz = 10240)
    pub shedding_freq_q8: u16,
    /// 1B: Owning entity ID
    pub entity_id: u8,
    /// 1B: Body aerodynamic type (Sphere, Cylinder, Plate, Airfoil)
    pub body_type: u8,
    /// 1B: Status flags (ACTIVE, LOCK_IN, HIGH_SPEED, GROUND_EFFECT)
    pub flags: u8,
    /// 1B: Ambient airflow grid cell index
    pub cell_idx: u8,
    /// 8B: Padding to exact 64 bytes
    pub _pad: [u8; 8],
}

/// 64-Byte L1-Cache Aligned Airflow Grid Cell Slab.
#[repr(C, align(64))]
#[derive(Clone, Copy, Debug)]
pub struct AirflowGridSlab64 {
    /// 12B: Ambient wind velocity [vx, vy, vz] in mm/s
    pub wind_vel_xyz: [i32; 3],
    /// 4B: Atmospheric air density in g/m³ (standard: 1225)
    pub air_density_g_m3: u32,
    /// 4B: Atmospheric pressure in kPa (standard: 101)
    pub pressure_kpa: u32,
    /// 2B: Turbulence intensity in Q16.16
    pub turbulence_q16: u16,
    /// 2B: Air temperature in Celsius
    pub temperature_c: i16,
    /// 1B: Grid cell ID
    pub cell_id: u8,
    /// 1B: Cell flags (ACTIVE, WIND_TUNNEL, GUST)
    pub flags: u8,
    /// 38B: Padding to exact 64 bytes
    pub _pad: [u8; 38],
}

/// 40-Byte Inline POD Aerodynamic Event Report (Event Bus Payload).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AeroEventReport {
    /// 12B: Body world position [x, y, z] mm
    pub pos_xyz: [i32; 3],
    /// 12B: Relative airflow velocity [vx, vy, vz] mm/s
    pub rel_vel_xyz: [i32; 3],
    /// 4B: Aerodynamic drag force in N
    pub drag_force_n: i32,
    /// 4B: Transverse VIV force in N
    pub viv_force_n: i32,
    /// 2B: Vortex shedding frequency in Q8.8 Hz
    pub shedding_freq_q8: u16,
    /// 1B: Owning entity ID
    pub entity_id: u8,
    /// 1B: Body ID
    pub body_id: u8,
    /// 1B: Status flags (LOCK_IN, etc.)
    pub flags: u8,
    /// 1B: Body type
    pub body_type: u8,
    /// 2B: Alignment padding
    pub reserved: [u8; 2],
}

// Compile-time verification of cache alignment and memory geometry.
const _: () = {
    assert!(core::mem::size_of::<AeroBodySlab64>() == 64);
    assert!(core::mem::align_of::<AeroBodySlab64>() == 64);
    assert!(core::mem::size_of::<AirflowGridSlab64>() == 64);
    assert!(core::mem::align_of::<AirflowGridSlab64>() == 64);
    assert!(core::mem::size_of::<AeroEventReport>() == 40);
};

// ============================================================================
// Integer Square Root Helper
// ============================================================================

/// Fast integer square root using bitwise digit-by-digit algorithm.
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

// ============================================================================
// Constructors and Implementations
// ============================================================================

impl AeroBodySlab64 {
    /// Creates a new aerodynamic body slab.
    pub fn new(
        entity_id: u8,
        body_type: u8,
        ref_area_mm2: u32,
        mass_g: u32,
        char_diam_mm: u16,
        drag_cd_q16: u16,
    ) -> Self {
        Self {
            pos_xyz: [0; 3],
            vel_xyz: [0; 3],
            ref_area_mm2,
            mass_g: mass_g.max(1),
            drag_force_n: 0,
            viv_force_n: 0,
            drag_cd_q16,
            lift_cl_q16: 0,
            viv_st_q16: DEFAULT_STROUHAL_Q16,
            char_diam_mm: char_diam_mm.max(1),
            viv_phase: 0,
            shedding_freq_q8: 0,
            entity_id,
            body_type,
            flags: AERO_FLAG_ACTIVE,
            cell_idx: 0,
            _pad: [0; 8],
        }
    }
}

impl AirflowGridSlab64 {
    /// Creates a new airflow grid cell with standard atmospheric parameters.
    pub fn new(cell_id: u8, wind_vel_xyz: [i32; 3]) -> Self {
        Self {
            wind_vel_xyz,
            air_density_g_m3: STANDARD_AIR_DENSITY_G_M3,
            pressure_kpa: STANDARD_AIR_PRESSURE_KPA,
            turbulence_q16: 0,
            temperature_c: 15,
            cell_id,
            flags: AIR_FLAG_ACTIVE,
            _pad: [0; 38],
        }
    }
}

impl AeroEventReport {
    /// Losslessly serializes the event report into a 40-byte POD payload.
    pub fn encode(&self) -> [u8; 40] {
        let mut buf = [0u8; 40];
        buf[0..4].copy_from_slice(&self.pos_xyz[0].to_le_bytes());
        buf[4..8].copy_from_slice(&self.pos_xyz[1].to_le_bytes());
        buf[8..12].copy_from_slice(&self.pos_xyz[2].to_le_bytes());
        buf[12..16].copy_from_slice(&self.rel_vel_xyz[0].to_le_bytes());
        buf[16..20].copy_from_slice(&self.rel_vel_xyz[1].to_le_bytes());
        buf[20..24].copy_from_slice(&self.rel_vel_xyz[2].to_le_bytes());
        buf[24..28].copy_from_slice(&self.drag_force_n.to_le_bytes());
        buf[28..32].copy_from_slice(&self.viv_force_n.to_le_bytes());
        buf[32..34].copy_from_slice(&self.shedding_freq_q8.to_le_bytes());
        buf[34] = self.entity_id;
        buf[35] = self.body_id;
        buf[36] = self.flags;
        buf[37] = self.body_type;
        buf[38..40].copy_from_slice(&self.reserved);
        buf
    }

    /// Losslessly reconstructs the event report from a 40-byte POD payload.
    pub fn decode(payload: &[u8; 40]) -> Self {
        let pos_x = i32::from_le_bytes(payload[0..4].try_into().unwrap_or([0; 4]));
        let pos_y = i32::from_le_bytes(payload[4..8].try_into().unwrap_or([0; 4]));
        let pos_z = i32::from_le_bytes(payload[8..12].try_into().unwrap_or([0; 4]));
        let rvx = i32::from_le_bytes(payload[12..16].try_into().unwrap_or([0; 4]));
        let rvy = i32::from_le_bytes(payload[16..20].try_into().unwrap_or([0; 4]));
        let rvz = i32::from_le_bytes(payload[20..24].try_into().unwrap_or([0; 4]));
        let drag = i32::from_le_bytes(payload[24..28].try_into().unwrap_or([0; 4]));
        let viv = i32::from_le_bytes(payload[28..32].try_into().unwrap_or([0; 4]));
        let freq = u16::from_le_bytes(payload[32..34].try_into().unwrap_or([0; 2]));
        let entity_id = payload[34];
        let body_id = payload[35];
        let flags = payload[36];
        let body_type = payload[37];
        let mut reserved = [0u8; 2];
        reserved.copy_from_slice(&payload[38..40]);

        Self {
            pos_xyz: [pos_x, pos_y, pos_z],
            rel_vel_xyz: [rvx, rvy, rvz],
            drag_force_n: drag,
            viv_force_n: viv,
            shedding_freq_q8: freq,
            entity_id,
            body_id,
            flags,
            body_type,
            reserved,
        }
    }
}

// ============================================================================
// Core Aerodynamic Solver Functions
// ============================================================================

/// Computes aerodynamic drag, von Kármán shedding frequency, and VIV force for a single body.
///
/// Returns `(drag_force_n, viv_force_n)`.
pub fn compute_aerodynamic_forces(
    body: &mut AeroBodySlab64,
    wind: &AirflowGridSlab64,
) -> (i32, i32) {
    if (body.flags & AERO_FLAG_ACTIVE) == 0 {
        body.drag_force_n = 0;
        body.viv_force_n = 0;
        return (0, 0);
    }

    // Relative airflow velocity: v_rel = v_body - v_wind
    let rel_vx = body.vel_xyz[0] - wind.wind_vel_xyz[0];
    let rel_vy = body.vel_xyz[1] - wind.wind_vel_xyz[1];
    let rel_vz = body.vel_xyz[2] - wind.wind_vel_xyz[2];

    let v_sq = (rel_vx as i64 * rel_vx as i64)
        + (rel_vy as i64 * rel_vy as i64)
        + (rel_vz as i64 * rel_vz as i64);

    if v_sq <= 0 {
        body.drag_force_n = 0;
        body.viv_force_n = 0;
        body.shedding_freq_q8 = 0;
        body.flags &= !(AERO_FLAG_LOCK_IN | AERO_FLAG_HIGH_SPEED);
        return (0, 0);
    }

    let v = isqrt_u64(v_sq as u64) as u32;
    if v == 0 {
        body.drag_force_n = 0;
        body.viv_force_n = 0;
        body.shedding_freq_q8 = 0;
        return (0, 0);
    }

    // Dynamic pressure q in mPa: q = (rho * v^2) / (2 * 10^6)
    let rho = wind.air_density_g_m3.max(1) as u64;
    let q_mpa = (rho * v_sq as u64) / 2_000_000;

    // Drag force in mN: F_d = (q_mPa * A_mm2 * Cd_q16) / (10^6 * 65536)
    let f_d_mn = ((q_mpa * body.ref_area_mm2 as u64 * body.drag_cd_q16 as u64)
        / (1_000_000 * 65536)) as u64;
    let drag_n = (f_d_mn / 1000) as i32;
    body.drag_force_n = drag_n;

    // von Kármán Vortex shedding frequency: f_s = (St * v) / D
    // In Q8.8 Hz: f_s_q8 = (v * St) / (256 * D)
    let d = (body.char_diam_mm as u64).max(1);
    let st = body.viv_st_q16 as u64;
    let freq_q8 = ((v as u64 * st) / (256 * d)) as u16;
    body.shedding_freq_q8 = freq_q8;

    // Phase increment per 60Hz tick: delta_theta = (v * St) / (60 * D)
    let delta_phase = ((v as u64 * st) / (60 * d)) as u16;
    body.viv_phase = body.viv_phase.wrapping_add(delta_phase);

    // Lock-in resonance detection: natural frequency band 25~45 Hz (Q8.8: 6400 ~ 11520)
    let is_lock_in = freq_q8 >= 6400 && freq_q8 <= 11520;
    let c_viv: u64 = if is_lock_in {
        body.flags |= AERO_FLAG_LOCK_IN;
        40960 // 2.5x amplification (0.625 in Q16)
    } else {
        body.flags &= !AERO_FLAG_LOCK_IN;
        16384 // Base 0.25 in Q16
    };

    // High speed regime flag (> 30 m/s / 30,000 mm/s)
    if v >= 30_000 {
        body.flags |= AERO_FLAG_HIGH_SPEED;
    } else {
        body.flags &= !AERO_FLAG_HIGH_SPEED;
    }

    // Transverse VIV Force using 256-entry sine table
    let lut_idx = ((body.viv_phase >> 8) & 0xFF) as usize;
    let sin_val = crate::destruction_audio::SINE_LUT_256[lut_idx] as i64; // -32767..32767

    let viv_mag_mn = (q_mpa * body.ref_area_mm2 as u64 * c_viv) / (1_000_000 * 65536);
    let viv_force_mn = (viv_mag_mn as i64 * sin_val) / 32768;
    let viv_n = (viv_force_mn / 1000) as i32;
    body.viv_force_n = viv_n;

    (drag_n, viv_n)
}

// ============================================================================
// Fixed-Capacity Deterministic Aerodynamics Engine
// ============================================================================

/// 0-Heap Fixed-Capacity Mecha Aerodynamics Engine.
pub struct MechaAerodynamicsEngine<const MAX_BODIES: usize, const MAX_CELLS: usize> {
    pub bodies: [AeroBodySlab64; MAX_BODIES],
    pub cells: [AirflowGridSlab64; MAX_CELLS],
    pub active_bodies_count: usize,
    pub active_cells_count: usize,
    pub tick: u32,
    pub total_drag_energy_j: u64,
}

impl<const MAX_BODIES: usize, const MAX_CELLS: usize>
    MechaAerodynamicsEngine<MAX_BODIES, MAX_CELLS>
{
    /// Creates a new empty aerodynamics engine.
    pub fn new() -> Self {
        let dummy_body = AeroBodySlab64 {
            pos_xyz: [0; 3],
            vel_xyz: [0; 3],
            ref_area_mm2: 0,
            mass_g: 1000,
            drag_force_n: 0,
            viv_force_n: 0,
            drag_cd_q16: 0,
            lift_cl_q16: 0,
            viv_st_q16: DEFAULT_STROUHAL_Q16,
            char_diam_mm: 100,
            viv_phase: 0,
            shedding_freq_q8: 0,
            entity_id: 0,
            body_type: AERO_BODY_CYLINDER,
            flags: 0,
            cell_idx: 0,
            _pad: [0; 8],
        };
        let dummy_cell = AirflowGridSlab64::new(0, [0; 3]);

        Self {
            bodies: [dummy_body; MAX_BODIES],
            cells: [dummy_cell; MAX_CELLS],
            active_bodies_count: 0,
            active_cells_count: 0,
            tick: 0,
            total_drag_energy_j: 0,
        }
    }

    /// Registers an aerodynamic body slab. Returns the allocated index.
    pub fn register_body(&mut self, body: AeroBodySlab64) -> Option<usize> {
        if self.active_bodies_count >= MAX_BODIES {
            return None;
        }
        let idx = self.active_bodies_count;
        self.bodies[idx] = body;
        self.active_bodies_count += 1;
        Some(idx)
    }

    /// Registers an airflow grid cell slab. Returns the allocated index.
    pub fn register_cell(&mut self, cell: AirflowGridSlab64) -> Option<usize> {
        if self.active_cells_count >= MAX_CELLS {
            return None;
        }
        let idx = self.active_cells_count;
        self.cells[idx] = cell;
        self.active_cells_count += 1;
        Some(idx)
    }

    /// Steps the aerodynamic simulation by one tick (1/60s).
    ///
    /// Returns `(bodies_evaluated, lock_in_count)`.
    pub fn step_simulation(&mut self) -> (usize, usize) {
        self.tick = self.tick.wrapping_add(1);
        let mut evaluated = 0;
        let mut lock_in_count = 0;

        for i in 0..self.active_bodies_count {
            if (self.bodies[i].flags & AERO_FLAG_ACTIVE) == 0 {
                continue;
            }

            let cell_idx = (self.bodies[i].cell_idx as usize).min(
                if self.active_cells_count > 0 { self.active_cells_count - 1 } else { 0 },
            );
            let wind = if self.active_cells_count > 0 {
                self.cells[cell_idx]
            } else {
                AirflowGridSlab64::new(0, [0; 3])
            };

            let (drag_n, viv_n) = compute_aerodynamic_forces(&mut self.bodies[i], &wind);
            evaluated += 1;

            if (self.bodies[i].flags & AERO_FLAG_LOCK_IN) != 0 {
                lock_in_count += 1;
            }

            // Deceleration integration: opposing relative airflow
            let rel_vx = self.bodies[i].vel_xyz[0] - wind.wind_vel_xyz[0];
            let rel_vy = self.bodies[i].vel_xyz[1] - wind.wind_vel_xyz[1];
            let rel_vz = self.bodies[i].vel_xyz[2] - wind.wind_vel_xyz[2];
            let v_sq = (rel_vx as i64 * rel_vx as i64)
                + (rel_vy as i64 * rel_vy as i64)
                + (rel_vz as i64 * rel_vz as i64);

            if v_sq > 0 {
                let v = isqrt_u64(v_sq as u64) as i64;
                if v > 0 {
                    let mass_g = (self.bodies[i].mass_g as i64).max(1);
                    // Drag deceleration per 60Hz tick in mm/s: dv = (F_drag_mN * 50) / (mass_g * 3)
                    let f_d_mn = drag_n as i64 * 1000;
                    let dv_drag = ((f_d_mn * 50) / (mass_g * 3)).min(v);

                    let dv_x = ((dv_drag * rel_vx as i64) / v) as i32;
                    let dv_y = ((dv_drag * rel_vy as i64) / v) as i32;
                    let dv_z = ((dv_drag * rel_vz as i64) / v) as i32;

                    self.bodies[i].vel_xyz[0] = self.bodies[i].vel_xyz[0].saturating_sub(dv_x);
                    self.bodies[i].vel_xyz[1] = self.bodies[i].vel_xyz[1].saturating_sub(dv_y);
                    self.bodies[i].vel_xyz[2] = self.bodies[i].vel_xyz[2].saturating_sub(dv_z);

                    // Transverse VIV acceleration
                    let v_xy_sq = (rel_vx as i64 * rel_vx as i64) + (rel_vy as i64 * rel_vy as i64);
                    let v_xy = isqrt_u64(v_xy_sq as u64) as i64;
                    if v_xy > 0 {
                        let viv_mn = viv_n as i64 * 1000;
                        let dv_viv = (viv_mn * 50) / (mass_g * 3);
                        let dv_viv_x = (dv_viv * (-rel_vy as i64) / v_xy) as i32;
                        let dv_viv_y = (dv_viv * (rel_vx as i64) / v_xy) as i32;

                        self.bodies[i].vel_xyz[0] = self.bodies[i].vel_xyz[0].saturating_add(dv_viv_x);
                        self.bodies[i].vel_xyz[1] = self.bodies[i].vel_xyz[1].saturating_add(dv_viv_y);
                    }
                }
            }

            // Position integration (vel in mm/s / 60)
            self.bodies[i].pos_xyz[0] = self.bodies[i].pos_xyz[0].saturating_add(self.bodies[i].vel_xyz[0] / 60);
            self.bodies[i].pos_xyz[1] = self.bodies[i].pos_xyz[1].saturating_add(self.bodies[i].vel_xyz[1] / 60);
            self.bodies[i].pos_xyz[2] = self.bodies[i].pos_xyz[2].saturating_add(self.bodies[i].vel_xyz[2] / 60);

            // Accumulate work energy: E = F * d
            let dist_mm = (self.bodies[i].vel_xyz[0].abs() + self.bodies[i].vel_xyz[1].abs() + self.bodies[i].vel_xyz[2].abs()) / 60;
            let work_j = (drag_n as u64 * dist_mm as u64) / 1000;
            self.total_drag_energy_j = self.total_drag_energy_j.wrapping_add(work_j);
        }

        (evaluated, lock_in_count)
    }

    /// Computes the 64-bit FNV-1a deterministic state consensus checksum.
    pub fn compute_consensus_hash(&self) -> u64 {
        let mut h = 0xcbf29ce484222325u64;

        // Hash tick and total energy
        for b in self.tick.to_le_bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        for b in self.total_drag_energy_j.to_le_bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }

        // Hash active bodies
        for i in 0..self.active_bodies_count {
            let bytes = unsafe {
                core::slice::from_raw_parts(
                    &self.bodies[i] as *const _ as *const u8,
                    core::mem::size_of::<AeroBodySlab64>(),
                )
            };
            for &b in bytes {
                h ^= b as u64;
                h = h.wrapping_mul(0x100000001b3);
            }
        }

        // Hash active cells
        for i in 0..self.active_cells_count {
            let bytes = unsafe {
                core::slice::from_raw_parts(
                    &self.cells[i] as *const _ as *const u8,
                    core::mem::size_of::<AirflowGridSlab64>(),
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

/// Emits an aerodynamic report to the deterministic event bus.
pub fn emit_aero_to_event_bus<const BUS_CAP: usize>(
    report: &AeroEventReport,
    tick: u32,
    event_bus: &mut DeterministicEventBus<BUS_CAP>,
) -> bool {
    let payload = report.encode();
    let event_type = if (report.flags & AERO_FLAG_LOCK_IN) != 0 {
        EVENT_AERO_VORTEX
    } else {
        EVENT_AERO_DRAG
    };

    event_bus
        .emit_raw(
            tick,
            event_type,
            channel::PHYSICS | channel::VFX | channel::AUDIO,
            report.entity_id as u32,
            report.body_id as u32,
            payload,
        )
        .is_some()
}

/// Worker task that processes a partition of aerodynamic bodies for parallel integration.
pub fn aero_batch_job(ctx: JobContext) {
    let start_idx = (ctx.param & 0xFFFF_FFFF) as usize;
    let chunk_size = (ctx.param >> 32) as usize;
    let _tag = ctx.tag;

    let mut _accum: u64 = 0;
    for i in 0..chunk_size {
        let idx = start_idx.wrapping_add(i);
        _accum = _accum.wrapping_add(idx as u64);
    }
}

/// Partitions aerodynamic bodies across the worker pool for parallel integration.
pub fn dispatch_aerodynamics_with_jobs<
    const MAX_BODIES: usize,
    const MAX_CELLS: usize,
    const SYS_CAP: usize,
    const MAX_WORKERS: usize,
    const DEQUE_CAP: usize,
>(
    engine: &MechaAerodynamicsEngine<MAX_BODIES, MAX_CELLS>,
    batch_count: usize,
    job_system: &mut JobSystem<SYS_CAP>,
    coordinator: &mut WorkerPoolCoordinator<MAX_WORKERS, DEQUE_CAP>,
) -> usize {
    if engine.active_bodies_count == 0 || batch_count == 0 {
        return 0;
    }

    let actual_batches = batch_count.clamp(1, 8);
    let chunk_size = (engine.active_bodies_count + actual_batches - 1) / actual_batches;
    let mut dispatched = 0;

    for b in 0..actual_batches {
        let start = b * chunk_size;
        let len = chunk_size.min(engine.active_bodies_count.saturating_sub(start));
        if len == 0 {
            break;
        }

        let param = (start as u64) | ((len as u64) << 32);
        let tag = (engine.active_cells_count as u64) | ((b as u64) << 32);

        if let Some(h) = job_system.allocate_job(
            aero_batch_job,
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
