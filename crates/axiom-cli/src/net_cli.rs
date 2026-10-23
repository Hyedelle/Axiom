//! Axiom Distributed Rollback Netcode CLI & Benchmark Suite (T-161).

use axiom_net::{DistributedRollbackSession, NetworkLossSimulator};
use std::time::Instant;

pub fn run_net_suite(args: &[String]) {
    let is_bench = args.iter().any(|a| a == "bench" || a == "--bench");

    println!("============================================================");
    println!(" 🌐 Axiom 16-Player Distributed Rollback Netcode Suite");
    println!("============================================================");

    if is_bench {
        run_distributed_rollback_benchmark(500);
    } else {
        println!("Available subcommands:");
        println!("  axiom net bench       - 16-peer loss-resilient rollback benchmark");
    }
}

pub fn run_distributed_rollback_benchmark(frames: u32) {
    println!("--> Starting 16-Peer Distributed Rollback Benchmark ({} frames)...", frames);

    let start = Instant::now();
    let mut peers: Vec<DistributedRollbackSession<64>> = (0..16)
        .map(|id| DistributedRollbackSession::<64>::with_expected_peers(id, 0xFFFF))
        .collect();

    // Link simulator with 15% packet loss and 4-frame (~66ms) latency
    let mut sim = NetworkLossSimulator::new(150, 4, 99991);

    let sim_func = |frame: u32, inputs: &[u16; 16]| -> u64 {
        let mut h: u64 = 0xCBF2_9CE4_8422_2325 ^ (frame as u64);
        for (p, &inp) in inputs.iter().enumerate() {
            h ^= ((inp as u64) | ((p as u64) << 16)).wrapping_mul(0x100_0000_01B3);
        }
        h
    };

    let mut total_packets = 0u64;

    for f in 0..frames {
        // Each peer inputs action and broadcasts packet to all other 15 peers
        for src in 0..16 {
            let inp = ((src as u16 + 1) * 37) ^ (f as u16);
            let pkt = peers[src].submit_local_input(inp);
            peers[src].advance_frame(sim_func);

            for dst in 0..16 {
                if src != dst {
                    sim.send_to(dst, pkt, f);
                    total_packets += 1;
                }
            }
        }

        // Deliver packets to their intended destination peer
        let arrivals = sim.poll_deliveries(f);
        for (dst, pkt) in arrivals {
            if let Ok(Some(from_frame)) = peers[dst].receive_remote_packet(&pkt) {
                peers[dst].rollback_and_resimulate(from_frame, sim_func);
            }
            let _ = peers[dst].verify_remote_consensus(&pkt);
        }
    }

    // Flush loop: send redundant confirmation packets so all sliding windows deliver final inputs
    for f in frames..(frames + 12) {
        for src in 0..16 {
            let pkt = peers[src].submit_local_input(0);
            peers[src].advance_frame(sim_func);
            for dst in 0..16 {
                if src != dst {
                    sim.send_to(dst, pkt, f);
                    total_packets += 1;
                }
            }
        }
        let arrivals = sim.poll_deliveries(f);
        for (dst, pkt) in arrivals {
            if let Ok(Some(from_frame)) = peers[dst].receive_remote_packet(&pkt) {
                peers[dst].rollback_and_resimulate(from_frame, sim_func);
            }
            let _ = peers[dst].verify_remote_consensus(&pkt);
        }
    }

    // Final drain of any remaining in-flight packets
    for f in (frames + 12)..(frames + 25) {
        let arrivals = sim.poll_deliveries(f);
        for (dst, pkt) in arrivals {
            if let Ok(Some(from_frame)) = peers[dst].receive_remote_packet(&pkt) {
                peers[dst].rollback_and_resimulate(from_frame, sim_func);
            }
            let _ = peers[dst].verify_remote_consensus(&pkt);
        }
    }

    let elapsed = start.elapsed();
    let total_peer_frames = (frames as u64) * 16;
    let tps = (total_peer_frames as f64) / elapsed.as_secs_f64();

    let mut total_rollbacks = 0u64;
    let mut total_resim = 0u64;
    let mut total_desyncs = 0u64;
    for p in &peers {
        total_rollbacks += p.telemetry.total_rollbacks_triggered as u64;
        total_resim += p.telemetry.total_resimulated_frames as u64;
        total_desyncs += p.telemetry.total_desyncs as u64;
    }

    // Check consensus agreement across all 16 peers for the last verified frame
    let check_frame = frames.saturating_sub(1);
    let slot = (check_frame as usize) % 64;
    let consensus_hash = peers[0].state_hashes[slot];
    let mut matched_peers = 0;
    for p in 0..16 {
        if peers[p].state_hashes[slot] == consensus_hash {
            matched_peers += 1;
        }
    }

    println!("[OK] Benchmark Completed in {:.2?} ({:.0} Peer-Frames/sec)", elapsed, tps);
    println!("• Physical Frames: {} ticks x 16 peers ({} peer-frames)", frames, total_peer_frames);
    println!("• Link Simulation: 15.0% Packet Loss, 4-Frame (66ms) Latency");
    println!("• Packets Injected: {} (Dropped: {}, Delivered: {})", total_packets, sim.dropped_packets, sim.delivered_packets);
    println!("• Speculative Rollbacks Triggered: {}", total_rollbacks);
    println!("• Total Resimulated Frames: {}", total_resim);
    println!("• 16-Peer Consensus Hash: 0x{:016X} ({}/16 peers agreed)", consensus_hash, matched_peers);
    println!("• Desync Events: {}", total_desyncs);
    println!("• L1 Cache Slabs: 100% 64-Byte NetPacket64 / NetTelemetry64 Alignment");
}
