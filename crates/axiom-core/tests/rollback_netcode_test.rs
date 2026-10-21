//! Distributed Rollback Netcode (Lockstep/GGPO) Integration Benchmark (T-020)
//!
//! Validates:
//! - 200ms cross-continental simulated latency (12 discrete ticks at 60Hz)
//! - 100 frames of bidirectional real-time player input prediction
//! - Speculative execution and instantaneous rollbacks upon misprediction
//! - 100% bit-for-bit state synchronization across peers

use axiom_core::{
    FixedVec2, InputState, RollbackSession, SimulatedNetworkPipe, TickState,
};
use std::time::Instant;

#[test]
fn test_cross_continental_latency_rollback_benchmark() {
    let mut initial_state = TickState::<32>::new();
    // Spawn 2 player characters
    initial_state.spawn(1, FixedVec2::from_ints(10, 10), FixedVec2::ZERO);
    initial_state.spawn(2, FixedVec2::from_ints(50, 50), FixedVec2::ZERO);

    let mut peer0 = RollbackSession::<128, 32>::new(0, initial_state);
    let mut peer1 = RollbackSession::<128, 32>::new(1, initial_state);

    // 12 ticks latency = 200ms round-trip at 60Hz
    let mut pipe0_to_1 = SimulatedNetworkPipe::new(12);
    let mut pipe1_to_0 = SimulatedNetworkPipe::new(12);

    let total_frames = 100;
    let start_time = Instant::now();

    for tick in 0..total_frames {
        // Player 0 inputs: complex movement pattern
        let p0_x = match tick % 6 {
            0 | 1 => 1,
            2 | 3 => -1,
            _ => 0,
        };
        let p0_input = InputState::new(p0_x, 0, (tick % 2) as u32);

        // Player 1 inputs: independent orthogonal pattern
        let p1_y = match tick % 5 {
            0 | 1 | 2 => 1,
            _ => -1,
        };
        let p1_input = InputState::new(0, p1_y, ((tick + 1) % 2) as u32);

        // 1. Advance local frame immediately (Zero local input delay)
        peer0.advance_frame(p0_input);
        peer1.advance_frame(p1_input);

        // 2. Dispatch network packets into delayed pipe
        pipe0_to_1.send(tick, 0, tick, p0_input);
        pipe1_to_0.send(tick, 1, tick, p1_input);

        // 3. Receive and process arrived packets
        pipe0_to_1.deliver_arrived(tick, |sender, frame, input| {
            peer1.receive_remote_input(sender, frame, input);
        });
        pipe1_to_0.deliver_arrived(tick, |sender, frame, input| {
            peer0.receive_remote_input(sender, frame, input);
        });
    }

    // Flush remaining network pipeline
    for flush_tick in total_frames..(total_frames + 20) {
        pipe0_to_1.deliver_arrived(flush_tick, |sender, frame, input| {
            peer1.receive_remote_input(sender, frame, input);
        });
        pipe1_to_0.deliver_arrived(flush_tick, |sender, frame, input| {
            peer0.receive_remote_input(sender, frame, input);
        });
    }

    let elapsed = start_time.elapsed();
    let micros_per_frame = elapsed.as_micros() as f64 / total_frames as f64;

    println!(
        "\n==> [NETCODE BENCHMARK] 100 frames across 200ms latency simulated in {:.2?} ({:.2} µs/frame)",
        elapsed, micros_per_frame
    );
    println!(
        "==> Peer 0 Rollbacks: {} (resimulated frames: {})",
        peer0.total_rollbacks, peer0.total_frames_resimulated
    );
    println!(
        "==> Peer 1 Rollbacks: {} (resimulated frames: {})",
        peer1.total_rollbacks, peer1.total_frames_resimulated
    );

    // Assert that rollbacks actively occurred and reconciled
    assert!(peer0.total_rollbacks > 0, "Peer 0 must have handled mispredictions");
    assert!(peer1.total_rollbacks > 0, "Peer 1 must have handled mispredictions");
    assert_eq!(peer0.current_frame, total_frames);
    assert_eq!(peer1.current_frame, total_frames);

    // BIT-FOR-BIT SYNCHRONIZATION TEST
    let hash0 = peer0.compute_hash();
    let hash1 = peer1.compute_hash();
    assert_eq!(
        hash0, hash1,
        "Master hashes of Peer 0 and Peer 1 must be 100% bit-identical after 200ms latency!"
    );

    // Entity position verification
    assert_eq!(
        peer0.current_state.entities[0].pos,
        peer1.current_state.entities[0].pos
    );
    assert_eq!(
        peer0.current_state.entities[1].pos,
        peer1.current_state.entities[1].pos
    );
}
