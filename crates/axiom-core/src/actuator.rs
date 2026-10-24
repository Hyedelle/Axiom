//! Axiom 310-Series Procedural Modular Mecha Pneumatic / Hydraulic Actuators & Artificial Muscle Dynamics
//!
//! Enforces:
//! - 0-Float in Core (Pure integer mm, mN, kPa, and Q16.16 fixed-point math).
//! - 1 kPa * 1 mm^2 = 1 mN exact integer SI consistency.
//! - Exact 64-Byte L1 cache aligned slabs: `#[repr(C, align(64))]`.
//! - 0-Heap allocation in simulation tick loop (`MechaActuatorEngine<MAX_ACTUATORS, MAX_CHAMBERS>`).
//! - 64-Bit FNV-1a deterministic state consensus hashing.
//! - 40-Byte inline POD `ActuatorEventReport` for event bus & netcode rollback.

use core::mem::size_of;
use crate::ragdoll::isqrt_i64;
use crate::event_bus::{channel, DeterministicEventBus};

/// Actuator type: Hydraulic linear piston cylinder (heavy mineral oil).
pub const ACTUATOR_TYPE_HYDRAULIC: u8 = 0;
/// Actuator type: Pneumatic piston cylinder (compressible air / gas).
pub const ACTUATOR_TYPE_PNEUMATIC: u8 = 1;
/// Actuator type: McKibben Pneumatic Artificial Muscle (PAM - contraction only).
pub const ACTUATOR_TYPE_MCKIBBEN_PAM: u8 = 2;
/// Actuator type: Linear electromechanical power servo.
pub const ACTUATOR_TYPE_LINEAR_SERVO: u8 = 3;

/// Actuator flags: Active in simulation loop.
pub const ACTUATOR_FLAG_ACTIVE: u8 = 1 << 0;
/// Actuator flags: Overpressure safety relief valve currently open.
pub const ACTUATOR_FLAG_RELIEF: u8 = 1 << 1;
/// Actuator flags: Catastrophic pipe burst / seal failure (pressure lost).
pub const ACTUATOR_FLAG_BURST: u8 = 1 << 2;
/// Actuator flags: Reached maximum stroke extension limit.
pub const ACTUATOR_FLAG_EXTENDED: u8 = 1 << 3;
/// Actuator flags: Reached minimum stroke retraction limit.
pub const ACTUATOR_FLAG_RETRACTED: u8 = 1 << 4;

/// Chamber flags: Active in simulation loop.
pub const CHAMBER_FLAG_ACTIVE: u8 = 1 << 0;
/// Chamber flags: Supply pump actively charging.
pub const CHAMBER_FLAG_PUMPING: u8 = 1 << 1;
/// Chamber flags: Pressure relief valve venting to reservoir.
pub const CHAMBER_FLAG_RELIEF: u8 = 1 << 2;
/// Chamber flags: Chamber casing rupture.
pub const CHAMBER_FLAG_BURST: u8 = 1 << 3;

/// Fluid medium type: Heavy mineral hydraulic oil (ISO VG 46).
pub const FLUID_TYPE_MINERAL_OIL: u8 = 0;
/// Fluid medium type: High-pressure compressed dry air.
pub const FLUID_TYPE_COMPRESSED_AIR: u8 = 1;
/// Fluid medium type: Synthetic synthetic biopolymer conductive gel.
pub const FLUID_TYPE_BIO_GEL: u8 = 2;

/// Deterministic event type: Catastrophic actuator / pipe burst rupture.
pub const EVENT_ACTUATOR_BURST: u16 = 0x0310;
/// Deterministic event type: Overpressure relief valve vent event.
pub const EVENT_ACTUATOR_PRESSURE: u16 = 0x0311;

