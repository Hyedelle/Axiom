//! Comprehensive Unit Tests for XPBD Mecha Ragdoll Constraints & Elastic Strain Limits (`axiom-core::xpbd_ragdoll`)

use core::mem::{align_of, size_of};

use axiom_core::xpbd_ragdoll::{
    solve_xpbd_distance, XpbdBoneSlab64, XpbdConstraintSlab64, XpbdEventReport, XpbdRagdollEngine,
    EVENT_XPBD_STRAIN, EVENT_XPBD_TEAR,
};

#[test]
fn test_xpbd_slab_sizes_and_alignments() {
    assert_eq!(size_of::<XpbdBoneSlab64>(), 64, "XpbdBoneSlab64 must be exactly 64 bytes");
    assert_eq!(align_of::<XpbdBoneSlab64>(), 64, "XpbdBoneSlab64 must align to 64 bytes");

    assert_eq!(size_of::<XpbdConstraintSlab64>(), 64, "XpbdConstraintSlab64 must be exactly 64 bytes");
    assert_eq!(align_of::<XpbdConstraintSlab64>(), 64, "XpbdConstraintSlab64 must align to 64 bytes");

    assert_eq!(size_of::<XpbdEventReport>(), 40, "XpbdEventReport must be exactly 40 bytes");
    assert_eq!(align_of::<XpbdEventReport>(), 4, "XpbdEventReport must align to 4 bytes");

    assert_eq!(EVENT_XPBD_STRAIN, 0x0290);
    assert_eq!(EVENT_XPBD_TEAR, 0x0291);
}

#[test]
fn test_xpbd_report_lossless_roundtrip() {
    let original = XpbdEventReport {
        entity_id: 1042,
        bone_a: 1,
        bone_b: 2,
        constraint_id: 3,
        is_severed: 1,
        strain_q16: 15000,
        tension_force_n: 85000,
        rupture_pos_xyz: [1250, -450, 3100],
        impulse_xyz: [500, 1200, -800],
    };

    let encoded = original.encode();
    assert_eq!(encoded.len(), 40);

    let decoded = XpbdEventReport::decode(&encoded);
    assert_eq!(original, decoded);
}

#[test]
fn test_xpbd_distance_elastic_recovery() {
    // Body 1: Anchor at (0, 0, 0), static (inv_mass = 0)
    let mut b1 = XpbdBoneSlab64::new(0, 255, (0, 0, 0), 0, 100, 500);
    // Body 2: Dynamic at (0, 1000, 0), inv_mass = 65536 (1.0 kg^-1)
    let mut b2 = XpbdBoneSlab64::new(1, 0, (0, 1000, 0), 65536, 100, 500);

    // Rest length 980mm (strain = 20/980 ~ 2% < 5% yield strain)
    let mut joint = XpbdConstraintSlab64::new_distance(0, 1, 980, 0, 3277, 13107);

    let (plastic, ruptured) = solve_xpbd_distance(&mut b1, &mut b2, &mut joint);

    assert!(!plastic, "Strain is within elastic limit; no plastic deformation");
    assert!(!ruptured, "Strain is within rupture limit; no rupture");
    assert_eq!(b1.pos_y, 0, "Anchor bone remains static");
    assert_eq!(b2.pos_y, 980, "Dynamic bone pulled exactly to rest length 980mm");
    assert!(!joint.is_plastic_deformed());
    assert!(!joint.is_severed());
}

