//! 3D Mecha Kinematics and Forward Kinematics (FK) Hierarchy Pipeline.
//!
//! Evaluates discrete joint matrices for mecha body parts (torso, arms, gloves, legs)
//! adhering to Axiom's 100% deterministic fixed-point Q16.16 principles.

use crate::math3d::{Mat4Fixed, Vec3Fixed};

/// Pose description for 3D articulated mecha robotics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MechaPose3D {
    pub root_offset: Vec3Fixed,
    pub root_rot_y: u8,
    pub root_roll_z: u8,
    pub root_pitch_x: u8,

    pub torso_yaw_y: u8,
    pub torso_pitch_x: u8,

    pub head_pitch_x: u8,
    pub head_yaw_y: u8,

    pub arm_l_offset: Vec3Fixed,
    pub arm_l_pitch_x: u8,
    pub glove_l_offset: Vec3Fixed,

    pub arm_r_offset: Vec3Fixed,
    pub arm_r_pitch_x: u8,
    pub glove_r_offset: Vec3Fixed,

    pub leg_l_drop: Vec3Fixed,
    pub leg_r_drop: Vec3Fixed,
}

impl Default for MechaPose3D {
    fn default() -> Self {
        Self {
            root_offset: Vec3Fixed::ZERO,
            root_rot_y: 0,
            root_roll_z: 0,
            root_pitch_x: 0,
            torso_yaw_y: 0,
            torso_pitch_x: 0,
            head_pitch_x: 0,
            head_yaw_y: 0,
            arm_l_offset: Vec3Fixed::ZERO,
            arm_l_pitch_x: 0,
            glove_l_offset: Vec3Fixed::ZERO,
            arm_r_offset: Vec3Fixed::ZERO,
            arm_r_pitch_x: 0,
            glove_r_offset: Vec3Fixed::ZERO,
            leg_l_drop: Vec3Fixed::ZERO,
            leg_r_drop: Vec3Fixed::ZERO,
        }
    }
}

impl MechaPose3D {
    /// Idle mechanical breathing pose (subtle vertical bobbing).
    pub fn idle(tick: u64) -> Self {
        let bob_raw = ((tick & 0x1F) as i64 - 16).abs() * 200; // subtle bob
        let mut pose = Self::default();
        pose.root_offset = Vec3Fixed::from_raw(0, bob_raw, 0);
        pose
    }

    /// Lead Left Jab: left arm snaps forward +0.32m, glove extends.
    pub fn jab(permille: u32) -> Self {
        let mut pose = Self::default();
        // Snappy forward curve: reaches max around permille = 400
        let ext = if permille < 400 {
            (permille as i64 * 320) / 400
        } else {
            ((1000 - permille as i64) * 320) / 600
        };

        let fwd_z = (ext * 65536) / 1000;
        pose.arm_l_offset = Vec3Fixed::from_raw(0, 0, fwd_z / 2);
        pose.glove_l_offset = Vec3Fixed::from_raw(0, 0, fwd_z);
        pose.torso_yaw_y = 6; // slight counter twist
        pose
    }

    /// Rear Right Cross: deep forward thrust +0.45m, torso twists.
    pub fn cross(permille: u32) -> Self {
        let mut pose = Self::default();
        let ext = if permille < 450 {
            (permille as i64 * 450) / 450
        } else {
            ((1000 - permille as i64) * 450) / 550
        };

        let fwd_z = (ext * 65536) / 1000;
        pose.torso_yaw_y = 244; // Clockwise twist (-12 BAM)
        pose.arm_r_offset = Vec3Fixed::from_raw(0, 0, fwd_z / 2);
        pose.glove_r_offset = Vec3Fixed::from_raw(0, 0, fwd_z);
        pose.root_pitch_x = 4; // torso lean forward
        pose
    }

