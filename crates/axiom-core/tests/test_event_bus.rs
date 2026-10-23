//! Comprehensive verification suite for Axiom Deterministic Event Bus & Component Query Pipeline (T-183)

use core::mem::{align_of, size_of};
use axiom_core::entity::Entity;
use axiom_core::event_bus::{
    channel, component, filter_entities, ComponentMask64, ComponentQuery, DeterministicEventBus,
    EventSlab64,
};
use axiom_core::fixed::FixedVec2;

#[test]
fn test_event_slab_64b_l1_alignment_and_geometry() {
    assert_eq!(size_of::<EventSlab64>(), 64, "EventSlab64 must be exactly 64 bytes");
    assert_eq!(align_of::<EventSlab64>(), 64, "EventSlab64 must be aligned to 64 bytes");

    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    #[repr(C)]
    struct DamagePayload {
        pub attacker_id: u32,
        pub damage: u32,
        pub hit_x: i32,
        pub hit_y: i32,
        pub is_crit: u8,
    }

    let dmg = DamagePayload {
        attacker_id: 42,
        damage: 150,
        hit_x: 12000,
        hit_y: -4500,
        is_crit: 1,
    };

    let slab = EventSlab64::new(
        100, // tick
        1,   // sequence
        10,  // event_type
        channel::COMBAT | channel::AUDIO,
        1,   // source_id
        2,   // target_id
        &dmg,
    );

    assert_eq!(slab.tick, 100);
    assert_eq!(slab.sequence, 1);
    assert_eq!(slab.event_type, 10);
    assert_eq!(slab.channel_mask, channel::COMBAT | channel::AUDIO);
    assert_eq!(slab.source_id, 1);
    assert_eq!(slab.target_id, 2);
    assert!(slab.matches_channel(channel::COMBAT));
    assert!(slab.matches_channel(channel::AUDIO));
    assert!(!slab.matches_channel(channel::NETCODE));

    let read_dmg: Option<DamagePayload> = slab.read_payload();
    assert_eq!(read_dmg, Some(dmg));
}

#[test]
fn test_component_mask_operations() {
    let mask = ComponentMask64::EMPTY
        .with(component::POSITION)
        .with(component::VELOCITY)
        .with(component::COMBAT);

    assert!(mask.has(component::POSITION));
    assert!(mask.has(component::VELOCITY));
    assert!(mask.has(component::COMBAT));
    assert!(!mask.has(component::AI));
    assert_eq!(mask.count_ones(), 3);

    let required = ComponentMask64::EMPTY
        .with(component::POSITION)
        .with(component::VELOCITY);
    assert!(mask.contains_all(required));

    let extra = ComponentMask64::EMPTY.with(component::RENDERABLE);
    assert!(!mask.contains_all(extra));
    assert!(mask.contains_none(extra));

    let removed = mask.without(component::VELOCITY);
    assert!(!removed.has(component::VELOCITY));
    assert_eq!(removed.count_ones(), 2);

    let union_mask = mask.union(extra);
    assert_eq!(union_mask.count_ones(), 4);
    assert!(union_mask.has(component::RENDERABLE));
}

#[test]
fn test_component_query_matching_and_filter_entities() {
    let query = ComponentQuery::new()
        .with(component::POSITION)
        .with(component::COMBAT)
        .without(component::AI)
        .require_any(component::PLAYER);

    let mut entities: [Entity; 8] = [Entity::empty(); 8];

    // Entity 0: Match (Active, Pos, Combat, Player, Not AI)
    entities[0] = Entity {
        id: 101,
        pos: FixedVec2::ZERO,
        vel: FixedVec2::ZERO,
        flags: (ComponentMask64::EMPTY
            .with(component::POSITION)
            .with(component::COMBAT)
            .with(component::PLAYER)).0,
        user_data: 0,
        _reserved: [0; 8],
    };

    // Entity 1: No match (Has AI)
    entities[1] = Entity {
        id: 102,
        pos: FixedVec2::ZERO,
        vel: FixedVec2::ZERO,
        flags: (ComponentMask64::EMPTY
            .with(component::POSITION)
            .with(component::COMBAT)
            .with(component::PLAYER)
            .with(component::AI)).0,
        user_data: 0,
        _reserved: [0; 8],
    };

    // Entity 2: No match (Missing Combat)
    entities[2] = Entity {
        id: 103,
        pos: FixedVec2::ZERO,
        vel: FixedVec2::ZERO,
        flags: (ComponentMask64::EMPTY
            .with(component::POSITION)
            .with(component::PLAYER)).0,
        user_data: 0,
        _reserved: [0; 8],
    };

    // Entity 3: Inactive (id == 0)
    entities[3] = Entity {
        id: 0,
        pos: FixedVec2::ZERO,
        vel: FixedVec2::ZERO,
        flags: (ComponentMask64::EMPTY
            .with(component::POSITION)
            .with(component::COMBAT)
            .with(component::PLAYER)).0,
        user_data: 0,
        _reserved: [0; 8],
    };

    // Entity 4: Match (Active, Pos, Combat, Player, Velocity)
    entities[4] = Entity {
        id: 105,
        pos: FixedVec2::ZERO,
        vel: FixedVec2::ZERO,
        flags: (ComponentMask64::EMPTY
            .with(component::POSITION)
            .with(component::COMBAT)
            .with(component::PLAYER)
            .with(component::VELOCITY)).0,
        user_data: 0,
        _reserved: [0; 8],
    };

    assert!(query.matches_entity(&entities[0]));
    assert!(!query.matches_entity(&entities[1]));
    assert!(!query.matches_entity(&entities[2]));
    assert!(!query.matches_entity(&entities[3]));
    assert!(query.matches_entity(&entities[4]));

    let mut matched_ids = [0u64; 8];
    let matched_count = filter_entities(&entities, query, &mut matched_ids);
    assert_eq!(matched_count, 2);
    assert_eq!(matched_ids[0], 101);
    assert_eq!(matched_ids[1], 105);
}

