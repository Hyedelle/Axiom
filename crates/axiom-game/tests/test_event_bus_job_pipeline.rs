//! Integration verification of Deterministic Event Bus & Multi-Threaded Job System Pipeline (T-184)

use axiom_core::entity::Entity;
use axiom_core::event_bus::{
    channel, component, filter_entities_with_jobs, ComponentMask64, ComponentQuery,
};
use axiom_core::fixed::FixedVec2;
use axiom_core::job_system::JobSystem;
use axiom_core::worker_pool::WorkerPoolCoordinator;
use axiom_game::network::{
    CombatHitPayload, NetworkMatchDirector, EVENT_COMBAT_HIT, INPUT_LIGHT_ATTACK,
};

#[test]
fn test_event_bus_parallel_job_pipeline_in_match_director() {
    let mut dir = NetworkMatchDirector::<32>::new(0, 0x000F); // 4 peers
    let mut job_system = JobSystem::<256>::new();
    let mut coordinator = WorkerPoolCoordinator::<4, 64>::new(4);

    // Initial state: event bus is empty
    assert_eq!(dir.event_bus.count, 0);

    // Position player 0 and player 1 close to each other for attack testing
    dir.current_world.players[0].pos_x = 0;
    dir.current_world.players[0].pos_y = 0;
    dir.current_world.players[1].pos_x = 600; // within 1,250mm reach
    dir.current_world.players[1].pos_y = 0;

    // Advance ticks with light attack from player 0
    let mut total_hits_detected = 0;
    for tick in 0..10 {
        let input = if tick < 4 { INPUT_LIGHT_ATTACK } else { 0 };
        dir.advance_tick_with_jobs(input, &mut job_system, &mut coordinator);

        // Check if any combat events were recorded
        let count = dir.event_bus.for_each_matching(channel::COMBAT, |ev| {
            if ev.event_type == EVENT_COMBAT_HIT {
                total_hits_detected += 1;
                let payload: Option<CombatHitPayload> = ev.read_payload();
                assert!(payload.is_some());
                let p = payload.unwrap();
                assert_eq!(p.attacker_id, 0);
                assert_eq!(p.victim_id, 1);
                assert_eq!(p.damage, 45);
            }
        });
        let _ = count;
    }

    assert!(total_hits_detected >= 1, "At least one combat hit event must be detected");
    assert!(dir.event_bus.count > 0, "Event bus must contain combat events");

    // Verify deterministic consensus hash
    let hash = dir.event_bus.compute_consensus_hash();
    assert_ne!(hash, 0, "Event bus consensus hash must be non-zero");

    // Verify coordinator executed jobs across workers
    let jobs_done = coordinator.total_jobs_executed();
    assert!(jobs_done >= 10, "Workers must have executed kinematic and event dispatch jobs: got {}", jobs_done);
}

#[test]
fn test_filter_entities_parallel_with_jobs() {
    let mut job_system = JobSystem::<256>::new();
    let mut coordinator = WorkerPoolCoordinator::<4, 64>::new(4);

    // Setup 128 entities
    let mut entities: [Entity; 128] = [Entity::empty(); 128];
    for i in 0..128 {
        let is_active = i % 2 == 0;
        let has_combat = i % 3 == 0;
        let mut mask = ComponentMask64::EMPTY.with(component::POSITION);
        if has_combat {
            mask = mask.with(component::COMBAT);
        }
        entities[i] = Entity {
            id: if is_active { (i + 1) as u64 } else { 0 },
            pos: FixedVec2::ZERO,
            vel: FixedVec2::ZERO,
            flags: mask.0,
            user_data: 0,
            _reserved: [0; 8],
        };
    }

    let query = ComponentQuery::new()
        .with(component::POSITION)
        .with(component::COMBAT);

    let dispatched = filter_entities_with_jobs(
        &entities,
        query,
        4, // 4 batches
        &mut job_system,
        &mut coordinator,
    );

    assert_eq!(dispatched, 4, "Must dispatch 4 parallel query evaluation jobs");
    assert!(coordinator.total_jobs_executed() >= 4);
}