/// Exact 64-byte L1 cache aligned actuator slab descriptor.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(C, align(64))]
pub struct ActuatorSlab64 {
    /// 4B: Attachment point A world/local X in mm
    pub anchor_a_x: i32,
    /// 4B: Attachment point A world/local Y in mm
    pub anchor_a_y: i32,
    /// 4B: Attachment point A world/local Z in mm
    pub anchor_a_z: i32,
    /// 4B: Attachment point B world/local X in mm
    pub anchor_b_x: i32,
    /// 4B: Attachment point B world/local Y in mm
    pub anchor_b_y: i32,
    /// 4B: Attachment point B world/local Z in mm
    pub anchor_b_z: i32,
    /// 4B: Internal fluid/gas gauge pressure in kPa (1 kPa * 1 mm^2 = 1 mN)
    pub pressure_kpa: i32,
    /// 4B: Maximum mechanical tensile/compressive thrust limit in N
    pub max_force_n: u32,
    /// 4B: Catastrophic pipe burst rupture pressure limit in kPa
    pub burst_pressure_kpa: u32,
    /// 4B: Overpressure safety relief valve threshold in kPa
    pub relief_pressure_kpa: u32,
    /// 2B: Maximum stroke length in mm
    pub stroke_max_mm: u16,
    /// 2B: Current cylinder length in mm
    pub current_len_mm: u16,
    /// 2B: Target control length in mm (servo setpoint)
    pub target_len_mm: u16,
    /// 2B: Rest length L0 in mm
    pub rest_len_mm: u16,
    /// 2B: Piston bore cross-sectional area in mm^2
    pub piston_area_mm2: u16,
    /// 2B: Viscous fluid damping coefficient in Q16.16 (e.g. 16384 = 0.25)
    pub damping_coeff_q16: u16,
    /// 1B: Attached parent bone index in ragdoll (255 = static world anchor)
    pub bone_a: u8,
    /// 1B: Attached child bone index in ragdoll
    pub bone_b: u8,
    /// 1B: Actuator type (0: Hydraulic, 1: Pneumatic, 2: McKibben PAM, 3: Linear Servo)
    pub actuator_type: u8,
    /// 1B: Operational flags (`ACTUATOR_FLAG_*`)
    pub flags: u8,
    /// 1B: Linked hydraulic chamber index (255 = standalone)
    pub chamber_idx: u8,
    /// 1B: Entity ID owning this actuator
    pub entity_id: u8,
    /// 6B: Explicit alignment padding to exact 64 bytes
    pub _reserved: [u8; 6],
}

const _: () = assert!(size_of::<ActuatorSlab64>() == 64);
const _: () = assert!(core::mem::align_of::<ActuatorSlab64>() == 64);

impl Default for ActuatorSlab64 {
    #[inline(always)]
    fn default() -> Self {
        Self {
            anchor_a_x: 0,
            anchor_a_y: 0,
            anchor_a_z: 0,
            anchor_b_x: 0,
            anchor_b_y: 0,
            anchor_b_z: 0,
            pressure_kpa: 0,
            max_force_n: 100_000,
            burst_pressure_kpa: 45_000, // 450 bar burst limit
            relief_pressure_kpa: 35_000, // 350 bar relief threshold
            stroke_max_mm: 500,
            current_len_mm: 300,
            target_len_mm: 300,
            rest_len_mm: 300,
            piston_area_mm2: 2000, // ~50.4mm bore diameter
            damping_coeff_q16: 16384, // 0.25 damping
            bone_a: 0,
            bone_b: 1,
            actuator_type: ACTUATOR_TYPE_HYDRAULIC,
            flags: 0,
            chamber_idx: 255,
            entity_id: 0,
            _reserved: [0; 6],
        }
    }
}

