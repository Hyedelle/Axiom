//! # Axiom Core - Ragdoll & IK Physics Verification Suite (T-201)
//!
//! Verifies 64B L1 cache alignment, 0-Float Position-Based Dynamics (PBD),
//! Closed-form Two-Bone IK analytical solver, and Limb Severance.

use axiom_core::ragdoll::*;
use core::mem::{align_of, size_of};

#[test]
fn test_ragdoll_slab_64b_l1_cache_alignment() {
    assert_eq!(size_of::<RagdollBoneSlab64>(), 64, "RagdollBoneSlab64 must be exactly 64 bytes");
    assert_eq!(align_of::<RagdollBoneSlab64>(), 64, "RagdollBoneSlab64 must align to 64-byte L1 cache line");

    assert_eq!(size_of::<RagdollJointSlab64>(), 64, "RagdollJointSlab64 must be exactly 64 bytes");
    assert_eq!(align_of::<RagdollJointSlab64>(), 64, "RagdollJointSlab64 must align to 64-byte L1 cache line");
}

#[test]
fn test_distance_constraint_pbd_relaxation() {
    // Bone 0: Fixed shoulder at (0, 1000, 0) with mass = infinite (inv_mass = 0)
    let mut b0 = RagdollBoneSlab64::new(0, 0, (0, 1000, 0), 0, 50, 400);
    // Bone 1: Free elbow at (0, 1800, 0) with mass = 1.0 (inv_mass = 65536)
    let mut b1 = RagdollBoneSlab64::new(1, 0, (0, 1800, 0), 65536, 50, 400);

    // Rest length is 500mm, but currently separated by 800mm
    let rest_len = 500;
    let stiffness = 65535; // 100% stiffness

    assert_eq!(b0.pos_y, 1000);
    assert_eq!(b1.pos_y, 1800);

    // Relax constraint
    solve_distance_constraint(&mut b0, &mut b1, rest_len, stiffness);

    // Fixed bone 0 should not move at all
    assert_eq!(b0.pos_y, 1000, "Fixed bone must remain at rest");
    // Free bone 1 should be pulled down to exactly 1500 (1000 + 500)
    assert_eq!(b1.pos_y, 1500, "Free bone must be corrected to rest length 500mm");

    // Equal mass test: both free
    let mut b2 = RagdollBoneSlab64::new(2, 0, (0, 0, 0), 65536, 50, 300);
    let mut b3 = RagdollBoneSlab64::new(3, 2, (1000, 0, 0), 65536, 50, 300);

    solve_distance_constraint(&mut b2, &mut b3, 600, 65535);
    // Initial dist = 1000mm, target = 600mm. Diff = 400mm -> each moves 200mm towards each other
    assert_eq!(b2.pos_x, 200, "b2 moved 200mm forward");
    assert_eq!(b3.pos_x, 800, "b3 moved 200mm backward");
}

#[test]
fn test_closed_form_two_bone_ik_reach() {
    let p_shoulder = (0, 1500, 0);
    let l1 = 400; // Upper arm length 400mm
    let l2 = 400; // Forearm length 400mm
    let pole_vector = (0, 2000, 0); // Point elbow upwards along +Y

    // Target within reach: 500mm away along X
    let p_target = (500, 1500, 0);
    let elbow = solve_two_bone_ik(p_shoulder, l1, l2, p_target, pole_vector);

    // Verify distance from shoulder to elbow is ~400mm (allowing integer rounding +-2mm)
    let d1_sq = (elbow.0 - p_shoulder.0) as i64 * (elbow.0 - p_shoulder.0) as i64
        + (elbow.1 - p_shoulder.1) as i64 * (elbow.1 - p_shoulder.1) as i64
        + (elbow.2 - p_shoulder.2) as i64 * (elbow.2 - p_shoulder.2) as i64;
    let d1 = isqrt_i64(d1_sq);
    assert!((d1 - 400).abs() <= 2, "Upper arm length must be 400mm, got {}", d1);

    // Verify distance from elbow to target is ~400mm
    let d2_sq = (p_target.0 - elbow.0) as i64 * (p_target.0 - elbow.0) as i64
        + (p_target.1 - elbow.1) as i64 * (p_target.1 - elbow.1) as i64
        + (p_target.2 - elbow.2) as i64 * (p_target.2 - elbow.2) as i64;
    let d2 = isqrt_i64(d2_sq);
    assert!((d2 - 400).abs() <= 2, "Forearm length must be 400mm, got {}", d2);

    // Elbow should be elevated above shoulder/wrist along pole vector (+Y)
    assert!(elbow.1 > 1500, "Elbow must bend towards pole vector +Y, got Y={}", elbow.1);

    // Target out of reach (1200mm away): should solve elbow clamped to max reach
    let p_far = (1200, 1500, 0);
    let far_elbow = solve_two_bone_ik(p_shoulder, l1, l2, p_far, pole_vector);
    let far_d1 = isqrt_i64(
        (far_elbow.0 - p_shoulder.0) as i64 * (far_elbow.0 - p_shoulder.0) as i64
        + (far_elbow.1 - p_shoulder.1) as i64 * (far_elbow.1 - p_shoulder.1) as i64
        + (far_elbow.2 - p_shoulder.2) as i64 * (far_elbow.2 - p_shoulder.2) as i64
    );
    assert!((far_d1 - 400).abs() <= 2, "Upper arm length must still be 400mm even when out of reach");
}

