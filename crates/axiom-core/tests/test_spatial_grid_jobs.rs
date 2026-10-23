//! # Integration Tests for Spatial Grid Job System & Event Bus Pipeline (`test_spatial_grid_jobs.rs`)

use axiom_core::event_bus::{channel, DeterministicEventBus};
use axiom_core::job_system::{JobStatus, JobSystem};
use axiom_core::spatial_grid::{
    dispatch_broadphase_with_jobs, emit_collision_pairs_to_event_bus, AabbSlab64, CollisionPair64,
    SpatialHashGrid, COLLISION_FLAG_DYNAMIC, COLLISION_FLAG_STATIC, EVENT_BROADPHASE_PAIR,
};
use axiom_core::worker_pool::WorkerPoolCoordinator;

#[test]
fn test_spatial_grid_job_system_parallel_dispatch() {
    let mut job_system = JobSystem::<64>::new();
    let mut coordinator = WorkerPoolCoordinator::<4, 32>::new(4);

    // Create 16 entity slabs
    let mut slabs = [AabbSlab64::empty(); 16];
    for i in 0..16 {
        slabs[i] = AabbSlab64::new(
            (i + 1) as u32,
            1,
            0b01,
            COLLISION_FLAG_DYNAMIC,
            ((i as i32) * 500, (i as i32) * 500, 0),
            ((i as i32) * 500 + 800, (i as i32) * 500 + 800, 1800),
        );
    }

    // Dispatch broadphase jobs across 4 workers
    let dispatched = dispatch_broadphase_with_jobs(
        &slabs,
        4,
        &mut job_system,
        &mut coordinator,
    );
    assert_eq!(dispatched, 4, "Should dispatch exactly 4 worker batches");

    // Execute jobs cooperatively
    let executed = coordinator.drain_all(&mut job_system);
    assert_eq!(executed, 4, "All 4 broadphase jobs must be executed");

    // Check that all job descriptors are marked Done
    for i in 0..dispatched {
        assert_eq!(job_system.descriptors[i].status, JobStatus::Done as u8);
    }
}

#[test]
fn test_spatial_grid_event_bus_emission_and_rollback() {
    let mut event_bus = DeterministicEventBus::<64>::new();

    let pairs = [
        CollisionPair64::new(10, 20),
        CollisionPair64::new(20, 30),
        CollisionPair64::new(10, 30),
    ];

    let emitted = emit_collision_pairs_to_event_bus(&pairs, 100, &mut event_bus);
    assert_eq!(emitted, 3);
    assert_eq!(event_bus.count, 3);

    // Filter events by channel::PHYSICS
    let mut physics_events = 0;
    event_bus.for_each_matching(channel::PHYSICS, |ev| {
        assert_eq!(ev.event_type, EVENT_BROADPHASE_PAIR);
        assert_eq!(ev.tick, 100);

        // Read payload
        let mut key_bytes = [0u8; 8];
        key_bytes.copy_from_slice(&ev.payload[0..8]);
        let pair_key = u64::from_le_bytes(key_bytes);

        let mut id_a_bytes = [0u8; 4];
        id_a_bytes.copy_from_slice(&ev.payload[8..12]);
        let id_a = u32::from_le_bytes(id_a_bytes);

        let mut id_b_bytes = [0u8; 4];
        id_b_bytes.copy_from_slice(&ev.payload[12..16]);
        let id_b = u32::from_le_bytes(id_b_bytes);

        assert_eq!(pair_key, ((id_a as u64) << 32) | (id_b as u64));
        physics_events += 1;
    });
    assert_eq!(physics_events, 3);

    // Rollback netcode test: rewind speculative events
    let confirmed_seq = 1;
    event_bus.rollback_to(confirmed_seq);
    assert_eq!(event_bus.count, 1, "Events with sequence > 1 should be rolled back");
}

#[test]
fn test_spatial_grid_broadphase_end_to_end_pipeline() {
    let mut grid = SpatialHashGrid::<1024, 14>::new(1000);
    let mut event_bus = DeterministicEventBus::<64>::new();

    // 4 entities: Entity 1 & 2 collide, Entity 3 & 4 collide, 1 & 3 do not collide
    let slab1 = AabbSlab64::new(1, 1, 0b01, COLLISION_FLAG_DYNAMIC, (0, 0, 0), (800, 800, 800));
    let slab2 = AabbSlab64::new(2, 1, 0b01, COLLISION_FLAG_STATIC, (400, 400, 400), (1200, 1200, 1200));
    let slab3 = AabbSlab64::new(3, 1, 0b01, COLLISION_FLAG_DYNAMIC, (5000, 5000, 5000), (5800, 5800, 5800));
    let slab4 = AabbSlab64::new(4, 1, 0b01, COLLISION_FLAG_DYNAMIC, (5400, 5400, 5400), (6200, 6200, 6200));

    let slabs = [slab1, slab2, slab3, slab4];
    for s in &slabs {
        grid.insert(s);
    }

    let mut collision_pairs = [CollisionPair64::default(); 16];
    let pair_count = grid.query_all_collision_pairs(&slabs, &mut collision_pairs);

    assert_eq!(pair_count, 2, "Must detect exactly 2 collision pairs: (1, 2) and (3, 4)");
    assert_eq!(collision_pairs[0].entity_a, 1);
    assert_eq!(collision_pairs[0].entity_b, 2);
    assert_eq!(collision_pairs[1].entity_a, 3);
    assert_eq!(collision_pairs[1].entity_b, 4);

    let emitted = emit_collision_pairs_to_event_bus(&collision_pairs[..pair_count], 50, &mut event_bus);
    assert_eq!(emitted, 2);
    assert_eq!(event_bus.count, 2);

    let hash = event_bus.compute_consensus_hash();
    assert_ne!(hash, 0);
}
