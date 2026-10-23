use axiom_game::achievement::{
    AchievementId, AchievementState, AchievementTracker, TOTAL_ACHIEVEMENTS,
};

#[test]
fn test_achievement_state_memory_layout() {
    assert_eq!(core::mem::size_of::<AchievementState>(), 64);
    assert_eq!(core::mem::align_of::<AchievementState>(), 64);
}

#[test]
fn test_achievement_unlocks_and_checksum() {
    let mut tracker = AchievementTracker::new();
    assert_eq!(tracker.unlocked_count(), 0);
    assert!(tracker.is_valid());

    // 1. Centurion (100 punches)
    for _ in 0..99 {
        tracker.on_punch_thrown();
    }
    assert!(!tracker.is_unlocked(AchievementId::Centurion));
    tracker.on_punch_thrown();
    assert!(tracker.is_unlocked(AchievementId::Centurion));
    assert!(tracker.is_valid());

    // 2. IronWall and PerfectParry
    tracker.on_damage_mitigated(50);
    assert!(tracker.is_unlocked(AchievementId::PerfectParry));
    assert!(!tracker.is_unlocked(AchievementId::IronWall));
    tracker.on_damage_mitigated(60);
    assert!(tracker.is_unlocked(AchievementId::IronWall));

    // 3. CounterPuncher (3 in 1 match)
    tracker.on_match_start();
    tracker.on_counter_hit();
    tracker.on_counter_hit();
    assert!(!tracker.is_unlocked(AchievementId::CounterPuncher));
    tracker.on_counter_hit();
    assert!(tracker.is_unlocked(AchievementId::CounterPuncher));

    // 4. SteamOverdrive (finisher at heat >= 80)
    tracker.on_finisher_executed(79);
    assert!(!tracker.is_unlocked(AchievementId::SteamOverdrive));
    tracker.on_finisher_executed(85);
    assert!(tracker.is_unlocked(AchievementId::SteamOverdrive));

    // 5. Match conclusions: CleanSweep, FirstBlood, TitanSlayer, DeciderChamp
    tracker.on_match_concluded(true, true, true, true);
    assert!(tracker.is_unlocked(AchievementId::FirstBlood));
    assert!(tracker.is_unlocked(AchievementId::CleanSweep));
    assert!(tracker.is_unlocked(AchievementId::TitanSlayer));
    assert!(tracker.is_unlocked(AchievementId::DeciderChamp));

    // 6. WorkshopPioneer
    tracker.on_workshop_equipped();
    assert!(tracker.is_unlocked(AchievementId::WorkshopPioneer));

    // 7. ReplayArchivist
    tracker.on_replay_inspected();
    assert!(tracker.is_unlocked(AchievementId::ReplayArchivist));

    // 8. LabScholar
    tracker.on_lab_node_purchased(2);
    assert!(!tracker.is_unlocked(AchievementId::LabScholar));
    tracker.on_lab_node_purchased(3);
    assert!(tracker.is_unlocked(AchievementId::LabScholar));

    // All 12 achievements unlocked!
    assert_eq!(tracker.unlocked_count(), TOTAL_ACHIEVEMENTS);
    assert!(tracker.is_valid());

    // 9. Binary serialization roundtrip
    let raw = tracker.state.to_bytes();
    assert_eq!(raw.len(), 64);
    let restored = AchievementState::from_bytes(&raw).expect("Should deserialize valid state");
    assert_eq!(restored, tracker.state);

    // Verify tamper detection
    tracker.state.matches_won += 99;
    assert!(!tracker.is_valid());
}

#[test]
fn test_achievement_toast_lifecycle() {
    let mut tracker = AchievementTracker::new();
    tracker.on_workshop_equipped();

    // Check toast is present in queue, calling update_toast activates it
    assert!(tracker.active_toast.is_none());
    tracker.update_toast();
    let toast = tracker.active_toast.expect("Toast should be active");
    assert_eq!(toast.id, AchievementId::WorkshopPioneer);
    assert_eq!(toast.id.name(), "Workshop Pioneer");
    assert_eq!(toast.display_frames, 180);

    // Advance 1 frame
    tracker.update_toast();
    let toast = tracker.active_toast.unwrap();
    assert_eq!(toast.display_frames, 179);

    // Fast-forward until dismissed
    for _ in 0..180 {
        tracker.update_toast();
    }
    assert!(tracker.active_toast.is_none());
}
