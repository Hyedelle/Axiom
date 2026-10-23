//! # Axiom Distributed Rollback Netcode Engine (`axiom-net`)
//!
//! 16-Player zero-input-delay deterministic peer-to-peer rollback network protocol.
//!
//! - **0-Float & 64B L1 Cache Aligned**: `NetPacket64` and `NetTelemetry64` fit exactly into 64-byte slabs.
//! - **Loss-Tolerant Sliding Window**: 8-frame redundant input delivery recovering dropped packets with 0ms stall.
//! - **Instantaneous Speculative Rollback**: Rewinds and resimulates divergent state across high-latency peers.
//! - **64-bit FNV-1a Consensus**: Automated desync detection across 16 peers.

#![no_std]
extern crate alloc;

pub mod loss_sim;
pub mod packet;
pub mod rollback;
pub mod telemetry;

pub use loss_sim::NetworkLossSimulator;
pub use packet::{NetPacket64, INPUT_WINDOW_FRAMES, MAX_PEERS, NET_WIRE_MAGIC};
pub use rollback::{DistributedRollbackSession, NetError, RollbackStepResult, DEFAULT_NET_BUFFER_FRAMES};
pub use telemetry::NetTelemetry64;

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;

    #[test]
    fn test_packet_size_and_alignment() {
        assert_eq!(core::mem::size_of::<NetPacket64>(), 64);
        assert_eq!(core::mem::align_of::<NetPacket64>(), 64);
        assert_eq!(core::mem::size_of::<NetTelemetry64>(), 64);
        assert_eq!(core::mem::align_of::<NetTelemetry64>(), 64);

        let mut pkt = NetPacket64::default();
        pkt.sequence = 12345;
        pkt.current_frame = 42;
        pkt.window_inputs[0] = 0xABCD;

        let bytes = pkt.to_bytes();
        assert_eq!(bytes.len(), 64);

        let decoded = NetPacket64::from_bytes(&bytes).expect("Valid magic bytes");
        assert_eq!(decoded.sequence, 12345);
        assert_eq!(decoded.current_frame, 42);
        assert_eq!(decoded.window_inputs[0], 0xABCD);
    }

    #[test]
    fn test_two_peer_speculative_rollback_and_recovery() {
        // Peer 0 and Peer 1
        let mut peer0 = DistributedRollbackSession::<64>::new(0);
        let mut peer1 = DistributedRollbackSession::<64>::new(1);

        // Simple toy simulation: hash = sum(inputs) + frame
        let sim = |frame: u32, inputs: &[u16; 16]| -> u64 {
            let mut h: u64 = 0xCBF2_9CE4_8422_2325 ^ (frame as u64);
            for (p, &inp) in inputs.iter().enumerate() {
                h ^= ((inp as u64) | ((p as u64) << 16)).wrapping_mul(0x100_0000_01B3);
            }
            h
        };

        // Frame 0..3: advance peer0 locally with inputs 10, 11, 12, 13
        for f in 0..4 {
            let _ = peer0.submit_local_input(10 + f as u16);
            peer0.advance_frame(sim);
        }
        assert_eq!(peer0.current_frame, 4);

        // Peer 1 sends input 20 for frame 0, but it arrives late when peer0 is already at frame 4!
        let mut p1_pkt = peer1.submit_local_input(20);
        p1_pkt.current_frame = 0;
        p1_pkt.window_inputs[0] = 20;

        let rollback_frame = peer0.receive_remote_packet(&p1_pkt).expect("Valid packet");
        assert_eq!(rollback_frame, Some(0), "Delayed input must invalidate frame 0");

        // Execute rollback and resimulation from frame 0
        let res = peer0.rollback_and_resimulate(rollback_frame.unwrap(), sim);
        assert!(res.rolled_back);
        assert_eq!(res.rollback_depth, 4);
        assert_eq!(peer0.telemetry.total_rollbacks_triggered, 1);
        assert_eq!(peer0.telemetry.total_resimulated_frames, 4);
    }

    #[test]
    fn test_loss_simulator_and_sliding_window_redundancy() {
        let mut sim = NetworkLossSimulator::new(250, 2, 42); // 25% loss, 2 frames latency
        let mut peer0 = DistributedRollbackSession::<64>::new(0);
        let mut peer1 = DistributedRollbackSession::<64>::new(1);

        // Send 10 packets from peer0 to peer1
        for f in 0..10 {
            let pkt = peer0.submit_local_input((100 + f) as u16);
            peer0.advance_frame(|_, _| 0);
            sim.send(pkt, f);

            let arrivals = sim.poll_deliveries(f);
            for (_, p) in arrivals {
                let _ = peer1.receive_remote_packet(&p);
            }
        }

        // Advance simulated time to flush remaining packets
        for f in 10..15 {
            let arrivals = sim.poll_deliveries(f);
            for (_, p) in arrivals {
                let _ = peer1.receive_remote_packet(&p);
            }
        }

        assert!(sim.dropped_packets > 0, "Loss simulator must have dropped packets");
        assert!(sim.delivered_packets > 0, "Some packets must have arrived");

        // Thanks to 8-frame sliding window, peer1 should have confirmed inputs despite loss!
        let mut confirmed_count = 0;
        for f in 0..10 {
            let slot = (f as usize) % 64;
            if peer1.is_confirmed[0][slot] {
                confirmed_count += 1;
            }
        }
        assert!(confirmed_count >= 8, "Sliding window must recover lost frames (got {}/10)", confirmed_count);
    }

    #[test]
    fn test_sixteen_peer_distributed_rollback_consensus() {
        // Initialize 16 distributed peers
        let mut peers: Vec<DistributedRollbackSession<64>> = (0..16)
            .map(|id| DistributedRollbackSession::<64>::new(id))
            .collect();

        let sim = |frame: u32, inputs: &[u16; 16]| -> u64 {
            let mut h: u64 = 0xCBF2_9CE4_8422_2325 ^ (frame as u64);
            for (p, &inp) in inputs.iter().enumerate() {
                h ^= ((inp as u64) | ((p as u64) << 16)).wrapping_mul(0x100_0000_01B3);
            }
            h
        };

        // Advance 20 frames across all 16 peers with sliding window input broadcast
        for f in 0..20 {
            let mut packets = Vec::with_capacity(16);
            for p in 0..16 {
                let inp = ((p as u16 + 1) * 100) + (f as u16);
                let pkt = peers[p].submit_local_input(inp);
                peers[p].advance_frame(sim);
                packets.push(pkt);
            }

            // Cross-deliver packets to all other peers
            for (src_p, pkt) in packets.iter().enumerate() {
                for dst_p in 0..16 {
                    if src_p != dst_p {
                        if let Ok(Some(from_frame)) = peers[dst_p].receive_remote_packet(pkt) {
                            peers[dst_p].rollback_and_resimulate(from_frame, sim);
                        }
                    }
                }
            }
        }

        // Verify all 16 peers arrived at the exact same consensus hash for frame 19
        let slot = (19 as usize) % 64;
        let expected_hash = peers[0].state_hashes[slot];
        assert!(expected_hash > 0, "Consensus hash must be non-zero");

        for p in 0..16 {
            assert_eq!(peers[p].current_frame, 20);
            assert_eq!(
                peers[p].state_hashes[slot],
                expected_hash,
                "Peer {} state hash diverged from consensus!",
                p
            );
            assert_eq!(peers[p].telemetry.total_desyncs, 0, "Zero desync events allowed");
        }
    }
}