#[test]
fn test_event_bus_ring_buffer_emit_pop() {
    let mut bus = DeterministicEventBus::<16>::new();
    assert_eq!(bus.count, 0);

    for i in 1..=10u32 {
        let seq = bus.emit(
            i * 10,
            1,
            channel::COMBAT,
            i,
            0,
            &[i as u8; 4],
        );
        assert_eq!(seq, Some(i));
    }
    assert_eq!(bus.count, 10);

    // Pop 5
    for i in 1..=5u32 {
        let ev = bus.pop().expect("Event must be present");
        assert_eq!(ev.sequence, i);
        assert_eq!(ev.tick, i * 10);
        assert_eq!(ev.source_id, i);
    }
    assert_eq!(bus.count, 5);

    // Emit 6 more (total queued = 11, within capacity 16)
    for i in 11..=16u32 {
        let seq = bus.emit(
            i * 10,
            1,
            channel::COMBAT,
            i,
            0,
            &[i as u8; 4],
        );
        assert_eq!(seq, Some(i));
    }
    assert_eq!(bus.count, 11);

    // Drain remaining 11
    let mut popped = 0;
    while let Some(_) = bus.pop() {
        popped += 1;
    }
    assert_eq!(popped, 11);
    assert_eq!(bus.count, 0);
}

#[test]
fn test_event_bus_rollback_to_and_rewind() {
    let mut bus = DeterministicEventBus::<32>::new();

    // Emit 10 events (seq 1..=10)
    for i in 1..=10u32 {
        bus.emit(100 + i, 1, channel::NETCODE, i, 0, &[i as u8; 4]);
    }
    assert_eq!(bus.count, 10);

    // Confirm up to sequence 6
    bus.confirm(6);

    // Rollback speculative events beyond sequence 6
    bus.rollback_to(6);
    assert_eq!(bus.count, 6);

    // Next emitted event must receive sequence 7
    let seq7 = bus.emit(107, 1, channel::NETCODE, 77, 0, &[77u8; 4]);
    assert_eq!(seq7, Some(7));
    assert_eq!(bus.count, 7);

    // Verify events in bus are 1..=6 and then 7
    for expected_seq in 1..=7u32 {
        let ev = bus.pop().unwrap();
        assert_eq!(ev.sequence, expected_seq);
    }
    assert_eq!(bus.count, 0);
}

#[test]
fn test_deterministic_event_consensus_hash() {
    let mut bus1 = DeterministicEventBus::<64>::new();
    let mut bus2 = DeterministicEventBus::<64>::new();

    for i in 1..=20u32 {
        let payload = [i as u8, (i * 3) as u8, (i * 7) as u8, (i * 11) as u8];
        bus1.emit(i * 16, 2, channel::PHYSICS | channel::COMBAT, i, 99, &payload);
        bus2.emit(i * 16, 2, channel::PHYSICS | channel::COMBAT, i, 99, &payload);
    }

    let hash1 = bus1.compute_consensus_hash();
    let hash2 = bus2.compute_consensus_hash();
    assert_ne!(hash1, 0);
    assert_eq!(hash1, hash2, "Identical event streams must yield bit-identical consensus hashes");

    // Altering single payload bit in bus2 alters hash
    let mut bus3 = DeterministicEventBus::<64>::new();
    for i in 1..=20u32 {
        let mut payload = [i as u8, (i * 3) as u8, (i * 7) as u8, (i * 11) as u8];
        if i == 10 {
            payload[0] ^= 1; // 1-bit difference
        }
        bus3.emit(i * 16, 2, channel::PHYSICS | channel::COMBAT, i, 99, &payload);
    }
    let hash3 = bus3.compute_consensus_hash();
    assert_ne!(hash1, hash3, "Differing payload must change consensus hash");
}

#[test]
fn test_channel_filtering_for_each() {
    let mut bus = DeterministicEventBus::<32>::new();

    bus.emit(1, 1, channel::COMBAT, 1, 0, &[0u8; 4]);
    bus.emit(2, 2, channel::PHYSICS, 2, 0, &[0u8; 4]);
    bus.emit(3, 3, channel::COMBAT | channel::AUDIO, 3, 0, &[0u8; 4]);
    bus.emit(4, 4, channel::VFX, 4, 0, &[0u8; 4]);
    bus.emit(5, 5, channel::AUDIO, 5, 0, &[0u8; 4]);

    let mut combat_count = 0;
    bus.for_each_matching(channel::COMBAT, |_ev| {
        combat_count += 1;
    });
    assert_eq!(combat_count, 2); // Event 1 and 3

    let mut audio_count = 0;
    bus.for_each_matching(channel::AUDIO, |_ev| {
        audio_count += 1;
    });
    assert_eq!(audio_count, 2); // Event 3 and 5

    let mut combat_or_vfx = 0;
    bus.for_each_matching(channel::COMBAT | channel::VFX, |_ev| {
        combat_or_vfx += 1;
    });
    assert_eq!(combat_or_vfx, 3); // Event 1, 3, 4
}
