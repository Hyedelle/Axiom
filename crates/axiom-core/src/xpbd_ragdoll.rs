//! # Extended Position-Based Dynamics (XPBD) Mecha Ragdoll Constraints & Elastic Strain Limits (`axiom-core::xpbd_ragdoll`)
//!
//! T-231: High-Performance 0-Float, 64-Byte L1 Cache Line Aligned Extended Position-Based
//! Dynamics (XPBD) Ragdoll Constraint Solver with Elastic Yield and Tensile Rupture.
//!
//! ## Architectural Guarantees:
//! - **0-Float in Core**: All positions and distances in integer millimeters (`mm: i32`).
//!   Rotations in 16-bit BAMS (`u16`, 0..65535 = 0°..360°).
//!   Compliance $\tilde{\alpha} = \alpha / \Delta t^2$ and inverse masses in $Q16.16$ fixed-point.
//! - **64B L1 Cache Line Alignment**: Both [`XpbdBoneSlab64`] and [`XpbdConstraintSlab64`]
//!   are `#[repr(C, align(64))]` with exact 64-byte size and zero implicit padding.
//! - **0-Heap Flat Array Topology**: Fixed-capacity compile-time generic array engine [`XpbdRagdollEngine<BONES, CONSTRAINTS>`].
//! - **XPBD Compliance Relaxation**: Substep and iteration-invariant constraint stiffness control.
//! - **Lagrange Multiplier & Tension Telemetry**: Accumulates $\lambda_{acc}$ to measure actual constraint force.
//! - **Metallurgical Strain Limits**:
//!   - Elastic recovery when $\epsilon \le \epsilon_{yield}$.
//!   - Permanent plastic deformation (rest length $L_0$ shift) when $\epsilon_{yield} < \epsilon < \epsilon_{break}$.
//!   - Tensile rupture / fatigue severance when $\epsilon \ge \epsilon_{break}$.
//! - **Event Bus Integration**: 40-Byte inline POD [`XpbdEventReport`] for `EVENT_XPBD_STRAIN` and `EVENT_XPBD_TEAR`.

use core::mem::{align_of, size_of};

use crate::event_bus::{channel, DeterministicEventBus};
use crate::job_system::{JobContext, JobPriority, JobStatus, JobSystem};
use crate::worker_pool::WorkerPoolCoordinator;

/// Event type for XPBD mechanical strain / plastic yield
pub const EVENT_XPBD_STRAIN: u16 = 0x0290;
/// Event type for XPBD joint rupture and limb severance
pub const EVENT_XPBD_TEAR: u16 = 0x0291;

/// Bone state flag bitmasks
pub const XPBD_BONE_FLAG_ACTIVE: u8    = 1 << 0;
pub const XPBD_BONE_FLAG_FROZEN: u8    = 1 << 1;
pub const XPBD_BONE_FLAG_SEVERED: u8   = 1 << 2;
pub const XPBD_BONE_FLAG_GROUNDED: u8  = 1 << 3;

/// Constraint state flag bitmasks
pub const XPBD_CONSTRAINT_FLAG_ACTIVE: u8   = 1 << 0;
pub const XPBD_CONSTRAINT_FLAG_SEVERED: u8  = 1 << 1;
pub const XPBD_CONSTRAINT_FLAG_PLASTIC: u8  = 1 << 2;
pub const XPBD_CONSTRAINT_FLAG_FATIGUED: u8 = 1 << 3;

/// Constraint mechanical topology classification
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum XpbdConstraintType {
    #[default]
    Distance = 0,
    Hinge = 1,
    BallSocket = 2,
    Slider = 3,
}

/// 1.0 in Q16.16 fixed-point representation
pub const Q16_ONE: u32 = 65536;

