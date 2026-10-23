//! # Integration Tests for CCD Job System & Event Bus Pipeline (`test_ccd_jobs.rs`)

use axiom_core::ccd::{
    compute_ccd_consensus_hash, dispatch_ccd_hitboxes_with_jobs, emit_ccd_hits_to_event_bus,
    swept_hitbox_vs_hurtbox, HitType, HitboxSlab64, SweptAabbResult64, EVENT_CCD_HIT,
    HITBOX_FLAG_ACTIVE, HITBOX_FLAG_SWEPT_CCD, TOI_ONE_Q16,
};
use axiom_core::event_bus::{channel, DeterministicEventBus};
use axiom_core::job_system::{JobStatus, JobSystem};
use axiom_core::worker_pool::WorkerPoolCoordinator;

#[test]
fn test_ccd_job_system_parallel_dispatch() {
    let mut job_system = JobSystem::<64>::new();
    let mut coordinator = WorkerPoolCoordinator::<4, 32>::new(4);

    // Create 16 hitbox slabs
    let mut slabs = [HitboxSlab64::empty(); 16];
    for i in 0..16 {
        slabs[i] = HitboxSlab64::from_extents_and_disp(
            (i + 1) as u32,
            (i as u16) + 1,
            0xFFFF,
            ((i as i32) * 500, (i as i32) * 500, 1000),
            ((i as i32) * 500 + 200, (i as i32) * 500 + 200, 1200),
            (300, 0, 0),
            50,
            1500,
            HitType::Mid,
            0,
        );
    }

    // Dispatch CCD jobs across 4 workers
    let dispatched = dispatch_ccd_hitboxes_with_jobs(
        &slabs,
        4,
        &mut job_system,
        &mut coordinator,
    );
    assert_eq!(dispatched, 4, "Should dispatch exactly 4 worker batches");

    // Execute jobs cooperatively
    let executed = coordinator.drain_all(&mut job_system);
    assert_eq!(executed, 4, "All 4 CCD jobs must be executed");

    // Check that all job descriptors are marked Done
    for i in 0..dispatched {
        assert_eq!(job_system.descriptors[i].status, JobStatus::Done as u8);
    }
}

#[test]
fn test_ccd_event_bus_emission_and_rollback() {
    let mut event_bus = DeterministicEventBus::<64>::new();

    // Fabricate two valid SweptAabbResult64 hits
    let mut hit1 = SweptAabbResult64::empty();
    hit1.has_hit = 1;
    hit1.attacker_id = 1;
    hit1.victim_id = 2;
    hit1.pair_key = (1u64 << 32) | 2u64;
    hit1.toi = TOI_ONE_Q16 / 2; // 0.5 in Q16.16 (32768)
    hit1.hit_x = 250;
    hit1.hit_y = 100;
    hit1.hit_z = 1050;
    hit1.damage = 45;
    hit1.knockback = 1200;
    hit1.hitbox_id = 101;
    hit1.normal_x = -1;
    hit1.normal_y = 0;
    hit1.normal_z = 0;

    let mut hit2 = SweptAabbResult64::empty();
    hit2.has_hit = 1;
    hit2.attacker_id = 3;
    hit2.victim_id = 4;
    hit2.pair_key = (3u64 << 32) | 4u64;
    hit2.toi = TOI_ONE_Q16 / 4; // 0.25 in Q16.16 (16384)
    hit2.hit_x = 600;
    hit2.hit_y = 400;
    hit2.hit_z = 1100;
    hit2.damage = 110;
    hit2.knockback = 2800;
    hit2.hitbox_id = 202;
    hit2.normal_x = 0;
    hit2.normal_y = 1;
    hit2.normal_z = 0;

    let hits = [hit1, hit2];

    let emitted = emit_ccd_hits_to_event_bus(&hits, 42, &mut event_bus);
    assert_eq!(emitted, 2);
    assert_eq!(event_bus.count, 2);

    // Filter events by channel::COMBAT
    let mut combat_events = 0;
    event_bus.for_each_matching(channel::COMBAT, |ev| {
        assert_eq!(ev.event_type, EVENT_CCD_HIT);
        assert_eq!(ev.tick, 42);
        assert_eq!(ev.channel_mask, channel::COMBAT | channel::PHYSICS);

        // Unpack 40-byte inline payload
        let mut key_bytes = [0u8; 8];
        key_bytes.copy_from_slice(&ev.payload[0..8]);
        let pair_key = u64::from_le_bytes(key_bytes);

        let mut toi_bytes = [0u8; 4];
        toi_bytes.copy_from_slice(&ev.payload[8..12]);
        let toi = u32::from_le_bytes(toi_bytes);

        let mut x_bytes = [0u8; 4];
        x_bytes.copy_from_slice(&ev.payload[12..16]);
        let hit_x = i32::from_le_bytes(x_bytes);

        let mut dmg_bytes = [0u8; 2];
        dmg_bytes.copy_from_slice(&ev.payload[24..26]);
        let damage = u16::from_le_bytes(dmg_bytes);

        if ev.source_id == 1 {
            assert_eq!(pair_key, (1u64 << 32) | 2u64);
            assert_eq!(toi, 32768);
            assert_eq!(hit_x, 250);
            assert_eq!(damage, 45);
            assert_eq!(ev.payload[32] as i8, -1); // normal_x
        } else if ev.source_id == 3 {
            assert_eq!(pair_key, (3u64 << 32) | 4u64);
            assert_eq!(toi, 16384);
            assert_eq!(hit_x, 600);
            assert_eq!(damage, 110);
            assert_eq!(ev.payload[33] as i8, 1); // normal_y
        } else {
            panic!("Unexpected source_id: {}", ev.source_id);
        }

        combat_events += 1;
    });
    assert_eq!(combat_events, 2);

    // Rollback test: rewind to sequence 1
    event_bus.rollback_to(1);
    assert_eq!(event_bus.count, 1, "Only first event should remain after rollback");
}

