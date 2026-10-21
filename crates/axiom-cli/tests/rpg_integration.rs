//! End-to-End Open-World RPG Integration Tests (T-026).
//!
//! Validates seamless interoperation of all Axiom engine subsystems:
//! 1. Infinite procedural chunk streaming
//! 2. 100,000 living NPC simulation
//! 3. Quest state machine and NPC dialogue
//! 4. 2D A* pathfinding and obstacle navigation
//! 5. ATB combat with elemental mechanics & status effects
//! 6. 3D spatial audio simulator
//! 7. 6.13MB binary save / restore with bit-identical hash recovery
//! 8. Absolute cross-session determinism

use axiom_cli::rpg_demo::OpenWorldRpgSession;
use std::thread;

const SEED: u64 = 0x1234_5678_9ABC_DEF0;

#[test]
fn test_open_world_rpg_full_gameplay_lifecycle() {
    // Run in a thread with 16MB stack to safely house the test harness
    let builder = thread::Builder::new()
        .name("rpg_test".into())
        .stack_size(16 * 1024 * 1024);

    let handle = builder
        .spawn(|| {
            let mut session = OpenWorldRpgSession::new(SEED);

            // 1. Initial State Assertions
            assert_eq!(session.npcs.len(), 100_000);
            assert!(session.chunk_mgr.loaded_count() > 0);
            assert_eq!(session.tick, 0);

            // 2. Dialogue & Quest Acceptance
            // Accept Quest #100: Defeat Goblin (target 999), required 1, reward 500 XP
            let accepted = session.interact_npc_quest(10, 100, 999, 1, 500);
            assert!(accepted, "Quest acceptance must succeed");
            assert_eq!(session.journal.active_count, 1);

            // 3. A* Pathfinding AI Navigation
            // Find a walkable destination around (15, 15)
            let mut target_x = 15;
            let target_y = 15;
            while !session.chunk_mgr.tile_at(target_x as i64, target_y as i64).is_walkable() {
                target_x += 1;
            }
            let path_found = session.plan_path_to(target_x, target_y);
            assert!(path_found, "A* pathfinding must compute walkable route");
            let path = session.current_path.as_ref().unwrap();
            assert!(path.len > 0, "Waypoints must be non-empty");

            // 4. Trigger Combat Encounter with Goblin Mob (ID: 999)
            session.trigger_combat_encounter(999);
            assert!(session.in_combat, "Session should be in combat mode");

            // 5. Run Gameplay Simulation Loop (30 ticks)
            let report = session.run_loop(30);
            assert_eq!(report.ticks_simulated, 30);
            assert!(report.combat_damage > 0, "Combat damage should be dealt");
            assert!(report.audio_events_processed > 0, "Audio events should be spatialized");

            // 6. Verify Combat Resolution & Quest Auto-Turnin
            // Enemy goblin has 250 HP and takes ~75+ damage per strike with elemental bonus
            // Within 30 ticks, hero defeats goblin, turns in quest, and receives 500 XP
            assert!(!session.in_combat, "Combat must resolve in hero's victory");
            assert_eq!(session.journal.active_count, 0);
            assert_eq!(session.journal.completed_count, 1);
            assert_eq!(session.journal.total_xp_earned, 500);

            // 7. Lossless Binary Save / Advance / Restore Verification
            let saved_hash = session.compute_master_hash();
            let saved_bytes = session.save_binary();
            assert!(saved_bytes.len() > 6_000_000, "Save binary must contain 100k NPCs (~6.13MB)");

            // Advance 50 more ticks: state shifts
            session.run_loop(50);
            assert_eq!(session.tick, 80);
            let advanced_hash = session.compute_master_hash();
            assert_ne!(saved_hash, advanced_hash, "State hash must change across ticks");

            // Restore from binary archive
            let header = session.load_binary(&saved_bytes).expect("World loading must succeed");
            assert_eq!(header.tick, 30);
            assert_eq!(session.tick, 30);
            assert_eq!(session.journal.total_xp_earned, 500);

            // Verify 100% bit-for-bit state recovery
            let restored_hash = session.compute_master_hash();
            assert_eq!(
                saved_hash, restored_hash,
                "Restored state hash MUST match save-point hash with 100% bit-for-bit identity!"
            );
        })
        .unwrap();

    handle.join().unwrap();
}

#[test]
fn test_open_world_rpg_cross_session_determinism() {
    // Run two identical sessions from scratch and assert bit-for-bit hash equality
    let builder = thread::Builder::new()
        .name("determinism_test".into())
        .stack_size(16 * 1024 * 1024);

    let handle = builder
        .spawn(|| {
            let mut session_a = OpenWorldRpgSession::new(SEED);
            let mut session_b = OpenWorldRpgSession::new(SEED);

            // Execute identical actions on both sessions
            session_a.interact_npc_quest(10, 100, 999, 1, 500);
            session_b.interact_npc_quest(10, 100, 999, 1, 500);

            session_a.plan_path_to(10, 10);
            session_b.plan_path_to(10, 10);

            session_a.trigger_combat_encounter(999);
            session_b.trigger_combat_encounter(999);

            let report_a = session_a.run_loop(25);
            let report_b = session_b.run_loop(25);

            assert_eq!(
                report_a.final_hash, report_b.final_hash,
                "Independent RPG sessions with identical seeds and inputs MUST produce identical hashes!"
            );
            assert_eq!(report_a.combat_damage, report_b.combat_damage);
            assert_eq!(report_a.audio_master_volume, report_b.audio_master_volume);
        })
        .unwrap();

    handle.join().unwrap();
}
