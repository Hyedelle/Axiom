use axiom_net::nat::{
    NatCandidate64, NatConnectionState, NatTraversalSession, StunMessageType,
    StunPacket64, STUN_MAGIC_COOKIE,
};
use core::mem::{align_of, size_of};

#[test]
fn test_nat_structures_64b_alignment() {
    assert_eq!(size_of::<NatCandidate64>(), 64);
    assert_eq!(align_of::<NatCandidate64>(), 64);

    assert_eq!(size_of::<StunPacket64>(), 64);
    assert_eq!(align_of::<StunPacket64>(), 64);

    assert_eq!(STUN_MAGIC_COOKIE, 0x2112_A442);
}

#[test]
fn test_stun_xor_mapped_address_rfc5389() {
    let mut pkt = StunPacket64::default();
    let original_port: u16 = 49152;
    let original_ip: [u8; 4] = [198, 51, 100, 77];

    pkt.encode_xor_mapped(original_port, original_ip);

    // Mapped port and IP must be obscured (not equal to raw values)
    assert_ne!(pkt.mapped_port, original_port);
    assert_ne!(pkt.mapped_ipv4, original_ip);

    // Decoding must recover exact original values
    let (decoded_port, decoded_ip) = pkt.decode_xor_mapped();
    assert_eq!(decoded_port, original_port);
    assert_eq!(decoded_ip, original_ip);
}

#[test]
fn test_stun_packet_wire_serialization() {
    let mut pkt = StunPacket64::default();
    pkt.msg_type = StunMessageType::NatPunchPing as u16;
    pkt.sender_peer_id = 3;
    pkt.receiver_peer_id = 7;
    pkt.sequence = 1001;
    pkt.rtt_echo_micros = 54321;
    pkt.encode_xor_mapped(30000, [192, 168, 1, 100]);

    let bytes = pkt.to_bytes();
    assert_eq!(bytes.len(), 64);

    let decoded = StunPacket64::from_bytes(&bytes).expect("Valid STUN packet");
    assert_eq!(decoded.magic_cookie, STUN_MAGIC_COOKIE);
    assert_eq!(decoded.msg_type, StunMessageType::NatPunchPing as u16);
    assert_eq!(decoded.sender_peer_id, 3);
    assert_eq!(decoded.receiver_peer_id, 7);
    assert_eq!(decoded.sequence, 1001);
    assert_eq!(decoded.rtt_echo_micros, 54321);

    let (dec_port, dec_ip) = decoded.decode_xor_mapped();
    assert_eq!(dec_port, 30000);
    assert_eq!(dec_ip, [192, 168, 1, 100]);

    // Corrupt magic cookie must fail
    let mut corrupted = bytes;
    corrupted[0] = 0;
    assert!(StunPacket64::from_bytes(&corrupted).is_none());
}

#[test]
fn test_ice_candidate_pair_priority() {
    let prio_a = 2130706431; // Host candidate priority
    let prio_b = 1694498815; // Srflx candidate priority

    let pair_prio_1 = NatTraversalSession::<16>::compute_pair_priority(prio_a, prio_b);
    let pair_prio_2 = NatTraversalSession::<16>::compute_pair_priority(prio_b, prio_a);

    assert!(pair_prio_1 > 0);
    assert!(pair_prio_2 > 0);
    // Since prio_a > prio_b, pair_prio_1 should have higher tiebreaker
    assert!(pair_prio_1 > pair_prio_2);
}

#[test]
fn test_16_peer_full_mesh_nat_hole_punching_simulation() {
    const PEER_COUNT: usize = 16;
    let mut sessions: [NatTraversalSession<16>; PEER_COUNT] = core::array::from_fn(|i| {
        let ip = [10, 0, (i / 256) as u8, (i % 256) as u8];
        let port = 8000 + i as u16;
        let cand = NatCandidate64::new_ipv4_host(ip, port, 1000 + i as u32);
        NatTraversalSession::new(i as u8, cand)
    });

    // 1. Every peer registers all other 15 peers' candidates
    for i in 0..PEER_COUNT {
        for j in 0..PEER_COUNT {
            if i != j {
                let cand_j = sessions[j].local_candidate;
                sessions[i].add_peer_candidate(j, cand_j);
            }
        }
    }

    for i in 0..PEER_COUNT {
        assert_eq!(sessions[i].active_peer_mask, 0xFFFF & !(1 << i));
        assert!(!sessions[i].all_peers_connected());
    }

    // 2. Perform STUN Ping-Pong hole punching exchange across all peer pairs
    let current_time_micros = 100_000;

    for i in 0..PEER_COUNT {
        for j in 0..PEER_COUNT {
            if i != j {
                // Peer i sends probe ping to Peer j
                let ping = sessions[i].create_probe_ping(j as u8, (i * 100 + j) as u16, current_time_micros);
                
                // Peer j processes ping and generates pong
                let pong = sessions[j].process_stun_packet(&ping, current_time_micros + 25);
                assert!(pong.is_some(), "Ping must generate pong");

                // Peer i receives pong from Peer j 50 microseconds later
                let _ = sessions[i].process_stun_packet(&pong.unwrap(), current_time_micros + 50);
            }
        }
    }

    // 3. Verify that all 16 peers achieved direct 100% P2P hole-punched connectivity!
    for i in 0..PEER_COUNT {
        assert!(sessions[i].all_peers_connected(), "Peer {} must be connected to all other peers", i);
        for j in 0..PEER_COUNT {
            if i != j {
                assert!(sessions[i].is_peer_connected(j));
                assert_eq!(sessions[i].peer_candidates[j].state, NatConnectionState::Connected);
                assert!(sessions[i].peer_candidates[j].rtt_micros > 0, "RTT must be recorded");
            }
        }
    }
}
