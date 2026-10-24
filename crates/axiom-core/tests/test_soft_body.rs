use core::mem::{align_of, size_of};
use axiom_core::soft_body::{
    solve_shape_matching_cluster, SoftBodyClusterSlab64, SoftBodyEngine, SoftBodyEventReport,
    SoftBodyParticleSlab64, CLUSTER_FLAG_DEFORMED, PARTICLE_FLAG_FRACTURED, SOFT_MAT_RUBBER,
};

#[test]
fn test_soft_body_slab_alignment_64() {
    assert_eq!(size_of::<SoftBodyParticleSlab64>(), 64);
    assert_eq!(align_of::<SoftBodyParticleSlab64>(), 64);

    assert_eq!(size_of::<SoftBodyClusterSlab64>(), 64);
    assert_eq!(align_of::<SoftBodyClusterSlab64>(), 64);

    assert_eq!(size_of::<SoftBodyEventReport>(), 40);
}

#[test]
fn test_soft_body_event_report_roundtrip() {
    let report = SoftBodyEventReport {
        entity_id: 1042,
        cluster_id: 3,
        flags: 1,
        max_strain_q16: 12345,
        volume_ratio_q16: 64000,
        centroid_pos_xyz: [1500, -250, 4200],
        deform_impulse_xyz: [500, -1200, 300],
    };

    let encoded = report.encode();
    let decoded = SoftBodyEventReport::decode(&encoded);

    assert_eq!(report, decoded);
}

#[test]
fn test_horn_shape_matching_pure_integer() {
    let mut cluster = SoftBodyClusterSlab64::new(
        (0, 1000, 0),
        0,
        8,
        32768, // 0.5 stiffness
        0,     // volume stiffness 0 for pure shape matching test
        1_000_000,
        1,
    );

    // Create 8 cube corner particles
    let mut particles = [SoftBodyParticleSlab64::empty(); 8];
    let offsets = [
        (-100, -100, -100), (100, -100, -100), (-100, 100, -100), (100, 100, -100),
        (-100, -100, 100), (100, -100, 100), (-100, 100, 100), (100, 100, 100),
    ];

    for (i, &off) in offsets.iter().enumerate() {
        let pos = (off.0, 1000 + off.1, off.2);
        particles[i] = SoftBodyParticleSlab64::new(
            pos,
            off,
            65536,
            20,
            0,
            SOFT_MAT_RUBBER,
        );
    }

    // Displace particle 7 slightly to induce moderate elastic deformation
    particles[7].pos_x += 10;
    particles[7].pos_y += 10;

    let (max_strain, rupture) = solve_shape_matching_cluster(&mut cluster, &mut particles);

    assert!(max_strain > 0, "Displaced soft body must measure non-zero strain");
    assert!(!rupture, "Moderate strain should not cause rupture");
    assert!((cluster.flags & CLUSTER_FLAG_DEFORMED) != 0);

    // Particle 7 should have been pulled back towards its rest shape
    assert!(particles[7].pos_x < 110, "Particle 7 must be restored towards rest coordinate");
}

#[test]
fn test_volume_conservation_compression() {
    let mut cluster = SoftBodyClusterSlab64::new(
        (0, 1000, 0),
        0,
        4,
        0,     // shape stiffness 0 for pure volume test
        49152, // 0.75 volume stiffness
        2_000_000,
        2,
    );

    // 4 particles forming a tetrahedron
    let offsets = [(-200, 0, 0), (200, 0, 0), (0, -200, 0), (0, 200, 0)];
    let mut particles = [SoftBodyParticleSlab64::empty(); 4];
    for (i, &off) in offsets.iter().enumerate() {
        particles[i] = SoftBodyParticleSlab64::new(
            (off.0, 1000 + off.1, off.2),
            off,
            65536,
            20,
            0,
            SOFT_MAT_RUBBER,
        );
    }

    // Compress all particles inward by 50%
    for p in particles.iter_mut() {
        p.pos_x = (p.pos_x * 5) / 10;
        p.pos_y = 1000 + ((p.pos_y - 1000) * 5) / 10;
    }

    let initial_px0 = particles[0].pos_x;
    let _ = solve_shape_matching_cluster(&mut cluster, &mut particles);

    // Volume expansion impulse should push particle 0 back outward (more negative)
    assert!(
        particles[0].pos_x < initial_px0,
        "Volume conservation constraint must push compressed particle outward"
    );
}

#[test]
fn test_plastic_fracture_under_extreme_strain() {
    let mut cluster = SoftBodyClusterSlab64::new(
        (0, 1000, 0),
        0,
        2,
        65536,
        0,
        1_000_000,
        1,
    );
    cluster.plastic_yield_q16 = 3276; // 5% yield limit

    let mut particles = [SoftBodyParticleSlab64::empty(); 2];
    particles[0] = SoftBodyParticleSlab64::new((-100, 1000, 0), (-100, 0, 0), 65536, 20, 0, SOFT_MAT_RUBBER);
    particles[1] = SoftBodyParticleSlab64::new((100, 1000, 0), (100, 0, 0), 65536, 20, 0, SOFT_MAT_RUBBER);

    // Pull particle 1 extremely far to exceed 5% strain limit
    particles[1].pos_x = 500;

    let (_strain, rupture) = solve_shape_matching_cluster(&mut cluster, &mut particles);

    assert!(rupture, "Extreme strain must trigger plastic rupture");
    assert!((particles[1].flags & PARTICLE_FLAG_FRACTURED) != 0);
}

#[test]
fn test_engine_simulation_and_consensus_hash() {
    let mut engine1 = SoftBodyEngine::<16, 2>::new();
    let mut engine2 = SoftBodyEngine::<16, 2>::new();

    let cluster = SoftBodyClusterSlab64::new(
        (0, 500, 0),
        0,
        4,
        32768,
        32768,
        1_000_000,
        1,
    );
    engine1.add_cluster(cluster);
    engine2.add_cluster(cluster);

    let offsets = [(-50, 0, 0), (50, 0, 0), (0, -50, 0), (0, 50, 0)];
    for &off in &offsets {
        let p = SoftBodyParticleSlab64::new(
            (off.0, 500 + off.1, off.2),
            off,
            65536,
            15,
            0,
            SOFT_MAT_RUBBER,
        );
        engine1.add_particle(p);
        engine2.add_particle(p);
    }

    // Step both engines identically for 20 frames
    for _ in 0..20 {
        engine1.step_simulation(-9810, 0);
        engine2.step_simulation(-9810, 0);
    }

    let hash1 = engine1.compute_consensus_hash();
    let hash2 = engine2.compute_consensus_hash();

    assert_eq!(hash1, hash2, "Deterministic simulation must yield identical 64-bit hash");
    assert_ne!(hash1, 0);
}
