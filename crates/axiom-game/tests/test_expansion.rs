use axiom_core::dlc::DlcId;
use axiom_game::expansion::{ExpansionBoss, ExpansionTracker};

#[test]
fn test_expansion_tracker_64b_alignment() {
    assert_eq!(core::mem::size_of::<ExpansionTracker>(), 64);
    assert_eq!(core::mem::align_of::<ExpansionTracker>(), 64);

    let tracker = ExpansionTracker::default();
    assert_eq!(tracker.active_bosses().len(), 0);
}

#[test]
fn test_expansion_boss_dlc_unlock_flow() {
    let mut tracker = ExpansionTracker::new(3910000, 1);
    assert!(!tracker.is_boss_unlocked(ExpansionBoss::Kronos));
    assert!(!tracker.is_boss_unlocked(ExpansionBoss::Aether));

    // Unlock DLC 1 (Titan Chronicles)
    tracker.unlock_dlc(DlcId::TitanChronicles, 1729700000);
    assert!(tracker.is_boss_unlocked(ExpansionBoss::Kronos));
    assert!(!tracker.is_boss_unlocked(ExpansionBoss::Aether));

    let bosses = tracker.active_bosses();
    assert_eq!(bosses.len(), 1);
    assert_eq!(bosses[0], ExpansionBoss::Kronos);
    assert_eq!(bosses[0].base_hp(), 180);

    // Unlock DLC 2 (Void Frontier)
    tracker.unlock_dlc(DlcId::VoidFrontier, 1729700000);
    assert!(tracker.is_boss_unlocked(ExpansionBoss::Aether));

    let all_bosses = tracker.active_bosses();
    assert_eq!(all_bosses.len(), 2);
    assert_eq!(all_bosses[1], ExpansionBoss::Aether);
    assert_eq!(all_bosses[1].base_hp(), 140);
}