#[test]
fn test_ccd_multithreaded_pipeline_end_to_end() {
    let mut job_system = JobSystem::<64>::new();
    let mut coordinator = WorkerPoolCoordinator::<4, 32>::new(4);
    let mut event_bus = DeterministicEventBus::<64>::new();

    // Attacker at [0, 0, 1000], throwing a 30 m/s punch (500 mm/tick displacement) towards +X
    let punch_slab = HitboxSlab64::from_extents_and_disp(
        1,
        10,
        0xFFFF,
        (0, 0, 1000),
        (100, 100, 1100),
        (500, 0, 0),
        80,
        2000,
        HitType::High,
        HITBOX_FLAG_ACTIVE | HITBOX_FLAG_SWEPT_CCD,
    );

    // Defender at [300, 0, 0] with hurtbox [250, -50, 950]..[350, 50, 1150]
    let hurtbox_min = (250, -50, 950);
    let hurtbox_max = (350, 50, 1150);
    let hurtbox_disp = (0, 0, 0);

    let slabs = [punch_slab];
    let dispatched = dispatch_ccd_hitboxes_with_jobs(
        &slabs,
        1,
        &mut job_system,
        &mut coordinator,
    );
    assert_eq!(dispatched, 1);
    let executed = coordinator.drain_all(&mut job_system);
    assert_eq!(executed, 1);

    // Evaluate swept hitbox vs defender hurtbox
    let hit_res = swept_hitbox_vs_hurtbox(&punch_slab, hurtbox_min, hurtbox_max, hurtbox_disp, 2);
    assert!(hit_res.is_some(), "30 m/s high-speed punch must hit defender via swept CCD");

    let res = hit_res.unwrap();
    assert!(res.has_hit());
    assert_eq!(res.attacker_id, 1);
    assert_eq!(res.victim_id, 2);
    assert_eq!(res.damage, 80);
    assert_eq!(res.knockback, 2000);
    assert_eq!(res.normal_x, -1);

    let hash1 = compute_ccd_consensus_hash(&[res]);
    let hash2 = compute_ccd_consensus_hash(&[res]);
    assert_eq!(hash1, hash2, "CCD consensus hash must be strictly deterministic");

    let emitted = emit_ccd_hits_to_event_bus(&[res], 10, &mut event_bus);
    assert_eq!(emitted, 1);
    assert_eq!(event_bus.count, 1);
}
