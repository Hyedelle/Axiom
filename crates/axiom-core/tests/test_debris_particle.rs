//! Unit tests for Physics-Based Debris Particle Scattering & Collision Impulse (T-219)

use core::mem::{align_of, size_of};
use axiom_core::debris_particle::{
    DebrisParticlePool, DebrisParticleSlab64, ParticleContactSlab64, CONTACT_TYPE_FLOOR,
    DEBRIS_FLAG_BOUNCED, DEBRIS_FLAG_GROUNDED, DEBRIS_MAT_DIRT, DEBRIS_MAT_METAL,
    DEBRIS_MAT_STONE, GRAVITY_PER_TICK_MMS,
};

#[test]
fn test_debris_particle_slabs_64b_l1_alignment() {
    assert_eq!(
        size_of::<DebrisParticleSlab64>(),
        64,
        "DebrisParticleSlab64 must be exactly 64 bytes"
    );
    assert_eq!(
        align_of::<DebrisParticleSlab64>(),
        64,
        "DebrisParticleSlab64 must align to 64-byte L1 cache line"
    );

    assert_eq!(
        size_of::<ParticleContactSlab64>(),
        64,
        "ParticleContactSlab64 must be exactly 64 bytes"
    );
    assert_eq!(
        align_of::<ParticleContactSlab64>(),
        64,
        "ParticleContactSlab64 must align to 64-byte L1 cache line"
    );
}

#[test]
fn test_debris_burst_conical_scattering() {
    let mut pool = DebrisParticlePool::<64>::new(12345, 0, 6000);
    assert_eq!(pool.active_count, 0);

    let spawned = pool.scatter_debris_burst(
        0, 1000, 0,
        16,
        2000, 5000,
        DEBRIS_MAT_METAL,
        42,
        60_000,
    );

    assert_eq!(spawned, 16, "Must spawn exactly 16 particles");
    assert_eq!(pool.total_spawned, 16);

    for p in &pool.particles[..16] {
        assert!(p.is_active());
        assert!(p.vel_y > 0, "Debris burst must have positive upward initial velocity");
        assert_eq!(p.emitter_id, 42);
        assert_eq!(p.material, DEBRIS_MAT_METAL);
        assert!(p.mass_g >= 50 && p.mass_g <= 500);
    }
}

#[test]
fn test_symplectic_semi_implicit_euler_ballistic_drop_and_ground_rebound() {
    let mut pool = DebrisParticlePool::<16>::new(9999, 0, 6000);

    // Spawn 1 stone particle at (0, 1000mm, 0) with zero velocity
    let p = DebrisParticleSlab64::new(
        (0, 1000, 0),
        (0, 0, 0),
        100, // 100g
        1,
        DEBRIS_MAT_STONE,
        30,  // 30mm radius
        180, // 3 seconds
    );
    assert!(pool.spawn_particle(p).is_some());

    let mut contacts = [ParticleContactSlab64::empty(); 8];

    // Tick 1: Velocity should accelerate downwards by gravity
    let contact_count = pool.step_debris_simulation(&mut contacts, 1);
    assert_eq!(contact_count, 0, "No contact while airborne");

    let p0 = &pool.particles[0];
    assert!(p0.vel_y < 0, "Particle must gain downward velocity due to gravity");
    // Gravity step: (0 - 163) * 65208 >> 16 approx -162 or -163
    assert!(p0.vel_y <= GRAVITY_PER_TICK_MMS);
    assert!(p0.pos_y < 1000, "Position must decrease");

    // Advance simulation until particle impacts ground
    let mut ground_impacted = false;
    for tick in 2..60 {
        let hits = pool.step_debris_simulation(&mut contacts, tick);
        if hits > 0 {
            ground_impacted = true;
            assert_eq!(contacts[0].contact_type, CONTACT_TYPE_FLOOR);
            assert_eq!(contacts[0].normal_y_q16, 65536, "Normal must be +Y");
            assert!(contacts[0].impulse_m_ns > 0, "Impact impulse must be recorded");
            break;
        }
    }

    assert!(ground_impacted, "Particle must hit ground within 1 second");
    assert!(pool.total_bounces >= 1, "Must record at least 1 ground bounce");
    assert!((pool.particles[0].flags & DEBRIS_FLAG_GROUNDED) != 0);
    assert!((pool.particles[0].flags & DEBRIS_FLAG_BOUNCED) != 0);
}

#[test]
fn test_arena_circular_boundary_bounce() {
    let mut pool = DebrisParticlePool::<16>::new(5555, 0, 3000);

    // Spawn particle moving fast toward perimeter (+X direction at 12000 mm/s)
    let p = DebrisParticleSlab64::new(
        (2800, 1000, 0),
        (12000, 0, 0),
        150,
        2,
        DEBRIS_MAT_DIRT,
        25,
        120,
    );
    assert!(pool.spawn_particle(p).is_some());

    let mut contacts = [ParticleContactSlab64::empty(); 4];

    // Step 2 ticks to cross boundary
    for tick in 1..=2 {
        pool.step_debris_simulation(&mut contacts, tick);
    }

    let p0 = &pool.particles[0];
    // Position should be clamped to arena radius (3000mm)
    assert!(p0.pos_x <= 3000, "Position must remain within arena perimeter");
    // Velocity must be reflected back inwards (< 0)
    assert!(p0.vel_x < 0, "Horizontal velocity must reflect upon hitting boundary");
}

#[test]
fn test_debris_consensus_hash_determinism() {
    let mut pool1 = DebrisParticlePool::<32>::new(0x1234_5678_9ABC_DEF0, 0, 6000);
    let mut pool2 = DebrisParticlePool::<32>::new(0x1234_5678_9ABC_DEF0, 0, 6000);

    pool1.scatter_debris_burst(500, 200, -300, 12, 1500, 4000, DEBRIS_MAT_METAL, 99, 45_000);
    pool2.scatter_debris_burst(500, 200, -300, 12, 1500, 4000, DEBRIS_MAT_METAL, 99, 45_000);

    let mut c1 = [ParticleContactSlab64::empty(); 16];
    let mut c2 = [ParticleContactSlab64::empty(); 16];

    for tick in 1..=30 {
        pool1.step_debris_simulation(&mut c1, tick);
        pool2.step_debris_simulation(&mut c2, tick);
    }

    assert_eq!(
        pool1.compute_consensus_hash(),
        pool2.compute_consensus_hash(),
        "Debris pool state must be bit-identical across simulation ticks"
    );
}
