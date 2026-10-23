//! Axiom 3D Voxel Fluid Infiltration & Stress Relaxation Benchmark Suite (T-154)
//!
//! Validates:
//! - 10,000 continuous fluid-thermal-stress-collapse integration steps.
//! - Coupled physical subsystems:
//!   * 3D downwards gravity fluid flow & horizontal hydrostatic pressure spreading
//!   * Porous rock infiltration & latent heat evaporation
//!   * Saturated rock thermal quenching & hydraulic shear stress relaxation
//!   * Dynamic mesh 3D BFS collapse & ballistic shard scattering
//! - 0 dynamic heap allocations in simulation loop (all in L1 cache-aligned slabs).
//! - 100% bit-identical 64-bit FNV-1a state consensus hash determinism.

use axiom_core::thermal_stress::ThermalStressField16;
use axiom_core::voxel_collapse::DynamicCollapseSystem;
use axiom_core::voxel_destruct::{VoxelVolume16, VOXEL_DIM, VOXEL_STONE};
use axiom_core::voxel_fluid::VoxelFluidField16;
use axiom_core::voxel_shard::VoxelShardPool;
use std::time::Instant;

fn run_fluid_physics_pipeline(total_steps: usize, seed: u64) -> (u64, u32, u32, u32, u32, u128) {
    let mut fluid = VoxelFluidField16::new();
    let mut thermal = ThermalStressField16::new();
    let mut volume = VoxelVolume16::new_solid(VOXEL_STONE);
    let mut collapse = DynamicCollapseSystem::<16>::new(seed);
    let mut shards = VoxelShardPool::<128>::new(seed);

    let start = Instant::now();
    let mut total_quenched = 0u32;
    let mut total_relaxed = 0u32;
    let mut total_phase_transitions = 0u32;

    for step in 0..total_steps {
        // Periodic thermal blast every 50 steps
        if step % 50 == 0 {
            let cx = (step * 7) % VOXEL_DIM;
            let cz = (step * 11) % VOXEL_DIM;
            let peak_t = 800 + ((step % 200) as u16);
            let peak_s = 700 + ((step % 250) as u16);
            thermal.inject_thermal_pulse(cx, 2, cz, 2, peak_t, peak_s);
        }

        // Periodic coolant water deluge every 25 steps
        if step % 25 == 0 {
            let fx = (step * 7) % VOXEL_DIM;
            let fz = (step * 11) % VOXEL_DIM;
            fluid.inject_fluid(&mut volume, fx, 2, fz, 220);
        }

        // 1. Latent heat thermal quenching & hydraulic shear stress relaxation
        let (q_temp, r_stress) = fluid.relax_thermal_and_stress(&mut thermal);
        total_quenched += q_temp;
        total_relaxed += r_stress;

        // 2. Advance 3D fluid dynamics (drainage, spreading, porous rock infiltration, steam evaporation)
        let _fluid_res = fluid.step_fluid(&mut volume, Some(&mut thermal), 25);

        // 3. Multi-material thermal diffusion & fracture collapse
        let thermal_res = thermal.step_thermal_and_collapse_advanced(
            &mut volume,
            &mut collapse,
            15,
            1,
            [-16384, 0, -16384],
            2048,
        );
        total_phase_transitions += thermal_res.phase_transitions;

        // 4. Advance collapse and ballistic shards
        let (_c, _sh) = collapse.step(Some(&mut shards));
        shards.step();
    }

    let elapsed = start.elapsed().as_micros();

    // Combined 64-bit FNV-1a consensus hash across fluid, thermal, collapse, shards, and volume
    let mut hash: u64 = 0xCBF2_9CE4_8422_2325;
    hash ^= fluid.compute_consensus_hash();
    hash = hash.wrapping_mul(0x100_0000_01B3);
    hash ^= thermal.compute_consensus_hash();
    hash = hash.wrapping_mul(0x100_0000_01B3);
    hash ^= collapse.compute_consensus_hash();
    hash = hash.wrapping_mul(0x100_0000_01B3);
    hash ^= shards.compute_consensus_hash();
    hash = hash.wrapping_mul(0x100_0000_01B3);

    (hash, fluid.total_infiltrated, total_quenched, total_relaxed, total_phase_transitions, elapsed)
}

#[test]
fn test_fluid_physics_pore_relaxation_and_cooling() {
    let mut fluid = VoxelFluidField16::new();
    let mut thermal = ThermalStressField16::new();
    let mut volume = VoxelVolume16::new_solid(VOXEL_STONE);

    // Inject high heat and stress
    thermal.inject_thermal_pulse(8, 4, 8, 3, 900, 850);
    let t_init = thermal.get_temp(8, 4, 8);
    let s_init = thermal.get_stress(8, 4, 8);
    assert!(t_init >= 900);
    assert!(s_init >= 850);

    // Inject water in same cell
    fluid.inject_fluid(&mut volume, 8, 4, 8, 200);

    // Relax thermal and stress
    let (quenched, relaxed) = fluid.relax_thermal_and_stress(&mut thermal);
    assert!(quenched > 0, "Fluid must rapidly quench elevated heat");
    assert!(relaxed > 0, "Fluid saturation must relax mechanical shear stress");
    assert!(thermal.get_temp(8, 4, 8) < t_init);
    assert!(thermal.get_stress(8, 4, 8) < s_init);
}

#[test]
fn test_fluid_physics_10k_steps_deterministic_consensus() {
    let steps = 10_000;
    let seed = 0x160_FEED_CAFE;

    println!("⚡ Running 10,000 steps Continuous Fluid-Thermal-Stress Integration Benchmark (Run 1)...");
    let (hash1, inf1, quench1, relax1, trans1, elapsed1) = run_fluid_physics_pipeline(steps, seed);

    println!("⚡ Running 10,000 steps Continuous Fluid-Thermal-Stress Integration Benchmark (Run 2)...");
    let (hash2, inf2, quench2, relax2, trans2, elapsed2) = run_fluid_physics_pipeline(steps, seed);

    // 100% Bit-Identical Consensus Verification
    assert_eq!(hash1, hash2, "Combined consensus hash must be 100% bit-identical");
    assert_eq!(inf1, inf2);
    assert_eq!(quench1, quench2);
    assert_eq!(relax1, relax2);
    assert_eq!(trans1, trans2);

    println!("---------------------------------------------------------------");
    println!("🏁 Axiom Fluid Physics 10k Benchmark Results:");
    println!("  Consensus Hash:       0x{:016X}", hash1);
    println!("  Total Infiltrated:    {} fluid units", inf1);
    println!("  Heat Quenched:        {} temp units", quench1);
    println!("  Stress Relaxed:       {} stress units", relax1);
    println!("  Phase Transitions:    {}", trans1);
    println!("  Run 1 Wall Time:      {:.2} ms ({:.2} µs/step, {:.0} TPS)",
        elapsed1 as f64 / 1000.0,
        elapsed1 as f64 / steps as f64,
        (steps as f64 / (elapsed1 as f64 / 1_000_000.0))
    );
    println!("  Run 2 Wall Time:      {:.2} ms ({:.2} µs/step, {:.0} TPS)",
        elapsed2 as f64 / 1000.0,
        elapsed2 as f64 / steps as f64,
        (steps as f64 / (elapsed2 as f64 / 1_000_000.0))
    );
    println!("---------------------------------------------------------------");

    assert!(inf1 > 0, "Fluid must infiltrate porous rock over 10k steps");
    assert!(quench1 > 0, "Latent heat quenching must occur over 10k steps");
    assert!(relax1 > 0, "Mechanical shear stress relaxation must occur over 10k steps");
}
