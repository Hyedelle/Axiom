//! Comprehensive verification suite for the Axiom Steel The Show (STS) subsystem.
//!
//! Validates:
//! 1. 16-byte POD Part catalog, slots, and stats.
//! 2. Duplicate part synthesis & deterministic upgrade logic (+1 ATK, +1 DEF, -1 Heat).
//! 3. Robot loadout assembly and 3-set drive synergies (Steam, Gear, Hydraulic).
//! 4. 4-Robot Foundry League round-robin scheduler and standings tie-breaking.
//! 5. Playoff progression (1st/2nd Decider Match ➔ Title Match vs THESEUS ➔ Championship).
//! 6. Real-time season combat and inbound reverse-engineering draft.

use axiom_game::sts::{
    can_upgrade, upgrade_part, ContenderId, DriveTag, Part, PartId, PlayoffState,
    RobotAssembly, SetSynergy, Slot, StsLeague, StsPhase, StsSeasonEngine, UpgradeBonus,
};
use axiom_game::ActionKind;

#[test]
fn test_part_catalog_and_layout() {
    assert_eq!(core::mem::size_of::<Part>(), 16);

    let eye = PartId::EyeTarget.to_part();
    assert_eq!(eye.slot, Slot::Head);
    assert_eq!(eye.drive, DriveTag::Gear);
    assert_eq!(eye.tier, 1);
    assert_eq!(eye.effective_attack(), 12);
    assert_eq!(eye.effective_defense(), 1);
    assert_eq!(eye.effective_heat(), 4);

    let theseus_arm = PartId::ArmExecutioner.to_part();
    assert_eq!(theseus_arm.slot, Slot::Arm);
    assert_eq!(theseus_arm.drive, DriveTag::Steam);
    assert_eq!(theseus_arm.effective_attack(), 22);
}

#[test]
fn test_duplicate_part_synthesis_upgrade() {
    let mut base = PartId::ArmPiston.to_part();
    let mat = PartId::ArmPiston.to_part();
    let diff = PartId::ArmGuard.to_part();

    // Cannot upgrade with different part
    assert!(!can_upgrade(&base, &diff));

    // Can upgrade duplicate Tier 1 parts
    assert!(can_upgrade(&base, &mat));

    let pre_atk = base.effective_attack();
    let pre_def = base.effective_defense();
    let pre_heat = base.effective_heat();

    // Deterministic upgrade with seed 0 -> Attack +1
    let bonus = upgrade_part(&mut base, 0);
    assert_eq!(bonus, UpgradeBonus::AttackPlus1);
    assert_eq!(base.tier, 2);
    assert_eq!(base.effective_attack(), pre_atk + 1);
    assert_eq!(base.effective_defense(), pre_def);
    assert_eq!(base.effective_heat(), pre_heat);

    // Tier 2 cannot be upgraded again
    assert!(!can_upgrade(&base, &mat));

    // Deterministic upgrade with seed 1 -> Defense +1
    let mut base2 = PartId::ArmPiston.to_part();
    let bonus2 = upgrade_part(&mut base2, 1);
    assert_eq!(bonus2, UpgradeBonus::DefensePlus1);
    assert_eq!(base2.effective_defense(), pre_def + 1);

    // Deterministic upgrade with seed 2 -> Heat -1
    let mut base3 = PartId::ArmPiston.to_part();
    let bonus3 = upgrade_part(&mut base3, 2);
    assert_eq!(bonus3, UpgradeBonus::HeatMinus1);
    assert_eq!(base3.effective_heat(), pre_heat.saturating_sub(1));
}

#[test]
fn test_robot_assembly_stats_and_synergies() {
    assert_eq!(core::mem::align_of::<RobotAssembly>(), 64);

    // Starter ELPIS mecha
    let mut elpis = RobotAssembly::elpis_starter();
    assert!(elpis.total_attack() > 0);
    assert!(elpis.total_defense() > 0);
    assert!(elpis.total_heat() > 0);

    // Custom pure Steam assembly:
    // EyeRadar (Steam), CorePower (Steam), ArmPiston (Steam), LegSprint (Steam)
    let steam_bot = RobotAssembly::new(
        PartId::EyeRadar.to_part(),
        PartId::CorePower.to_part(),
        PartId::ArmPiston.to_part(),
        PartId::LegSprint.to_part(),
    );
    assert_eq!(steam_bot.drive_count(DriveTag::Steam), 4);
    assert_eq!(steam_bot.active_synergy(), SetSynergy::SteamBarrage);

    // Custom Hydraulic assembly (3 hydraulic parts):
    // EyeVisor (Hydraulic), CoreHeavy (Hydraulic), ArmGuard (Hydraulic), LegSprint (Steam)
    let hydro_bot = RobotAssembly::new(
        PartId::EyeVisor.to_part(),
        PartId::CoreHeavy.to_part(),
        PartId::ArmGuard.to_part(),
        PartId::LegSprint.to_part(),
    );
    assert_eq!(hydro_bot.drive_count(DriveTag::Hydraulic), 3);
    assert_eq!(hydro_bot.active_synergy(), SetSynergy::HydraulicDeflect);

    // Inventory addition and equipping
    let spare_arm = PartId::ArmHammer.to_part();
    assert!(elpis.add_to_inventory(spare_arm));
    assert_eq!(elpis.inventory_count, 1);

    // Equip from inventory (swaps with ArmPrecision)
    let pre_arm_id = elpis.arm.id;
    assert!(elpis.equip_from_inventory(0));
    assert_eq!(elpis.arm.id, PartId::ArmHammer);
    assert_eq!(elpis.inventory[0].unwrap().id, pre_arm_id);
}