impl ActuatorSlab64 {
    /// Creates an active hydraulic cylinder actuator slab.
    pub fn new_hydraulic(
        bone_a: u8,
        bone_b: u8,
        anchor_a: (i32, i32, i32),
        anchor_b: (i32, i32, i32),
        piston_area_mm2: u16,
        stroke_max_mm: u16,
        burst_pressure_kpa: u32,
    ) -> Self {
        let dx = anchor_b.0 - anchor_a.0;
        let dy = anchor_b.1 - anchor_a.1;
        let dz = anchor_b.2 - anchor_a.2;
        let dist = isqrt_i64((dx as i64) * (dx as i64) + (dy as i64) * (dy as i64) + (dz as i64) * (dz as i64)) as u16;

        Self {
            anchor_a_x: anchor_a.0,
            anchor_a_y: anchor_a.1,
            anchor_a_z: anchor_a.2,
            anchor_b_x: anchor_b.0,
            anchor_b_y: anchor_b.1,
            anchor_b_z: anchor_b.2,
            pressure_kpa: 15_000, // 150 bar nominal operating pressure
            max_force_n: ((15_000i64 * piston_area_mm2 as i64) / 1000) as u32,
            burst_pressure_kpa,
            relief_pressure_kpa: (burst_pressure_kpa * 8) / 10,
            stroke_max_mm,
            current_len_mm: dist,
            target_len_mm: dist,
            rest_len_mm: dist,
            piston_area_mm2,
            damping_coeff_q16: 16384,
            bone_a,
            bone_b,
            actuator_type: ACTUATOR_TYPE_HYDRAULIC,
            flags: ACTUATOR_FLAG_ACTIVE,
            chamber_idx: 255,
            entity_id: 1,
            _reserved: [0; 6],
        }
    }

    /// Creates an active McKibben Pneumatic Artificial Muscle (PAM) slab.
    pub fn new_mckibben_pam(
        bone_a: u8,
        bone_b: u8,
        anchor_a: (i32, i32, i32),
        anchor_b: (i32, i32, i32),
        nominal_area_mm2: u16,
        max_pressure_kpa: u32,
    ) -> Self {
        let dx = anchor_b.0 - anchor_a.0;
        let dy = anchor_b.1 - anchor_a.1;
        let dz = anchor_b.2 - anchor_a.2;
        let dist = isqrt_i64((dx as i64) * (dx as i64) + (dy as i64) * (dy as i64) + (dz as i64) * (dz as i64)) as u16;

        Self {
            anchor_a_x: anchor_a.0,
            anchor_a_y: anchor_a.1,
            anchor_a_z: anchor_a.2,
            anchor_b_x: anchor_b.0,
            anchor_b_y: anchor_b.1,
            anchor_b_z: anchor_b.2,
            pressure_kpa: 600, // 6 bar nominal pneumatic pressure
            max_force_n: ((max_pressure_kpa as i64 * nominal_area_mm2 as i64) / 1000) as u32,
            burst_pressure_kpa: max_pressure_kpa * 2,
            relief_pressure_kpa: (max_pressure_kpa * 12) / 10,
            stroke_max_mm: (dist * 25) / 100, // 25% max contraction
            current_len_mm: dist,
            target_len_mm: dist,
            rest_len_mm: dist,
            piston_area_mm2: nominal_area_mm2,
            damping_coeff_q16: 8192,
            bone_a,
            bone_b,
            actuator_type: ACTUATOR_TYPE_MCKIBBEN_PAM,
            flags: ACTUATOR_FLAG_ACTIVE,
            chamber_idx: 255,
            entity_id: 1,
            _reserved: [0; 6],
        }
    }

    #[inline(always)]
    pub fn is_active(&self) -> bool {
        (self.flags & ACTUATOR_FLAG_ACTIVE) != 0
    }

    #[inline(always)]
    pub fn is_burst(&self) -> bool {
        (self.flags & ACTUATOR_FLAG_BURST) != 0
    }
}

