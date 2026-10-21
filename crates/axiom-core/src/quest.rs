//! # Deterministic Quest & Dialogue State Machine (Phase 4 RPG System)
//!
//! Provides integer-only quest progression, dialogue graph transitions,
//! and objective tracking with zero heap allocations and 100% rollback compatibility.

use crate::hash::deterministic_hash;

pub const DEFAULT_MAX_ACTIVE_QUESTS: usize = 32;
pub const DEFAULT_MAX_COMPLETED_QUESTS: usize = 64;

/// Lifecycle state of an in-game quest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u8)]
pub enum QuestStatus {
    #[default]
    Unassigned = 0,
    Active = 1,
    ObjectiveComplete = 2,
    TurnedIn = 3,
    Failed = 4,
}

/// Category of quest objective to fulfill.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u8)]
pub enum ObjectiveType {
    #[default]
    TalkToNpc = 0,
    GatherItem = 1,
    DefeatEnemies = 2,
    ExploreArea = 3,
}

/// Compact 64-byte aligned quest record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(C, align(64))]
pub struct QuestRecord {
    pub quest_id: u32,
    pub status: QuestStatus,
    pub objective_type: ObjectiveType,
    pub target_id: u32,
    pub current_count: u32,
    pub required_count: u32,
    pub reward_xp: u32,
    pub reward_item: u8,
    pub reward_item_count: u8,
    pub start_tick: u64,
    pub finish_tick: u64,
    _padding: [u8; 16],
}

impl Default for QuestRecord {
    fn default() -> Self {
        Self::empty()
    }
}

impl QuestRecord {
    pub const fn empty() -> Self {
        Self {
            quest_id: 0,
            status: QuestStatus::Unassigned,
            objective_type: ObjectiveType::TalkToNpc,
            target_id: 0,
            current_count: 0,
            required_count: 0,
            reward_xp: 0,
            reward_item: 0,
            reward_item_count: 0,
            start_tick: 0,
            finish_tick: 0,
            _padding: [0; 16],
        }
    }

    pub fn new(
        quest_id: u32,
        objective_type: ObjectiveType,
        target_id: u32,
        required_count: u32,
        reward_xp: u32,
        reward_item: u8,
        reward_item_count: u8,
        tick: u64,
    ) -> Self {
        Self {
            quest_id,
            status: QuestStatus::Active,
            objective_type,
            target_id,
            current_count: 0,
            required_count,
            reward_xp,
            reward_item,
            reward_item_count,
            start_tick: tick,
            finish_tick: 0,
            _padding: [0; 16],
        }
    }

    /// Progresses objective count. Transitions to ObjectiveComplete if target reached.
    pub fn add_progress(&mut self, amount: u32) -> bool {
        if self.status != QuestStatus::Active {
            return false;
        }

        self.current_count = self.current_count.saturating_add(amount);
        if self.current_count >= self.required_count {
            self.current_count = self.required_count;
            self.status = QuestStatus::ObjectiveComplete;
            true
        } else {
            false
        }
    }
}

/// Action triggered upon selecting a dialogue branch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u8)]
pub enum DialogueAction {
    #[default]
    None = 0,
    AcceptQuest(u32) = 1,
    TurnInQuest(u32) = 2,
    Trade = 3,
    Exit = 4,
}

/// Single choice branch in a dialogue node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(C)]
pub struct DialogueChoice {
    pub choice_text_id: u32,
    pub next_node_id: u32,
    pub action: DialogueAction,
}

/// Dialogue node in an NPC dialogue graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(C)]
pub struct DialogueNode {
    pub node_id: u32,
    pub prompt_text_id: u32,
    pub choices: [DialogueChoice; 4],
    pub choices_count: u8,
}

/// Active conversation session between player and NPC.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct DialogueSession {
    pub speaker_npc_id: u32,
    pub current_node_id: u32,
    pub is_active: bool,
}

