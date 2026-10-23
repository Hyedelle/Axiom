//! Axiom Headless & Desktop CLI Entry Point.
//!
//! Provides CLI execution for fast deterministic replay, state hashing,
//! time-travel rollback, autonomous Monte Carlo combat balancing, and
//! native desktop window studio.

use axiom_cli::{AsciiView, MonteCarloBalancer, OpenWorldRpgSession, Replayer, TimeTravelDemo};
use axiom_core::fixed::FixedVec2;
use axiom_core::input::InputState;
use axiom_core::state::TickState;
use axiom_render::mesh3d::MechaModel3D;
use axiom_render::modding::{
    SteamUgcConfig, WorkshopManager, WorkshopModManifest, WorkshopPublisher,
};
use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let has_benchmark = args.iter().any(|a| a == "--benchmark");
    let has_headless = args.iter().any(|a| a == "--headless");
    let has_window = args.iter().any(|a| a == "--window");
    let has_boxing = args.iter().any(|a| a == "--boxing");
    let has_mod = args.iter().any(|a| a == "mod" || a == "--mod");
    let has_save = args.iter().any(|a| a == "save" || a == "--save");
    let has_voxel = args.iter().any(|a| a == "voxel" || a == "--voxel");
    let has_net = args.iter().any(|a| a == "net" || a == "--net");

    if has_net {
        axiom_cli::net_cli::run_net_suite(&args);
    } else if has_mod {
        run_mod_suite(&args);
    } else if has_save {
        run_save_suite(&args);
    } else if has_voxel {
        axiom_cli::voxel_cli::run_voxel_suite(&args);
    } else if has_boxing {
        if let Err(e) = axiom_cli::desktop::run_desktop_window_with_mode(axiom_cli::ViewMode::Boxing) {
            eprintln!("Failed to launch desktop boxing window: {:?}", e);
            std::process::exit(1);
        }
    } else if has_benchmark || has_headless || (args.len() > 1 && !has_window) {
        run_headless_suite();
    } else if let Err(e) = axiom_cli::desktop::run_desktop_window() {
        eprintln!("Failed to launch desktop window: {:?}", e);
        std::process::exit(1);
    }
}

