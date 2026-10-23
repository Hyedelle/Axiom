//! # Unit Tests for Continuous Collision Detection (CCD) & Nightmare Boxing Hitboxes
//!
//! Verifies zero-float arithmetic, 64-byte L1 cache alignment, analytical TOI,
//! high-velocity tunneling prevention, and deterministic FNV-1a checksums.

use core::mem::{align_of, size_of};
use axiom_core::ccd::{
    compute_ccd_consensus_hash, raycast_aabb_toi, swept_aabb_toi, swept_hitbox_vs_hurtbox,
    HitType, HitboxSlab64, SweptAabbResult64, HITBOX_FLAG_COUNTER_HIT, HITBOX_FLAG_GUARD_BREAK,
    TOI_ONE_Q16,
};

#[test]
fn test_ccd_l1_cache_alignment_and_sizes() {
    assert_eq!(size_of::<HitboxSlab64>(), 64, "HitboxSlab64 must be exactly 64 bytes");
    assert_eq!(align_of::<HitboxSlab64>(), 64, "HitboxSlab64 must be 64-byte aligned");

    assert_eq!(size_of::<SweptAabbResult64>(), 64, "SweptAabbResult64 must be exactly 64 bytes");
    assert_eq!(align_of::<SweptAabbResult64>(), 64, "SweptAabbResult64 must be 64-byte aligned");
}

#[test]
fn test_raycast_aabb_toi_cardinal_axes() {
    let b_min = (-100, -100, -100);
    let b_max = (100, 100, 100);

    // 1. Ray moving along +X towards box
    let r_orig = (-500, 0, 0);
    let r_disp = (1000, 0, 0);
    let res = raycast_aabb_toi(r_orig, r_disp, b_min, b_max);
    assert!(res.is_some());
    let (toi, normal) = res.unwrap();
    // Distance to box min_x (-100) is 400. Total disp is 1000. TOI = 400/1000 = 0.4 * 65536 = 26214
    assert_eq!(toi, 26214);
    assert_eq!(normal.to_vector(), (-1, 0, 0));

    // 2. Ray moving along -X towards box
    let r_orig_neg = (500, 0, 0);
    let r_disp_neg = (-1000, 0, 0);
    let res_neg = raycast_aabb_toi(r_orig_neg, r_disp_neg, b_min, b_max);
    assert!(res_neg.is_some());
    let (toi_neg, norm_neg) = res_neg.unwrap();
    // Distance to box max_x (100) is 400. TOI = 0.4 * 65536 = 26214
    assert_eq!(toi_neg, 26214);
    assert_eq!(norm_neg.to_vector(), (1, 0, 0));

    // 3. Ray moving along +Y towards box
    let res_y = raycast_aabb_toi((0, -300, 0), (0, 600, 0), b_min, b_max);
    assert!(res_y.is_some());
    let (toi_y, norm_y) = res_y.unwrap();
    // Distance to -100 is 200. Total is 600. TOI = 200/600 = 1/3 * 65536 = 21845
    assert_eq!(toi_y, 21845);
    assert_eq!(norm_y.to_vector(), (0, -1, 0));

    // 4. Ray moving along +Z towards box
    let res_z = raycast_aabb_toi((0, 0, -400), (0, 0, 800), b_min, b_max);
    assert!(res_z.is_some());
    let (toi_z, norm_z) = res_z.unwrap();
    // Distance to -100 is 300. Total is 800. TOI = 300/800 = 0.375 * 65536 = 24576
    assert_eq!(toi_z, 24576);
    assert_eq!(norm_z.to_vector(), (0, 0, -1));
}

#[test]
fn test_raycast_aabb_miss_and_diverging() {
    let b_min = (0, 0, 0);
    let b_max = (100, 100, 100);

    // Ray pointing away from box
    assert!(raycast_aabb_toi((-100, 50, 50), (-500, 0, 0), b_min, b_max).is_none());

    // Ray parallel to box and outside
    assert!(raycast_aabb_toi((-100, 200, 50), (500, 0, 0), b_min, b_max).is_none());

    // Ray stops before reaching box
    assert!(raycast_aabb_toi((-200, 50, 50), (50, 0, 0), b_min, b_max).is_none());
}