#[test]
fn test_xpbd_distance_plastic_deformation() {
    // Body 1: Anchor at (0, 0, 0)
    let mut b1 = XpbdBoneSlab64::new(0, 255, (0, 0, 0), 0, 100, 500);
    // Body 2: Dynamic stretched to (0, 1100, 0)
    let mut b2 = XpbdBoneSlab64::new(1, 0, (0, 1100, 0), 65536, 100, 500);

    // Rest length 1000mm. Stretch is 100mm -> strain 10% (6553 in Q16.16)
    // Yield strain 5% (3277), Break strain 20% (13107) -> Plastic deformation regime!
    let mut joint = XpbdConstraintSlab64::new_distance(0, 1, 1000, 0, 3277, 13107);
    let orig_rest_len = joint.rest_length_mm;

    let (plastic, ruptured) = solve_xpbd_distance(&mut b1, &mut b2, &mut joint);

    assert!(plastic, "Plastic deformation must be detected");
    assert!(!ruptured, "Not yet ruptured");
    assert!(joint.is_plastic_deformed());
    assert!(joint.rest_length_mm > orig_rest_len, "Rest length must permanently increase due to tensile yielding");
}

#[test]
fn test_xpbd_distance_rupture() {
    // Body 1: Anchor at (0, 0, 0)
    let mut b1 = XpbdBoneSlab64::new(0, 255, (0, 0, 0), 0, 100, 500);
    // Body 2: Overextended to (0, 1350, 0)
    let mut b2 = XpbdBoneSlab64::new(1, 0, (0, 1350, 0), 65536, 100, 500);

    // Rest length 1000mm. Stretch is 350mm -> strain 35% > 20% break strain -> Rupture!
    let mut joint = XpbdConstraintSlab64::new_distance(0, 1, 1000, 0, 3277, 13107);

    let (plastic, ruptured) = solve_xpbd_distance(&mut b1, &mut b2, &mut joint);

    assert!(!plastic);
    assert!(ruptured, "Strain exceeds rupture threshold; joint must tear");
    assert!(joint.is_severed());
    assert!(b2.is_severed());
}

#[test]
fn test_xpbd_ragdoll_engine_simulation_and_consensus() {
    let mut engine1 = XpbdRagdollEngine::<8, 8>::new();
    let mut engine2 = XpbdRagdollEngine::<8, 8>::new();

    // Setup 4-bone mecha chain:
    // Root (torso): at (0, 2000, 0)
    // Upper arm: at (300, 2000, 0)
    // Forearm: at (600, 2000, 0)
    // Hand: at (900, 2000, 0)
    for engine in [&mut engine1, &mut engine2] {
        let b0 = engine.add_bone(XpbdBoneSlab64::new(0, 255, (0, 2000, 0), 32768, 150, 400)).unwrap();
        let b1 = engine.add_bone(XpbdBoneSlab64::new(1, 0, (300, 2000, 0), 65536, 80, 300)).unwrap();
        let b2 = engine.add_bone(XpbdBoneSlab64::new(2, 1, (600, 2000, 0), 65536, 70, 300)).unwrap();
        let b3 = engine.add_bone(XpbdBoneSlab64::new(3, 2, (900, 2000, 0), 65536, 60, 200)).unwrap();

        engine.add_constraint(XpbdConstraintSlab64::new_distance(b0 as u8, b1 as u8, 300, 0, 3277, 13107));
        engine.add_constraint(XpbdConstraintSlab64::new_distance(b1 as u8, b2 as u8, 300, 0, 3277, 13107));
        engine.add_constraint(XpbdConstraintSlab64::new_distance(b2 as u8, b3 as u8, 300, 0, 3277, 13107));
    }

    // Step both engines identically for 60 ticks with -9810 mm/s^2 gravity and floor at y=0
    for tick in 0..60 {
        engine1.step_xpbd(-9810, 4, 0, 5000);
        engine2.step_xpbd(-9810, 4, 0, 5000);

        let hash1 = engine1.compute_consensus_hash();
        let hash2 = engine2.compute_consensus_hash();
        assert_eq!(hash1, hash2, "Deterministic state hashes must be 100% identical at tick {}", tick);
    }

    // Check that bones fell towards floor and grounded
    assert!(engine1.bones[0].pos_y < 2000, "Torso must have fallen under gravity");
    assert!(engine1.bones[0].pos_y >= 0, "Torso must not penetrate floor");
}