impl DialogueSession {
    pub const fn new() -> Self {
        Self {
            speaker_npc_id: 0,
            current_node_id: 0,
            is_active: false,
        }
    }

    pub fn start(&mut self, npc_id: u32, root_node_id: u32) {
        self.speaker_npc_id = npc_id;
        self.current_node_id = root_node_id;
        self.is_active = true;
    }

    pub fn close(&mut self) {
        self.is_active = false;
        self.speaker_npc_id = 0;
        self.current_node_id = 0;
    }
}

/// Flat journal tracking active quests and completed quest history.
#[derive(Debug, Clone, PartialEq, Eq)]
#[repr(C, align(64))]
pub struct QuestJournal<
    const MAX_ACTIVE: usize = DEFAULT_MAX_ACTIVE_QUESTS,
    const MAX_COMPLETED: usize = DEFAULT_MAX_COMPLETED_QUESTS,
> {
    pub active_quests: [QuestRecord; MAX_ACTIVE],
    pub completed_quest_ids: [u32; MAX_COMPLETED],
    pub active_count: usize,
    pub completed_count: usize,
    pub total_xp_earned: u32,
}

impl<const MAX_ACTIVE: usize, const MAX_COMPLETED: usize> Default
    for QuestJournal<MAX_ACTIVE, MAX_COMPLETED>
{
    fn default() -> Self {
        Self::new()
    }
}

impl<const MAX_ACTIVE: usize, const MAX_COMPLETED: usize> QuestJournal<MAX_ACTIVE, MAX_COMPLETED> {
    pub const fn new() -> Self {
        Self {
            active_quests: [QuestRecord::empty(); MAX_ACTIVE],
            completed_quest_ids: [0; MAX_COMPLETED],
            active_count: 0,
            completed_count: 0,
            total_xp_earned: 0,
        }
    }

    /// Accepts and registers a new quest into the journal.
    pub fn accept_quest(&mut self, quest: QuestRecord) -> bool {
        if quest.quest_id == 0 || self.active_count >= MAX_ACTIVE || self.is_completed(quest.quest_id) {
            return false;
        }

        // Avoid duplicate active quest
        if self.get_quest(quest.quest_id).is_some() {
            return false;
        }

        for slot in &mut self.active_quests {
            if slot.quest_id == 0 {
                *slot = quest;
                self.active_count += 1;
                return true;
            }
        }
        false
    }

    /// Checks if a quest is already completed.
    pub fn is_completed(&self, quest_id: u32) -> bool {
        for &id in &self.completed_quest_ids[0..self.completed_count] {
            if id == quest_id {
                return true;
            }
        }
        false
    }

    /// Retrieves active quest record by quest ID.
    pub fn get_quest(&self, quest_id: u32) -> Option<&QuestRecord> {
        for quest in &self.active_quests {
            if quest.quest_id == quest_id {
                return Some(quest);
            }
        }
        None
    }

    /// Retrieves mutable active quest record by quest ID.
    pub fn get_quest_mut(&mut self, quest_id: u32) -> Option<&mut QuestRecord> {
        for quest in &mut self.active_quests {
            if quest.quest_id == quest_id {
                return Some(quest);
            }
        }
        None
    }

    /// Dispatches NPC dialogue interaction to active quests.
    pub fn notify_npc_talk(&mut self, npc_id: u32) -> u32 {
        let mut completed_count = 0;
        for quest in &mut self.active_quests {
            if quest.status == QuestStatus::Active
                && quest.objective_type == ObjectiveType::TalkToNpc
                && quest.target_id == npc_id
            {
                if quest.add_progress(1) {
                    completed_count += 1;
                }
            }
        }
        completed_count
    }

    /// Dispatches item gathering to active quests.
    pub fn notify_item_gathered(&mut self, item_type: u8, count: u32) -> u32 {
        let mut completed_count = 0;
        for quest in &mut self.active_quests {
            if quest.status == QuestStatus::Active
                && quest.objective_type == ObjectiveType::GatherItem
                && quest.target_id == item_type as u32
            {
                if quest.add_progress(count) {
                    completed_count += 1;
                }
            }
        }
        completed_count
    }

