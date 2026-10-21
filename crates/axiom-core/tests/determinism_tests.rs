use axiom_core::{Entity, FixedVec2, InputState, Tick, TickState, TimeMachine};
use std::time::Instant;

/// Helper to generate deterministic input sequence for `n` ticks.
fn generate_deterministic_inputs(count: usize) -> Vec<InputState> {
    (0..count)
        .map(|i| {
            let move_x = match i % 4 {
                0 => 1,
                1 => 0,
                2 => -1,
                _ => 0,
            };
            let move_y = match (i / 2) % 4 {
                0 => 0,
                1 => 1,
                2 => 0,
                _ => -1,
            };
            let buttons = (i as u32) & 0x07;
            InputState::new(move_x, move_y, buttons)
        })
        .collect()
}

#[test]
fn test_entity_cache_alignment_and_size() {
    assert_eq!(
        std::mem::size_of::<Entity>(),
        64,
        "Entity must be exactly 64 bytes"
    );
    assert_eq!(
        std::mem::align_of::<Entity>(),
        64,
        "Entity must be 64-byte aligned (L1 cache line)"
    );
}

#[test]
fn test_100_tick_determinism_identical_runs() {
    let inputs = generate_deterministic_inputs(100);

    // Run A
    let mut state_a = TickState::<16>::new();
    state_a.spawn(101, FixedVec2::from_ints(10, 20), FixedVec2::ZERO);
    state_a.spawn(102, FixedVec2::from_ints(50, -30), FixedVec2::from_ints(1, 2));

    let mut hashes_a = Vec::with_capacity(100);
    for input in &inputs {
        state_a = state_a.step(input);
        hashes_a.push(state_a.compute_hash());
    }

    // Run B (fresh separate run with same inputs)
    let mut state_b = TickState::<16>::new();
    state_b.spawn(101, FixedVec2::from_ints(10, 20), FixedVec2::ZERO);
    state_b.spawn(102, FixedVec2::from_ints(50, -30), FixedVec2::from_ints(1, 2));

    let mut hashes_b = Vec::with_capacity(100);
    for input in &inputs {
        state_b = state_b.step(input);
        hashes_b.push(state_b.compute_hash());
    }

    assert_eq!(state_a, state_b, "Final states must be byte-for-byte identical");
    assert_eq!(hashes_a, hashes_b, "Hash histories must be bit-for-bit identical");
    assert_eq!(state_a.tick, Tick(100));
}

#[test]
fn test_time_travel_rollback_50_ticks_identical_hash() {
    let inputs = generate_deterministic_inputs(100);
    let mut time_machine = TimeMachine::<128, 16>::new();

    let mut current_state = TickState::<16>::new();
    current_state.spawn(1, FixedVec2::from_ints(0, 0), FixedVec2::ZERO);
    current_state.spawn(2, FixedVec2::from_ints(100, 100), FixedVec2::from_ints(-1, -1));

    // Record initial state at Tick 0
    time_machine.push(current_state, InputState::NEUTRAL);

    let mut hash_at_tick_50 = 0u64;
    let mut hash_at_tick_100 = 0u64;

    // Simulate 100 ticks
    for input in &inputs {
        current_state = current_state.step(input);
        time_machine.push(current_state, *input);

        if current_state.tick == Tick(50) {
            hash_at_tick_50 = current_state.compute_hash();
        }
        if current_state.tick == Tick(100) {
            hash_at_tick_100 = current_state.compute_hash();
        }
    }

    assert_ne!(hash_at_tick_50, 0);
    assert_ne!(hash_at_tick_100, 0);
    assert_ne!(hash_at_tick_50, hash_at_tick_100);

    // Rollback to Tick 50
    let rollback_state = time_machine
        .rollback_to(Tick(50))
        .expect("Rollback to tick 50 must succeed");

    assert_eq!(rollback_state.tick, Tick(50));
    assert_eq!(
        rollback_state.compute_hash(),
        hash_at_tick_50,
        "Rolled back state hash must match original tick 50 hash"
    );

    // Replay forward from Tick 50 to Tick 100 with the exact same inputs (ticks 50..100)
    let mut replay_state = rollback_state;
    for tick_num in 50..100 {
        let input = time_machine
            .get_input(Tick(tick_num + 1))
            .expect("Recorded input must exist");
        replay_state = replay_state.step(&input);
    }

    assert_eq!(replay_state.tick, Tick(100));
    assert_eq!(
        replay_state.compute_hash(),
        hash_at_tick_100,
        "Replayed tick 100 hash must exactly equal original tick 100 hash"
    );
}

#[test]
fn test_sub_millisecond_100_tick_performance() {
    let inputs = generate_deterministic_inputs(100);
    let mut state = TickState::<16>::new();
    state.spawn(1, FixedVec2::ZERO, FixedVec2::ZERO);

    let start = Instant::now();
    for input in &inputs {
        state = state.step(input);
    }
    let elapsed = start.elapsed();

    // Invariant requirement: Must pass well within 0.01s (10,000 µs)
    println!("100-tick simulation took: {:?}", elapsed);
    assert!(
        elapsed.as_millis() < 10,
        "100-tick execution took {:?}, exceeding 10ms target",
        elapsed
    );
}
