//! Integration test for Axiom 16-Player Deterministic Physical State Synchronization (T-162).

use axiom_game::network::*;
use axiom_net::loss_sim::NetworkLossSimulator;
use std::vec::Vec;

#[test]
fn test_player_physical_state_size_and_alignment() {
    assert_eq!(core::mem::size_of::<PlayerPhysicalState64>(), 64);
    assert_eq!(core::mem::align_of::<PlayerPhysicalState64>(), 64);
    assert_eq!(core::mem::size_of::<PhysicalWorldSnapshot>(), 1088);
    assert_eq!(core::mem::align_of::<PhysicalWorldSnapshot>(), 64);
}

#[test]
fn test_arena_boundary_and_collision_physics() {
    let mut world = PhysicalWorldSnapshot::default();
    let mut inputs = [0u16; 16];

    // Peer 0 dashes to the right (+X) repeatedly
    inputs[0] = INPUT_RIGHT | INPUT_DASH;

    for _ in 0..120 {
        step_physical_world(&mut world, &inputs);
    }

    // Must be clamped at ARENA_RADIUS_MM - PLAYER_RADIUS_MM = 6000 - 450 = 5550 mm
    let max_allowed = ARENA_RADIUS_MM - PLAYER_RADIUS_MM;
    assert!(
        world.players[0].pos_x <= max_allowed + 10,
        "Player 0 X ({}) exceeded boundary ({})",
        world.players[0].pos_x,
        max_allowed
    );
}

#[test]
fn test_combat_hit_detection_and_guard_reduction() {
    let mut world = PhysicalWorldSnapshot::default();

    // Place player 0 and player 1 close to each other
    world.players[0].pos_x = 0;
    world.players[0].pos_y = 0;
    world.players[1].pos_x = 800; // 800 mm away (within reach)
    world.players[1].pos_y = 0;
    world.players[1].flags |= 1 << 2; // Guarding

    let mut inputs = [0u16; 16];
    inputs[0] = INPUT_HEAVY_ATTACK; // Player 0 heavy attacks
    inputs[1] = INPUT_GUARD;        // Player 1 guards

    for _ in 0..10 {
        step_physical_world(&mut world, &inputs);
    }

    // Heavy attack does 110 dmg, reduced by 75% due to guard -> 110 / 4 = 27 dmg
    assert_eq!(world.players[1].health, 1000 - 27);
    assert_eq!(world.players[1].action_state, STATE_HIT_STUN);
    assert!(world.players[1].vel_x > 0, "Knockback must push victim away");
    assert_eq!(world.total_hits_landed, 1);
    assert_eq!(world.total_damage_dealt, 110);
}

#[test]
fn test_sixteen_player_physical_rollback_under_loss_and_latency() {
    // Initialize 16-peer match director instances
    let mut directors: Vec<NetworkMatchDirector<64>> = (0..16)
        .map(|id| NetworkMatchDirector::<64>::new(id, 0xFFFF))
        .collect();

    // Link simulator with 20% packet loss and 4-frame latency
    let mut sim = NetworkLossSimulator::new(200, 4, 1234567);

    let total_ticks = 40;

    // Simulation loop
    for t in 0..total_ticks {
        // Step all 16 peers with deterministic inputs
        for src in 0..16 {
            let inp = match (src + t as usize) % 5 {
                0 => INPUT_UP | INPUT_DASH,
                1 => INPUT_RIGHT | INPUT_LIGHT_ATTACK,
                2 => INPUT_DOWN | INPUT_GUARD,
                3 => INPUT_LEFT | INPUT_HEAVY_ATTACK,
                _ => 0,
            };
            let pkt = directors[src].advance_tick(inp);

            for dst in 0..16 {
                if src != dst {
                    sim.send_to(dst, pkt, t);
                }
            }
        }

        // Deliver packets
        let arrivals = sim.poll_deliveries(t);
        for (dst, pkt) in arrivals {
            let _ = directors[dst].receive_packet(&pkt);
        }
    }

    // Flush loop: redundant packets so that 8-frame sliding window recovers any loss
    for t in total_ticks..(total_ticks + 16) {
        for src in 0..16 {
            let pkt = directors[src].advance_tick(0);
            for dst in 0..16 {
                if src != dst {
                    sim.send_to(dst, pkt, t);
                }
            }
        }
        let arrivals = sim.poll_deliveries(t);
        for (dst, pkt) in arrivals {
            let _ = directors[dst].receive_packet(&pkt);
        }
    }

    // Final drain of in-flight packets
    for t in (total_ticks + 16)..(total_ticks + 30) {
        let arrivals = sim.poll_deliveries(t);
        for (dst, pkt) in arrivals {
            let _ = directors[dst].receive_packet(&pkt);
        }
    }

    // Verification: all 16 peers must have zero desync events!
    for (p, dir) in directors.iter().enumerate() {
        assert_eq!(
            dir.session.telemetry.total_desyncs, 0,
            "Peer {} encountered desync!",
            p
        );
    }

    // Physical state consensus verification across all 16 peers
    let expected_hash = compute_world_hash(&directors[0].current_world);
    assert!(expected_hash > 0, "World hash must be non-zero");

    for p in 1..16 {
        let p_hash = compute_world_hash(&directors[p].current_world);
        assert_eq!(
            p_hash, expected_hash,
            "Peer {} physical world state diverged from Peer 0 consensus!",
            p
        );
        // Verify all 16 player positions are bit-exact identical
        for i in 0..16 {
            assert_eq!(
                directors[p].current_world.players[i].pos_x,
                directors[0].current_world.players[i].pos_x
            );
            assert_eq!(
                directors[p].current_world.players[i].pos_y,
                directors[0].current_world.players[i].pos_y
            );
            assert_eq!(
                directors[p].current_world.players[i].health,
                directors[0].current_world.players[i].health
            );
        }
    }
}
