//! Cooperative Multiplayer Raid Battle Integration Tests (T-031).
//!
//! Validates 2-player P2P cooperative combat against a Raid Boss with
//! network latency, command delays, and deterministic speculative rollbacks.

use axiom_cli::coop::{MultiplayerRaidPeer, RaidCommand, HERO_1_ID, HERO_2_ID};

#[test]
fn test_coop_raid_battle_with_network_rollback() {
    let mut peer_0 = MultiplayerRaidPeer::new(0); // Controls Hero 1 (Fire Knight)
    let mut peer_1 = MultiplayerRaidPeer::new(1); // Controls Hero 2 (Lightning Mage)

    // Simulated network transport pipeline (latency = 6 ticks)
    let latency = 6;
    let mut pipe_0_to_1: Vec<(u64, RaidCommand)> = Vec::new();
    let mut pipe_1_to_0: Vec<(u64, RaidCommand)> = Vec::new();

    let mut battle_ticks = 0;

    // Run battle loop until boss is defeated or 80 ticks
    while battle_ticks < 80 && (!peer_0.is_boss_defeated() || !peer_1.is_boss_defeated()) {
        battle_ticks += 1;

        // Player 0 issues commands on specific frames
        let cmd_0 = match battle_ticks {
            5 => Some(RaidCommand {
                player_id: HERO_1_ID,
                frame: battle_ticks,
                action_type: 2, // Haste
            }),
            15 => Some(RaidCommand {
                player_id: HERO_1_ID,
                frame: battle_ticks,
                action_type: 1, // Elemental Strike
            }),
            30 => Some(RaidCommand {
                player_id: HERO_1_ID,
                frame: battle_ticks,
                action_type: 3, // Heavy Slam
            }),
            _ => None,
        };

        // Player 1 issues commands on specific frames
        let cmd_1 = match battle_ticks {
            8 => Some(RaidCommand {
                player_id: HERO_2_ID,
                frame: battle_ticks,
                action_type: 1, // Elemental Strike
            }),
            22 => Some(RaidCommand {
                player_id: HERO_2_ID,
                frame: battle_ticks,
                action_type: 3, // Heavy Slam
            }),
            35 => Some(RaidCommand {
                player_id: HERO_2_ID,
                frame: battle_ticks,
                action_type: 1, // Elemental Strike
            }),
            _ => None,
        };

        // Transmit across network
        if let Some(cmd) = cmd_0 {
            pipe_0_to_1.push((battle_ticks + latency, cmd));
        }
        if let Some(cmd) = cmd_1 {
            pipe_1_to_0.push((battle_ticks + latency, cmd));
        }

        // Deliver arrived packets
        let arrived_0: Vec<RaidCommand> = pipe_1_to_0
            .iter()
            .filter(|(arrive, _)| *arrive <= battle_ticks)
            .map(|(_, cmd)| *cmd)
            .collect();
        pipe_1_to_0.retain(|(arrive, _)| *arrive > battle_ticks);

        for cmd in arrived_0 {
            peer_0.receive_remote_command(cmd);
        }

        let arrived_1: Vec<RaidCommand> = pipe_0_to_1
            .iter()
            .filter(|(arrive, _)| *arrive <= battle_ticks)
            .map(|(_, cmd)| *cmd)
            .collect();
        pipe_0_to_1.retain(|(arrive, _)| *arrive > battle_ticks);

        for cmd in arrived_1 {
            peer_1.receive_remote_command(cmd);
        }

        // Advance local simulation frame
        peer_0.step(cmd_0);
        peer_1.step(cmd_1);
    }

    // Deliver any remaining in-flight commands
    for (_, cmd) in pipe_1_to_0 {
        peer_0.receive_remote_command(cmd);
    }
    for (_, cmd) in pipe_0_to_1 {
        peer_1.receive_remote_command(cmd);
    }

    // Verify Boss Defeat
    assert!(peer_0.is_boss_defeated(), "Raid Boss must be defeated on Peer 0");
    assert!(peer_1.is_boss_defeated(), "Raid Boss must be defeated on Peer 1");

    // Verify rollbacks were executed when remote commands arrived late
    assert!(
        peer_0.rollbacks_executed > 0 || peer_1.rollbacks_executed > 0,
        "Network latency must trigger speculative rollbacks"
    );

    // BIT-FOR-BIT IDENTICAL MASTER STATE HASH CONSENSUS!
    let hash_0 = peer_0.compute_hash();
    let hash_1 = peer_1.compute_hash();

    assert_eq!(
        hash_0, hash_1,
        "Co-op raid peers MUST reach 100% bit-identical hash consensus after battle victory!"
    );
    assert_eq!(peer_0.arena.total_damage_dealt, peer_1.arena.total_damage_dealt);
}
