//! Multiplayer Cooperative Shared Quest & Party Progression Tests (T-035).
//!
//! Validates shared quest objective tracking (distributed mob defeats and item gathering),
//! out-of-order network delta synchronization, and bit-identical party XP reward consensus.

use axiom_core::quest::{
    ObjectiveType, QuestJournal, QuestRecord, QuestStatus, DEFAULT_MAX_ACTIVE_QUESTS,
    DEFAULT_MAX_COMPLETED_QUESTS,
};

/// Party quest progress delta packet transmitted over network.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuestDelta {
    EnemyDefeated { killer_id: u32, enemy_type: u32, tick: u64 },
    ItemGathered { gatherer_id: u32, item_type: u8, count: u32, tick: u64 },
    TurnedIn { quest_id: u32, tick: u64 },
}

#[test]
fn test_multiplayer_shared_party_quest_progression() {
    let mut journal_0 = QuestJournal::<DEFAULT_MAX_ACTIVE_QUESTS, DEFAULT_MAX_COMPLETED_QUESTS>::new();
    let mut journal_1 = QuestJournal::<DEFAULT_MAX_ACTIVE_QUESTS, DEFAULT_MAX_COMPLETED_QUESTS>::new();

    // Quest #201: Defeat 4 Goblins (target_id: 888), Reward: 800 XP
    let quest_a = QuestRecord::new(201, ObjectiveType::DefeatEnemies, 888, 4, 800, 1, 2, 0);

    // Quest #202: Gather 6 Iron Ores (target_id: 1), Reward: 600 XP
    let quest_b = QuestRecord::new(202, ObjectiveType::GatherItem, 1, 6, 600, 2, 1, 0);

    assert!(journal_0.accept_quest(quest_a));
    assert!(journal_0.accept_quest(quest_b));
    assert!(journal_1.accept_quest(quest_a));
    assert!(journal_1.accept_quest(quest_b));

    // Network pipelines with simulated latency (6 ticks)
    let latency = 6;
    let mut pipe_0_to_1: Vec<(u64, QuestDelta)> = Vec::new();
    let mut pipe_1_to_0: Vec<(u64, QuestDelta)> = Vec::new();

    // Simulation loop across 40 frames
    for current_tick in 1..=40 {
        // Player 0 actions:
        // Tick 5: Player 0 slays 1 Goblin
        // Tick 15: Player 0 slays another Goblin
        // Tick 25: Player 0 gathers 3 Iron Ores
        if current_tick == 5 {
            journal_0.notify_enemy_defeated(888);
            pipe_0_to_1.push((
                current_tick + latency,
                QuestDelta::EnemyDefeated { killer_id: 0, enemy_type: 888, tick: current_tick },
            ));
        } else if current_tick == 15 {
            journal_0.notify_enemy_defeated(888);
            pipe_0_to_1.push((
                current_tick + latency,
                QuestDelta::EnemyDefeated { killer_id: 0, enemy_type: 888, tick: current_tick },
            ));
        } else if current_tick == 25 {
            journal_0.notify_item_gathered(1, 3);
            pipe_0_to_1.push((
                current_tick + latency,
                QuestDelta::ItemGathered { gatherer_id: 0, item_type: 1, count: 3, tick: current_tick },
            ));
        }

        // Player 1 actions:
        // Tick 8: Player 1 gathers 3 Iron Ores
        // Tick 18: Player 1 slays 2 Goblins
        if current_tick == 8 {
            journal_1.notify_item_gathered(1, 3);
            pipe_1_to_0.push((
                current_tick + latency,
                QuestDelta::ItemGathered { gatherer_id: 1, item_type: 1, count: 3, tick: current_tick },
            ));
        } else if current_tick == 18 {
            journal_1.notify_enemy_defeated(888);
            journal_1.notify_enemy_defeated(888);
            pipe_1_to_0.push((
                current_tick + latency,
                QuestDelta::EnemyDefeated { killer_id: 1, enemy_type: 888, tick: current_tick },
            ));
            pipe_1_to_0.push((
                current_tick + latency,
                QuestDelta::EnemyDefeated { killer_id: 1, enemy_type: 888, tick: current_tick },
            ));
        }

        // Deliver arrived packets to Peer 0
        let arrived_0: Vec<QuestDelta> = pipe_1_to_0
            .iter()
            .filter(|(arr, _)| *arr <= current_tick)
            .map(|(_, d)| *d)
            .collect();
        pipe_1_to_0.retain(|(arr, _)| *arr > current_tick);

        for delta in arrived_0 {
            match delta {
                QuestDelta::EnemyDefeated { enemy_type, .. } => {
                    journal_0.notify_enemy_defeated(enemy_type);
                }
                QuestDelta::ItemGathered { item_type, count, .. } => {
                    journal_0.notify_item_gathered(item_type, count);
                }
                QuestDelta::TurnedIn { quest_id, tick } => {
                    journal_0.turn_in_quest(quest_id, tick);
                }
            }
        }

        // Deliver arrived packets to Peer 1
        let arrived_1: Vec<QuestDelta> = pipe_0_to_1
            .iter()
            .filter(|(arr, _)| *arr <= current_tick)
            .map(|(_, d)| *d)
            .collect();
        pipe_0_to_1.retain(|(arr, _)| *arr > current_tick);

        for delta in arrived_1 {
            match delta {
                QuestDelta::EnemyDefeated { enemy_type, .. } => {
                    journal_1.notify_enemy_defeated(enemy_type);
                }
                QuestDelta::ItemGathered { item_type, count, .. } => {
                    journal_1.notify_item_gathered(item_type, count);
                }
                QuestDelta::TurnedIn { quest_id, tick } => {
                    journal_1.turn_in_quest(quest_id, tick);
                }
            }
        }
    }

    // Flush remaining network packets
    for (_, delta) in pipe_1_to_0 {
        match delta {
            QuestDelta::EnemyDefeated { enemy_type, .. } => {
                journal_0.notify_enemy_defeated(enemy_type);
            }
            QuestDelta::ItemGathered { item_type, count, .. } => {
                journal_0.notify_item_gathered(item_type, count);
            }
            QuestDelta::TurnedIn { quest_id, tick } => {
                journal_0.turn_in_quest(quest_id, tick);
            }
        }
    }
    for (_, delta) in pipe_0_to_1 {
        match delta {
            QuestDelta::EnemyDefeated { enemy_type, .. } => {
                journal_1.notify_enemy_defeated(enemy_type);
            }
            QuestDelta::ItemGathered { item_type, count, .. } => {
                journal_1.notify_item_gathered(item_type, count);
            }
            QuestDelta::TurnedIn { quest_id, tick } => {
                journal_1.turn_in_quest(quest_id, tick);
            }
        }
    }

    // Both quests should now be ObjectiveComplete on both peers
    assert_eq!(journal_0.get_quest(201).unwrap().status, QuestStatus::ObjectiveComplete);
    assert_eq!(journal_0.get_quest(202).unwrap().status, QuestStatus::ObjectiveComplete);
    assert_eq!(journal_1.get_quest(201).unwrap().status, QuestStatus::ObjectiveComplete);
    assert_eq!(journal_1.get_quest(202).unwrap().status, QuestStatus::ObjectiveComplete);

    // Turn in both quests synchronously at tick 50
    journal_0.turn_in_quest(201, 50);
    journal_0.turn_in_quest(202, 50);
    journal_1.turn_in_quest(201, 50);
    journal_1.turn_in_quest(202, 50);

    assert_eq!(journal_0.active_count, 0);
    assert_eq!(journal_1.active_count, 0);
    assert_eq!(journal_0.completed_count, 2);
    assert_eq!(journal_1.completed_count, 2);
    assert_eq!(journal_0.total_xp_earned, 1400); // 800 + 600
    assert_eq!(journal_1.total_xp_earned, 1400);

    // Assert 100% Bit-for-Bit Identical Hash Consensus
    assert_eq!(
        journal_0.compute_hash(),
        journal_1.compute_hash(),
        "Party quest journals MUST have 100% bit-identical hash consensus after shared completion!"
    );
}
