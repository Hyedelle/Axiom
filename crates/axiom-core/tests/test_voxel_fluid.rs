//! Test suite for 3D Voxel Fluid Dynamics & Porous Infiltration Core Engine.

use axiom_core::thermal_stress::ThermalStressField16;
use axiom_core::voxel_destruct::{VoxelVolume16, VOXEL_AIR, VOXEL_STONE, VOXEL_WATER};
use axiom_core::voxel_fluid::VoxelFluidField16;
use core::mem::{align_of, size_of};

#[test]
fn test_voxel_fluid_l1_alignment() {
    assert_eq!(size_of::<VoxelFluidField16>(), 8256);
    assert_eq!(align_of::<VoxelFluidField16>(), 64);
}

#[test]
fn test_voxel_fluid_downwards_gravity_and_pooling() {
    let mut fluid = VoxelFluidField16::new();
    let mut volume = VoxelVolume16::new_empty();

    // Place a stone floor at y=2
    for x in 0..16 {
        for z in 0..16 {
            volume.set(x, 2, z, VOXEL_STONE);
        }
    }

    // Inject fluid at y=10, x=8, z=8
    fluid.inject_fluid(&mut volume, 8, 10, 8, 200);
    assert_eq!(fluid.get_level(8, 10, 8), 200);
    assert_eq!(volume.get(8, 10, 8), VOXEL_WATER);

    // Step 15 ticks of fluid dynamics
    for _ in 0..15 {
        fluid.step_fluid(&mut volume, None, 0);
    }

    // Fluid should have drained downwards from y=10 to the floor above y=2 (y=3)
    assert_eq!(fluid.get_level(8, 10, 8), 0, "Fluid must have drained from source height");
    assert_eq!(volume.get(8, 10, 8), VOXEL_AIR);

    let floor_level = fluid.get_level(8, 3, 8);
    assert!(floor_level > 0, "Fluid must pool on the stone floor at y=3");
}

#[test]
fn test_voxel_fluid_mass_conservation_invariant() {
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

    // Inject total 600 units across multiple cells
    fluid.inject_fluid(&mut volume, 6, 3, 6, 200);
    fluid.inject_fluid(&mut volume, 7, 3, 7, 200);
    fluid.inject_fluid(&mut volume, 8, 3, 8, 200);

    let initial_mass = fluid.total_fluid_units;
    assert_eq!(initial_mass, 600);

    // Run 100 ticks with zero infiltration / zero evaporation
    for _ in 0..100 {
        fluid.step_fluid(&mut volume, None, 0);
    }

    // Total fluid mass must be exactly conserved (0 loss)
    assert_eq!(
        fluid.total_fluid_units, initial_mass,
        "Total fluid mass must be 100% conserved in closed basin"
    );
    assert!(fluid.active_fluid_cells > 0);
}

#[test]
fn test_voxel_fluid_porous_rock_infiltration() {
    let mut fluid = VoxelFluidField16::new();
    let mut volume = VoxelVolume16::new_solid(VOXEL_STONE);

    // Carve a small hollow pit at (8, 8, 8)
    volume.set(8, 8, 8, VOXEL_AIR);

    // Inject fluid into the pit surrounded by stone
    fluid.inject_fluid(&mut volume, 8, 8, 8, 100);

    // Step 20 ticks with 50 permille infiltration
    for _ in 0..20 {
        fluid.step_fluid(&mut volume, None, 50);
    }

    // Free fluid should be partially absorbed into surrounding rock
    assert!(fluid.total_infiltrated > 0, "Fluid must infiltrate into surrounding stone");
    let rock_inf = fluid.get_infiltrated(8, 7, 8); // bottom rock
    assert!(rock_inf > 0, "Bottom rock must absorb infiltrated fluid");
}

#[test]
fn test_voxel_fluid_thermal_evaporation() {
    let mut fluid = VoxelFluidField16::new();
    let mut thermal = ThermalStressField16::new();
    let mut volume = VoxelVolume16::new_empty();

    // Floor at y=0
    for x in 0..16 {
        for z in 0..16 {
            volume.set(x, 0, z, VOXEL_STONE);
        }
    }

    // Inject fluid at (8, 1, 8)
    fluid.inject_fluid(&mut volume, 8, 1, 8, 200);

    // Inject extreme heat at (8, 1, 8)
    thermal.inject_thermal_pulse(8, 1, 8, 2, 900, 100);

    // Step 15 ticks with thermal interaction
    for _ in 0..15 {
        fluid.step_fluid(&mut volume, Some(&thermal), 0);
    }

    assert!(fluid.total_evaporated > 0, "High temperature must evaporate fluid");
    assert!(fluid.total_fluid_units < 200, "Fluid units must decrease due to evaporation");
}

#[test]
fn test_voxel_fluid_consensus_hash_determinism() {
    let mut f1 = VoxelFluidField16::new();
    let mut f2 = VoxelFluidField16::new();
    let mut v1 = VoxelVolume16::new_empty();
    let mut v2 = VoxelVolume16::new_empty();

    // Solid floor at y=0
    for x in 0..16 {
        for z in 0..16 {
            v1.set(x, 0, z, VOXEL_STONE);
            v2.set(x, 0, z, VOXEL_STONE);
        }
    }

    f1.inject_fluid(&mut v1, 8, 6, 8, 250);
    f2.inject_fluid(&mut v2, 8, 6, 8, 250);

    for _ in 0..50 {
        f1.step_fluid(&mut v1, None, 10);
        f2.step_fluid(&mut v2, None, 10);
    }

    assert_eq!(f1.compute_consensus_hash(), f2.compute_consensus_hash());
    assert_eq!(f1.total_fluid_units, f2.total_fluid_units);
    assert_eq!(f1.total_infiltrated, f2.total_infiltrated);
    assert_eq!(f1.level, f2.level);
    assert_eq!(v1.data, v2.data);
}