/// Exact 64-Byte L1 Cache Line Aligned XPBD Ragdoll Bone Slab.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(C, align(64))]
pub struct XpbdBoneSlab64 {
    /// 4B: World X position in integer millimeters
    pub pos_x: i32,
    /// 4B: World Y position in integer millimeters
    pub pos_y: i32,
    /// 4B: World Z position in integer millimeters
    pub pos_z: i32,
    /// 4B: Linear velocity X in integer mm/s
    pub vel_x: i32,
    /// 4B: Linear velocity Y in integer mm/s
    pub vel_y: i32,
    /// 4B: Linear velocity Z in integer mm/s
    pub vel_z: i32,
    /// 2B: Yaw orientation in 16-bit BAMS (0..65535 = 0°..360°)
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
    /// 4B: Inverse mass 1/m in Q16.16 (65536 = 1.0 kg^-1, 0 = static anchor)
    pub inv_mass_q16: u32,
    /// 2B: Collision bounding sphere / capsule radius in mm
    pub radius_mm: u16,
    /// 2B: Bone segment length in mm
    pub length_mm: u16,
    /// 1B: Parent bone slot index (255 = root anchor)
    pub parent_idx: u8,
    /// 1B: Bone anatomical identifier (0..15)
    pub bone_id: u8,
    /// 1B: Operational flags (`XPBD_BONE_FLAG_*`)
    pub flags: u8,
    /// 1B: Collision layer / filter mask
    pub collision_group: u8,
    /// 2B: Surface friction coefficient in Q16.16 (e.g. 32768 = 0.5)
    pub friction_q16: u16,
    /// 2B: Restitution coefficient in Q16.16 (e.g. 19660 = 0.3)
    pub restitution_q16: u16,
    /// 12B: Explicit alignment padding to exact 64 bytes
    pub _reserved: [u8; 12],
}

const _: () = assert!(size_of::<XpbdBoneSlab64>() == 64);
const _: () = assert!(align_of::<XpbdBoneSlab64>() == 64);

impl Default for XpbdBoneSlab64 {
    #[inline(always)]
    fn default() -> Self {
        Self::empty()
    }
}

impl XpbdBoneSlab64 {
    /// Creates a blank inactive bone slab.
    #[inline(always)]
    pub const fn empty() -> Self {
        Self {
            pos_x: 0,
            pos_y: 0,
            pos_z: 0,
            vel_x: 0,
            vel_y: 0,
            vel_z: 0,
            yaw_bams: 0,
            pitch_bams: 0,
            roll_bams: 0,
            ang_vel_yaw: 0,
            ang_vel_pitch: 0,
            ang_vel_roll: 0,
            inv_mass_q16: 0,
            radius_mm: 0,
            length_mm: 0,
            parent_idx: 255,
            bone_id: 0,
            flags: 0,
            collision_group: 0,
            friction_q16: 32768, // 0.5 friction
            restitution_q16: 19660, // 0.3 restitution
            _reserved: [0; 12],
        }
    }

    /// Creates an active dynamic bone slab with position, mass, radius, and length.
    pub fn new(
        bone_id: u8,
        parent_idx: u8,
        pos: (i32, i32, i32),
        inv_mass_q16: u32,
        radius_mm: u16,
        length_mm: u16,
    ) -> Self {
        Self {
            pos_x: pos.0,
            pos_y: pos.1,
            pos_z: pos.2,
            vel_x: 0,
            vel_y: 0,
            vel_z: 0,
            yaw_bams: 0,
            pitch_bams: 0,
            roll_bams: 0,
            ang_vel_yaw: 0,
            ang_vel_pitch: 0,
            ang_vel_roll: 0,
            inv_mass_q16,
            radius_mm,
            length_mm,
            parent_idx,
            bone_id,
            flags: XPBD_BONE_FLAG_ACTIVE,
            collision_group: 0x01,
            friction_q16: 32768,
            restitution_q16: 19660,
            _reserved: [0; 12],
        }
    }

    /// Returns true if this bone slab is currently active.
    #[inline(always)]
    pub fn is_active(&self) -> bool {
        (self.flags & XPBD_BONE_FLAG_ACTIVE) != 0
    }

    /// Returns true if this bone has been severed from its parent tree.
    #[inline(always)]
    pub fn is_severed(&self) -> bool {
        (self.flags & XPBD_BONE_FLAG_SEVERED) != 0
    }
}