    /// Left Hook: horizontal elbow arc lever.
    pub fn hook(permille: u32) -> Self {
        let mut pose = Self::default();
        let ext = if permille < 400 {
            (permille as i64 * 280) / 400
        } else {
            ((1000 - permille as i64) * 280) / 600
        };

        let sweep_x = (ext * 65536) / 1000;
        pose.arm_l_offset = Vec3Fixed::from_raw(sweep_x / 2, 0, sweep_x / 3);
        pose.glove_l_offset = Vec3Fixed::from_raw(sweep_x, 0, sweep_x / 2);
        pose.torso_yaw_y = 14;
        pose
    }

    /// Peek-a-boo Guard: elbows tucked, both gloves raised to chin.
    pub fn guard() -> Self {
        let mut pose = Self::default();
        let lift_y = (180 * 65536) / 1000; // raise gloves
        let tuck_z = (100 * 65536) / 1000;
        pose.glove_l_offset = Vec3Fixed::from_raw(4000, lift_y, tuck_z);
        pose.glove_r_offset = Vec3Fixed::from_raw(-4000, lift_y, tuck_z);
        pose.head_pitch_x = 6; // chin tucked down
        pose
    }

    /// Slip evasion (lateral weave tilt).
    pub fn slip(is_left: bool) -> Self {
        let mut pose = Self::default();
        if is_left {
            pose.root_roll_z = 14;
            pose.root_offset = Vec3Fixed::from_f32(-0.15, -0.05, 0.0);
        } else {
            pose.root_roll_z = 242; // -14 BAM
            pose.root_offset = Vec3Fixed::from_f32(0.15, -0.05, 0.0);
        }
        pose
    }

    /// Hit Stagger: recoil back and head snaps back.
    pub fn hit_stagger() -> Self {
        let mut pose = Self::default();
        pose.head_pitch_x = 246; // snap back (-10 BAM)
        pose.torso_pitch_x = 248; // torso recoils back
        pose.root_offset = Vec3Fixed::from_f32(0.0, 0.0, -0.12);
        pose
    }

    /// Knockdown fall.
    pub fn down() -> Self {
        let mut pose = Self::default();
        pose.root_offset = Vec3Fixed::from_f32(0.0, -0.5, -0.2);
        pose.root_pitch_x = 220;
        pose
    }
}

/// A fixed lookup table for mecha part world matrices.
#[derive(Debug, Clone, Copy)]
pub struct PartTransformTable {
    pub transforms: [Mat4Fixed; 16],
}

impl Default for PartTransformTable {
    fn default() -> Self {
        Self {
            transforms: [Mat4Fixed::IDENTITY; 16],
        }
    }
}