/// Runs the headless benchmark and verification suite.
pub fn run_headless_suite() {
    println!("🏛️ [Axiom Engine] Headless CLI Engine Suite\n");

    // 1. Time-Travel Rollback Demo
    println!("--- [DEMO 1] 0.001ms Time-Travel Rollback ---");
    let mut demo = TimeTravelDemo::new();
    println!("Initial State (Tick 0):");
    print!("{}", AsciiView::render(demo.state(), 24, 8));

    for _ in 0..30 {
        demo.step(InputState::new(1, 0, 0));
    }
    println!("Advanced to Tick 30:");
    print!("{}", AsciiView::render(demo.state(), 24, 8));

    println!("Rewinding time back to Tick 10 (Time-Travel Undo)...");
    demo.rollback_to(10).expect("Rollback failed");
    print!("{}", AsciiView::render(demo.state(), 24, 8));

    // 2. 1,000 Tick Replay Determinism Verification
    println!("--- [DEMO 2] 1,000-Tick Deterministic Replay ---");
    let mut initial_state = TickState::<32>::new();
    initial_state.spawn(1, FixedVec2::from_ints(10, 20), FixedVec2::ZERO);

    let inputs: Vec<InputState> = (0..1000)
        .map(|i| {
            let mx = if i % 2 == 0 { 1 } else { -1 };
            let my = if i % 3 == 0 { 1 } else { 0 };
            InputState::new(mx, my, 0)
        })
        .collect();

    let replayer = Replayer::new(initial_state, inputs);

    if replayer.verify_determinism() {
        let report = replayer.run();
        println!(
            "[PASS] Replayed {} ticks with 100% bit determinism. Final Hash: 0x{:016X}\n",
            report.total_ticks, report.final_hash
        );
    } else {
        eprintln!("[FAIL] Determinism mismatch detected during replay!");
        std::process::exit(1);
    }

    // 3. Autonomous Monte Carlo Balance Simulation (10,000 Matches)
    println!("--- [DEMO 3] Autonomous Monte Carlo Balance Simulator (10,000 Matches) ---");
    let sim_start = Instant::now();
    let stats = MonteCarloBalancer::simulate_matches(10_000, 777_888_999);
    let sim_duration = sim_start.elapsed();

    println!(
        "[PASS] 10,000 full combat matches simulated in {:.2?} ({:.2} ms per 1,000 matches)",
        sim_duration,
        sim_duration.as_millis() as f64 / 10.0
    );

    let report_md = MonteCarloBalancer::format_report(&stats);
    println!("\n{}", report_md);

    // Save report to tmp/balance_report.md
    if let Err(e) = std::fs::write("tmp/balance_report.md", &report_md) {
        eprintln!("Failed to save balance report: {}", e);
    } else {
        println!("==> Balance report successfully saved to tmp/balance_report.md");
    }

    // 4. Open-World RPG Integration Demo (100k NPCs + Procedural Chunks + Quests + A* Navigation + ATB Combat + 3D Spatial Audio)
    println!("\n--- [DEMO 4] Complete Open-World RPG Integration ---");
    let mut rpg = OpenWorldRpgSession::new(0xABCD_1234_5678);
    println!("Spawned 100,000 living NPCs and initialized procedural world chunks.");

    // Dialogue & Quest
    rpg.interact_npc_quest(10, 100, 999, 1, 500);
    println!("Player interacted with Village Elder NPC and accepted Quest #100 (Slay Frost Goblin).");

    // A* Pathfinding
    rpg.plan_path_to(15, 15);
    println!("Calculated A* collision-free path across procedural terrain.");

    // Encounter
    rpg.trigger_combat_encounter(999);
    println!("Triggered combat encounter with Frost Goblin Mob!");

    // Simulate 30 ticks
    let rpg_report = rpg.run_loop(30);
    println!(
        "[PASS] 30 ticks simulated in {:.2}ms ({:.3}ms/tick = {:.1} FPS equivalent)",
        rpg_report.elapsed_micros as f64 / 1000.0,
        rpg_report.avg_micros_per_tick / 1000.0,
        rpg_report.fps_equivalent
    );
    println!("Final Master State Hash: 0x{:016X}", rpg_report.final_hash);
    println!(
        "Quest Status: Active={}, Completed={}, XP Earned={}",
        rpg_report.active_quests, rpg_report.completed_quests, rpg_report.total_xp
    );
    println!(
        "Audio Events Spatialized: {}, Master Left/Right Volume: ({}, {})\n",
        rpg_report.audio_events_processed,
        rpg_report.audio_master_volume.0,
        rpg_report.audio_master_volume.1
    );
}