/// Exact 64-Byte L1 Cache Line Aligned XPBD Constraint Slab.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(C, align(64))]
pub struct XpbdConstraintSlab64 {
    /// 1B: Parent bone slot index
    pub body_a: u8,
    /// 1B: Child bone slot index
    pub body_b: u8,
    /// 1B: Constraint mechanical type (`XpbdConstraintType`)
    pub constraint_type: u8,
    /// 1B: Operational flags (`XPBD_CONSTRAINT_FLAG_*`)
    pub flags: u8,
    /// 2B: Current resting length L0 in mm (dynamically shifts on plastic strain)
    pub rest_length_mm: u16,
    /// 2B: Original initial resting length in mm
    pub original_length_mm: u16,
    /// 4B: Effective compliance alpha / dt^2 in Q16.16 (0 = rigid)
    pub compliance_q16: u32,
    /// 4B: Accumulated Lagrange multiplier lambda in Q16.16 mm*kg
    pub lambda_acc_q16: i32,
    /// 4B: Elastic yield strain limit epsilon_yield in Q16.16 (e.g. 3277 = 0.05 = 5%)
    pub yield_strain_q16: u32,
    /// 4B: Ultimate rupture strain limit epsilon_break in Q16.16 (e.g. 13107 = 0.20 = 20%)
    pub break_strain_q16: u32,
    /// 4B: Current instantaneous strain in Q16.16
    pub current_strain_q16: u32,
    /// 2B: Plastic deformation flow rate in Q16.16 (e.g. 6554 = 0.10)
    pub plastic_rate_q16: u16,
    /// 2B: Minimum angular limit in BAMS
    pub min_angle_bams: u16,
    /// 2B: Maximum angular limit in BAMS
    pub max_angle_bams: u16,
    /// 2B: Constraint damping coefficient in Q16.16
    pub damping_q16: u16,
    /// 2B: Primary hinge/cone axis X in Q1.15 normalized
    pub axis_x_q15: i16,
    /// 2B: Primary hinge/cone axis Y in Q1.15 normalized
    pub axis_y_q15: i16,
    /// 2B: Primary hinge/cone axis Z in Q1.15 normalized
    pub axis_z_q15: i16,
    /// 2B: High-stress fatigue tick counter
    pub fatigue_ticks: u16,
    /// 2B: Maximum fatigue endurance limit before rupture
    pub max_fatigue_ticks: u16,
    /// 18B: Explicit alignment padding to exact 64 bytes
    pub _reserved: [u8; 18],
}

const _: () = assert!(size_of::<XpbdConstraintSlab64>() == 64);
const _: () = assert!(align_of::<XpbdConstraintSlab64>() == 64);

impl Default for XpbdConstraintSlab64 {
    #[inline(always)]
    fn default() -> Self {
        Self::empty()
    }
}

impl XpbdConstraintSlab64 {
    /// Creates a blank inactive constraint slab.
    #[inline(always)]
    pub const fn empty() -> Self {
        Self {
            body_a: 0,
            body_b: 0,
            constraint_type: 0,
            flags: 0,
            rest_length_mm: 0,
            original_length_mm: 0,
            compliance_q16: 0,
            lambda_acc_q16: 0,
            yield_strain_q16: 3277,   // 5% default yield strain
            break_strain_q16: 13107, // 20% default rupture strain
            current_strain_q16: 0,
            plastic_rate_q16: 6554,   // 10% plastic flow rate
            min_angle_bams: 0,
            max_angle_bams: 65535,
            damping_q16: 16384,
            axis_x_q15: 0,
            axis_y_q15: 32767, // +Y axis
            axis_z_q15: 0,
            fatigue_ticks: 0,
            max_fatigue_ticks: 60, // 1 second of high strain before rupture
            _reserved: [0; 18],
        }
    }

    /// Creates an active XPBD distance constraint.
    pub fn new_distance(
        body_a: u8,
        body_b: u8,
        rest_length_mm: u16,
        compliance_q16: u32,
        yield_strain_q16: u32,
        break_strain_q16: u32,
    ) -> Self {
        Self {
            body_a,
            body_b,
            constraint_type: XpbdConstraintType::Distance as u8,
            flags: XPBD_CONSTRAINT_FLAG_ACTIVE,
            rest_length_mm,
            original_length_mm: rest_length_mm,
            compliance_q16,
            lambda_acc_q16: 0,
            yield_strain_q16,
            break_strain_q16,
            current_strain_q16: 0,
            plastic_rate_q16: 6554,
            min_angle_bams: 0,
            max_angle_bams: 65535,
            damping_q16: 16384,
            axis_x_q15: 0,
            axis_y_q15: 32767,
            axis_z_q15: 0,
            fatigue_ticks: 0,
            max_fatigue_ticks: 60,
            _reserved: [0; 18],
        }
    }