#[test]
fn test_mecha_ragdoll_simulation_and_consensus_hash() {
    let mut engine1 = RagdollEngine::<16, 16>::new();
    let mut engine2 = RagdollEngine::<16, 16>::new();

    // Setup identical ragdoll configurations
    for engine in [&mut engine1, &mut engine2] {
        // Bone 0: Root Torso at (0, 1000, 0)
        let b0 = RagdollBoneSlab64::new(0, 0, (0, 1000, 0), 65536, 100, 200);
        // Bone 1: Left Arm at (300, 1000, 0)
        let b1 = RagdollBoneSlab64::new(1, 0, (300, 1000, 0), 65536, 50, 300);
        // Bone 2: Left Forearm at (600, 1000, 0)
        let b2 = RagdollBoneSlab64::new(2, 1, (600, 1000, 0), 65536, 50, 300);

        engine.add_bone(b0);
        engine.add_bone(b1);
        engine.add_bone(b2);

        // Joint 0: Torso - Left Arm
        let j0 = RagdollJointSlab64::new(0, 1, RagdollJointType::BallSocket, 300, 100_000);
        // Joint 1: Left Arm - Left Forearm
        let j1 = RagdollJointSlab64::new(1, 2, RagdollJointType::Hinge, 300, 80_000);

        engine.add_joint(j0);
        engine.add_joint(j1);
    }

    // Step both engines identically for 30 ticks
    for _ in 0..30 {
        engine1.step_ragdoll(-10, 4, 0, 5000);
        engine2.step_ragdoll(-10, 4, 0, 5000);
    }

    // Bit-exact consensus hash must match 100%
    let hash1 = engine1.compute_consensus_hash();
    let hash2 = engine2.compute_consensus_hash();
    assert_ne!(hash1, 0);
    assert_eq!(hash1, hash2, "Ragdoll state must be 100% deterministic across runs");

    // Torso should have dropped due to gravity (-10 mm/tick^2)
    assert!(engine1.bones[0].pos_y < 1000, "Torso should fall under gravity");
}

#[test]
fn test_ragdoll_limb_severance() {
    let mut engine = RagdollEngine::<8, 8>::new();
    let b0 = RagdollBoneSlab64::new(0, 0, (0, 1000, 0), 0, 100, 200); // Fixed root
    let b1 = RagdollBoneSlab64::new(1, 0, (500, 1000, 0), 65536, 50, 500); // Arm

    engine.add_bone(b0);
    engine.add_bone(b1);

    let j0 = RagdollJointSlab64::new(0, 1, RagdollJointType::Hinge, 500, 100);
    engine.add_joint(j0);

    assert_eq!(engine.total_severances, 0);

    // Sever joint
    let severed = engine.sever_joint(0);
    assert!(severed, "Joint 0 severance must succeed");
    assert_eq!(engine.total_severances, 1);
    assert_eq!(engine.joints[0].is_severed, 1);
    assert!(engine.bones[1].is_severed());

    // Displace severed arm far away (2000mm)
    engine.bones[1].pos_x = 2000;
    engine.step_ragdoll(0, 4, 0, 5000);

    // Severed arm should NOT be pulled back by joint 0
    assert_eq!(engine.bones[1].pos_x, 2000, "Severed limb must not be constrained by broken joint");
}