/// Exact 64-byte L1 cache aligned hydraulic / pneumatic chamber slab descriptor.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(C, align(64))]
pub struct HydraulicChamberSlab64 {
    /// 8B: Current fluid/gas volume in mm^3
    pub fluid_vol_mm3: i64,
    /// 8B: Maximum chamber fluid volume capacity in mm^3
    pub max_vol_mm3: i64,
    /// 4B: Current chamber gauge pressure in kPa
    pub pressure_kpa: i32,
    /// 4B: Gas accumulator pre-charge pressure in kPa
    pub accumulator_kpa: i32,
    /// 4B: Fluid bulk modulus K in kPa (e.g. 1,500,000 kPa for mineral oil)
    pub bulk_modulus_kpa: u32,
    /// 4B: Supply pump flow rate in mm^3/tick
    pub pump_flow_rate_mm3: i32,
    /// 4B: Overpressure safety relief threshold in kPa
    pub relief_threshold_kpa: u32,
    /// 2B: Servo proportional valve opening spool position (0..65535 = 0%..100%)
    pub valve_opening_q16: u16,
    /// 2B: Low-pressure return reservoir fill level (0..65535 = 0%..100%)
    pub reservoir_level_q16: u16,
    /// 2B: Valve orifice effective cross-sectional area in mm^2 * 256 (Q8)
    pub orifice_area_q8: u16,
    /// 1B: Operational flags (`CHAMBER_FLAG_*`)
    pub flags: u8,
    /// 1B: Chamber logical channel / ID
    pub chamber_id: u8,
    /// 1B: Owning entity ID
    pub entity_id: u8,
    /// 1B: Fluid medium type (0: Oil, 1: Compressed Air, 2: Bio-gel)
    pub fluid_type: u8,
    /// 18B: Explicit alignment padding to exact 64 bytes
    pub _reserved: [u8; 18],
}

const _: () = assert!(size_of::<HydraulicChamberSlab64>() == 64);
const _: () = assert!(core::mem::align_of::<HydraulicChamberSlab64>() == 64);

impl Default for HydraulicChamberSlab64 {
    #[inline(always)]
    fn default() -> Self {
        Self {
            fluid_vol_mm3: 1_000_000, // 1 Liter nominal
            max_vol_mm3: 2_000_000,
            pressure_kpa: 15_000,     // 150 bar
            accumulator_kpa: 18_000,  // 180 bar pre-charge
            bulk_modulus_kpa: 1_500_000, // 1.5 GPa bulk modulus
            pump_flow_rate_mm3: 50_000,
            relief_threshold_kpa: 35_000,
            valve_opening_q16: 32768, // 50% open
            reservoir_level_q16: 49152, // 75% fill
            orifice_area_q8: 512,     // 2.0 mm^2
            flags: 0,
            chamber_id: 0,
            entity_id: 0,
            fluid_type: FLUID_TYPE_MINERAL_OIL,
            _reserved: [0; 18],
        }
    }
}

impl HydraulicChamberSlab64 {
    /// Creates an active hydraulic reservoir / pump chamber slab.
    pub fn new(
        chamber_id: u8,
        entity_id: u8,
        capacity_mm3: i64,
        nominal_pressure_kpa: i32,
        bulk_modulus_kpa: u32,
    ) -> Self {
        Self {
            fluid_vol_mm3: capacity_mm3 / 2,
            max_vol_mm3: capacity_mm3,
            pressure_kpa: nominal_pressure_kpa,
            accumulator_kpa: nominal_pressure_kpa + 3000,
            bulk_modulus_kpa,
            pump_flow_rate_mm3: 25_000,
            relief_threshold_kpa: (nominal_pressure_kpa as u32 * 14) / 10,
            valve_opening_q16: 0,
            reservoir_level_q16: 32768,
            orifice_area_q8: 256,
            flags: CHAMBER_FLAG_ACTIVE | CHAMBER_FLAG_PUMPING,
            chamber_id,
            entity_id,
            fluid_type: FLUID_TYPE_MINERAL_OIL,
            _reserved: [0; 18],
        }
    }

    #[inline(always)]
    pub fn is_active(&self) -> bool {
        (self.flags & CHAMBER_FLAG_ACTIVE) != 0
    }
}

