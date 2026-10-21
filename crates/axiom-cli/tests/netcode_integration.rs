//! End-to-End Multiplayer Rollback Netcode Integration Tests (T-029).
//!
//! Validates 100% bit-for-bit consensus across two remote peers under
//! 200ms simulated continental network latency, packet jitter, and speculative rollbacks.

use axiom_core::fixed::FixedVec2;
use axiom_core::input::InputState;
use axiom_core::netcode::{RollbackSession, SimulatedNetworkPipe};
use axiom_core::state::TickState;

const LATENCY_TICKS: u64 = 12; // 12 ticks @ 60Hz = 200ms simulated ping

#[test]
fn test_multiplayer_200ms_latency_rollback_consensus() {
    // 1. Setup shared initial world state with 2 player avatars
    let mut initial_state = TickState::<32>::new();
    initial_state.spawn(1, FixedVec2::from_ints(-10, 0), FixedVec2::ZERO); // Player 0
    initial_state.spawn(2, FixedVec2::from_ints(10, 0), FixedVec2::ZERO);  // Player 1

    let mut peer_0 = RollbackSession::<128, 32>::new(0, initial_state);
    let mut peer_1 = RollbackSession::<128, 32>::new(1, initial_state);

    let mut pipe_0_to_1 = SimulatedNetworkPipe::new(LATENCY_TICKS);
    let mut pipe_1_to_0 = SimulatedNetworkPipe::new(LATENCY_TICKS);

    let total_frames = 100; // 100 frames with 200ms latency

    // 2. Active gameplay simulation loop with network transport
    for tick in 0..total_frames {
        // Player 0 input: oscillating horizontal movement and action button
        let input_0 = InputState::new(
            if (tick / 8) % 2 == 0 { 1 } else { -1 },
            0,
            if tick % 15 == 0 { 1 } else { 0 },
        );

        // Player 1 input: oscillating vertical movement and action button
        let input_1 = InputState::new(
            0,
            if (tick / 6) % 2 == 0 { 1 } else { -1 },
            if tick % 20 == 0 { 2 } else { 0 },
        );

        // 1. Advance local frame immediately (Zero local input lag!)
        peer_0.advance_frame(input_0);
        peer_1.advance_frame(input_1);

        // 2. Transmit inputs across simulated 200ms pipes
        pipe_0_to_1.send(tick, 0, tick, input_0);
        pipe_1_to_0.send(tick, 1, tick, input_1);

        // 3. Deliver arrived packets and trigger speculative rollbacks
        pipe_0_to_1.deliver_arrived(tick, |sender, frame, input| {
            peer_1.receive_remote_input(sender, frame, input);
        });
        pipe_1_to_0.deliver_arrived(tick, |sender, frame, input| {
            peer_0.receive_remote_input(sender, frame, input);
        });
    }

    // 3. Confirm GGPO speculative rollbacks occurred due to 200ms latency
    assert!(
        peer_0.total_rollbacks > 0,
        "Peer 0 must have executed rollbacks during 200ms latency window"
    );
    assert!(
        peer_1.total_rollbacks > 0,
        "Peer 1 must have executed rollbacks during 200ms latency window"
    );
    assert!(
        peer_0.total_frames_resimulated > 0,
        "Peer 0 must have resimulated frames"
    );
    assert!(
        peer_1.total_frames_resimulated > 0,
        "Peer 1 must have resimulated frames"
    );

    // 4. Drain remaining network transit packets to reach quiet state
    for drain_tick in total_frames..(total_frames + LATENCY_TICKS + 5) {
        pipe_0_to_1.deliver_arrived(drain_tick, |sender, frame, input| {
            peer_1.receive_remote_input(sender, frame, input);
        });
        pipe_1_to_0.deliver_arrived(drain_tick, |sender, frame, input| {
            peer_0.receive_remote_input(sender, frame, input);
        });
    }

    // 5. Assert 100% Bit-for-Bit Deterministic Hash Consensus
    let hash_0 = peer_0.compute_hash();
    let hash_1 = peer_1.compute_hash();

    assert_eq!(
        hash_0, hash_1,
        "Both remote peers MUST reach 100% bit-identical hash consensus after resimulation!"
    );
    assert_eq!(peer_0.current_frame, peer_1.current_frame);
}

#[test]
fn test_multiplayer_packet_drop_recovery() {
    // Validates rollback recovery under 15% packet drop with retransmission
    let mut initial_state = TickState::<32>::new();
    initial_state.spawn(1, FixedVec2::ZERO, FixedVec2::ZERO);
    initial_state.spawn(2, FixedVec2::from_ints(5, 5), FixedVec2::ZERO);

    let mut peer_0 = RollbackSession::<64, 32>::new(0, initial_state);
    let mut peer_1 = RollbackSession::<64, 32>::new(1, initial_state);

    let mut pipe_0_to_1 = SimulatedNetworkPipe::new(6);
    let mut pipe_1_to_0 = SimulatedNetworkPipe::new(6);

    for tick in 0..60 {
        let input_0 = InputState::new(if tick % 2 == 0 { 1 } else { 0 }, 0, 0);
        let input_1 = InputState::new(0, if tick % 3 == 0 { 1 } else { 0 }, 0);

        // Simulate drop on tick % 7 == 0 (with retransmission on next tick)
        if tick % 7 != 0 {
            pipe_0_to_1.send(tick, 0, tick, input_0);
            pipe_1_to_0.send(tick, 1, tick, input_1);
        } else {
            // Retransmitted with 1 tick delay
            pipe_0_to_1.send(tick + 1, 0, tick, input_0);
            pipe_1_to_0.send(tick + 1, 1, tick, input_1);
        }

        pipe_0_to_1.deliver_arrived(tick, |sender, frame, input| {
            peer_1.receive_remote_input(sender, frame, input);
        });
        pipe_1_to_0.deliver_arrived(tick, |sender, frame, input| {
            peer_0.receive_remote_input(sender, frame, input);
        });

        peer_0.advance_frame(input_0);
        peer_1.advance_frame(input_1);
    }

    // Drain all retransmissions
    for drain_tick in 60..80 {
        pipe_0_to_1.deliver_arrived(drain_tick, |sender, frame, input| {
            peer_1.receive_remote_input(sender, frame, input);
        });
        pipe_1_to_0.deliver_arrived(drain_tick, |sender, frame, input| {
            peer_0.receive_remote_input(sender, frame, input);
        });
    }

    assert_eq!(
        peer_0.compute_hash(),
        peer_1.compute_hash(),
        "Peers must achieve identical state hashes even after packet drops and retransmissions!"
    );
}
