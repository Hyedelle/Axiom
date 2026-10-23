use axiom_core::dlc::{DlcId, DlcState};

#[test]
fn test_dlc_state_64b_alignment() {
    assert_eq!(core::mem::size_of::<DlcState>(), 64);
    assert_eq!(core::mem::align_of::<DlcState>(), 64);

    let state = DlcState::default();
    assert_eq!(state.owned_mask, 0);
    assert_eq!(state.active_mask, 0);
    assert!(state.verify_integrity());
}

#[test]
fn test_dlc_unlock_and_verify() {
    let mut state = DlcState::new(3910000, 100);
    assert!(!state.is_owned(DlcId::TitanChronicles));
    assert!(!state.is_active(DlcId::TitanChronicles));

    // Unlock DLC 1
    state.unlock(DlcId::TitanChronicles, 1729700000);
    assert!(state.is_owned(DlcId::TitanChronicles));
    assert!(state.is_active(DlcId::TitanChronicles));
    assert!(state.verify_integrity());

    // Toggle active state
    assert!(state.set_active(DlcId::TitanChronicles, false));
    assert!(state.is_owned(DlcId::TitanChronicles));
    assert!(!state.is_active(DlcId::TitanChronicles));
    assert!(state.verify_integrity());

    // Unlock DLC 2
    state.unlock(DlcId::VoidFrontier, 1729700000);
    assert!(state.is_owned(DlcId::VoidFrontier));
    assert!(state.is_active(DlcId::VoidFrontier));
    assert!(state.verify_integrity());

    // Requirements check
    let required = DlcId::TitanChronicles.bitmask() | DlcId::VoidFrontier.bitmask();
    assert!(state.meets_requirements(required));

    let unowned_req = required | DlcId::GoldenDecals.bitmask();
    assert!(!state.meets_requirements(unowned_req));
}

#[test]
fn test_dlc_anti_tamper_signature() {
    let mut state = DlcState::new(3910000, 100);
    state.unlock(DlcId::TitanChronicles, 1000);
    assert!(state.verify_integrity());

    // Maliciously modify owned_mask directly without updating signature_hash
    state.owned_mask |= DlcId::VoidFrontier.bitmask();
    assert!(!state.verify_integrity(), "Tampered state must fail verification");
}