    /// Dispatches enemy defeat to active quests.
    pub fn notify_enemy_defeated(&mut self, enemy_id: u32) -> u32 {
        let mut completed_count = 0;
        for quest in &mut self.active_quests {
            if quest.status == QuestStatus::Active
                && quest.objective_type == ObjectiveType::DefeatEnemies
                && quest.target_id == enemy_id
            {
                if quest.add_progress(1) {
                    completed_count += 1;
                }
            }
        }
        completed_count
    }

    /// Turns in a completed quest, grants rewards, and moves to completion archive.
    pub fn turn_in_quest(&mut self, quest_id: u32, tick: u64) -> Option<QuestRecord> {
        let mut target_idx = None;
        for (i, quest) in self.active_quests.iter().enumerate() {
            if quest.quest_id == quest_id && quest.status == QuestStatus::ObjectiveComplete {
                target_idx = Some(i);
                break;
            }
        }

        if let Some(idx) = target_idx {
            let mut record = self.active_quests[idx];
            record.status = QuestStatus::TurnedIn;
            record.finish_tick = tick;
            self.total_xp_earned = self.total_xp_earned.saturating_add(record.reward_xp);

            // Archive to completed list
            if self.completed_count < MAX_COMPLETED {
                self.completed_quest_ids[self.completed_count] = quest_id;
                self.completed_count += 1;
            }

            self.active_quests[idx] = QuestRecord::empty();
            self.active_count = self.active_count.saturating_sub(1);
            Some(record)
        } else {
            None
        }
    }

    /// Computes deterministic state hash across active quests, completions, and earned XP.
    pub fn compute_hash(&self) -> u64 {
        let mut h = (self.total_xp_earned as u64).wrapping_mul(0x9E3779B97F4A7C15);
        h ^= (self.active_count as u64).rotate_left(11);
        for quest in &self.active_quests {
            if quest.quest_id != 0 {
                h ^= deterministic_hash(quest);
                h = h.rotate_left(7);
            }
        }
        for &id in &self.completed_quest_ids[0..self.completed_count] {
            h ^= (id as u64).wrapping_mul(0xBF58476D1CE4E5B9);
        }
        h
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_quest_record_size() {
        assert_eq!(core::mem::size_of::<QuestRecord>(), 64);
        assert_eq!(core::mem::align_of::<QuestRecord>(), 64);
    }

    #[test]
    fn test_quest_journal_lifecycle() {
        let mut journal = QuestJournal::<32, 64>::new();

        // 1. Accept Gather Iron Ore Quest (ID: 101, Target Item: 1, Req: 5)
        let quest = QuestRecord::new(101, ObjectiveType::GatherItem, 1, 5, 250, 2, 1, 10);
        assert!(journal.accept_quest(quest));
        assert_eq!(journal.active_count, 1);

        // 2. Gather 3 ores -> not complete yet
        journal.notify_item_gathered(1, 3);
        let q = journal.get_quest(101).unwrap();
        assert_eq!(q.current_count, 3);
        assert_eq!(q.status, QuestStatus::Active);

        // 3. Gather 2 more ores -> objective complete
        journal.notify_item_gathered(1, 2);
        let q = journal.get_quest(101).unwrap();
        assert_eq!(q.current_count, 5);
        assert_eq!(q.status, QuestStatus::ObjectiveComplete);

        // 4. Turn in quest at tick 50
        let finished = journal.turn_in_quest(101, 50).expect("Turn-in must succeed");
        assert_eq!(finished.status, QuestStatus::TurnedIn);
        assert_eq!(journal.active_count, 0);
        assert_eq!(journal.completed_count, 1);
        assert!(journal.is_completed(101));
        assert_eq!(journal.total_xp_earned, 250);
    }
}
