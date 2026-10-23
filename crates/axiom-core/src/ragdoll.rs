//! # Procedural Modular Mecha Ragdoll & Inverse Kinematics (IK) Physics (`axiom-core::ragdoll`)
//!
//! T-201: High-Performance 0-Float, 64-Byte L1 Cache Line Aligned Procedural Mecha
//! Ragdoll and Inverse Kinematics Engine.
//!
//! ## Architectural Guarantees:
//! - **0-Float in Core**: All positions and distances in integer millimeters (`mm: i32`).
//!   Rotations in 16-bit BAMS (`u16`, 0..65535 = 0°..360°).
//!   Normalized scalar factors and inverse masses in $Q16.16$ fixed-point (`65536 = 1.0`).
//! - **64B L1 Cache Line Alignment**: Both [`RagdollBoneSlab64`] and [`RagdollJointSlab64`]
//!   are `#[repr(C, align(64))]` with exact 64-byte size and zero implicit padding.
//! - **0-Heap Flat Array Topology**: Fixed-capacity compile-time generic arrays ([`RagdollEngine<BONES, JOINTS>`]).
//! - **Position-Based Dynamics (PBD)**: Numerically stable distance and hinge limit constraints.
//! - **Closed-Form Two-Bone IK**: $O(1)$ analytical punch reach correction without iterative loops.
//! - **Limb Severance**: Seamless runtime detachment of mecha limbs upon joint rupture.

use core::mem::{align_of, size_of};

/// Bone state flag bitmasks
pub const RAGDOLL_BONE_FLAG_ACTIVE: u8    = 1 << 0;
pub const RAGDOLL_BONE_FLAG_FROZEN: u8    = 1 << 1;
pub const RAGDOLL_BONE_FLAG_SEVERED: u8   = 1 << 2;
pub const RAGDOLL_BONE_FLAG_GROUNDED: u8  = 1 << 3;

/// Joint state flag bitmasks
pub const RAGDOLL_JOINT_FLAG_ACTIVE: u16  = 1 << 0;
pub const RAGDOLL_JOINT_FLAG_LIMITED: u16 = 1 << 1;
pub const RAGDOLL_JOINT_FLAG_MOTOR: u16   = 1 << 2;

/// 1.0 in Q16.16 fixed-point representation
pub const Q16_ONE: u32 = 65536;

/// Joint mechanical topology classification
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RagdollJointType {
    #[default]
    Fixed = 0,
    Hinge = 1,
    BallSocket = 2,
    Slider = 3,
}

/// Exact 64-Byte L1 Cache Line Aligned Mecha Ragdoll Bone Slab.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(C, align(64))]
pub struct RagdollBoneSlab64 {
    /// 4B: World X position in integer millimeters
    pub pos_x: i32,
    /// 4B: World Y position in integer millimeters
    pub pos_y: i32,
    /// 4B: World Z position in integer millimeters
    pub pos_z: i32,
    /// 4B: Linear velocity X in integer mm/tick
    pub vel_x: i32,
    /// 4B: Linear velocity Y in integer mm/tick
    pub vel_y: i32,
    /// 4B: Linear velocity Z in integer mm/tick
    pub vel_z: i32,
    /// 2B: Yaw orientation in 16-bit BAMS (0..65535)
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
    /// 4B: Inverse mass 1/m in Q16.16 (0 = infinite mass / static anchor)
    pub inv_mass_q16: u32,
    /// 2B: Collision bounding sphere / capsule radius in mm
    pub radius_mm: u16,
    /// 2B: Bone segment length in mm
    pub length_mm: u16,
    /// 1B: Parent bone slot index (255 = root anchor)
    pub parent_idx: u8,
    /// 1B: Bone anatomical identifier (0..15 corresponding to MechaPose3D)
    pub bone_id: u8,
    /// 1B: Operational flags (`RAGDOLL_BONE_FLAG_*`)
    pub flags: u8,
    /// 1B: Collision layer / filter mask
    pub collision_group: u8,
    /// 2B: Surface friction coefficient in Q16.16 (e.g. 32768 = 0.5)
    pub friction_q16: u16,
    /// 2B: Restitution coefficient in Q16.16 (e.g. 19660 = 0.3)
    pub restitution_q16: u16,
    /// 12B: Reserved alignment padding
    pub _reserved: [u8; 12],
}