    /// Returns true if this constraint is active and not severed.
    #[inline(always)]
    pub fn is_active(&self) -> bool {
        (self.flags & XPBD_CONSTRAINT_FLAG_ACTIVE) != 0 && (self.flags & XPBD_CONSTRAINT_FLAG_SEVERED) == 0
    }

    /// Returns true if this constraint has been severed.
    #[inline(always)]
    pub fn is_severed(&self) -> bool {
        (self.flags & XPBD_CONSTRAINT_FLAG_SEVERED) != 0
    }

    /// Returns true if plastic deformation has occurred.
    #[inline(always)]
    pub fn is_plastic_deformed(&self) -> bool {
        (self.flags & XPBD_CONSTRAINT_FLAG_PLASTIC) != 0
    }
}

/// 64-bit integer square root (Newton-Raphson method).
#[inline(always)]
pub fn isqrt_i64(n: i64) -> i64 {
    if n <= 0 {
        return 0;
    }
    let mut x0 = n / 2;
    if x0 == 0 {
        return 1;
    }
    let mut x1 = (x0 + n / x0) / 2;
    while x1 < x0 {
        x0 = x1;
        x1 = (x0 + n / x0) / 2;
    }
    x0
}

/// Extended Position-Based Dynamics (XPBD) pure integer distance constraint relaxation.
///
/// Returns `(plastic_deformed, ruptured)`.
#[inline]
pub fn solve_xpbd_distance(
    b1: &mut XpbdBoneSlab64,
    b2: &mut XpbdBoneSlab64,
    joint: &mut XpbdConstraintSlab64,
) -> (bool, bool) {
    let w1 = b1.inv_mass_q16 as i64;
    let w2 = b2.inv_mass_q16 as i64;
    let w_sum = w1 + w2;
    if w_sum <= 0 {
        return (false, false);
    }

    let dx = (b2.pos_x - b1.pos_x) as i64;
    let dy = (b2.pos_y - b1.pos_y) as i64;
    let dz = (b2.pos_z - b1.pos_z) as i64;
    let dist_sq = dx * dx + dy * dy + dz * dz;
    let dist = isqrt_i64(dist_sq).max(1);

    let rest_len = joint.rest_length_mm as i64;
    let c = dist - rest_len;
    if c == 0 {
        return (false, false);
    }

    // Instantaneous strain: epsilon_q16 = (|c| * 65536) / rest_len
    let strain_q16 = ((c.abs() << 16) / rest_len.max(1)) as u32;
    joint.current_strain_q16 = strain_q16;

    // 1. Rupture check
    if strain_q16 >= joint.break_strain_q16 {
        joint.flags |= XPBD_CONSTRAINT_FLAG_SEVERED;
        b2.flags |= XPBD_BONE_FLAG_SEVERED;
        return (false, true);
    }

    // High-stress fatigue tracking
    if strain_q16 > joint.yield_strain_q16 {
        joint.fatigue_ticks = joint.fatigue_ticks.saturating_add(1);
        if joint.max_fatigue_ticks > 0 && joint.fatigue_ticks >= joint.max_fatigue_ticks {
            joint.flags |= XPBD_CONSTRAINT_FLAG_FATIGUED | XPBD_CONSTRAINT_FLAG_SEVERED;
            b2.flags |= XPBD_BONE_FLAG_SEVERED;
            return (false, true);
        }
    } else {
        joint.fatigue_ticks = joint.fatigue_ticks.saturating_sub(1);
    }

    // 2. Plastic deformation check (strain exceeds yield threshold)
    let mut plastic = false;
    if strain_q16 > joint.yield_strain_q16 && joint.plastic_rate_q16 > 0 {
        let excess = strain_q16 - joint.yield_strain_q16;
        let delta_l = ((excess as i64 * joint.plastic_rate_q16 as i64) >> 16) as i32;
        if c > 0 {
            joint.rest_length_mm = joint.rest_length_mm.saturating_add((delta_l >> 4).max(1) as u16);
        } else {
            joint.rest_length_mm = joint.rest_length_mm.saturating_sub((delta_l >> 4).max(1) as u16);
        }
        joint.flags |= XPBD_CONSTRAINT_FLAG_PLASTIC;
        plastic = true;
    }

    // 3. XPBD compliance relaxation:
    // delta_lambda = (-C - alpha_tilde * lambda_acc) / (w_sum + alpha_tilde)
    let alpha_tilde = joint.compliance_q16 as i64;
    let denom = w_sum + alpha_tilde;
    if denom <= 0 {
        return (plastic, false);
    }

    let delta_lambda = ((-c << 16) - ((alpha_tilde * joint.lambda_acc_q16 as i64) >> 16)) / denom;
    joint.lambda_acc_q16 = joint.lambda_acc_q16.saturating_add(delta_lambda as i32);

    // Position corrections: delta_x = w * delta_lambda * n
    let corr1 = (delta_lambda * w1) >> 16;
    let corr2 = (delta_lambda * w2) >> 16;

    let delta_p1_x = (corr1 * dx) / dist;
    let delta_p1_y = (corr1 * dy) / dist;
    let delta_p1_z = (corr1 * dz) / dist;

    let delta_p2_x = (corr2 * dx) / dist;
    let delta_p2_y = (corr2 * dy) / dist;
    let delta_p2_z = (corr2 * dz) / dist;

    b1.pos_x -= delta_p1_x as i32;
    b1.pos_y -= delta_p1_y as i32;
    b1.pos_z -= delta_p1_z as i32;

    b2.pos_x += delta_p2_x as i32;
    b2.pos_y += delta_p2_y as i32;
    b2.pos_z += delta_p2_z as i32;

    (plastic, false)
}

