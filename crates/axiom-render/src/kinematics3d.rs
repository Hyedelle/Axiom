//! 3D Mecha Kinematics and Forward Kinematics (FK) Hierarchy Pipeline.
//!
//! Evaluates discrete joint matrices for mecha body parts (torso, arms, gloves, legs)
//! adhering to Axiom's 100% deterministic fixed-point Q16.16 principles.

use crate::math3d::{Mat4Fixed, Vec3Fixed};

/// Deterministic linear interpolation between two Vec3Fixed values (t in 0..=1000).
#[inline]
pub fn lerp_vec3_permille(a: Vec3Fixed, b: Vec3Fixed, t_permille: u32) -> Vec3Fixed {
    let t = t_permille.min(1000) as i64;
    let inv_t = 1000 - t;
    Vec3Fixed::from_raw(
        (a.x.0 * inv_t + b.x.0 * t) / 1000,
        (a.y.0 * inv_t + b.y.0 * t) / 1000,
        (a.z.0 * inv_t + b.z.0 * t) / 1000,
    )
}

/// Shortest-arc BAMS angle interpolation (t in 0..=1000).
#[inline]
pub fn lerp_bams_permille(a: u8, b: u8, t_permille: u32) -> u8 {
    let diff = (b as i32) - (a as i32);
    let wrapped = if diff > 128 {
        diff - 256
    } else if diff < -128 {
        diff + 256
    } else {
        diff
    };
    let t = t_permille.min(1000) as i32;
    let delta = (wrapped * t) / 1000;
    ((a as i32 + delta).rem_euclid(256)) as u8
}

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
            pose.root_offset = Vec3Fixed::from_raw(-9830, -3277, 0);
        } else {
            pose.root_roll_z = 242; // -14 BAM
            pose.root_offset = Vec3Fixed::from_raw(9830, -3277, 0);
        }
        pose
    }

    /// Hit Stagger: recoil back and head snaps back.
    pub fn hit_stagger() -> Self {
        let mut pose = Self::default();
        pose.head_pitch_x = 246; // snap back (-10 BAM)
        pose.torso_pitch_x = 248; // torso recoils back
        pose.root_offset = Vec3Fixed::from_raw(0, 0, -7864);
        pose
    }

    /// Knockdown fall.
    pub fn down() -> Self {
        let mut pose = Self::default();
        pose.root_offset = Vec3Fixed::from_raw(0, -32768, -13107);
        pose.root_pitch_x = 220;
        pose
    }

    /// Overcharged Nova Finisher Windup: coils both arms backward, torso compresses.
    pub fn finisher_windup(permille: u32) -> Self {
        let mut pose = Self::default();
        let p = permille.min(1000) as i64;
        let pull_z = -(p * 35000) / 1000;
        let raise_y = (p * 18000) / 1000;
        pose.arm_l_offset = Vec3Fixed::from_raw(-4000, raise_y / 2, pull_z / 2);
        pose.glove_l_offset = Vec3Fixed::from_raw(-8000, raise_y, pull_z);
        pose.arm_r_offset = Vec3Fixed::from_raw(4000, raise_y / 2, pull_z / 2);
        pose.glove_r_offset = Vec3Fixed::from_raw(8000, raise_y, pull_z);
        pose.torso_pitch_x = (p * 8 / 1000) as u8; // slight crouch
        pose.head_pitch_x = (p * 4 / 1000) as u8;
        pose
    }

    /// Overcharged Nova Finisher Strike: both fists blast forward with catastrophic force.
    pub fn finisher_impact() -> Self {
        let mut pose = Self::default();
        let blast_z = 45000i64; // massive forward thrust
        pose.arm_l_offset = Vec3Fixed::from_raw(-2000, 2000, blast_z / 2);
        pose.glove_l_offset = Vec3Fixed::from_raw(-3000, 4000, blast_z);
        pose.arm_r_offset = Vec3Fixed::from_raw(2000, 2000, blast_z / 2);
        pose.glove_r_offset = Vec3Fixed::from_raw(3000, 4000, blast_z);
        pose.torso_pitch_x = 12; // deep forward lean
        pose.root_pitch_x = 8;
        pose
    }

    /// Victory Roar Pose: right fist pumped into the stratosphere.
    pub fn victory() -> Self {
        let mut pose = Self::default();
        let high_y = 48000i64;
        pose.arm_r_offset = Vec3Fixed::from_raw(6000, high_y / 2, 0);
        pose.glove_r_offset = Vec3Fixed::from_raw(8000, high_y, 0);
        pose.head_pitch_x = 244; // looking up at the crowd (-12 BAM)
        pose.torso_yaw_y = 6;
        pose
    }

    /// Interpolates between self and other pose by t_permille (0..=1000).
    pub fn blend(&self, other: &Self, t_permille: u32) -> Self {
        Self {
            root_offset: lerp_vec3_permille(self.root_offset, other.root_offset, t_permille),
            root_rot_y: lerp_bams_permille(self.root_rot_y, other.root_rot_y, t_permille),
            root_roll_z: lerp_bams_permille(self.root_roll_z, other.root_roll_z, t_permille),
            root_pitch_x: lerp_bams_permille(self.root_pitch_x, other.root_pitch_x, t_permille),
            torso_yaw_y: lerp_bams_permille(self.torso_yaw_y, other.torso_yaw_y, t_permille),
            torso_pitch_x: lerp_bams_permille(self.torso_pitch_x, other.torso_pitch_x, t_permille),
            head_pitch_x: lerp_bams_permille(self.head_pitch_x, other.head_pitch_x, t_permille),
            head_yaw_y: lerp_bams_permille(self.head_yaw_y, other.head_yaw_y, t_permille),
            arm_l_offset: lerp_vec3_permille(self.arm_l_offset, other.arm_l_offset, t_permille),
            arm_l_pitch_x: lerp_bams_permille(self.arm_l_pitch_x, other.arm_l_pitch_x, t_permille),
            glove_l_offset: lerp_vec3_permille(self.glove_l_offset, other.glove_l_offset, t_permille),
            arm_r_offset: lerp_vec3_permille(self.arm_r_offset, other.arm_r_offset, t_permille),
            arm_r_pitch_x: lerp_bams_permille(self.arm_r_pitch_x, other.arm_r_pitch_x, t_permille),
            glove_r_offset: lerp_vec3_permille(self.glove_r_offset, other.glove_r_offset, t_permille),
            leg_l_drop: lerp_vec3_permille(self.leg_l_drop, other.leg_l_drop, t_permille),
            leg_r_drop: lerp_vec3_permille(self.leg_r_drop, other.leg_r_drop, t_permille),
        }
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
        // 12: cape
        table.transforms[12] = torso_mat;
    } else {
        // Opponent slot mappings (normalized 1:1 with get_part_matrix):
        // 0: foe_pelvis / root
        table.transforms[0] = root_mat;
        // 1: foe_torso / foe_body
        table.transforms[1] = torso_mat;
        // 2: foe_head / head
        table.transforms[2] = head_mat;
        // 3: foe_eye / eye
        table.transforms[3] = head_mat;
        // 4: foe_arm_L
        table.transforms[4] = arm_l_mat;
        // 5: foe_arm_R
        table.transforms[5] = arm_r_mat;
        // 6: foe_glove_L
        table.transforms[6] = glove_l_mat;
        // 7: foe_glove_R
        table.transforms[7] = glove_r_mat;
        // 8: foe_leg_L
        table.transforms[8] = leg_l_mat;
        // 9: foe_leg_R
        table.transforms[9] = leg_r_mat;
        // 10: foe_boot_L
        table.transforms[10] = leg_l_mat;
        // 11: foe_boot_R
        table.transforms[11] = leg_r_mat;
        // 12: foe_cape
        table.transforms[12] = torso_mat;
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
        table.transforms[12]
    } else {
        table.transforms[0]
    }
}

