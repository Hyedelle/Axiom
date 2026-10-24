//! Integration test for Deformable Mesh & Crater Pipeline within NetworkMatchDirector (T-208)

use axiom_core::deformable_mesh::{
    CraterSlab64, DeformableMeshGrid, EVENT_TERRAIN_CRATER, TERRAIN_MAT_STONE,
};
use axiom_core::job_system::JobSystem;
use axiom_core::worker_pool::WorkerPoolCoordinator;
use axiom_game::network::NetworkMatchDirector;

#[test]
fn test_network_match_director_terrain_crater_step() {
    let mut director = NetworkMatchDirector::<64>::new(0, 0xFFFF);
    let mut grid = DeformableMeshGrid::<17, 289, 8>::new_flat(-1600, -1600, 0, 200);

    let crater = CraterSlab64::from_kinetic_impact(
        0, 0, 0,
        45_000,
        1_500,
        12_000,
        TERRAIN_MAT_STONE,
        1,
        director.current_world.tick,
    );

    let deformed = director.step_terrain_crater(&mut grid, &crater);
    assert!(deformed > 0, "Crater displacement must deform mesh vertices");
    assert_eq!(director.event_bus.count, 1, "Must record 1 crater event to event bus");

    let event = &director.event_bus.events[0];
    assert_eq!(event.event_type, EVENT_TERRAIN_CRATER);
    assert_eq!(event.source_id, 1);
}

#[test]
fn test_network_match_director_terrain_crater_with_jobs() {
    let mut director = NetworkMatchDirector::<64>::new(0, 0xFFFF);
    let mut grid = DeformableMeshGrid::<17, 289, 8>::new_flat(-1600, -1600, 0, 200);
    let mut job_sys = JobSystem::<64>::new();
    let mut coordinator = WorkerPoolCoordinator::<4, 64>::new(4);

    let crater = CraterSlab64::from_kinetic_impact(
        200, 0, -200,
        60_000,
        2_000,
        10_000,
        TERRAIN_MAT_STONE,
        2,
        director.current_world.tick,
    );

    let deformed = director.step_terrain_crater_with_jobs(
        &mut grid,
        &crater,
        &mut job_sys,
        &mut coordinator,
    );
    assert!(deformed > 0, "Parallel crater step must deform mesh");
    assert_eq!(director.event_bus.count, 1);
    assert_eq!(director.event_bus.events[0].event_type, EVENT_TERRAIN_CRATER);
    assert_eq!(director.event_bus.events[0].source_id, 2);
}