#[test]
fn test_inventory_synthesis_mechanics() {
    let mut bot = RobotAssembly::elpis_starter();

    let p1 = PartId::CoreHeavy.to_part();
    let p2 = PartId::CoreHeavy.to_part();

    bot.add_to_inventory(p1);
    bot.add_to_inventory(p2);
    assert_eq!(bot.inventory_count, 2);

    let bonus = bot.synthesize_inventory(0, 1, 42);
    assert!(bonus.is_some());
    assert_eq!(bot.inventory_count, 1);
    assert_eq!(bot.inventory[0].unwrap().tier, 2);
    assert!(bot.inventory[1].is_none());
}

#[test]
fn test_4_robot_league_round_robin_and_standings() {
    let mut league = StsLeague::new();
    assert_eq!(league.matches.len(), 6);
    assert_eq!(league.playoff_state, PlayoffState::LeagueInProgress);

    // Round 1:
    // Match 0: Elpis vs Tinker -> Elpis wins 10-5 (KO)
    league.record_match_result(0, 10, 5, true, false);
    // Match 1: Cipher vs Pike -> Cipher wins 8-6
    league.record_match_result(1, 8, 6, false, false);

    // Round 2:
    // Match 2: Elpis vs Cipher -> Elpis wins 10-7 (KO)
    league.record_match_result(2, 10, 7, true, false);
    // Match 3: Tinker vs Pike -> Pike wins 9-4
    league.record_match_result(3, 4, 9, false, false);

    // Round 3:
    // Match 4: Elpis vs Pike -> Elpis wins 10-6
    league.record_match_result(4, 10, 6, false, false);
    // Match 5: Tinker vs Cipher -> Cipher wins 7-5
    league.record_match_result(5, 5, 7, false, false);

    // League concluded
    assert_eq!(league.current_match_idx, 6);
    assert_eq!(league.playoff_state, PlayoffState::DeciderReady);

    // Standings check:
    let sorted = league.sorted_standings();
    // Elpis: 3 wins (9 pts) -> 1st place!
    assert_eq!(sorted[0].contender, ContenderId::Elpis);
    assert_eq!(sorted[0].points, 9);
    assert_eq!(sorted[0].wins, 3);
    assert_eq!(sorted[0].kos, 2);

    // Cipher: 2 wins, 1 loss (6 pts) -> 2nd place!
    assert_eq!(sorted[1].contender, ContenderId::Cipher);
    assert_eq!(sorted[1].points, 6);
}

#[test]
fn test_playoff_ladder_championship_victory() {
    let mut league = StsLeague::new();

    // Fast-forward league with Elpis dominating
    league.record_match_result(0, 10, 5, true, false);
    league.record_match_result(1, 8, 6, false, false);
    league.record_match_result(2, 10, 7, true, false);
    league.record_match_result(3, 4, 9, false, false);
    league.record_match_result(4, 10, 6, false, false);
    league.record_match_result(5, 5, 7, false, false);

    assert_eq!(league.playoff_state, PlayoffState::DeciderReady);

    // Decider Match: Elpis vs Cipher -> Elpis wins!
    league.record_decider_result(ContenderId::Elpis);
    assert_eq!(league.playoff_state, PlayoffState::TitleMatchReady);

    // Title Match: Elpis vs THESEUS -> Elpis wins the Championship!
    league.record_title_result(ContenderId::Elpis);
    assert_eq!(league.playoff_state, PlayoffState::ChampionshipWon);
}

#[test]
fn test_playoff_ladder_elimination_defeat() {
    let mut league = StsLeague::new();

    // Fast-forward league where Elpis loses all matches
    league.record_match_result(0, 3, 10, false, true);
    league.record_match_result(1, 8, 6, false, false);
    league.record_match_result(2, 4, 10, false, true);
    league.record_match_result(3, 4, 9, false, false);
    league.record_match_result(4, 5, 10, false, false);
    league.record_match_result(5, 5, 7, false, false);

    // Elpis is 4th place -> Direct Elimination (Game Over)
    assert_eq!(league.playoff_state, PlayoffState::EliminatedDefeat);
}

#[test]
fn test_sts_season_engine_combat_and_inbound_draft() {
    let mut engine = StsSeasonEngine::new(12345);
    assert_eq!(engine.phase, StsPhase::Garage);

    // Start Round 1 match against TINKER
    engine.start_match(ContenderId::Tinker);
    assert_eq!(engine.phase, StsPhase::Combat);

    // Step frames delivering punches
    for _ in 0..120 {
        engine.step_frame(ActionKind::Jab);
    }
    assert!(engine.ring.opponent.hp < engine.ring.opponent.max_hp);

    // Force KO on opponent to test match conclusion and inbound drafting
    engine.ring.opponent.hp = 0;
    engine.step_frame(ActionKind::Idle);

    assert_eq!(engine.phase, StsPhase::InboundDraft);
    assert_eq!(engine.matches_won, 1);

    // Verify 3 inbound choices were offered
    let draft = engine.draft;
    let mut valid_choices = 0;
    for choice in draft.choices {
        if choice.is_some() {
            valid_choices += 1;
        }
    }
    assert_eq!(valid_choices, 3);

    // Draft choice 0 into inventory
    let pre_inv_count = engine.player.inventory_count;
    assert!(engine.select_inbound_part(0));
    assert_eq!(engine.player.inventory_count, pre_inv_count + 1);

    // Returned to Garage for next preparations
    assert_eq!(engine.phase, StsPhase::Garage);
}
