//! # Integration Tests for JobSystem Parallel Physics & Fluid Dispatch (T-178)

use axiom_core::job_system::JobSystem;
use axiom_core::voxel_destruct::VoxelVolume16;
use axiom_core::voxel_fluid::VoxelFluidField16;
use axiom_core::voxel_fluid_simd::SimdFluidEngine;
use axiom_core::worker_pool::WorkerPoolCoordinator;

#[test]
fn test_simd_fluid_job_dispatch_determinism() {
    let mut engine = SimdFluidEngine::new();
    let mut field = VoxelFluidField16::new();
    let mut volume = VoxelVolume16::new_empty();

    // Inject water column
    field.inject_fluid(&mut volume, 8, 12, 8, 255);
    field.inject_fluid(&mut volume, 8, 11, 8, 255);

    let mut job_system = JobSystem::<128>::new();
    let mut coordinator = WorkerPoolCoordinator::<4, 64>::new(4);

    for _ in 0..10 {
        let res = engine.step_fluid_with_jobs(
            &mut field,
            &mut volume,
            None,
            20,
            &mut job_system,
            &mut coordinator,
        );
        assert!(res.units_moved > 0 || res.active_fluid_cells > 0);
    }

    // 10 ticks x 8 jobs per tick = 80 jobs executed across 4 workers
    assert_eq!(coordinator.total_jobs_executed(), 80);
    assert!(engine.total_moved > 0);
    assert_eq!(field.total_fluid_units, 255 * 2, "Mass must be 100% conserved");
}
