//! Axiom Engine Extreme Stress & Optimization Test Suite for 3D Thermal Stress Field & Cascading Collapse.

use axiom_core::thermal_stress::{ThermalStressField16, TEMP_AMBIENT};
use axiom_core::voxel_collapse::DynamicCollapseSystem;
use axiom_core::voxel_destruct::{VoxelVolume16, VOXEL_DIM, VOXEL_STONE};

#[test]
fn test_continuous_thermal_cycles_and_invariants() {
    let mut field = ThermalStressField16::new();
    let mut volume = VoxelVolume16::new_solid(VOXEL_STONE);

    // 500 Continuous Thermal Pulse & Diffusion Cycles
    for step in 0..500 {
        let cx = (step * 3) % VOXEL_DIM;
        let cy = (step * 5) % VOXEL_DIM;
        let cz = (step * 7) % VOXEL_DIM;
        let radius = (step % 4) + 1;
        let temp = (step % 500 + 500) as u16;
        let stress = (step % 400 + 400) as u16;

        field.inject_thermal_pulse(cx, cy, cz, radius, temp, stress);
        let (_trans, _fract) = field.step_thermal_diffusion(&mut volume, 25);

        // Strict Numerical Bounds Invariants
        let max_t = field.max_temp();
        assert!(max_t <= 1000, "Temperature cannot exceed 1000°C ceiling");

        let s = field.get_stress(cx, cy, cz);
        assert!(s <= 1000, "Stress cannot exceed 1000 units ceiling");
    }

    // Verify all 4,096 cells satisfy ambient baseline
    for y in 0..VOXEL_DIM {
        for z in 0..VOXEL_DIM {
            for x in 0..VOXEL_DIM {
                let t = field.get_temp(x, y, z);
                assert!(t >= TEMP_AMBIENT, "Cell temperature dropped below ambient 20°C");
                assert!(t <= 1000);
            }
        }
    }

    assert!(field.total_dissipated > 0);
    assert!(field.phase_transitions > 0);
}

#[test]
fn test_cascading_thermal_collapse_determinism() {
    let mut f1 = ThermalStressField16::new();
    let mut f2 = ThermalStressField16::new();
    let mut v1 = VoxelVolume16::new_solid(VOXEL_STONE);
    let mut v2 = VoxelVolume16::new_solid(VOXEL_STONE);
    let mut c1 = DynamicCollapseSystem::<16>::new(12345);
    let mut c2 = DynamicCollapseSystem::<16>::new(12345);

    // 100-Tick synchronized cascading collapse run
    for tick in 0..100 {
        if tick % 10 == 0 {
            let cx = (tick * 4) % VOXEL_DIM;
            let cz = (tick * 6) % VOXEL_DIM;
            f1.inject_thermal_pulse(cx, 2, cz, 3, 980, 920);
            f2.inject_thermal_pulse(cx, 2, cz, 3, 980, 920);
        }

        let res1 = f1.step_thermal_and_collapse(&mut v1, &mut c1, 20, 0, [-16384, 0, -16384], 2048);
        let res2 = f2.step_thermal_and_collapse(&mut v2, &mut c2, 20, 0, [-16384, 0, -16384], 2048);

        assert_eq!(res1, res2, "Tick {} thermal collapse results must be bit-identical", tick);
    }

    assert_eq!(f1.compute_consensus_hash(), f2.compute_consensus_hash());
    assert_eq!(c1.compute_consensus_hash(), c2.compute_consensus_hash());
    assert_eq!(f1.total_dissipated, f2.total_dissipated);
    assert_eq!(f1.phase_transitions, f2.phase_transitions);
    assert_eq!(c1.total_collapsed, c2.total_collapsed);
    assert_eq!(v1.data, v2.data);
}
