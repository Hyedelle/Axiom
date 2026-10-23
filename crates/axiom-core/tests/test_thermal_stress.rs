//! Test suite for 3D Thermal Stress Field & Phase Transformation Core Engine.

use axiom_core::thermal_stress::{ThermalStressField16, TEMP_AMBIENT};
use axiom_core::voxel_destruct::{VoxelVolume16, VOXEL_MOLTEN, VOXEL_STONE};
use core::mem::size_of;

#[test]
fn test_thermal_stress_field_l1_alignment() {
    assert_eq!(size_of::<ThermalStressField16>(), 16448);
    assert_eq!(core::mem::align_of::<ThermalStressField16>(), 64);
}

#[test]
fn test_thermal_pulse_and_diffusion() {
    let mut field = ThermalStressField16::new();
    let mut volume = VoxelVolume16::new_solid(VOXEL_STONE);

    assert_eq!(field.get_temp(8, 8, 8), TEMP_AMBIENT);
    assert_eq!(field.get_stress(8, 8, 8), 0);

    // Inject thermal pulse at center
    field.inject_thermal_pulse(8, 8, 8, 3, 600, 500);

    let peak_t = field.get_temp(8, 8, 8);
    assert!(peak_t >= 600);
    let peak_s = field.get_stress(8, 8, 8);
    assert!(peak_s >= 500);

    // Step 50 diffusion ticks
    for _ in 0..50 {
        field.step_thermal_diffusion(&mut volume, 15);
    }

    // Thermal energy should diffuse outward and cool
    let cooled_t = field.get_temp(8, 8, 8);
    assert!(cooled_t < peak_t);
    assert!(field.total_dissipated > 0);
    assert!(field.get_stress(8, 8, 8) < peak_s);
}

#[test]
fn test_thermal_phase_transition_stone_to_molten() {
    let mut field = ThermalStressField16::new();
    let mut volume = VoxelVolume16::new_solid(VOXEL_STONE);

    // Inject high-temperature thermal pulse exceeding melt threshold
    field.inject_thermal_pulse(8, 8, 8, 2, 950, 850);

    let (transitions, _fractures) = field.step_thermal_diffusion(&mut volume, 10);

    assert!(transitions > 0, "High temperature pulse must trigger Stone -> Molten phase transition");
    assert!(field.phase_transitions > 0);
    assert_eq!(volume.get(8, 8, 8), VOXEL_MOLTEN);
}

#[test]
fn test_thermal_stress_state_hash_determinism() {
    let mut f1 = ThermalStressField16::new();
    let mut f2 = ThermalStressField16::new();
    let mut v1 = VoxelVolume16::new_solid(VOXEL_STONE);
    let mut v2 = VoxelVolume16::new_solid(VOXEL_STONE);

    f1.inject_thermal_pulse(6, 6, 6, 4, 800, 600);
    f2.inject_thermal_pulse(6, 6, 6, 4, 800, 600);

    for _ in 0..100 {
        f1.step_thermal_diffusion(&mut v1, 20);
        f2.step_thermal_diffusion(&mut v2, 20);
    }

    assert_eq!(f1.compute_consensus_hash(), f2.compute_consensus_hash());
    assert_eq!(f1.total_dissipated, f2.total_dissipated);
    assert_eq!(f1.phase_transitions, f2.phase_transitions);
    assert_eq!(v1.damaged_count, v2.damaged_count);
    assert_eq!(v1.data, v2.data);
}