#[test]
fn test_high_speed_punch_tunneling_prevention() {
    // Punch moves at 30 m/s (500 mm in 1 tick at 60 FPS)
    // Box A: Punch fist (100x100x100mm), starts at x: [0, 100], disp_x = +500
    // At t=0: [0, 100]. At t=1: [500, 600].
    let fist_min = (0, 0, 0);
    let fist_max = (100, 100, 100);
    let fist_disp = (500, 0, 0);

    // Target: Defender face (100x100x100mm), stationary at x: [250, 350]
    let face_min = (250, 0, 0);
    let face_max = (350, 100, 100);
    let face_disp = (0, 0, 0);

    // Note: In discrete physics, [0, 100] does not intersect [250, 350],
    // and [500, 600] does not intersect [250, 350] -> TUNNELING FAILURE!
    // But CCD must detect the collision!
    let ccd_hit = swept_aabb_toi(fist_min, fist_max, fist_disp, face_min, face_max, face_disp);
    assert!(ccd_hit.is_some(), "CCD must prevent tunneling through the face barrier");

    let hit = ccd_hit.unwrap();
    assert!(hit.has_hit());
    // Fist front reaches face front (x=250) when fist front (100 + 500*t) = 250
    // 500*t = 150 -> t = 150/500 = 0.3
    // In Q16.16: 0.3 * 65536 = 19660
    assert_eq!(hit.toi, 19660);
    assert_eq!(hit.normal_x, -1);
    assert_eq!(hit.normal_y, 0);
    assert_eq!(hit.normal_z, 0);
    assert_eq!(hit.hit_x, 250);
}

#[test]
fn test_swept_aabb_relative_motion_and_counter_punch() {
    // Both boxes are moving towards each other (Counter hit!)
    // Box A starts at [0, 100], moves +300 in X
    let box_a_min = (0, -50, -50);
    let box_a_max = (100, 50, 50);
    let box_a_disp = (300, 0, 0);

    // Box B starts at [500, 600], moves -300 in X
    let box_b_min = (500, -50, -50);
    let box_b_max = (600, 50, 50);
    let box_b_disp = (-300, 0, 0);

    // Relative displacement = 300 - (-300) = 600 mm
    // Gap between them = 500 - 100 = 400 mm
    // TOI = 400 / 600 = 2/3 * 65536 = 43690
    let res = swept_aabb_toi(box_a_min, box_a_max, box_a_disp, box_b_min, box_b_max, box_b_disp);
    assert!(res.is_some());
    let hit = res.unwrap();
    assert_eq!(hit.toi, 43690);
    assert_eq!(hit.normal_x, -1);
}

#[test]
fn test_swept_hitbox_vs_hurtbox_and_consensus_hash() {
    let hitbox = HitboxSlab64::from_extents_and_disp(
        101, // Attacker ID
        1,   // Hitbox ID: Left Jab
        0b01,// Target layer
        (-50, -50, -50),
        (50, 50, 50),
        (0, 0, 800), // Punching forward along +Z
        45,  // Damage
        250, // Knockback
        HitType::High,
        HITBOX_FLAG_COUNTER_HIT | HITBOX_FLAG_GUARD_BREAK,
    );

    assert!(hitbox.is_active());
    assert_eq!(hitbox.damage, 45);
    assert_eq!(hitbox.knockback, 250);

    // Hurtbox of defender located at Z: [300, 500]
    let hurtbox_min = (-100, -100, 300);
    let hurtbox_max = (100, 100, 500);
    let hurtbox_disp = (0, 0, 0);

    let hit_res = swept_hitbox_vs_hurtbox(&hitbox, hurtbox_min, hurtbox_max, hurtbox_disp, 102);
    assert!(hit_res.is_some());
    let hit = hit_res.unwrap();

    assert_eq!(hit.attacker_id, 101);
    assert_eq!(hit.victim_id, 102);
    assert_eq!(hit.hitbox_id, 1);
    assert_eq!(hit.damage, 45);
    assert_eq!(hit.knockback, 250);
    assert_eq!(hit.normal_z, -1);
    assert!(hit.toi > 0 && hit.toi < TOI_ONE_Q16);

    let pair_key = ((101u64) << 32) | 102u64;
    assert_eq!(hit.pair_key, pair_key);

    // Self-hit must be rejected
    assert!(swept_hitbox_vs_hurtbox(&hitbox, hurtbox_min, hurtbox_max, hurtbox_disp, 101).is_none());

    // Deterministic FNV-1a checksum calculation
    let results = [hit];
    let hash1 = compute_ccd_consensus_hash(&results);
    let hash2 = compute_ccd_consensus_hash(&results);
    assert_eq!(hash1, hash2, "CCD Consensus hash must be strictly deterministic");
    assert!(hash1 != 0, "Consensus hash must be non-zero");
}
