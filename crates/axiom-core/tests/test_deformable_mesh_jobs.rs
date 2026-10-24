//! Unit tests for Deformable Mesh Job System & Event Bus Pipeline (T-208)

use axiom_core::deformable_mesh::{
    dispatch_terrain_deform_with_jobs, emit_crater_to_event_bus, CraterSlab64, DeformableMeshGrid,
    TerrainCraterReport, CRATER_FLAG_ACTIVE, EVENT_TERRAIN_CRATER, TERRAIN_MAT_DIRT,
};
use axiom_core::event_bus::{channel, DeterministicEventBus};
use axiom_core::job_system::JobSystem;
use axiom_core::worker_pool::WorkerPoolCoordinator;

#[test]
fn test_terrain_deform_job_dispatch_and_drain() {
    let grid = DeformableMeshGrid::<33, 1089, 32>::new_flat(-8000, -8000, 0, 500);
    let mut job_sys = JobSystem::<64>::new();
    let mut coordinator = WorkerPoolCoordinator::<4, 64>::new(4);

    let dispatched = dispatch_terrain_deform_with_jobs(&grid, 4, &mut job_sys, &mut coordinator);
    assert_eq!(dispatched, 4, "Must dispatch 4 worker partition jobs");

    let executed = coordinator.drain_all(&mut job_sys);
    assert_eq!(executed, 4, "Must drain all 4 jobs cooperatively");
    assert_eq!(job_sys.active_count, 0, "No dangling active jobs should remain");
}

#[test]
fn test_crater_event_bus_emission_and_payload_roundtrip() {
    let mut bus = DeterministicEventBus::<64>::new();

    let crater = CraterSlab64::from_kinetic_impact(
        -1200, 0, 850,
        120_000,    // 120 kJ
        3_500,
        15_000,
        TERRAIN_MAT_DIRT,
        77,         // source_id
        105,        // tick
    );

    let report = TerrainCraterReport::from_crater(&crater);
    let success = emit_crater_to_event_bus(&report, 105, &mut bus);
    assert!(success, "Event emission must succeed");
    assert_eq!(bus.count, 1, "Bus must hold exactly 1 event");

    let event = &bus.events[0];
    assert_eq!(event.tick, 105);
    assert_eq!(event.event_type, EVENT_TERRAIN_CRATER);
    assert_eq!(event.channel_mask, channel::PHYSICS | channel::COMBAT);
    assert_eq!(event.source_id, 77);

    // Decode 40-byte inline POD payload
    let center_x = i32::from_le_bytes([event.payload[0], event.payload[1], event.payload[2], event.payload[3]]);
    let center_y = i32::from_le_bytes([event.payload[4], event.payload[5], event.payload[6], event.payload[7]]);
    let center_z = i32::from_le_bytes([event.payload[8], event.payload[9], event.payload[10], event.payload[11]]);
    let r_in = u32::from_le_bytes([event.payload[12], event.payload[13], event.payload[14], event.payload[15]]);
    let r_out = u32::from_le_bytes([event.payload[16], event.payload[17], event.payload[18], event.payload[19]]);
    let depth = i32::from_le_bytes([event.payload[20], event.payload[21], event.payload[22], event.payload[23]]);
    let rim_h = i32::from_le_bytes([event.payload[24], event.payload[25], event.payload[26], event.payload[27]]);
    let energy = u32::from_le_bytes([event.payload[28], event.payload[29], event.payload[30], event.payload[31]]);
    let src_id = u32::from_le_bytes([event.payload[32], event.payload[33], event.payload[34], event.payload[35]]);
    let flags = event.payload[36];

    assert_eq!(center_x, -1200);
    assert_eq!(center_y, 0);
    assert_eq!(center_z, 850);
    assert_eq!(r_in, crater.radius_inner_mm);
    assert_eq!(r_out, crater.radius_outer_mm);
    assert_eq!(depth, crater.depth_mm);
    assert_eq!(rim_h, crater.rim_height_mm);
    assert_eq!(energy, 120_000);
    assert_eq!(src_id, 77);
    assert_eq!(flags, CRATER_FLAG_ACTIVE);
}