/// Smooth transition animation blender.
#[derive(Debug, Clone, Copy)]
pub struct AnimBlendTree {
    pub from_pose: MechaPose3D,
    pub to_pose: MechaPose3D,
    pub elapsed_ticks: u32,
    pub duration_ticks: u32,
}

impl Default for AnimBlendTree {
    fn default() -> Self {
        Self {
            from_pose: MechaPose3D::default(),
            to_pose: MechaPose3D::default(),
            elapsed_ticks: 0,
            duration_ticks: 0,
        }
    }
}

impl AnimBlendTree {
    pub fn new(initial_pose: MechaPose3D) -> Self {
        Self {
            from_pose: initial_pose,
            to_pose: initial_pose,
            elapsed_ticks: 0,
            duration_ticks: 0,
        }
    }

    /// Transitions to a new target pose over `duration_ticks`.
    pub fn transition_to(&mut self, next_pose: MechaPose3D, duration_ticks: u32) {
        self.from_pose = self.current_pose();
        self.to_pose = next_pose;
        self.elapsed_ticks = 0;
        self.duration_ticks = duration_ticks.max(1);
    }

    /// Advances the blend transition by 1 tick.
    pub fn step(&mut self) {
        if self.elapsed_ticks < self.duration_ticks {
            self.elapsed_ticks += 1;
        }
    }