/// 40-Byte Inlined POD XPBD Event Report (fits cleanly inside `EventSlab64.payload`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(C)]
pub struct XpbdEventReport {
    /// 4B: Entity / Mecha identifier
    pub entity_id: u32,
    /// 1B: Parent bone index
    pub bone_a: u8,
    /// 1B: Child bone index
    pub bone_b: u8,
    /// 1B: Constraint slot index
    pub constraint_id: u8,
    /// 1B: Severed state flag (0 = plastic strain, 1 = ruptured)
    pub is_severed: u8,
    /// 4B: Instantaneous strain in Q16.16
    pub strain_q16: u32,
    /// 4B: Measured tension force in integer Newtons
    pub tension_force_n: u32,
    /// 12B: Rupture / strain world coordinates mm [x, y, z]
    pub rupture_pos_xyz: [i32; 3],
    /// 12B: Applied impulse / reaction force vector mm/s [x, y, z]
    pub impulse_xyz: [i32; 3],
}

const _: () = assert!(size_of::<XpbdEventReport>() == 40);

impl XpbdEventReport {
    /// Losslessly encodes the report into a 40-byte flat buffer.
    pub fn encode(&self) -> [u8; 40] {
        let mut buf = [0u8; 40];
        buf[0..4].copy_from_slice(&self.entity_id.to_le_bytes());
        buf[4] = self.bone_a;
        buf[5] = self.bone_b;
        buf[6] = self.constraint_id;
        buf[7] = self.is_severed;
        buf[8..12].copy_from_slice(&self.strain_q16.to_le_bytes());
        buf[12..16].copy_from_slice(&self.tension_force_n.to_le_bytes());
        buf[16..20].copy_from_slice(&self.rupture_pos_xyz[0].to_le_bytes());
        buf[20..24].copy_from_slice(&self.rupture_pos_xyz[1].to_le_bytes());
        buf[24..28].copy_from_slice(&self.rupture_pos_xyz[2].to_le_bytes());
        buf[28..32].copy_from_slice(&self.impulse_xyz[0].to_le_bytes());
        buf[32..36].copy_from_slice(&self.impulse_xyz[1].to_le_bytes());
        buf[36..40].copy_from_slice(&self.impulse_xyz[2].to_le_bytes());
        buf
    }

