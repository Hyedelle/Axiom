//! Test suite for 3D Voxel SIMD & Red-Black Checkerboard Fluid Simulator Engine (T-158).

use axiom_core::thermal_stress::ThermalStressField16;
use axiom_core::voxel_destruct::{VoxelVolume16, VOXEL_AIR, VOXEL_STONE};
use axiom_core::voxel_fluid::VoxelFluidField16;
use axiom_core::voxel_fluid_simd::SimdFluidEngine;
use core::mem::{align_of, size_of};

#[test]
fn test_simd_fluid_engine_l1_alignment() {
    assert_eq!(size_of::<SimdFluidEngine>(), 64);
    assert_eq!(align_of::<SimdFluidEngine>(), 64);
}

#[test]
fn test_simd_fluid_downwards_gravity_and_pooling() {
    let mut simd = SimdFluidEngine::new();
    let mut fluid = VoxelFluidField16::new();
    let mut volume = VoxelVolume16::new_empty();

    // Place stone floor at y=2
    for x in 0..16 {
        for z in 0..16 {
            volume.set(x, 2, z, VOXEL_STONE);
        }
    }

    // Inject water at (8, 10, 8)
    fluid.inject_fluid(&mut volume, 8, 10, 8, 200);
    assert_eq!(fluid.get_level(8, 10, 8), 200);

    // Step 15 ticks of SIMD fluid dynamics
    for _ in 0..15 {
        simd.step_fluid_simd(&mut fluid, &mut volume, None, 0);
    }

    // Fluid must have drained from y=10 down to floor at y=3
    assert_eq!(fluid.get_level(8, 10, 8), 0, "Fluid must have drained from source height");
    assert_eq!(volume.get(8, 10, 8), VOXEL_AIR);

    let floor_level = fluid.get_level(8, 3, 8);
    assert!(floor_level > 0, "Fluid must pool on the stone floor at y=3");
    assert!(simd.total_moved > 0);
}

#[test]
fn test_simd_fluid_mass_conservation_invariant() {
    let mut simd = SimdFluidEngine::new();
    let mut fluid = VoxelFluidField16::new();
    let mut volume = VoxelVolume16::new_empty();

    // Create a 5x5 basin with stone walls and floor
    for y in 0..=4 {
        for x in 4..=10 {
            for z in 4..=10 {
                if y == 0 || x == 4 || x == 10 || z == 4 || z == 10 {
                    volume.set(x, y, z, VOXEL_STONE);
                }
            }
        }
    }

    // Inject 600 total units
    fluid.inject_fluid(&mut volume, 6, 3, 6, 200);
    fluid.inject_fluid(&mut volume, 7, 3, 7, 200);
    fluid.inject_fluid(&mut volume, 8, 3, 8, 200);

    let initial_mass = fluid.total_fluid_units;
    assert_eq!(initial_mass, 600);

    // Step 100 ticks
    for _ in 0..100 {
        simd.step_fluid_simd(&mut fluid, &mut volume, None, 0);
    }

    // 100% Mass conservation in closed basin
    assert_eq!(
        fluid.total_fluid_units, initial_mass,
        "Total fluid mass must be 100% conserved in closed basin"
    );
    assert!(fluid.active_fluid_cells > 0);
}

#[test]
fn test_simd_fluid_active_slice_sleeping_optimization() {
    let mut simd = SimdFluidEngine::new();
    let mut fluid = VoxelFluidField16::new();
    let mut volume = VoxelVolume16::new_empty();

    // Floor at y=0
    for x in 0..16 {
        for z in 0..16 {
            volume.set(x, 0, z, VOXEL_STONE);
        }
    }

    // Inject water only at y=1
    fluid.inject_fluid(&mut volume, 8, 1, 8, 100);

    // Run 20 ticks
    for _ in 0..20 {
        simd.step_fluid_simd(&mut fluid, &mut volume, None, 0);
    }

    // Higher slices (y >= 2) must be inactive and skipped
    assert!(simd.sleep_cycles_saved > 0, "SimdFluidEngine must skip inactive upper slices");
    // Only lower slices should be active in bitmask
    assert_eq!(simd.active_slices & 0xFFF0, 0, "Upper slices 4..15 must be asleep");
}

#[test]
fn test_simd_fluid_steam_explosion_and_softened_rock() {
    let mut simd = SimdFluidEngine::new();
    let mut fluid = VoxelFluidField16::new();
    let mut thermal = ThermalStressField16::new();
    let mut volume = VoxelVolume16::new_empty();

    // Floor at y=0
    for x in 0..16 {
        for z in 0..16 {
            volume.set(x, 0, z, VOXEL_STONE);
        }
    }

    // Extreme heat at (8, 1, 8)
    thermal.inject_thermal_pulse(8, 1, 8, 1, 900, 150);
    let init_t = thermal.get_temp(8, 1, 8);
    let init_s = thermal.get_stress(8, 1, 8);

    // Inject water
    fluid.inject_fluid(&mut volume, 8, 1, 8, 150);

    // Step 1 tick
    let res = simd.step_fluid_simd(&mut fluid, &mut volume, Some(&mut thermal), 0);
    assert_eq!(res.steam_explosions, 1, "Water touching 900°C must trigger steam explosion");
    assert_eq!(simd.steam_explosions, 1);
    assert_eq!(fluid.get_level(8, 1, 8), 0);
    assert_eq!(volume.get(8, 1, 8), VOXEL_AIR);

    // Verify quench and stress shockwave
    assert_eq!(thermal.get_temp(8, 1, 8), init_t - 250);
    assert_eq!(thermal.get_stress(8, 1, 8), init_s + 300);
}

#[test]
fn test_simd_fluid_deterministic_consensus_hash() {
    let mut s1 = SimdFluidEngine::new();
    let mut s2 = SimdFluidEngine::new();
    let mut f1 = VoxelFluidField16::new();
    let mut f2 = VoxelFluidField16::new();
    let mut v1 = VoxelVolume16::new_empty();
    let mut v2 = VoxelVolume16::new_empty();

    // Floor at y=0
    for x in 0..16 {
        for z in 0..16 {
            v1.set(x, 0, z, VOXEL_STONE);
            v2.set(x, 0, z, VOXEL_STONE);
        }
    }

    f1.inject_fluid(&mut v1, 8, 5, 8, 220);
    f2.inject_fluid(&mut v2, 8, 5, 8, 220);

    for _ in 0..30 {
        s1.step_fluid_simd(&mut f1, &mut v1, None, 15);
        s2.step_fluid_simd(&mut f2, &mut v2, None, 15);
    }

    assert_eq!(s1.compute_consensus_hash(), s2.compute_consensus_hash());
    assert_eq!(f1.compute_consensus_hash(), f2.compute_consensus_hash());
    assert_eq!(f1.level, f2.level);
    assert_eq!(f1.infiltrated, f2.infiltrated);
    assert_eq!(v1.data, v2.data);
}
