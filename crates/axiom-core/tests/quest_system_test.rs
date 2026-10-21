//! RPG Quest & Dialogue State Machine Integration Test (T-018)
//!
//! Validates:
//! - Deterministic quest lifecycle: accept -> progress -> complete -> turn-in
//! - Dialogue graph state transitions and action triggers
//! - Time-travel rollback integration for quest state

use axiom_core::{
    DialogueAction, DialogueChoice, DialogueNode, DialogueSession, ObjectiveType, QuestJournal,
    QuestRecord, QuestStatus,
};

#[test]
fn test_quest_and_dialogue_full_flow_with_rollback() {
    let mut journal = QuestJournal::<16, 32>::new();
    let mut dialogue = DialogueSession::new();

    // 1. Dialogue Graph Setup:
    // Node 1: "Elder: Greetings traveler, can you defeat 3 bandits for our village?"
    // Choice 0: "I accept the task." -> Next Node 2, Action: AcceptQuest(501)
    // Choice 1: "Not now." -> Next Node 0, Action: Exit
    let root_node = DialogueNode {
        node_id: 1,
        prompt_text_id: 1001,
        choices: [
            DialogueChoice {
                choice_text_id: 2001,
                next_node_id: 2,
                action: DialogueAction::AcceptQuest(501),
            },
            DialogueChoice {
                choice_text_id: 2002,
                next_node_id: 0,
                action: DialogueAction::Exit,
            },
            DialogueChoice::default(),
            DialogueChoice::default(),
        ],
        choices_count: 2,
    };

    // 2. Start conversation with Elder (NPC ID: 7)
    dialogue.start(7, root_node.node_id);
    assert!(dialogue.is_active);
    assert_eq!(dialogue.speaker_npc_id, 7);

    // Player chooses Choice 0 ("I accept")
    let selected_choice = root_node.choices[0];
    if let DialogueAction::AcceptQuest(qid) = selected_choice.action {
        let quest = QuestRecord::new(qid, ObjectiveType::DefeatEnemies, 99, 3, 500, 1, 2, 10);
        assert!(journal.accept_quest(quest));
    }
    dialogue.close();
    assert!(!dialogue.is_active);
    assert_eq!(journal.active_count, 1);

    // 3. World simulation: Defeat 2 bandits at tick 20
    journal.notify_enemy_defeated(99);
    journal.notify_enemy_defeated(99);
    let q = journal.get_quest(501).unwrap();
    assert_eq!(q.current_count, 2);
    assert_eq!(q.status, QuestStatus::Active);

    // Snapshot at tick 20 for rollback test
    let journal_snapshot_at_tick_20 = journal.clone();
    let hash_at_tick_20 = journal.compute_hash();

    // 4. Advance into future: Defeat 3rd bandit at tick 30 -> ObjectiveComplete
    journal.notify_enemy_defeated(99);
    assert_eq!(journal.get_quest(501).unwrap().status, QuestStatus::ObjectiveComplete);

    // Turn in quest to Elder at tick 40
    let turned_in = journal.turn_in_quest(501, 40).expect("Turn in must succeed");
    assert_eq!(turned_in.status, QuestStatus::TurnedIn);
    assert_eq!(journal.total_xp_earned, 500);
    assert_eq!(journal.active_count, 0);
    assert!(journal.is_completed(501));
    assert_ne!(journal.compute_hash(), hash_at_tick_20);

    // 5. Time-Travel Rollback to Tick 20!
    let rolled_back_journal = journal_snapshot_at_tick_20;

    // Verifications on rolled back journal:
    assert_eq!(rolled_back_journal.active_count, 1);
    assert_eq!(rolled_back_journal.total_xp_earned, 0);
    assert!(!rolled_back_journal.is_completed(501));

    let rolled_back_quest = rolled_back_journal.get_quest(501).unwrap();
    assert_eq!(rolled_back_quest.current_count, 2);
    assert_eq!(rolled_back_quest.status, QuestStatus::Active);

    // 100% Bit-identical Hash Match
    assert_eq!(
        rolled_back_journal.compute_hash(),
        hash_at_tick_20,
        "Rolled back journal hash must match tick 20 hash bit-for-bit"
    );
}