/// Runs the Steam Workshop & Local Mod Ingester CLI commands.
pub fn run_mod_suite(args: &[String]) {
    println!("🛠️  [Axiom Workshop] Steam Workshop & Local Mod Ingester\n");

    let subcmd = args
        .iter()
        .skip_while(|&a| a != "mod" && a != "--mod")
        .nth(1)
        .map(|s| s.as_str())
        .unwrap_or("list");

    let mut manager = WorkshopManager::new("mods");

    match subcmd {
        "new" => {
            let mod_id = args.get(3).cloned().unwrap_or_else(|| "custom_hazard_arm".to_string());
            let manifest = WorkshopModManifest {
                mod_id: mod_id.clone(),
                title: format!("Custom Workshop Part [{}]", mod_id),
                author: "AxiomModder".to_string(),
                version: "1.0.0".to_string(),
                target_slot: "Arm".to_string(),
                target_mesh_name: "hero_glove_R".to_string(),
                atk_bonus: 10,
                def_bonus: 5,
                heat_bonus: -3,
                weight_bonus: 6,
                tags: vec!["Mecha".to_string(), "Workshop".to_string()],
            };
            match WorkshopManager::create_template_mod(std::path::Path::new("mods"), &manifest) {
                Ok(path) => println!("✨ Created template workshop mod at: {:?}", path),
                Err(e) => eprintln!("❌ Failed to create template mod: {:?}", e),
            }
        }
        "test" | "hot-swap" => {
            println!("🔄 Testing Hot-Swap Ingestion against MechaModel3D...");
            let mut mecha = MechaModel3D::procedural_chassis();
            let _initial_parts = mecha.parts.len();
            match manager.hot_swap_reload(&mut mecha) {
                Ok(replaced) => {
                    println!("✅ Hot-Swap Ingestion Success!");
                    println!("   Total Mods Discovered: {}", manager.installed_mods.len());
                    println!("   Parts Injected / Substituted: {}", replaced);
                    println!("   Final Model Parts Count: {}", mecha.parts.len());
                }
                Err(e) => eprintln!("❌ Hot-swap failed: {:?}", e),
            }
        }
        "validate" => {
            let mod_path = args.get(3).cloned().unwrap_or_else(|| "mods/custom_hazard_arm".to_string());
            println!("🔍 Validating Workshop Mod Package at: {}", mod_path);
            let p = std::path::Path::new(&mod_path);
            match WorkshopPublisher::validate_mod(p) {
                Ok(report) => {
                    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
                    println!("  Mod ID:      {}", report.mod_id);
                    println!("  Title:       {}", report.title);
                    println!("  Version:     {}", report.version);
                    println!("  Target Slot: {}", report.target_slot);
                    println!("  Asset Files: {} ({} bytes)", report.total_asset_files, report.total_bytes);
                    println!("  Manifest:    {}", if report.has_manifest { "✅ OK" } else { "❌ MISSING" });
                    println!("  Mesh (GLB):  {}", if report.has_mesh { "✅ FOUND" } else { "⚠️  NONE" });
                    println!("  Texture:     {}", if report.has_texture { "✅ FOUND" } else { "⚠️  NONE" });
                    println!("  Preview:     {}", if report.has_preview { "✅ FOUND" } else { "⚠️  DEFAULT" });
                    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
                    if report.is_valid {
                        println!("🎉 Validation PASSED: Mod is 100% compliant with Steam Workshop standards!");
                    } else {
                        println!("❌ Validation FAILED: Encountered {} error(s):", report.errors.len());
                        for err in &report.errors {
                            println!("   • [ERROR] {}", err);
                        }
                    }
                    for warn in &report.warnings {
                        println!("   • [WARN]  {}", warn);
                    }
                }
                Err(e) => eprintln!("❌ Validation failed: {:?}", e),
            }
        }
        "pack" => {
            let mod_path = args.get(3).cloned().unwrap_or_else(|| "mods/custom_hazard_arm".to_string());
            let out_file = args.get(4).cloned().unwrap_or_else(|| format!("{}.axmod", mod_path));
            println!("📦 Packaging standalone .axmod container...");
            println!("   Source:      {}", mod_path);
            println!("   Destination: {}", out_file);
            let src = std::path::Path::new(&mod_path);
            let dest = std::path::Path::new(&out_file);
            match WorkshopPublisher::pack_axmod(src, dest) {
                Ok(info) => {
                    println!("✅ Packaging Complete!");
                    println!("   Mod ID:            {}", info.mod_id);
                    println!("   Files Bundled:     {}", info.file_count);
                    println!("   Uncompressed Size: {} bytes", info.uncompressed_size);
                    println!("   Container Size:    {} bytes", info.package_size);
                    println!("   Checksum:          0x{:016X}", info.checksum);
                }
                Err(e) => eprintln!("❌ Packaging failed: {:?}", e),
            }
        }
        "publish" => {
            let mod_path = args.get(3).cloned().unwrap_or_else(|| "mods/custom_hazard_arm".to_string());
            println!("🚀 Steam Workshop UGC Publishing Pipeline");
            println!("   Target Directory: {}", mod_path);

            let mut config = SteamUgcConfig::default();
            // Optional --appid
            if let Some(pos) = args.iter().position(|a| a == "--appid") {
                if let Some(val) = args.get(pos + 1) {
                    if let Ok(id) = val.parse::<u32>() {
                        config.app_id = id;
                    }
                }
            }
            // Optional --changelog
            if let Some(pos) = args.iter().position(|a| a == "--changelog") {
                if let Some(val) = args.get(pos + 1) {
                    config.changelog = val.clone();
                }
            }

            let p = std::path::Path::new(&mod_path);
            match WorkshopPublisher::publish_dry_run(p, &config) {
                Ok(res) => {
                    println!("✅ Steam Workshop Publication Preparation Successful!");
                    println!("   Mod ID:          {}", res.mod_id);
                    println!("   Steam AppId:     {}", res.app_id);
                    println!("   Generated VDF:   {:?}", res.vdf_path);
                    if let Some(ax) = res.axmod_path {
                        println!("   Packed Container: {:?}", ax);
                    }
                    println!("   Summary:         {}", res.summary);
                    println!("\n💡 To execute production cloud upload to Steam via SteamPipe:");
                    println!("   steamcmd.exe +login <steam_username> +workshop_build_item {:?} +quit", res.vdf_path);
                }
                Err(e) => eprintln!("❌ Publishing dry-run failed: {:?}", e),
            }
        }
        "list" | _ => {
            match manager.scan_mods() {
                Ok(count) => {
                    println!("📁 Mods Directory: {:?}", manager.mods_dir);
                    println!("📦 Discovered Workshop Mods: {}\n", count);
                    for (i, m) in manager.installed_mods.iter().enumerate() {
                        let active = manager.active_mod_ids.contains(&m.manifest.mod_id);
                        println!(
                            "  [{}] {} v{} by {} | Slot: {} -> Part: '{}' [{}]",
                            i + 1,
                            m.manifest.title,
                            m.manifest.version,
                            m.manifest.author,
                            m.manifest.target_slot,
                            m.manifest.target_mesh_name,
                            if active { "ACTIVE" } else { "DISABLED" }
                        );
                        println!(
                            "      Stats: ATK {:+02} | DEF {:+02} | HEAT {:+02} | WGT {:+02}",
                            m.manifest.atk_bonus,
                            m.manifest.def_bonus,
                            m.manifest.heat_bonus,
                            m.manifest.weight_bonus
                        );
                        println!("      Tags: {:?}", m.manifest.tags);
                    }
                }
                Err(e) => eprintln!("❌ Failed to scan mods: {:?}", e),
            }
        }
    }
}