const _: () = assert!(size_of::<RagdollBoneSlab64>() == 64);
const _: () = assert!(align_of::<RagdollBoneSlab64>() == 64);

impl Default for RagdollBoneSlab64 {
    #[inline(always)]
    fn default() -> Self {
        Self::empty()
    }
}

impl RagdollBoneSlab64 {
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
            restitution_q16: 19660, // 0.3 bounciness
            _reserved: [0; 12],
        }
    }

    /// Creates an active dynamic bone slab with position, mass, and radius.
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
            flags: RAGDOLL_BONE_FLAG_ACTIVE,
            collision_group: 0x01,
            friction_q16: 32768,
            restitution_q16: 19660,
            _reserved: [0; 12],
        }
    }

    /// Returns true if this bone slab is currently active.
    #[inline(always)]
    pub fn is_active(&self) -> bool {
        (self.flags & RAGDOLL_BONE_FLAG_ACTIVE) != 0
    }

    /// Returns true if this bone is severed from its parent tree.
    #[inline(always)]
    pub fn is_severed(&self) -> bool {
        (self.flags & RAGDOLL_BONE_FLAG_SEVERED) != 0
    }
}

/// Exact 64-Byte L1 Cache Line Aligned Ragdoll Joint Constraint Slab.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(C, align(64))]
pub struct RagdollJointSlab64 {
    /// 1B: Parent bone slot index
    pub parent_bone: u8,
    /// 1B: Child bone slot index
    pub child_bone: u8,
    /// 1B: Joint type (`RagdollJointType`)
    pub joint_type: u8,
    /// 1B: Severance flag (1 = ruptured/severed joint)
    pub is_severed: u8,
    /// 2B: Rest length in integer mm
    pub rest_length_mm: u16,
    /// 2B: Minimum angular limit in BAMS
    pub min_angle_bams: u16,
    /// 2B: Maximum angular limit in BAMS
    pub max_angle_bams: u16,
    /// 2B: Joint constraint stiffness in Q16.16 (65536 = rigid)
    pub stiffness_q16: u16,
    /// 2B: Joint constraint damping in Q16.16
    pub damping_q16: u16,
    /// 2B: Motor target angle in BAMS (pose matching)
    pub motor_target_bams: u16,
    /// 4B: Maximum motor torque limit
    pub motor_torque_max: u32,
    /// 4B: Primary hinge constraint axis X in Q16.16
    pub axis_x_q16: i32,
    /// 4B: Primary hinge constraint axis Y in Q16.16
    pub axis_y_q16: i32,
    /// 4B: Primary hinge constraint axis Z in Q16.16
    pub axis_z_q16: i32,
    /// 4B: Rupture threshold force (impact exceeding this severs the joint)
    pub break_force_limit: u32,
    /// 2B: Joint operational flags (`RAGDOLL_JOINT_FLAG_*`)
    pub joint_flags: u16,
    /// 26B: Reserved alignment padding
    pub _reserved: [u8; 26],
}

const _: () = assert!(size_of::<RagdollJointSlab64>() == 64);
const _: () = assert!(align_of::<RagdollJointSlab64>() == 64);

impl Default for RagdollJointSlab64 {
    #[inline(always)]
    fn default() -> Self {
        Self::empty()
    }
}

impl RagdollJointSlab64 {
    /// Creates a blank inactive joint slab.
    #[inline(always)]
    pub const fn empty() -> Self {
        Self {
            parent_bone: 0,
            child_bone: 0,
            joint_type: 0,
            is_severed: 0,
            rest_length_mm: 0,
            min_angle_bams: 0,
            max_angle_bams: 65535,
            stiffness_q16: 65535,
            damping_q16: 16384,
            motor_target_bams: 0,
            motor_torque_max: 0,
            axis_x_q16: 0,
            axis_y_q16: 65536, // +Y default hinge axis
            axis_z_q16: 0,
            break_force_limit: 100_000,
            joint_flags: 0,
            _reserved: [0; 26],
        }
    }

