use axiom_game::network::{compute_world_hash, NetworkMatchDirector};
use axiom_net::nat::{NatCandidate64, StunMessageType};
use axiom_net::NetError;

#[test]
fn test_nat_traversal_blocks_wire_packets_until_hole_punched() {
    let mut dir0 = NetworkMatchDirector::<64>::new(0, 0b11);
    let mut dir1 = NetworkMatchDirector::<64>::new(1, 0b11);

    let cand0 = NatCandidate64::new_ipv4_host([192, 168, 1, 10], 8000, 1000);
    let cand1 = NatCandidate64::new_ipv4_host([192, 168, 1, 20], 8001, 1000);

    dir0.enable_nat_traversal(cand0);
    dir1.enable_nat_traversal(cand1);

    dir0.add_peer_nat_candidate(1, cand1);
    dir1.add_peer_nat_candidate(0, cand0);

    assert!(!dir0.is_peer_ready(1));
    assert!(!dir1.is_peer_ready(0));

    // 1. Peer 0 attempts to send a wire packet before hole-punching
    let pkt0 = dir0.advance_tick(1);
    let recv_res = dir1.receive_packet(&pkt0);
    assert_eq!(recv_res, Err(NetError::NatPunchBlocked { sender_id: 0 }));

    // 2. Perform STUN Hole-Punching handshake
    let ping = dir0.session.nat_session.as_mut().unwrap().create_probe_ping(1, 101, 1000);
    assert_eq!(ping.msg_type, StunMessageType::NatPunchPing as u16);

    let pong = dir1.process_stun_packet(&ping, 1025).expect("Peer 1 must reply with Pong");
    assert_eq!(pong.msg_type, StunMessageType::NatPunchPong as u16);

    let _ = dir0.process_stun_packet(&pong, 1050);

    assert!(dir0.is_peer_ready(1), "Peer 0 must now be hole-punched with Peer 1");
    assert!(dir1.is_peer_ready(0), "Peer 1 must now be hole-punched with Peer 0");
    assert!(dir0.all_peers_ready());
    assert!(dir1.all_peers_ready());

    // 3. Now wire packets must be successfully ingested!
    let pkt0_new = dir0.advance_tick(1);
    let success_recv = dir1.receive_packet(&pkt0_new);
    assert!(success_recv.is_ok(), "Packet must be accepted once hole-punched");
}

#[test]
fn test_four_peer_nat_full_mesh_rollback_consensus() {
    const PEERS: usize = 4;
    let mask = 0b1111;

    let mut directors: Vec<Box<NetworkMatchDirector<64>>> = (0..PEERS)
        .map(|i| {
            let mut d = Box::new(NetworkMatchDirector::<64>::new(i, mask));
            let cand = NatCandidate64::new_ipv4_host([10, 0, 0, (i + 1) as u8], 9000 + i as u16, 2000 + i as u32);
            d.enable_nat_traversal(cand);
            d
        })
        .collect();

    // 1. Cross-register candidates
    for i in 0..PEERS {
        for j in 0..PEERS {
            if i != j {
                let cand_j = directors[j].session.nat_session.as_ref().unwrap().local_candidate;
                directors[i].add_peer_nat_candidate(j, cand_j);
            }
        }
    }

    // 2. Full-mesh STUN ping-pong handshake
    let now = 50_000;
    for i in 0..PEERS {
        for j in 0..PEERS {
            if i != j {
                let ping = directors[i].session.nat_session.as_mut().unwrap().create_probe_ping(j as u8, (i * 10 + j) as u16, now);
                let pong = directors[j].process_stun_packet(&ping, now + 10).unwrap();
                let _ = directors[i].process_stun_packet(&pong, now + 20);
            }
        }
    }

    for i in 0..PEERS {
        assert!(directors[i].all_peers_ready());
    }

    // 3. Run 20 physical simulation frames with complete wire packet cross-ingestion
    for frame in 0..20 {
        let mut packets = Vec::new();
        for p in 0..PEERS {
            let input = if frame % 5 == p as u32 { 1 << (p + 2) } else { 0 };
            let pkt = directors[p].advance_tick(input as u16);
            packets.push(pkt);
        }

        for (sender_idx, pkt) in packets.iter().enumerate() {
            for recv_idx in 0..PEERS {
                if sender_idx != recv_idx {
                    directors[recv_idx].receive_packet(pkt).expect("Ingest wire packet");
                }
            }
        }
    }

    // 4. Verify 100% bit-exact physical world consensus across all 4 peers!
    let hash0 = compute_world_hash(&directors[0].current_world);
    for p in 1..PEERS {
        let hash_p = compute_world_hash(&directors[p].current_world);
        assert_eq!(hash0, hash_p, "Peer {} must match Peer 0 physical state hash", p);
        assert_eq!(directors[p].session.telemetry.total_desyncs, 0);
    }
}