/// 40-byte inline POD event report emitted to DeterministicEventBus.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[repr(C)]
pub struct ActuatorEventReport {
    /// 4B: Entity ID owning the actuator
    pub entity_id: u32,
    /// 1B: Actuator slot index
    pub actuator_id: u8,
    /// 1B: Linked chamber index
    pub chamber_id: u8,
    /// 1B: Event flags (e.g. 1 = relief vent, 2 = catastrophic burst)
    pub flags: u8,
    /// 1B: Actuator type (Hydraulic, Pneumatic, PAM)
    pub actuator_type: u8,
    /// 4B: Gauge pressure in kPa at time of event
    pub pressure_kpa: i32,
    /// 4B: Mechanical thrust output in N
    pub thrust_n: i32,
    /// 2B: Current length in mm
    pub current_len_mm: u16,
    /// 2B: Target control setpoint in mm
    pub target_len_mm: u16,
    /// 4B: World anchor coordinate X mm
    pub pos_x: i32,
    /// 4B: World anchor coordinate Y mm
    pub pos_y: i32,
    /// 4B: World anchor coordinate Z mm
    pub pos_z: i32,
    /// 4B: Shock impulse energy / mN*s
    pub impulse_m_ns: u32,
    /// 4B: Explicit alignment padding to exact 40 bytes
    pub _reserved: [u8; 4],
}

const _: () = assert!(size_of::<ActuatorEventReport>() == 40);

impl ActuatorEventReport {
    /// Losslessly encodes the report into a 40-byte flat buffer.
    pub fn encode(&self) -> [u8; 40] {
        let mut buf = [0u8; 40];
        buf[0..4].copy_from_slice(&self.entity_id.to_le_bytes());
        buf[4] = self.actuator_id;
        buf[5] = self.chamber_id;
        buf[6] = self.flags;
        buf[7] = self.actuator_type;
        buf[8..12].copy_from_slice(&self.pressure_kpa.to_le_bytes());
        buf[12..16].copy_from_slice(&self.thrust_n.to_le_bytes());
        buf[16..18].copy_from_slice(&self.current_len_mm.to_le_bytes());
        buf[18..20].copy_from_slice(&self.target_len_mm.to_le_bytes());
        buf[20..24].copy_from_slice(&self.pos_x.to_le_bytes());
        buf[24..28].copy_from_slice(&self.pos_y.to_le_bytes());
        buf[28..32].copy_from_slice(&self.pos_z.to_le_bytes());
        buf[32..36].copy_from_slice(&self.impulse_m_ns.to_le_bytes());
        buf
    }

    /// Losslessly decodes a report from a 40-byte flat buffer.
    pub fn decode(buf: &[u8; 40]) -> Self {
        Self {
            entity_id: u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]),
            actuator_id: buf[4],
            chamber_id: buf[5],
            flags: buf[6],
            actuator_type: buf[7],
            pressure_kpa: i32::from_le_bytes([buf[8], buf[9], buf[10], buf[11]]),
            thrust_n: i32::from_le_bytes([buf[12], buf[13], buf[14], buf[15]]),
            current_len_mm: u16::from_le_bytes([buf[16], buf[17]]),
            target_len_mm: u16::from_le_bytes([buf[18], buf[19]]),
            pos_x: i32::from_le_bytes([buf[20], buf[21], buf[22], buf[23]]),
            pos_y: i32::from_le_bytes([buf[24], buf[25], buf[26], buf[27]]),
            pos_z: i32::from_le_bytes([buf[28], buf[29], buf[30], buf[31]]),
            impulse_m_ns: u32::from_le_bytes([buf[32], buf[33], buf[34], buf[35]]),
            _reserved: [0; 4],
        }
    }
}