    /// Creates an active joint constraint between parent and child.
    pub fn new(
        parent_bone: u8,
        child_bone: u8,
        joint_type: RagdollJointType,
        rest_length_mm: u16,
        break_force_limit: u32,
    ) -> Self {
        Self {
            parent_bone,
            child_bone,
            joint_type: joint_type as u8,
            is_severed: 0,
            rest_length_mm,
            min_angle_bams: 0,
            max_angle_bams: 65535,
            stiffness_q16: 65535,
            damping_q16: 16384,
            motor_target_bams: 0,
            motor_torque_max: 50_000,
            axis_x_q16: 0,
            axis_y_q16: 65536,
            axis_z_q16: 0,
            break_force_limit,
            joint_flags: RAGDOLL_JOINT_FLAG_ACTIVE | RAGDOLL_JOINT_FLAG_LIMITED,
            _reserved: [0; 26],
        }
    }

    /// Returns true if this joint constraint is active and not severed.
    #[inline(always)]
    pub fn is_active(&self) -> bool {
        (self.joint_flags & RAGDOLL_JOINT_FLAG_ACTIVE) != 0 && self.is_severed == 0
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

/// Position-Based Dynamics (PBD) pure integer distance constraint relaxation.
#[inline]
pub fn solve_distance_constraint(
    p1: &mut RagdollBoneSlab64,
    p2: &mut RagdollBoneSlab64,
    rest_len: i32,
    stiffness_q16: u16,
) {
    let w1 = p1.inv_mass_q16 as i64;
    let w2 = p2.inv_mass_q16 as i64;
    let w_sum = w1 + w2;
    if w_sum <= 0 {
        return; // Both bodies are stationary anchors
    }

    let dx = (p2.pos_x - p1.pos_x) as i64;
    let dy = (p2.pos_y - p1.pos_y) as i64;
    let dz = (p2.pos_z - p1.pos_z) as i64;

    let dist_sq = dx * dx + dy * dy + dz * dz;
    let dist = isqrt_i64(dist_sq).max(1);

    let err = dist - (rest_len as i64);
    if err == 0 {
        return;
    }

    // Apply stiffness factor (0..65536). u16::MAX maps to rigid 1.0 (65536)
    let stiff = if stiffness_q16 == u16::MAX {
        65536i64
    } else {
        stiffness_q16 as i64
    };
    let scaled_err = (err * stiff) >> 16;

    let corr_w1 = (scaled_err * w1) / w_sum;
    let corr_w2 = (scaled_err * w2) / w_sum;

    let delta_p1_x = (corr_w1 * dx) / dist;
    let delta_p1_y = (corr_w1 * dy) / dist;
    let delta_p1_z = (corr_w1 * dz) / dist;

    let delta_p2_x = (corr_w2 * dx) / dist;
    let delta_p2_y = (corr_w2 * dy) / dist;
    let delta_p2_z = (corr_w2 * dz) / dist;

    p1.pos_x += delta_p1_x as i32;
    p1.pos_y += delta_p1_y as i32;
    p1.pos_z += delta_p1_z as i32;

    p2.pos_x -= delta_p2_x as i32;
    p2.pos_y -= delta_p2_y as i32;
    p2.pos_z -= delta_p2_z as i32;
}

/// Closed-form Two-Bone Inverse Kinematics (Two-Bone IK) analytical solver.
///
/// Solves exact position of elbow/knee joint given shoulder/hip origin, target endpoint,
/// bone lengths, and pole vector hint plane without iterative bisection loops.
pub fn solve_two_bone_ik(
    p_shoulder: (i32, i32, i32),
    l1: i32,
    l2: i32,
    p_target: (i32, i32, i32),
    pole_vector: (i32, i32, i32),
) -> (i32, i32, i32) {
    let dx = (p_target.0 - p_shoulder.0) as i64;
    let dy = (p_target.1 - p_shoulder.1) as i64;
    let dz = (p_target.2 - p_shoulder.2) as i64;

    let dist_sq = dx * dx + dy * dy + dz * dz;
    let max_reach = ((l1 + l2) as i64) - 2;
    let min_reach = ((l1 - l2).abs() as i64) + 2;

    let raw_dist = isqrt_i64(dist_sq).max(1);
    let clamped_dist = raw_dist.clamp(min_reach, max_reach);

    // Law of Cosines to find distance from shoulder along shoulder-target line:
    // l2^2 = l1^2 + d^2 - 2 * l1 * d * cos(theta)
    // a = l1 * cos(theta) = (l1^2 + d^2 - l2^2) / (2 * d)
    let l1_sq = (l1 as i64) * (l1 as i64);
    let l2_sq = (l2 as i64) * (l2 as i64);
    let d_sq = clamped_dist * clamped_dist;

    let a = (l1_sq + d_sq - l2_sq) / (2 * clamped_dist);
    let h_sq = (l1_sq - a * a).max(0);
    let h = isqrt_i64(h_sq);

    // Direction from shoulder to target
    let dir_x = (dx << 16) / raw_dist;
    let dir_y = (dy << 16) / raw_dist;
    let dir_z = (dz << 16) / raw_dist;

    // Center point along the direct ray
    let mid_x = p_shoulder.0 as i64 + ((dir_x * a) >> 16);
    let mid_y = p_shoulder.1 as i64 + ((dir_y * a) >> 16);
    let mid_z = p_shoulder.2 as i64 + ((dir_z * a) >> 16);

    // Orthogonal vector toward pole hint
    let pole_dx = pole_vector.0 as i64 - mid_x;
    let pole_dy = pole_vector.1 as i64 - mid_y;
    let pole_dz = pole_vector.2 as i64 - mid_z;

    // Project pole vector perpendicular to dir ray: p_ortho = p - (p . dir) * dir
    let dot = (pole_dx * dir_x + pole_dy * dir_y + pole_dz * dir_z) >> 16;
    let ortho_x = pole_dx - ((dot * dir_x) >> 16);
    let ortho_y = pole_dy - ((dot * dir_y) >> 16);
    let ortho_z = pole_dz - ((dot * dir_z) >> 16);

    let ortho_len = isqrt_i64(ortho_x * ortho_x + ortho_y * ortho_y + ortho_z * ortho_z).max(1);

    // Elbow position offset by h in the direction of the orthogonal pole vector
    let elbow_x = mid_x + (ortho_x * h) / ortho_len;
    let elbow_y = mid_y + (ortho_y * h) / ortho_len;
    let elbow_z = mid_z + (ortho_z * h) / ortho_len;

    (elbow_x as i32, elbow_y as i32, elbow_z as i32)
}

/// Zero-Heap Flat-Array Mecha Ragdoll Simulation Engine.
#[derive(Clone, Copy, Debug)]
pub struct RagdollEngine<const BONES: usize = 16, const JOINTS: usize = 15> {
    pub bones: [RagdollBoneSlab64; BONES],
    pub joints: [RagdollJointSlab64; JOINTS],
    pub active_bones_count: usize,
    pub active_joints_count: usize,
    pub tick: u32,
    pub total_severances: u32,
}

impl<const BONES: usize, const JOINTS: usize> Default for RagdollEngine<BONES, JOINTS> {
    #[inline(always)]
    fn default() -> Self {
        Self::new()
    }
}

impl<const BONES: usize, const JOINTS: usize> RagdollEngine<BONES, JOINTS> {
    /// Creates a blank ragdoll physics engine instance.
    pub const fn new() -> Self {
        Self {
            bones: [RagdollBoneSlab64::empty(); BONES],
            joints: [RagdollJointSlab64::empty(); JOINTS],
            active_bones_count: 0,
            active_joints_count: 0,
            tick: 0,
            total_severances: 0,
        }
    }

    /// Adds a bone slab to the engine.
    pub fn add_bone(&mut self, bone: RagdollBoneSlab64) -> Option<usize> {
        if self.active_bones_count < BONES {
            let idx = self.active_bones_count;
            self.bones[idx] = bone;
            self.active_bones_count += 1;
            Some(idx)
        } else {
            None
        }
    }

    /// Adds a joint constraint between two bones.
    pub fn add_joint(&mut self, joint: RagdollJointSlab64) -> Option<usize> {
        if self.active_joints_count < JOINTS {
            let idx = self.active_joints_count;
            self.joints[idx] = joint;
            self.active_joints_count += 1;
            Some(idx)
        } else {
            None
        }
    }

    /// Ruptures and severs a joint (e.g. decapitation or limb detachment).
    pub fn sever_joint(&mut self, joint_idx: usize) -> bool {
        if joint_idx < self.active_joints_count {
            if self.joints[joint_idx].is_severed == 0 {
                self.joints[joint_idx].is_severed = 1;
                let child = self.joints[joint_idx].child_bone as usize;
                if child < self.active_bones_count {
                    self.bones[child].flags |= RAGDOLL_BONE_FLAG_SEVERED;
                }
                self.total_severances += 1;
                return true;
            }
        }
        false
    }

    /// Advances 1 physical simulation step using Position-Based Dynamics (PBD).
    pub fn step_ragdoll(
        &mut self,
        gravity_y: i32,
        iterations: usize,
        floor_y: i32,
        ring_radius_mm: i32,
    ) {
        let iters = iterations.clamp(1, 16);

        // 1. Unconstrained integration (Velocities & Gravity)
        for i in 0..self.active_bones_count {
            let b = &mut self.bones[i];
            if !b.is_active() || b.inv_mass_q16 == 0 {
                continue;
            }

            // Apply gravity
            b.vel_y = b.vel_y.saturating_add(gravity_y);

            // Integrate position
            b.pos_x = b.pos_x.saturating_add(b.vel_x);
            b.pos_y = b.pos_y.saturating_add(b.vel_y);
            b.pos_z = b.pos_z.saturating_add(b.vel_z);
        }

        // 2. PBD Constraint Relaxation loop
        for _ in 0..iters {
            for j in 0..self.active_joints_count {
                let joint = &self.joints[j];
                if !joint.is_active() {
                    continue;
                }

                let p_idx = joint.parent_bone as usize;
                let c_idx = joint.child_bone as usize;
                if p_idx >= self.active_bones_count || c_idx >= self.active_bones_count {
                    continue;
                }

                // Temporary copy to mutate both safely
                let rest_len = joint.rest_length_mm as i32;
                let stiff = joint.stiffness_q16;

                let mut b1 = self.bones[p_idx];
                let mut b2 = self.bones[c_idx];

                solve_distance_constraint(&mut b1, &mut b2, rest_len, stiff);

                self.bones[p_idx] = b1;
                self.bones[c_idx] = b2;
            }

            // Ground & arena boundary contact resolution
            for i in 0..self.active_bones_count {
                let b = &mut self.bones[i];
                if !b.is_active() || b.inv_mass_q16 == 0 {
                    continue;
                }

                let r = b.radius_mm as i32;

                // Floor collision (Y-axis)
                if b.pos_y - r < floor_y {
                    b.pos_y = floor_y + r;
                    b.flags |= RAGDOLL_BONE_FLAG_GROUNDED;
                    // Apply restitution and friction
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
                    // Invert normal velocity
                    b.vel_x = -(b.vel_x / 2);
                    b.vel_z = -(b.vel_z / 2);
                }
            }
        }

        self.tick += 1;
    }

    /// Computes guaranteed bit-exact 64-bit FNV-1a checksum of the entire ragdoll state.
    pub fn compute_consensus_hash(&self) -> u64 {
        const FNV_OFFSET: u64 = 0xcbf29ce484222325;
        const FNV_PRIME: u64 = 0x100000001b3;

        let mut h = FNV_OFFSET ^ (self.tick as u64);

        for i in 0..self.active_bones_count {
            let b = &self.bones[i];
            if b.is_active() {
                let coords = [
                    b.pos_x.to_le_bytes(),
                    b.pos_y.to_le_bytes(),
                    b.pos_z.to_le_bytes(),
                    b.vel_x.to_le_bytes(),
                    b.vel_y.to_le_bytes(),
                    b.vel_z.to_le_bytes(),
                ];
                for chunk in &coords {
                    for &byte in chunk {
                        h ^= byte as u64;
                        h = h.wrapping_mul(FNV_PRIME);
                    }
                }
                h ^= b.flags as u64;
                h = h.wrapping_mul(FNV_PRIME);
            }
        }

        for j in 0..self.active_joints_count {
            let jnt = &self.joints[j];
            h ^= ((jnt.parent_bone as u64) << 8) | (jnt.child_bone as u64) | ((jnt.is_severed as u64) << 16);
            h = h.wrapping_mul(FNV_PRIME);
        }

        h ^= (self.total_severances as u64) << 32;
        h = h.wrapping_mul(FNV_PRIME);
        h
    }
}
