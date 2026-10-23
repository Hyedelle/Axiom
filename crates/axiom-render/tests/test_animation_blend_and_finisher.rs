use axiom_core::Fixed;
use axiom_render::kinematics3d::{
    lerp_bams_permille, lerp_vec3_permille, AnimBlendTree, CinematicFinisherDirector,
    FinisherStage, MechaPose3D,
};
use axiom_render::math3d::Vec3Fixed;

#[test]
fn test_cinematic_finisher_director_64b_alignment() {
    assert_eq!(core::mem::size_of::<CinematicFinisherDirector>(), 64);
    assert_eq!(core::mem::align_of::<CinematicFinisherDirector>(), 64);
}

#[test]
fn test_lerp_vec3_permille() {
    let a = Vec3Fixed::from_raw(0, 1000, 2000);
    let b = Vec3Fixed::from_raw(1000, 3000, 4000);

    let start = lerp_vec3_permille(a, b, 0);
    assert_eq!(start, a);

    let mid = lerp_vec3_permille(a, b, 500);
    assert_eq!(mid, Vec3Fixed::from_raw(500, 2000, 3000));

    let end = lerp_vec3_permille(a, b, 1000);
    assert_eq!(end, b);
}

#[test]
fn test_lerp_bams_permille() {
    // Normal interpolation
    assert_eq!(lerp_bams_permille(10, 50, 0), 10);
    assert_eq!(lerp_bams_permille(10, 50, 500), 30);
    assert_eq!(lerp_bams_permille(10, 50, 1000), 50);

    // Shortest-arc wrap-around: 250 (which is -6 BAM) to 10 (+10 BAM)
    // Total distance = 16 BAM.
    // Halfway (500 permille) should be 250 + 8 = 258 -> 2 BAM!
    let mid_wrap = lerp_bams_permille(250, 10, 500);
    assert_eq!(mid_wrap, 2);
}

#[test]
fn test_mecha_pose_blend() {
    let idle = MechaPose3D::default();
    let jab = MechaPose3D::jab(400);

    let blended = idle.blend(&jab, 500);
    assert_eq!(blended.glove_l_offset.z.raw(), jab.glove_l_offset.z.raw() / 2);
    assert_eq!(blended.arm_l_offset.z.raw(), jab.arm_l_offset.z.raw() / 2);
    assert_eq!(blended.torso_yaw_y, jab.torso_yaw_y / 2);
}

#[test]
fn test_anim_blend_tree_lifecycle() {
    let idle = MechaPose3D::default();
    let cross = MechaPose3D::cross(450);

    let mut tree = AnimBlendTree::new(idle);
    assert!(tree.is_finished());
    assert_eq!(tree.current_pose(), idle);

    tree.transition_to(cross, 10);
    assert!(!tree.is_finished());

    for _ in 0..5 {
        tree.step();
    }
    assert!(!tree.is_finished());
    let mid_pose = tree.current_pose();
    assert_eq!(mid_pose.glove_r_offset.z.raw(), cross.glove_r_offset.z.raw() / 2);

    for _ in 0..5 {
        tree.step();
    }
    assert!(tree.is_finished());
    assert_eq!(tree.current_pose().glove_r_offset.z, cross.glove_r_offset.z);
}

#[test]
fn test_cinematic_finisher_sequence_phases() {
    let mut dir = CinematicFinisherDirector::new();
    assert_eq!(dir.stage, FinisherStage::Inactive);
    assert_eq!(dir.is_active, 0);

    // Trigger
    dir.trigger_finisher();
    assert_eq!(dir.stage, FinisherStage::WindupCharging);
    assert_eq!(dir.is_active, 1);
    assert_eq!(dir.camera_shot_preset, 3); // ImpactCloseUp
    assert_eq!(dir.sound_cue, 1); // Charge SFX

    // Step through Phase 1 (45 ticks)
    for _ in 0..45 {
        let active = dir.step();
        assert!(active);
    }

    // Now in Phase 2: NovaStrike
    assert_eq!(dir.stage, FinisherStage::NovaStrike);
    assert_eq!(dir.camera_shake, 18); // Max violent shake
    assert_eq!(dir.spark_burst_count, 40); // 40 golden debris sparks
    assert_eq!(dir.rim_light_mode, 2); // Supernova Pure White
    assert_eq!(dir.sound_cue, 2); // Catastrophic Boom

    let hero_pose_impact = dir.evaluate_hero_pose();
    assert!(hero_pose_impact.glove_l_offset.z > Fixed::ZERO);
    assert!(hero_pose_impact.glove_r_offset.z > Fixed::ZERO);

    // Step through Phase 2 (20 ticks)
    for _ in 0..20 {
        let active = dir.step();
        assert!(active);
    }

    // Now in Phase 3: DecapitateRecoil
    assert_eq!(dir.stage, FinisherStage::DecapitateRecoil);
    assert_eq!(dir.camera_shot_preset, 4); // KnockdownLowAngle

    let foe_pose_recoil = dir.evaluate_foe_pose();
    assert!(foe_pose_recoil.root_offset.y <= Fixed::ZERO); // Collapsing down

    // Step through Phase 3 (35 ticks)
    for _ in 0..35 {
        let active = dir.step();
        assert!(active);
    }

    // Now in Phase 4: VictoryRoar
    assert_eq!(dir.stage, FinisherStage::VictoryRoar);
    assert_eq!(dir.sound_cue, 3); // Victory cheer

    let hero_pose_vic = dir.evaluate_hero_pose();
    assert!(hero_pose_vic.glove_r_offset.y > Fixed::ZERO); // Fist raised high

    // Step through Phase 4 (60 ticks)
    for _ in 0..60 {
        dir.step();
    }

    // Final state: Inactive
    assert_eq!(dir.stage, FinisherStage::Inactive);
    assert_eq!(dir.is_active, 0);
}