/// Evaluates instantaneous piston thrust for linear hydraulic / pneumatic actuators.
/// Returns thrust in Newtons (N).
pub fn compute_actuator_thrust(actuator: &ActuatorSlab64, rel_vel_mm_tick: i32) -> i32 {
    if !actuator.is_active() || actuator.is_burst() {
        return 0;
    }

    match actuator.actuator_type {
        ACTUATOR_TYPE_MCKIBBEN_PAM => {
            // McKibben Artificial Muscle: tension only (F >= 0, slack when stretched past L0)
            if actuator.current_len_mm > actuator.rest_len_mm {
                return 0;
            }
            let stroke = (actuator.rest_len_mm - actuator.current_len_mm) as i64;
            let contraction_factor = (4 * stroke * 65536) / (actuator.rest_len_mm.max(1) as i64);
            let eff_factor = 65536i64.saturating_sub(contraction_factor).max(0);
            let raw_m_n = (actuator.pressure_kpa as i64 * actuator.piston_area_mm2 as i64 * eff_factor) >> 16;
            let thrust_n = (raw_m_n / 1000) as i32;
            thrust_n.min(actuator.max_force_n as i32).max(0)
        }
        _ => {
            // Standard double-acting / single-acting piston cylinder
            // 1 kPa * 1 mm^2 = 1 mN
            let static_thrust_m_n = actuator.pressure_kpa as i64 * actuator.piston_area_mm2 as i64;
            let damping_m_n = (actuator.damping_coeff_q16 as i64 * rel_vel_mm_tick as i64 * 1000) >> 16;
            let net_m_n = static_thrust_m_n - damping_m_n;
            let thrust_n = (net_m_n / 1000) as i32;
            thrust_n.clamp(-(actuator.max_force_n as i32), actuator.max_force_n as i32)
        }
    }
}

/// Updates hydraulic fluid volume and calculates pressure update using Bulk Modulus K.
pub fn step_chamber_pressure(chamber: &mut HydraulicChamberSlab64, delta_v_mm3: i64) {
    if !chamber.is_active() {
        return;
    }

    // 1. Pump supply inflow
    let net_dv = if (chamber.flags & CHAMBER_FLAG_PUMPING) != 0 {
        delta_v_mm3 + chamber.pump_flow_rate_mm3 as i64
    } else {
        delta_v_mm3
    };

    chamber.fluid_vol_mm3 = (chamber.fluid_vol_mm3 + net_dv).clamp(100, chamber.max_vol_mm3);

    // 2. Bulk Modulus equation: delta_P = K * (-delta_V / V0)
    let v0 = chamber.max_vol_mm3.max(1);
    let delta_p_kpa = ((chamber.bulk_modulus_kpa as i64) * net_dv) / v0;

    let new_p = (chamber.pressure_kpa as i64 + delta_p_kpa).clamp(0, 100_000) as i32;
    chamber.pressure_kpa = new_p;

    // 3. Overpressure relief valve check
    if chamber.pressure_kpa > chamber.relief_threshold_kpa as i32 {
        chamber.flags |= CHAMBER_FLAG_RELIEF;
        // Vent excess fluid back to reservoir
        let excess_p = chamber.pressure_kpa - chamber.relief_threshold_kpa as i32;
        let relief_bleed = (excess_p * 8) / 10;
        chamber.pressure_kpa -= relief_bleed;
    } else {
        chamber.flags &= !CHAMBER_FLAG_RELIEF;
    }
}

