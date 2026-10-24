//! Integration and unit tests for Volumetric Shockwave & Dust Storm Engine (T-225).

use core::mem::{align_of, size_of};
use axiom_core::debris_particle::{DebrisParticlePool, DEBRIS_MAT_STONE};
use axiom_core::shockwave::{
    DustEmitterSlab64, ShockwaveSlab64, VolumetricShockwaveEngine,
    SHOCKWAVE_TYPE_SPHERICAL, SHOCKWAVE_TYPE_TOROIDAL, SPEED_OF_SOUND_MMS,
};

#[test]
fn test_slab_alignment_and_sizes() {
    assert_eq!(
        size_of::<ShockwaveSlab64>(),
        64,
        "ShockwaveSlab64 must be exactly 64 bytes"
    );
    assert_eq!(
        align_of::<ShockwaveSlab64>(),
        64,
        "ShockwaveSlab64 must align to 64-byte L1 cache line"
    );

    assert_eq!(
        size_of::<DustEmitterSlab64>(),
        64,
        "DustEmitterSlab64 must be exactly 64 bytes"
    );
    assert_eq!(
        align_of::<DustEmitterSlab64>(),
        64,
        "DustEmitterSlab64 must align to 64-byte L1 cache line"
    );
}

#[test]
fn test_shockwave_wavefront_propagation_and_decay() {
    let mut engine = VolumetricShockwaveEngine::<8, 8>::new();
    assert_eq!(engine.active_shockwaves_count(), 0);

    let slot = engine.trigger_shockwave(
        (0, 500, 0),
        200,   // initial radius mm
        5000,  // max radius mm
        800_000, // supersonic 800 m/s
        150,   // shell thickness mm
        80_000,// 80 kJ
        1,
        1,
        SHOCKWAVE_TYPE_SPHERICAL,
        100,
    );
    assert!(slot.is_some());
    assert_eq!(engine.active_shockwaves_count(), 1);
    assert_eq!(engine.total_shockwaves_spawned, 1);

    let initial_r = engine.shockwaves[0].radius_curr_mm;
    let initial_impulse = engine.shockwaves[0].current_impulse_j;

    // Step 5 ticks
    for tick in 1..=5 {
        engine.step_simulation(100 + tick);
    }

    assert!(
        engine.shockwaves[0].radius_curr_mm > initial_r,
        "Wavefront radius must expand over time"
    );
    assert!(
        engine.shockwaves[0].current_impulse_j < initial_impulse,
        "Peak impulse must decay inversely with distance"
    );
    assert!(
        engine.shockwaves[0].speed_mms >= SPEED_OF_SOUND_MMS,
        "Shockwave propagation speed must not fall below speed of sound"
    );
}

#[test]
fn test_dust_cloud_expansion_and_settling() {
    let mut engine = VolumetricShockwaveEngine::<8, 8>::new();
    assert_eq!(engine.active_dust_count(), 0);

    let slot = engine.trigger_dust_cloud(
        (0, 1500, 0), // 1.5m height
        300,
        3000,
        25_000, // 25 m/s expansion
        200,    // high density
        1,      // stone
        2,
        60,     // 60 ticks lifetime
        100,
    );
    assert!(slot.is_some());
    assert_eq!(engine.active_dust_count(), 1);

    let initial_y = engine.dust_emitters[0].origin_y;
    let initial_radius = engine.dust_emitters[0].radius_mm;
    let initial_density = engine.dust_emitters[0].density_q8;

    for tick in 1..=10 {
        engine.step_simulation(100 + tick);
    }

    assert!(
        engine.dust_emitters[0].radius_mm > initial_radius,
        "Dust cloud radius must expand"
    );
    assert!(
        engine.dust_emitters[0].origin_y < initial_y,
        "Dust cloud must sink due to gravity settling"
    );
    assert!(
        engine.dust_emitters[0].density_q8 < initial_density,
        "Dust density must exponentially decay over time"
    );
}

#[test]
fn test_shockwave_repulsive_impulse_on_debris() {
    let mut engine = VolumetricShockwaveEngine::<8, 8>::new();
    let mut pool = DebrisParticlePool::<64>::new(12345, 0, 3000);

    // Spawn 10 particles at origin
    pool.scatter_debris_burst(
        0, 100, 0,
        10,
        1000, 2000,
        DEBRIS_MAT_STONE,
        1,
        10_000,
    );
    assert_eq!(pool.active_count(), 10);

    // Spawn shockwave right at the particles (r=0..500)
    engine.trigger_shockwave(
        (0, 100, 0),
        50,
        3000,
        500_000,
        500, // 500mm shell thickness covers the particles
        60_000,
        10,
        1,
        SHOCKWAVE_TYPE_TOROIDAL,
        100,
    );

    let initial_vel_y = pool.particles[0].vel_y;
    let affected = engine.apply_to_debris_particles(&mut pool);
    assert!(affected > 0, "Shockwave must push particles within wavefront shell");
    assert!(
        pool.particles[0].vel_y > initial_vel_y,
        "Particle vertical velocity must receive upward repulsive impulse boost"
    );
}

#[test]
fn test_shockwave_fog_transmittance_and_consensus_hash() {
    let mut engine = VolumetricShockwaveEngine::<8, 8>::new();

    // In a clean scene, transmittance is 100% (65536)
    let clear_transmittance = engine.sample_fog_transmittance((0, 500, 0));
    assert_eq!(clear_transmittance, 65536, "Clean air must have 100% transmittance");

    // Spawn a dense dust cloud at (0, 500, 0) with radius 1000mm
    engine.trigger_dust_cloud(
        (0, 500, 0),
        500,
        2000,
        10_000,
        250, // very dense
        1,   // stone
        99,
        120,
        100,
    );

    let cloud_transmittance = engine.sample_fog_transmittance((0, 500, 0));
    assert!(
        cloud_transmittance < clear_transmittance,
        "Inside dust cloud transmittance must drop significantly"
    );

    // Verify 64-bit FNV-1a consensus hash determinism
    let hash1 = engine.compute_consensus_hash();
    assert_ne!(hash1, 0);

    let mut engine2 = VolumetricShockwaveEngine::<8, 8>::new();
    engine2.trigger_dust_cloud(
        (0, 500, 0),
        500,
        2000,
        10_000,
        250,
        1,
        99,
        120,
        100,
    );
    let hash2 = engine2.compute_consensus_hash();
    assert_eq!(hash1, hash2, "Identical shockwave & dust state must yield identical consensus hash");
}