/// Computes the forward kinematics transform table for a mecha pose.
/// Supports both hero (`hero_*`) and opponent (`foe_*`) part naming conventions.
pub fn evaluate_forward_kinematics(pose: &MechaPose3D, is_opponent: bool) -> PartTransformTable {
    let mut table = PartTransformTable::default();

    // Root matrix
    let root_trans = Mat4Fixed::from_translation(pose.root_offset);
    let root_rot_y = Mat4Fixed::from_rotation_y(pose.root_rot_y);
    let root_rot_z = Mat4Fixed::from_rotation_z(pose.root_roll_z);
    let root_rot_x = Mat4Fixed::from_rotation_x(pose.root_pitch_x);
    let root_mat = root_trans
        .mul_mat(&root_rot_y)
        .mul_mat(&root_rot_z)
        .mul_mat(&root_rot_x);

    // Torso matrix (relative to root)
    let torso_rot_y = Mat4Fixed::from_rotation_y(pose.torso_yaw_y);
    let torso_rot_x = Mat4Fixed::from_rotation_x(pose.torso_pitch_x);
    let torso_local = torso_rot_y.mul_mat(&torso_rot_x);
    let torso_mat = root_mat.mul_mat(&torso_local);

    // Head matrix (relative to torso)
    let head_rot_x = Mat4Fixed::from_rotation_x(pose.head_pitch_x);
    let head_rot_y = Mat4Fixed::from_rotation_y(pose.head_yaw_y);
    let head_local = head_rot_x.mul_mat(&head_rot_y);
    let head_mat = torso_mat.mul_mat(&head_local);

    // Left Arm & Glove
    let arm_l_trans = Mat4Fixed::from_translation(pose.arm_l_offset);
    let arm_l_rot_x = Mat4Fixed::from_rotation_x(pose.arm_l_pitch_x);
    let arm_l_mat = torso_mat.mul_mat(&arm_l_trans).mul_mat(&arm_l_rot_x);

    let glove_l_trans = Mat4Fixed::from_translation(pose.glove_l_offset);
    let glove_l_mat = arm_l_mat.mul_mat(&glove_l_trans);

    // Right Arm & Glove
    let arm_r_trans = Mat4Fixed::from_translation(pose.arm_r_offset);
    let arm_r_rot_x = Mat4Fixed::from_rotation_x(pose.arm_r_pitch_x);
    let arm_r_mat = torso_mat.mul_mat(&arm_r_trans).mul_mat(&arm_r_rot_x);

    let glove_r_trans = Mat4Fixed::from_translation(pose.glove_r_offset);
    let glove_r_mat = arm_r_mat.mul_mat(&glove_r_trans);

    // Legs
    let leg_l_trans = Mat4Fixed::from_translation(pose.leg_l_drop);
    let leg_l_mat = root_mat.mul_mat(&leg_l_trans);

    let leg_r_trans = Mat4Fixed::from_translation(pose.leg_r_drop);
    let leg_r_mat = root_mat.mul_mat(&leg_r_trans);

    if !is_opponent {
        // Hero slot mappings:
        // 0: hero_pelvis
        table.transforms[0] = root_mat;
        // 1: hero_torso
        table.transforms[1] = torso_mat;
        // 2: hero_head
        table.transforms[2] = head_mat;
        // 3: hero_eye
        table.transforms[3] = head_mat;
        // 4: hero_arm_L
        table.transforms[4] = arm_l_mat;
        // 5: hero_arm_R
        table.transforms[5] = arm_r_mat;
        // 6: hero_glove_L
        table.transforms[6] = glove_l_mat;
        // 7: hero_glove_R
        table.transforms[7] = glove_r_mat;
        // 8: hero_leg_L
        table.transforms[8] = leg_l_mat;
        // 9: hero_leg_R
        table.transforms[9] = leg_r_mat;
        // 10: hero_boot_L
        table.transforms[10] = leg_l_mat;
        // 11: hero_boot_R
        table.transforms[11] = leg_r_mat;
    } else {
        // Opponent slot mappings (foe_* convention):
        // 0: foe_body
        table.transforms[0] = torso_mat;
        // 1: foe_arm_L
        table.transforms[1] = arm_l_mat;
        // 2: foe_arm_R
        table.transforms[2] = arm_r_mat;
        // 3: foe_glove_L
        table.transforms[3] = glove_l_mat;
        // 4: foe_glove_R
        table.transforms[4] = glove_r_mat;
        // 5: foe_cape_1
        table.transforms[5] = torso_mat;
        // 6: foe_cape_2
        table.transforms[6] = torso_mat;
    }

    table
}

/// Returns the part matrix corresponding to the given part name.
pub fn get_part_matrix(table: &PartTransformTable, part_name: &str) -> Mat4Fixed {
    if part_name.contains("pelvis") {
        table.transforms[0]
    } else if part_name.contains("torso") || part_name.contains("body") {
        table.transforms[1]
    } else if part_name.contains("head") || part_name.contains("eye") {
        table.transforms[2]
    } else if part_name.contains("arm_L") {
        table.transforms[4]
    } else if part_name.contains("arm_R") {
        table.transforms[5]
    } else if part_name.contains("glove_L") {
        table.transforms[6]
    } else if part_name.contains("glove_R") {
        table.transforms[7]
    } else if part_name.contains("leg_L") || part_name.contains("boot_L") {
        table.transforms[8]
    } else if part_name.contains("leg_R") || part_name.contains("boot_R") {
        table.transforms[9]
    } else if part_name.contains("cape") {
        table.transforms[5]
    } else {
        table.transforms[0]
    }
}