/// Solves a single actuator step, handling length updates, pressure dynamics, overpressure, and burst rupture.
/// Returns (thrust_n, relief_vented, burst_occurred).
pub fn solve_actuator_step(
    actuator: &mut ActuatorSlab64,
    chamber: Option<&mut HydraulicChamberSlab64>,
) -> (i32, bool, bool) {
    if !actuator.is_active() {
        return (0, false, false);
    }

    let mut burst = false;
    let mut relief = false;

    // 1. Check Catastrophic Pipe Burst Rupture limit
    if (actuator.pressure_kpa as u32) >= actuator.burst_pressure_kpa {
        actuator.flags |= ACTUATOR_FLAG_BURST;
        actuator.pressure_kpa = 0;
        return (0, false, true);
    }

    // 2. Check Overpressure Safety Relief Valve
    if (actuator.pressure_kpa as u32) > actuator.relief_pressure_kpa {
        actuator.flags |= ACTUATOR_FLAG_RELIEF;
        relief = true;
        let bleed = ((actuator.pressure_kpa as u32 - actuator.relief_pressure_kpa) * 7) / 10;
        actuator.pressure_kpa = (actuator.pressure_kpa - bleed as i32).max(0);
    } else {
        actuator.flags &= !ACTUATOR_FLAG_RELIEF;
    }

    // 3. Servo positioning and stroke extension
    let target = actuator.target_len_mm;
    let curr = actuator.current_len_mm;
    let diff = (target as i32) - (curr as i32);
    let step_speed = diff.clamp(-25, 25);
    let new_len = (curr as i32 + step_speed).clamp(
        actuator.rest_len_mm.saturating_sub(actuator.stroke_max_mm) as i32,
        (actuator.rest_len_mm + actuator.stroke_max_mm) as i32,
    ) as u16;

    let rel_vel = (new_len as i32) - (curr as i32);
    actuator.current_len_mm = new_len;

    // 4. If linked to chamber, transfer fluid and sync pressure
    if let Some(ch) = chamber {
        let delta_v = -(rel_vel as i64) * (actuator.piston_area_mm2 as i64);
        step_chamber_pressure(ch, delta_v);
        actuator.pressure_kpa = ch.pressure_kpa;

        if (actuator.pressure_kpa as u32) >= actuator.burst_pressure_kpa {
            actuator.flags |= ACTUATOR_FLAG_BURST;
            actuator.pressure_kpa = 0;
            ch.flags |= CHAMBER_FLAG_BURST;
            burst = true;
        }
    }

    let thrust = compute_actuator_thrust(actuator, rel_vel);
    (thrust, relief, burst)
}

/// 0-Heap fixed-capacity simulator for modular pneumatic/hydraulic actuators.
pub struct MechaActuatorEngine<const MAX_ACTUATORS: usize, const MAX_CHAMBERS: usize> {
    pub actuators: [ActuatorSlab64; MAX_ACTUATORS],
    pub chambers: [HydraulicChamberSlab64; MAX_CHAMBERS],
    pub active_actuators_count: usize,
    pub active_chambers_count: usize,
    pub tick: u32,
    pub total_bursts: u32,
    pub total_relief_events: u32,
    pub peak_thrust_n: u32,
}

