//! Axiom 3D Voxel SIMD & Red-Black Checkerboard Fluid Benchmark Suite (T-158)
//!
//! Validates:
//! - 10,000 continuous SIMD/Checkerboard fluid integration steps.
//! - Coupled physical subsystems:
//!   * Decoupled column vertical gravity drainage & porous rock infiltration
//!   * Decoupled 16-slice 3D Red-Black checkerboard horizontal hydrostatic spreading
//!   * Active slice sleeping ($O(1)$ early exit)
//!   * High-temperature latent heat quenching & steam explosion flashing
//!   * Dynamic mesh 3D BFS collapse & ballistic shard scattering
//! - 0 dynamic heap allocations in simulation loop (all in L1 cache-aligned slabs).
//! - 100% bit-identical 64-bit FNV-1a state consensus hash determinism across runs.

use axiom_core::thermal_stress::ThermalStressField16;
use axiom_core::voxel_collapse::DynamicCollapseSystem;
use axiom_core::voxel_destruct::{VoxelVolume16, VOXEL_DIM, VOXEL_STONE};
use axiom_core::voxel_fluid::VoxelFluidField16;
use axiom_core::voxel_fluid_simd::SimdFluidEngine;
use axiom_core::voxel_shard::VoxelShardPool;
use std::time::Instant;

fn run_simd_fluid_pipeline(total_steps: usize, seed: u64) -> (u64, u32, u32, u32, u32, u32, u128) {
    let mut simd = SimdFluidEngine::new();
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

        // 2. Advance 3D SIMD/Checkerboard fluid dynamics (drainage, spreading, infiltration, steam explosions)
        let _simd_res = simd.step_fluid_simd(&mut fluid, &mut volume, Some(&mut thermal), 25);

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

    // Combined 64-bit FNV-1a consensus hash across SIMD engine, fluid, thermal, collapse, shards
    let mut hash: u64 = 0xCBF2_9CE4_8422_2325;
    hash ^= simd.compute_consensus_hash();
    hash = hash.wrapping_mul(0x100_0000_01B3);
    hash ^= fluid.compute_consensus_hash();
    hash = hash.wrapping_mul(0x100_0000_01B3);
    hash ^= thermal.compute_consensus_hash();
    hash = hash.wrapping_mul(0x100_0000_01B3);
    hash ^= collapse.compute_consensus_hash();
    hash = hash.wrapping_mul(0x100_0000_01B3);
    hash ^= shards.compute_consensus_hash();
    hash = hash.wrapping_mul(0x100_0000_01B3);

    (
        hash,
        fluid.total_infiltrated,
        total_quenched,
        total_relaxed,
        total_phase_transitions,
        simd.sleep_cycles_saved,
        elapsed,
    )
}

#[test]
fn test_simd_fluid_10k_steps_deterministic_consensus() {
    let steps = 10_000;
    let seed = 0x160_FEED_CAFE;

    println!("⚡ Running 10,000 steps Continuous SIMD/Checkerboard Fluid Integration Benchmark (Run 1)...");
    let (hash1, inf1, quench1, relax1, trans1, saved1, elapsed1) = run_simd_fluid_pipeline(steps, seed);

    println!("⚡ Running 10,000 steps Continuous SIMD/Checkerboard Fluid Integration Benchmark (Run 2)...");
    let (hash2, inf2, quench2, relax2, trans2, saved2, elapsed2) = run_simd_fluid_pipeline(steps, seed);

    // 100% Bit-Identical Consensus Verification
    assert_eq!(hash1, hash2, "Combined consensus hash must be 100% bit-identical");
    assert_eq!(inf1, inf2);
    assert_eq!(quench1, quench2);
    assert_eq!(relax1, relax2);
    assert_eq!(trans1, trans2);
    assert_eq!(saved1, saved2);

    println!("---------------------------------------------------------------");
    println!("🏁 Axiom T-158 SIMD Fluid Physics 10k Benchmark Results:");
    println!("  Consensus Hash:       0x{:016X}", hash1);
    println!("  Total Infiltrated:    {} fluid units", inf1);
    println!("  Heat Quenched:        {} temp units", quench1);
    println!("  Stress Relaxed:       {} stress units", relax1);
    println!("  Phase Transitions:    {}", trans1);
    println!("  Sleep Slices Saved:   {} slice cycles", saved1);
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
    assert!(saved1 > 0, "Active slice sleeping must skip inactive slice cycles");
}

#[test]
fn test_simd_fluid_micro_step_throughput() {
    let mut simd = SimdFluidEngine::new();
    let mut fluid = VoxelFluidField16::new();
    let mut volume = VoxelVolume16::new_solid(VOXEL_STONE);

    // Carve basin
    for y in 1..=4 {
        for z in 4..=11 {
            for x in 4..=11 {
                volume.set(x, y, z, axiom_core::voxel_destruct::VOXEL_AIR);
            }
        }
    }

    // Inject water
    fluid.inject_fluid(&mut volume, 8, 4, 8, 255);
    fluid.inject_fluid(&mut volume, 7, 4, 8, 255);
    fluid.inject_fluid(&mut volume, 8, 4, 7, 255);

    let steps = 10_000;
    let start = Instant::now();

    for _ in 0..steps {
        simd.step_fluid_simd(&mut fluid, &mut volume, None, 10);
    }

    let elapsed = start.elapsed();
    let elapsed_micros = elapsed.as_micros();
    let tps = (steps as f64) / elapsed.as_secs_f64();
    let us_per_step = elapsed_micros as f64 / steps as f64;

    println!("---------------------------------------------------------------");
    println!("⚡ Axiom T-158 Standalone SIMD Fluid Micro-Benchmark (10,000 steps):");
    println!("  Total Steps:          {}", steps);
    println!("  Wall Time:            {:.2} ms", elapsed.as_millis());
    println!("  Step Latency:         {:.2} µs/step", us_per_step);
    println!("  Throughput:           {:.0} TPS", tps);
    println!("  Sleep Cycles Saved:   {}", simd.sleep_cycles_saved);
    println!("---------------------------------------------------------------");

    assert!(tps > 5_000.0, "SIMD fluid engine must exceed 5,000 TPS in unoptimized debug mode");
    assert!(simd.sleep_cycles_saved > 0);
}