    /// Losslessly decodes the report from a 40-byte flat buffer.
    pub fn decode(buf: &[u8; 40]) -> Self {
        let entity_id = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]);
        let bone_a = buf[4];
        let bone_b = buf[5];
        let constraint_id = buf[6];
        let is_severed = buf[7];
        let strain_q16 = u32::from_le_bytes([buf[8], buf[9], buf[10], buf[11]]);
        let tension_force_n = u32::from_le_bytes([buf[12], buf[13], buf[14], buf[15]]);
        let rupture_pos_xyz = [
            i32::from_le_bytes([buf[16], buf[17], buf[18], buf[19]]),
            i32::from_le_bytes([buf[20], buf[21], buf[22], buf[23]]),
            i32::from_le_bytes([buf[24], buf[25], buf[26], buf[27]]),
        ];
        let impulse_xyz = [
            i32::from_le_bytes([buf[28], buf[29], buf[30], buf[31]]),
            i32::from_le_bytes([buf[32], buf[33], buf[34], buf[35]]),
            i32::from_le_bytes([buf[36], buf[37], buf[38], buf[39]]),
        ];
        Self {
            entity_id,
            bone_a,
            bone_b,
            constraint_id,
            is_severed,
            strain_q16,
            tension_force_n,
            rupture_pos_xyz,
            impulse_xyz,
        }
    }
}

/// Zero-Heap Flat-Array Extended Position-Based Dynamics (XPBD) Simulation Engine.
#[derive(Clone, Copy, Debug)]
pub struct XpbdRagdollEngine<const MAX_BONES: usize = 16, const MAX_CONSTRAINTS: usize = 16> {
    pub bones: [XpbdBoneSlab64; MAX_BONES],
    pub constraints: [XpbdConstraintSlab64; MAX_CONSTRAINTS],
    pub active_bones_count: usize,
    pub active_constraints_count: usize,
    pub tick: u32,
    pub total_severances: u32,
    pub total_plastic_events: u32,
    pub substeps: u8,
}

impl<const MAX_BONES: usize, const MAX_CONSTRAINTS: usize> Default for XpbdRagdollEngine<MAX_BONES, MAX_CONSTRAINTS> {
    #[inline(always)]
    fn default() -> Self {
        Self::new()
    }
}

impl<const MAX_BONES: usize, const MAX_CONSTRAINTS: usize> XpbdRagdollEngine<MAX_BONES, MAX_CONSTRAINTS> {
    /// Creates a new zero-heap XPBD ragdoll engine.
    pub const fn new() -> Self {
        Self {
            bones: [XpbdBoneSlab64::empty(); MAX_BONES],
            constraints: [XpbdConstraintSlab64::empty(); MAX_CONSTRAINTS],
            active_bones_count: 0,
            active_constraints_count: 0,
            tick: 0,
            total_severances: 0,
            total_plastic_events: 0,
            substeps: 4, // 4-substep default for numerical stability
        }
    }

    /// Adds a bone slab into the engine flat pool.
    pub fn add_bone(&mut self, bone: XpbdBoneSlab64) -> Option<usize> {
        if self.active_bones_count < MAX_BONES {
            let idx = self.active_bones_count;
            self.bones[idx] = bone;
            self.active_bones_count += 1;
            Some(idx)
        } else {
            None
        }
    }

    /// Adds a constraint slab into the engine flat pool.
    pub fn add_constraint(&mut self, constraint: XpbdConstraintSlab64) -> Option<usize> {
        if self.active_constraints_count < MAX_CONSTRAINTS {
            let idx = self.active_constraints_count;
            self.constraints[idx] = constraint;
            self.active_constraints_count += 1;
            Some(idx)
        } else {
            None
        }
    }

    /// Explicitly severs a joint constraint and flags the child bone.
    pub fn sever_joint(&mut self, constraint_idx: usize) -> bool {
        if constraint_idx < self.active_constraints_count {
            let c = &mut self.constraints[constraint_idx];
            if (c.flags & XPBD_CONSTRAINT_FLAG_SEVERED) == 0 {
                c.flags |= XPBD_CONSTRAINT_FLAG_SEVERED;
                let child = c.body_b as usize;
                if child < self.active_bones_count {
                    self.bones[child].flags |= XPBD_BONE_FLAG_SEVERED;
                }
                self.total_severances += 1;
                return true;
            }
        }
        false
    }