impl<const MAX_ACTUATORS: usize, const MAX_CHAMBERS: usize>
    MechaActuatorEngine<MAX_ACTUATORS, MAX_CHAMBERS>
{
    pub const fn new() -> Self {
        Self {
            actuators: [ActuatorSlab64 {
                anchor_a_x: 0,
                anchor_a_y: 0,
                anchor_a_z: 0,
                anchor_b_x: 0,
                anchor_b_y: 0,
                anchor_b_z: 0,
                pressure_kpa: 0,
                max_force_n: 0,
                burst_pressure_kpa: 0,
                relief_pressure_kpa: 0,
                stroke_max_mm: 0,
                current_len_mm: 0,
                target_len_mm: 0,
                rest_len_mm: 0,
                piston_area_mm2: 0,
                damping_coeff_q16: 0,
                bone_a: 0,
                bone_b: 0,
                actuator_type: 0,
                flags: 0,
                chamber_idx: 255,
                entity_id: 0,
                _reserved: [0; 6],
            }; MAX_ACTUATORS],
            chambers: [HydraulicChamberSlab64 {
                fluid_vol_mm3: 0,
                max_vol_mm3: 0,
                pressure_kpa: 0,
                accumulator_kpa: 0,
                bulk_modulus_kpa: 0,
                pump_flow_rate_mm3: 0,
                relief_threshold_kpa: 0,
                valve_opening_q16: 0,
                reservoir_level_q16: 0,
                orifice_area_q8: 0,
                flags: 0,
                chamber_id: 0,
                entity_id: 0,
                fluid_type: 0,
                _reserved: [0; 18],
            }; MAX_CHAMBERS],
            active_actuators_count: 0,
            active_chambers_count: 0,
            tick: 0,
            total_bursts: 0,
            total_relief_events: 0,
            peak_thrust_n: 0,
        }
    }

    /// Registers a hydraulic chamber slab into the flat pool.
    pub fn add_chamber(&mut self, chamber: HydraulicChamberSlab64) -> Option<usize> {
        if self.active_chambers_count < MAX_CHAMBERS {
            let idx = self.active_chambers_count;
            self.chambers[idx] = chamber;
            self.active_chambers_count += 1;
            Some(idx)
        } else {
            None
        }
    }

    /// Registers an actuator slab into the flat pool.
    pub fn add_actuator(&mut self, actuator: ActuatorSlab64) -> Option<usize> {
        if self.active_actuators_count < MAX_ACTUATORS {
            let idx = self.active_actuators_count;
            self.actuators[idx] = actuator;
            self.active_actuators_count += 1;
            Some(idx)
        } else {
            None
        }
    }

    /// Advances the procedural actuator and chamber simulation by 1 tick.
    /// Returns (active_actuators, total_bursts).
    pub fn step_simulation(&mut self) -> (usize, usize) {
        self.tick = self.tick.wrapping_add(1);
        let mut peak = 0u32;

        for i in 0..self.active_actuators_count {
            if !self.actuators[i].is_active() {
                continue;
            }

            let chamber_idx = self.actuators[i].chamber_idx as usize;
            let (thrust, relief, burst) = if chamber_idx < self.active_chambers_count {
                let ch_ptr = &mut self.chambers[chamber_idx] as *mut HydraulicChamberSlab64;
                solve_actuator_step(&mut self.actuators[i], unsafe { Some(&mut *ch_ptr) })
            } else {
                solve_actuator_step(&mut self.actuators[i], None)
            };

            if thrust.unsigned_abs() > peak {
                peak = thrust.unsigned_abs();
            }
            if relief {
                self.total_relief_events += 1;
            }
            if burst {
                self.total_bursts += 1;
            }
        }

        self.peak_thrust_n = peak;
        (self.active_actuators_count, self.total_bursts as usize)
    }

    /// Computes a bit-exact 64-bit FNV-1a state consensus hash.
    pub fn compute_consensus_hash(&self) -> u64 {
        let mut hash = 0xcbf29ce484222325u64;
        const FNV_PRIME: u64 = 0x100000001b3;

        for act in &self.actuators[..self.active_actuators_count] {
            hash ^= act.pressure_kpa as u64;
            hash = hash.wrapping_mul(FNV_PRIME);
            hash ^= act.current_len_mm as u64;
            hash = hash.wrapping_mul(FNV_PRIME);
            hash ^= act.target_len_mm as u64;
            hash = hash.wrapping_mul(FNV_PRIME);
            hash ^= act.flags as u64;
            hash = hash.wrapping_mul(FNV_PRIME);
        }

        for ch in &self.chambers[..self.active_chambers_count] {
            hash ^= ch.fluid_vol_mm3 as u64;
            hash = hash.wrapping_mul(FNV_PRIME);
            hash ^= ch.pressure_kpa as u64;
            hash = hash.wrapping_mul(FNV_PRIME);
            hash ^= ch.flags as u64;
            hash = hash.wrapping_mul(FNV_PRIME);
        }

        hash ^= self.tick as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
        hash ^= self.total_bursts as u64;
        hash = hash.wrapping_mul(FNV_PRIME);

        hash
    }
}

/// Dispatches an actuator event report to the deterministic event bus.
pub fn emit_actuator_to_event_bus<const BUS_CAP: usize>(
    report: &ActuatorEventReport,
    tick: u32,
    event_bus: &mut DeterministicEventBus<BUS_CAP>,
) -> bool {
    let payload = report.encode();
    let event_type = if (report.flags & ACTUATOR_FLAG_BURST) != 0 {
        EVENT_ACTUATOR_BURST
    } else {
        EVENT_ACTUATOR_PRESSURE
    };

    event_bus
        .emit_raw(
            tick,
            event_type,
            channel::PHYSICS | channel::VFX | channel::AUDIO,
            report.entity_id,
            report.actuator_id as u32,
            payload,
        )
        .is_some()
}