    /// Computes the blended pose at the current transition progress.
    pub fn current_pose(&self) -> MechaPose3D {
        if self.duration_ticks == 0 || self.elapsed_ticks >= self.duration_ticks {
            self.to_pose
        } else {
            let permille = (self.elapsed_ticks * 1000) / self.duration_ticks;
            self.from_pose.blend(&self.to_pose, permille)
        }
    }

    /// Returns true if the blend transition has completed.
    pub fn is_finished(&self) -> bool {
        self.elapsed_ticks >= self.duration_ticks
    }
}

/// Stage progression of a cinematic finisher sequence.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FinisherStage {
    #[default]
    Inactive = 0,
    WindupCharging = 1,   // Phase 1: slow-mo charging core, camera zoom in
    NovaStrike = 2,       // Phase 2: explosive impact, max camera shake, debris sparks
    DecapitateRecoil = 3, // Phase 3: opponent collapse and stagger
    VictoryRoar = 4,      // Phase 4: champion victory orbit camera
}

/// 64-byte L1 cache aligned cinematic finisher sequencer.
#[repr(C, align(64))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CinematicFinisherDirector {
    pub stage: FinisherStage,      // 1B: Current sequence phase
    pub camera_shot_preset: u8,    // 1B: Recommended camera shot (0: Center, 3: CloseUp, 4: LowAngle)
    pub camera_shake: u8,          // 1B: Screen shake intensity (0..20)
    pub rim_light_mode: u8,        // 1B: 0=normal, 1=overcharge molten red, 2=supernova pure white
    pub spark_burst_count: u8,     // 1B: 3D sparks/debris to spawn on this tick
    pub slow_mo_divisor: u8,       // 1B: Slow motion frame skip divisor (1=normal, 3=1/3 speed)
    pub slow_mo_counter: u8,       // 1B: Internal slow motion tick counter
    pub sound_cue: u8,             // 1B: Audio cue (0=none, 1=charge, 2=impact boom, 3=victory cheer)
    pub current_tick: u16,         // 2B: Elapsed ticks in current stage
    pub stage_duration_ticks: u16, // 2B: Max duration of current stage
    pub progress_permille: u16,    // 2B: 0..=1000 completion within current stage
    pub total_elapsed_ticks: u16,  // 2B: Total duration of entire finisher
    pub is_active: u8,             // 1B: Whether finisher sequence is controlling the scene
    pub _padding: [u8; 47],        // 47B: Exact padding to 64 bytes
}

const _: () = assert!(core::mem::size_of::<CinematicFinisherDirector>() == 64);
const _: () = assert!(core::mem::align_of::<CinematicFinisherDirector>() == 64);

impl Default for CinematicFinisherDirector {
    fn default() -> Self {
        Self {
            stage: FinisherStage::Inactive,
            camera_shot_preset: 0,
            camera_shake: 0,
            rim_light_mode: 0,
            spark_burst_count: 0,
            slow_mo_divisor: 1,
            slow_mo_counter: 0,
            sound_cue: 0,
            current_tick: 0,
            stage_duration_ticks: 0,
            progress_permille: 0,
            total_elapsed_ticks: 0,
            is_active: 0,
            _padding: [0u8; 47],
        }
    }
}

impl CinematicFinisherDirector {
    pub fn new() -> Self {
        Self::default()
    }

    /// Triggers the cinematic finisher sequence.
    pub fn trigger_finisher(&mut self) {
        self.stage = FinisherStage::WindupCharging;
        self.is_active = 1;
        self.current_tick = 0;
        self.stage_duration_ticks = 45; // 45 ticks windup
        self.progress_permille = 0;
        self.total_elapsed_ticks = 0;
        self.slow_mo_divisor = 3; // 3x slow motion
        self.slow_mo_counter = 0;
        self.camera_shot_preset = 3; // ImpactCloseUp
        self.camera_shake = 2; // subtle rumble
        self.rim_light_mode = 1; // Molten Red
        self.spark_burst_count = 0;
        self.sound_cue = 1; // Charging SFX
    }