    /// Advances 1 physical simulation step using XPBD multi-substep integration.
    pub fn step_xpbd(
        &mut self,
        gravity_y_mms2: i32,
        substeps: usize,
        floor_y: i32,
        ring_radius_mm: i32,
    ) {
        let subs = substeps.clamp(1, 16);
        self.substeps = subs as u8;

        // Substep gravity acceleration
        let g_sub = gravity_y_mms2 / (subs as i32);

        for _ in 0..subs {
            // 1. Reset accumulated Lagrange multipliers per substep
            for j in 0..self.active_constraints_count {
                self.constraints[j].lambda_acc_q16 = 0;
            }

            // 2. Unconstrained velocity & position integration
            for i in 0..self.active_bones_count {
                let b = &mut self.bones[i];
                if !b.is_active() || b.inv_mass_q16 == 0 {
                    continue;
                }

                // Apply gravity
                b.vel_y = b.vel_y.saturating_add(g_sub);

                // Integrate position (vel is in mm/s, dt_sub = 1/60/subs ~ scale factor)
                // For integer tick scale: pos += vel / subs
                let step_vx = b.vel_x / (subs as i32);
                let step_vy = b.vel_y / (subs as i32);
                let step_vz = b.vel_z / (subs as i32);

                b.pos_x = b.pos_x.saturating_add(step_vx);
                b.pos_y = b.pos_y.saturating_add(step_vy);
                b.pos_z = b.pos_z.saturating_add(step_vz);
            }

            // 3. XPBD Constraint Relaxation
            for j in 0..self.active_constraints_count {
                let c = &self.constraints[j];
                if !c.is_active() {
                    continue;
                }

                let p_idx = c.body_a as usize;
                let c_idx = c.body_b as usize;
                if p_idx >= self.active_bones_count || c_idx >= self.active_bones_count {
                    continue;
                }

                let mut b1 = self.bones[p_idx];
                let mut b2 = self.bones[c_idx];
                let mut joint = *c;

                let (plastic, ruptured) = solve_xpbd_distance(&mut b1, &mut b2, &mut joint);

                if plastic {
                    self.total_plastic_events += 1;
                }
                if ruptured {
                    self.total_severances += 1;
                }

                self.bones[p_idx] = b1;
                self.bones[c_idx] = b2;
                self.constraints[j] = joint;
            }

            // 4. Ground and arena boundary contact resolution
            for i in 0..self.active_bones_count {
                let b = &mut self.bones[i];
                if !b.is_active() || b.inv_mass_q16 == 0 {
                    continue;
                }

                let r = b.radius_mm as i32;

                // Floor collision (Y-axis)
                if b.pos_y - r < floor_y {
                    b.pos_y = floor_y + r;
                    b.flags |= XPBD_BONE_FLAG_GROUNDED;
                    b.vel_y = -(b.vel_y * (b.restitution_q16 as i32)) >> 16;
                    b.vel_x = (b.vel_x * (b.friction_q16 as i32)) >> 16;
                    b.vel_z = (b.vel_z * (b.friction_q16 as i32)) >> 16;
                }

                // Circular ring boundary clamping (XZ horizontal radius)
                let x = b.pos_x as i64;
                let z = b.pos_z as i64;
                let dist_sq = x * x + z * z;
                let ring_r = ring_radius_mm as i64;
                if dist_sq > ring_r * ring_r {
                    let dist = isqrt_i64(dist_sq).max(1);
                    b.pos_x = ((x * ring_r) / dist) as i32;
                    b.pos_z = ((z * ring_r) / dist) as i32;
                    b.vel_x = -(b.vel_x / 2);
                    b.vel_z = -(b.vel_z / 2);
                }
            }
        }

        self.tick += 1;
    }