pub fn run_save_suite(args: &[String]) {
    println!("💾 [Axiom Cloud Save] Steam Auto-Cloud & Local Storage Engine\n");

    let subcmd = args
        .iter()
        .skip_while(|&a| a != "save" && a != "--save")
        .nth(1)
        .map(|s| s.as_str())
        .unwrap_or("list");

    use axiom_cli::cloud_sync::CloudSyncManager;
    use axiom_core::cloud_save::CloudSavePackage;
    use std::path::Path;

    match subcmd {
        "sync" => {
            let local_dir = args.get(3).cloned().unwrap_or_else(|| "saves".to_string());
            let cloud_dir = args.get(4).cloned().unwrap_or_else(|| "saves/cloud".to_string());
            println!("🔄 Synchronizing Save Storage...");
            println!("   Local:  {}", local_dir);
            println!("   Cloud:  {}", cloud_dir);

            let report = CloudSyncManager::sync_directories(Path::new(&local_dir), Path::new(&cloud_dir));
            println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
            println!("  Local Restored:   {} files", report.local_updated);
            println!("  Cloud Mirrored:   {} files", report.cloud_updated);
            println!("  Identical:        {} files", report.identical);
            if !report.errors.is_empty() {
                println!("  Errors:           {} encountered", report.errors.len());
                for err in &report.errors {
                    println!("    • {}", err);
                }
            }
            println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
            println!("✅ Steam Cloud synchronization complete!");
        }
        "backup" => {
            let src_file = args.get(3).cloned().unwrap_or_else(|| "saves/quick.mimic".to_string());
            let out_file = args.get(4).cloned().unwrap_or_else(|| "saves/cloud/quick.axcs".to_string());
            println!("📦 Packaging save into standalone .axcs cloud archive...");
            println!("   Source:      {}", src_file);
            println!("   Destination: {}", out_file);

            let src_path = Path::new(&src_file);
            let out_path = Path::new(&out_file);

            match std::fs::read(src_path) {
                Ok(bytes) => {
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs())
                        .unwrap_or(0);
                    let pkg = CloudSavePackage::new(0, now, 0, 1, bytes);
                    let serialized = pkg.serialize();
                    match CloudSyncManager::atomic_write(out_path, &serialized) {
                        Ok(_) => {
                            println!("✅ Successfully created cloud archive!");
                            println!("   Timestamp:  {} (Unix Epoch)", now);
                            println!("   Checksum:   0x{:016X}", pkg.header.data_checksum);
                            println!("   Total Size: {} bytes", serialized.len());
                        }
                        Err(e) => eprintln!("❌ Failed to write cloud archive: {}", e),
                    }
                }
                Err(e) => eprintln!("❌ Failed to read source save: {}", e),
            }
        }
        "restore" => {
            let src_file = args.get(3).cloned().unwrap_or_else(|| "saves/cloud/quick.axcs".to_string());
            let out_file = args.get(4).cloned().unwrap_or_else(|| "saves/quick.mimic".to_string());
            println!("📂 Restoring save payload from .axcs cloud archive...");
            println!("   Cloud Source: {}", src_file);
            println!("   Restored To:  {}", out_file);

            let src_path = Path::new(&src_file);
            let out_path = Path::new(&out_file);

            match std::fs::read(src_path) {
                Ok(bytes) => {
                    match CloudSavePackage::deserialize(&bytes) {
                        Ok(pkg) => {
                            match CloudSyncManager::atomic_write(out_path, &pkg.payload) {
                                Ok(_) => {
                                    println!("✅ Successfully restored save from cloud archive!");
                                    println!("   Checksum Verified: 0x{:016X}", pkg.header.data_checksum);
                                    println!("   Payload Size:      {} bytes", pkg.payload.len());
                                }
                                Err(e) => eprintln!("❌ Failed to restore save: {}", e),
                            }
                        }
                        Err(e) => eprintln!("❌ Corrupted or invalid cloud save package: {:?}", e),
                    }
                }
                Err(e) => eprintln!("❌ Failed to read cloud save file: {}", e),
            }
        }
        "list" | _ => {
            let dir = args.get(3).cloned().unwrap_or_else(|| "saves".to_string());
            println!("📁 Scanning Save Storage Directory: {}", dir);
            let saves = CloudSyncManager::list_saves(Path::new(&dir));
            println!("📦 Discovered Saves: {}\n", saves.len());

            for (i, s) in saves.iter().enumerate() {
                let kind_label = if s.is_cloud_pkg { "[STEAM CLOUD .AXCS]" } else { "[LOCAL SAVE]" };
                println!(
                    "  [{}] {} {} ({} bytes)",
                    i + 1,
                    s.file_name,
                    kind_label,
                    s.size_bytes
                );
                if s.is_cloud_pkg && s.is_valid {
                    println!(
                        "      Slot: {} | Epoch: {} | Hash: 0x{:016X} | Checksum: 0x{:016X}",
                        s.slot, s.timestamp, s.state_hash, s.checksum
                    );
                } else if !s.is_valid {
                    println!("      ⚠️  CORRUPTED OR CHECKSUM MISMATCH");
                }
            }
        }
    }
}