    /// Steps the finisher director forward by 1 tick.
    /// Returns true while finisher sequence is active.
    pub fn step(&mut self) -> bool {
        if self.is_active == 0 {
            return false;
        }

        self.spark_burst_count = 0;
        self.sound_cue = 0;
        self.total_elapsed_ticks = self.total_elapsed_ticks.saturating_add(1);
        self.current_tick += 1;

        if self.stage_duration_ticks > 0 {
            self.progress_permille = ((self.current_tick as u32 * 1000)
                / self.stage_duration_ticks as u32)
                .min(1000) as u16;
        }

        match self.stage {
            FinisherStage::WindupCharging => {
                self.camera_shake = (self.progress_permille / 150) as u8; // building up
                if self.current_tick >= self.stage_duration_ticks {
                    // Transition to NovaStrike
                    self.stage = FinisherStage::NovaStrike;
                    self.current_tick = 0;
                    self.progress_permille = 0;
                    self.stage_duration_ticks = 20;
                    self.camera_shot_preset = 3; // ImpactCloseUp
                    self.camera_shake = 18; // Max impact violent shake
                    self.rim_light_mode = 2; // Supernova Pure White
                    self.spark_burst_count = 40; // 40 golden debris sparks
                    self.slow_mo_divisor = 1; // full impact speed
                    self.sound_cue = 2; // Catastrophic Boom SFX
                }
            }
            FinisherStage::NovaStrike => {
                // Decay shake
                self.camera_shake = self.camera_shake.saturating_sub(1);
                if self.current_tick >= self.stage_duration_ticks {
                    // Transition to DecapitateRecoil
                    self.stage = FinisherStage::DecapitateRecoil;
                    self.current_tick = 0;
                    self.progress_permille = 0;
                    self.stage_duration_ticks = 35;
                    self.camera_shot_preset = 4; // KnockdownLowAngle
                    self.camera_shake = 4;
                    self.rim_light_mode = 1;
                    self.slow_mo_divisor = 2;
                }
            }
            FinisherStage::DecapitateRecoil => {
                self.camera_shake = self.camera_shake.saturating_sub(1);
                if self.current_tick >= self.stage_duration_ticks {
                    // Transition to VictoryRoar
                    self.stage = FinisherStage::VictoryRoar;
                    self.current_tick = 0;
                    self.progress_permille = 0;
                    self.stage_duration_ticks = 60;
                    self.camera_shot_preset = 0; // BroadcastCenter Orbit
                    self.camera_shake = 0;
                    self.rim_light_mode = 0;
                    self.slow_mo_divisor = 1;
                    self.sound_cue = 3; // Victory Fanfare / Roar
                }
            }
            FinisherStage::VictoryRoar => {
                if self.current_tick >= self.stage_duration_ticks {
                    // Finisher complete
                    self.stage = FinisherStage::Inactive;
                    self.is_active = 0;
                    self.camera_shot_preset = 0;
                    self.camera_shake = 0;
                    self.rim_light_mode = 0;
                }
            }
            FinisherStage::Inactive => {
                self.is_active = 0;
            }
        }

        self.is_active != 0
    }

    /// Evaluates the hero mecha pose during finisher.
    pub fn evaluate_hero_pose(&self) -> MechaPose3D {
        match self.stage {
            FinisherStage::WindupCharging => {
                MechaPose3D::finisher_windup(self.progress_permille as u32)
            }
            FinisherStage::NovaStrike => {
                let p = self.progress_permille as u32;
                let strike = MechaPose3D::finisher_impact();
                if p < 200 {
                    strike
                } else {
                    strike.blend(&MechaPose3D::default(), p)
                }
            }
            FinisherStage::DecapitateRecoil => {
                let p = self.progress_permille as u32;
                MechaPose3D::finisher_impact().blend(&MechaPose3D::victory(), p)
            }
            FinisherStage::VictoryRoar => MechaPose3D::victory(),
            FinisherStage::Inactive => MechaPose3D::default(),
        }
    }

    /// Evaluates the opponent mecha pose during finisher.
    pub fn evaluate_foe_pose(&self) -> MechaPose3D {
        match self.stage {
            FinisherStage::WindupCharging => MechaPose3D::guard(),
            FinisherStage::NovaStrike => MechaPose3D::hit_stagger(),
            FinisherStage::DecapitateRecoil | FinisherStage::VictoryRoar => {
                let p = self.progress_permille as u32;
                MechaPose3D::hit_stagger().blend(&MechaPose3D::down(), p)
            }
            FinisherStage::Inactive => MechaPose3D::default(),
        }
    }
}