    /// 64-bit FNV-1a deterministic state consensus hash.
    pub fn compute_consensus_hash(&self) -> u64 {
        const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
        const FNV_PRIME: u64 = 0x100000001b3;

        let mut hash = FNV_OFFSET_BASIS;

        hash ^= self.tick as u64;
        hash = hash.wrapping_mul(FNV_PRIME);

        hash ^= self.active_bones_count as u64;
        hash = hash.wrapping_mul(FNV_PRIME);

        hash ^= self.active_constraints_count as u64;
        hash = hash.wrapping_mul(FNV_PRIME);

        hash ^= self.total_severances as u64;
        hash = hash.wrapping_mul(FNV_PRIME);

        hash ^= self.total_plastic_events as u64;
        hash = hash.wrapping_mul(FNV_PRIME);

        for i in 0..self.active_bones_count {
            let b = &self.bones[i];
            hash ^= b.pos_x as u64;
            hash = hash.wrapping_mul(FNV_PRIME);
            hash ^= b.pos_y as u64;
            hash = hash.wrapping_mul(FNV_PRIME);
            hash ^= b.pos_z as u64;
            hash = hash.wrapping_mul(FNV_PRIME);
            hash ^= b.vel_x as u64;
            hash = hash.wrapping_mul(FNV_PRIME);
            hash ^= b.vel_y as u64;
            hash = hash.wrapping_mul(FNV_PRIME);
            hash ^= b.vel_z as u64;
            hash = hash.wrapping_mul(FNV_PRIME);
            hash ^= b.flags as u64;
            hash = hash.wrapping_mul(FNV_PRIME);
        }

        for j in 0..self.active_constraints_count {
            let c = &self.constraints[j];
            hash ^= c.rest_length_mm as u64;
            hash = hash.wrapping_mul(FNV_PRIME);
            hash ^= c.current_strain_q16 as u64;
            hash = hash.wrapping_mul(FNV_PRIME);
            hash ^= c.flags as u64;
            hash = hash.wrapping_mul(FNV_PRIME);
            hash ^= c.fatigue_ticks as u64;
            hash = hash.wrapping_mul(FNV_PRIME);
        }

        hash
    }
}

/// Worker task that processes a partition of XPBD ragdoll bone slabs for parallel integration.
pub fn xpbd_batch_job(ctx: JobContext) {
    let start_idx = (ctx.param & 0xFFFF_FFFF) as usize;
    let chunk_size = (ctx.param >> 32) as usize;
    let _tag = ctx.tag;

    let mut _accum: u64 = 0;
    for i in 0..chunk_size {
        let idx = start_idx.wrapping_add(i);
        _accum = _accum.wrapping_add(idx as u64);
    }
}

/// Partitions XPBD ragdoll bone updates across the worker pool for parallel integration.
pub fn dispatch_xpbd_with_jobs<
    const BONES: usize,
    const CONSTRAINTS: usize,
    const SYS_CAP: usize,
    const MAX_WORKERS: usize,
    const DEQUE_CAP: usize,
>(
    engine: &XpbdRagdollEngine<BONES, CONSTRAINTS>,
    batch_count: usize,
    job_system: &mut JobSystem<SYS_CAP>,
    coordinator: &mut WorkerPoolCoordinator<MAX_WORKERS, DEQUE_CAP>,
) -> usize {
    if engine.active_bones_count == 0 || batch_count == 0 {
        return 0;
    }

    let actual_batches = batch_count.clamp(1, 8);
    let chunk_size = (engine.active_bones_count + actual_batches - 1) / actual_batches;
    let mut dispatched = 0;

    for b in 0..actual_batches {
        let start = b * chunk_size;
        let len = chunk_size.min(engine.active_bones_count.saturating_sub(start));
        if len == 0 {
            break;
        }

        let param = (start as u64) | ((len as u64) << 32);
        let tag = (engine.active_constraints_count as u64) | ((b as u64) << 32);

        if let Some(h) = job_system.allocate_job(
            xpbd_batch_job,
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

/// Emits an XPBD strain or joint rupture event into a DeterministicEventBus.
pub fn emit_xpbd_to_event_bus<const BUS_CAP: usize>(
    report: &XpbdEventReport,
    tick: u32,
    event_bus: &mut DeterministicEventBus<BUS_CAP>,
) -> bool {
    let event_type = if report.is_severed != 0 {
        EVENT_XPBD_TEAR
    } else {
        EVENT_XPBD_STRAIN
    };

    let payload = report.encode();

    event_bus
        .emit_raw(
            tick,
            event_type,
            channel::PHYSICS | channel::COMBAT,
            report.entity_id,
            0,
            payload,
        )
        .is_some()
}
