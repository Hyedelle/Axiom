//! # Axiom WASM Web Viewer Bridge (`axiom-wasm`)
//!
//! Exposes zero-copy C-ABI interfaces for WebAssembly execution in the browser.
//! Allows running 100% deterministic simulations and rendering in 60fps canvas.

pub mod audio;

extern crate alloc;

use alloc::boxed::Box;
use axiom_core::audio::{SoundCategory, SoundEvent, SpatialAudioSimulator, DEFAULT_MAX_VOICES};
use axiom_core::chunk::ChunkManager;
use axiom_core::combat::BattleArena;
use axiom_core::effector::{AtbArena, AtbUnit, Element, StatusKind, MAX_ACTIVE_UNITS};
use axiom_core::fixed::{Fixed, FixedVec2};
use axiom_core::grid_netcode::GridRollbackSession;
use axiom_core::input::InputState;
use axiom_core::netcode::RollbackSession;
use axiom_core::npc_crowd::NpcCrowdPool;
use axiom_core::pathfinding::{Path, PathFinder, DEFAULT_GRID_H, DEFAULT_GRID_W, MAX_PATH_WAYPOINTS};
use axiom_core::pcg::TileMap;
use axiom_core::quest::{ObjectiveType, QuestJournal, QuestRecord, QuestStatus};
use axiom_core::schema::{RecipeDefinition, RecipeRegistry};
use axiom_core::state::TickState;
pub use axiom_core::tile_entity::grid_action_kind;
use axiom_core::tile_entity::{tile_flags, GridAction, TileCell, TileGrid};
use axiom_game::boxing::{ActionKind, AiPhase, BoxingEvent, BoxingRingEngine, FightPhase};
use axiom_game::boxing_netcode::{BoxingRollbackSession, NetboxWirePacket, SpectatorRelaySession};
use axiom_game::lobby::{
    LeaderboardTable, LobbyRole, SteamLobby, MAX_LOBBY_MEMBERS,
};
use axiom_game::tournament::{
    TournamentBroadcastTelemetry, TournamentDirector, TournamentEngine, TournamentFighter,
};
use axiom_core::voxel_collapse::DynamicCollapseSystem;
use axiom_core::voxel_destruct::VoxelVolume16;
use axiom_core::voxel_fluid::VoxelFluidField16;
use axiom_core::voxel_shard::VoxelShardPool;
use axiom_render::cutin::{render_cutin_overlay, CutinKind};
use axiom_render::fluid_render::render_voxel_fluid;
use axiom_render::math3d::{Mat4Fixed, Vec3Fixed};
use axiom_render::mesh3d::{parse_glb, MechaModel3D, PartMesh};
use axiom_render::raster3d::{CelShadingPalette, Rasterizer3D};
use axiom_render::texture::{DecalKind, TextureMap};
use axiom_render::voxel_collapse_render::render_dynamic_collapse_chunks;
use axiom_render::voxel_map::{deserialize_axvm, generate_voxel_preset, serialize_axvm, VoxelPreset};
use axiom_render::voxel_mesh::extract_voxel_mesh;
use axiom_render::voxel_shard_render::render_voxel_shards;
use axiom_core::thermal_stress::ThermalStressField16;
use axiom_render::thermal_render::render_thermal_glow;
use axiom_render::voxel_raymarch_render::{
    render_voxel_raymarch, VoxelRaymarchRenderConfig, VOXEL_RAYMARCH_WGSL,
};
use axiom_render::{cos_bams, sin_bams, FrameBuffer};

#[allow(static_mut_refs)]
struct WasmEngine {
    arena: BattleArena<256>,
    tilemap: TileMap<32, 24>,
    framebuffer: FrameBuffer,
}

#[allow(static_mut_refs)]
static mut ENGINE: Option<WasmEngine> = None;

/// Initializes battle arena, tilemap, and framebuffer.
#[no_mangle]
pub extern "C" fn axiom_wasm_init(seed: u64, width: u32, height: u32) {
    let mut arena = BattleArena::<256>::new();
    let tilemap = TileMap::<32, 24>::generate(seed);
    let framebuffer = FrameBuffer::new(width as usize, height as usize);

    // Spawn 50 Blue Knights (Team 1) on left side
    for i in 0..50 {
        let x = 40 + (i % 5) as i64 * 16;
        let y = 40 + (i / 5) as i64 * 16;
        arena.spawn_unit(
            (i + 1) as u32,
            1,
            100, // HP
            20,  // ATK
            16,  // Range
            3,   // Cooldown
            FixedVec2::from_ints(x, y),
        );
    }

    // Spawn 50 Red Goblins (Team 2) on right side
    for i in 0..50 {
        let x = 400 + (i % 5) as i64 * 16;
        let y = 40 + (i / 5) as i64 * 16;
        arena.spawn_unit(
            (50 + i + 1) as u32,
            2,
            75, // HP
            25, // ATK
            16, // Range
            2,  // Cooldown
            FixedVec2::from_ints(x, y),
        );
    }

    unsafe {
        ENGINE = Some(WasmEngine {
            arena,
            tilemap,
            framebuffer,
        });
    }
}

/// Advances battle simulation by 1 discrete tick.
#[no_mangle]
pub extern "C" fn axiom_wasm_step() {
    unsafe {
        if let Some(ref mut engine) = ENGINE {
            engine.arena.step();
        }
    }
}

/// Renders terrain and all active combat units to internal framebuffer.
#[no_mangle]
pub extern "C" fn axiom_wasm_render() {
    unsafe {
        if let Some(ref mut engine) = ENGINE {
            // 1. Render terrain tiles (tile size: 20 pixels)
            engine.framebuffer.render_tilemap(&engine.tilemap, 20);

            // 2. Render combat units as 6x6 colored squares
            for unit in engine.arena.units.iter() {
                if unit.is_alive() {
                    let px = unit.pos.x.to_int() as i32;
                    let py = unit.pos.y.to_int() as i32;
                    let color = if unit.team == 1 {
                        0xFF00BFFF // Deep Sky Blue
                    } else {
                        0xFFFF4500 // Orange Red
                    };
                    engine.framebuffer.draw_rect(px - 3, py - 3, 6, 6, color);
                }
            }
        }
    }
}

/// Returns raw pointer to the ARGB framebuffer memory.
#[no_mangle]
pub extern "C" fn axiom_wasm_get_framebuffer_ptr() -> *const u32 {
    unsafe {
        if let Some(ref engine) = ENGINE {
            engine.framebuffer.as_slice().as_ptr()
        } else {
            core::ptr::null()
        }
    }
}

/// Returns pixel count in the framebuffer.
#[no_mangle]
pub extern "C" fn axiom_wasm_get_framebuffer_len() -> u32 {
    unsafe {
        if let Some(ref engine) = ENGINE {
            engine.framebuffer.pixels.len() as u32
        } else {
            0
        }
    }
}

/// Returns current discrete simulation tick.
#[no_mangle]
pub extern "C" fn axiom_wasm_get_tick() -> u64 {
    unsafe {
        if let Some(ref engine) = ENGINE {
            engine.arena.current_tick
        } else {
            0
        }
    }
}

/// Returns alive units for blue team.
#[no_mangle]
pub extern "C" fn axiom_wasm_get_blue_alive() -> u32 {
    unsafe {
        if let Some(ref engine) = ENGINE {
            engine.arena.team_alive_count(1) as u32
        } else {
            0
        }
    }
}

/// Returns alive units for red team.
#[no_mangle]
pub extern "C" fn axiom_wasm_get_red_alive() -> u32 {
    unsafe {
        if let Some(ref engine) = ENGINE {
            engine.arena.team_alive_count(2) as u32
        } else {
            0
        }
    }
}

// =========================================================================
// Open-World 100,000 NPC & Chunk Streaming WebAssembly Bindings (T-019)
// =========================================================================

#[allow(static_mut_refs)]
struct OpenWorldEngine {
    chunk_mgr: ChunkManager<32, 32>,
    npcs: Box<NpcCrowdPool<100>>,
    player_pos: FixedVec2,
    framebuffer: FrameBuffer,
    tick: u64,
}

#[allow(static_mut_refs)]
static mut OPEN_WORLD: Option<OpenWorldEngine> = None;

/// Initializes Open-World streaming environment with living NPC crowd.
#[no_mangle]
pub extern "C" fn axiom_openworld_init(seed: u64, width: u32, height: u32, npc_count: u32) {
    let npc_layout = core::alloc::Layout::new::<NpcCrowdPool<100>>();
    let npc_ptr = unsafe { alloc::alloc::alloc_zeroed(npc_layout) as *mut NpcCrowdPool<100> };
    assert!(!npc_ptr.is_null());
    let mut npcs = unsafe { Box::from_raw(npc_ptr) };

    let count = (npc_count as usize).min(100_000);
    for i in 0..count {
        let id = (i + 1) as u32;
        let home = ((i % 1000) as i32, (i / 1000) as i32);
        let work = (((i + 500) % 1000) as i32, ((i + 500) / 1000) as i32);
        let shift = (i % 3) as u8;
        npcs.spawn(id, home, work, shift, 256);
    }

    let mut chunk_mgr = ChunkManager::new(seed, 2);
    let player_pos = FixedVec2::new(Fixed::ZERO, Fixed::ZERO);
    chunk_mgr.update_player_pos(player_pos, 0);

    let framebuffer = FrameBuffer::new(width as usize, height as usize);

    unsafe {
        OPEN_WORLD = Some(OpenWorldEngine {
            chunk_mgr,
            npcs,
            player_pos,
            framebuffer,
            tick: 0,
        });
    }
}

/// Advances open world simulation and player position.
#[no_mangle]
pub extern "C" fn axiom_openworld_step(move_x: i32, move_y: i32) {
    unsafe {
        if let Some(ref mut engine) = OPEN_WORLD {
            engine.tick += 1;
            engine.player_pos.x += Fixed::from_int(move_x as i64);
            engine.player_pos.y += Fixed::from_int(move_y as i64);

            engine.chunk_mgr.update_player_pos(engine.player_pos, engine.tick);
            engine.npcs.update(engine.tick);
        }
    }
}

/// Renders open world chunks, player avatar, and 100k NPC minimap.
#[no_mangle]
pub extern "C" fn axiom_openworld_render(tile_size: u32) {
    unsafe {
        if let Some(ref mut engine) = OPEN_WORLD {
            engine.framebuffer.clear(0xFF0F172A);

            // 1. Render loaded chunks centered at player
            engine.framebuffer.render_chunks(
                &engine.chunk_mgr,
                engine.player_pos,
                tile_size as usize,
            );

            // 2. Render Player indicator at screen center
            let cx = (engine.framebuffer.width / 2) as i32;
            let cy = (engine.framebuffer.height / 2) as i32;
            engine.framebuffer.draw_rect(cx - 4, cy - 4, 8, 8, 0xFFFFFFFF); // White border
            engine.framebuffer.draw_rect(cx - 2, cy - 2, 4, 4, 0xFFFF0000); // Red core

            // 3. Render 100k NPC Minimap in top-right corner
            let minimap_w = 160;
            let minimap_h = 120;
            let minimap_x = engine.framebuffer.width as i32 - minimap_w as i32 - 15;
            let minimap_y = 15;
            engine.framebuffer.render_npc_minimap(
                &engine.npcs,
                minimap_x,
                minimap_y,
                minimap_w,
                minimap_h,
                8,
            );
        }
    }
}

/// Returns raw pointer to open-world framebuffer.
#[no_mangle]
pub extern "C" fn axiom_openworld_get_framebuffer_ptr() -> *const u32 {
    unsafe {
        if let Some(ref engine) = OPEN_WORLD {
            engine.framebuffer.as_slice().as_ptr()
        } else {
            core::ptr::null()
        }
    }
}

/// Returns current open-world simulation tick.
#[no_mangle]
pub extern "C" fn axiom_openworld_get_tick() -> u64 {
    unsafe {
        if let Some(ref engine) = OPEN_WORLD {
            engine.tick
        } else {
            0
        }
    }
}

/// Returns total active NPC count in the open world.
#[no_mangle]
pub extern "C" fn axiom_openworld_get_npc_count() -> u32 {
    unsafe {
        if let Some(ref engine) = OPEN_WORLD {
            engine.npcs.len() as u32
        } else {
            0
        }
    }
}

// =========================================================================
// Interactive Open-World RPG WebAssembly Bindings (T-027)
// =========================================================================

#[allow(static_mut_refs)]
struct RpgWasmEngine {
    chunk_mgr: ChunkManager<32, 32>,
    npcs: Box<NpcCrowdPool<100>>,
    journal: QuestJournal<32, 64>,
    pathfinder: Box<PathFinder<64, 64>>,
    arena: AtbArena<MAX_ACTIVE_UNITS>,
    audio: SpatialAudioSimulator<DEFAULT_MAX_VOICES>,
    player_pos: FixedVec2,
    current_path: Option<Path<MAX_PATH_WAYPOINTS>>,
    path_index: usize,
    in_combat: bool,
    framebuffer: FrameBuffer,
    tick: u64,
}

#[allow(static_mut_refs)]
static mut RPG_ENGINE: Option<RpgWasmEngine> = None;

/// Initializes the interactive Open-World RPG engine.
#[no_mangle]
pub extern "C" fn axiom_rpg_init(seed: u64, width: u32, height: u32, npc_count: u32) {
    let npc_layout = core::alloc::Layout::new::<NpcCrowdPool<100>>();
    let npc_ptr = unsafe { alloc::alloc::alloc_zeroed(npc_layout) as *mut NpcCrowdPool<100> };
    assert!(!npc_ptr.is_null());
    let mut npcs = unsafe { Box::from_raw(npc_ptr) };

    let count = (npc_count as usize).min(100_000);
    for i in 0..count {
        let id = (i + 1) as u32;
        let home = ((i % 1000) as i32, (i / 1000) as i32);
        let work = (((i + 500) % 1000) as i32, ((i + 500) / 1000) as i32);
        let shift = (i % 3) as u8;
        npcs.spawn(id, home, work, shift, 256);
    }

    // Stream terrain around the origin, then spawn on the nearest open ground so the
    // hero never starts inside a walled-in procedural pocket.
    let mut chunk_mgr = ChunkManager::new(seed, 2);
    chunk_mgr.update_player_pos(FixedVec2::new(Fixed::ZERO, Fixed::ZERO), 0);
    let (spawn_x, spawn_y) = chunk_mgr.find_open_spawn(
        DEFAULT_GRID_W,
        DEFAULT_GRID_H,
        axiom_core::chunk::DEFAULT_SPAWN_MIN_OPEN_AREA,
    );
    let player_pos = FixedVec2::from_ints(spawn_x, spawn_y);
    chunk_mgr.update_player_pos(player_pos, 0);

    let pathfinder = Box::new(PathFinder::<DEFAULT_GRID_W, DEFAULT_GRID_H>::new());
    let journal = QuestJournal::new();
    let arena = AtbArena::new();
    let audio = SpatialAudioSimulator::new();
    let framebuffer = FrameBuffer::new(width as usize, height as usize);

    unsafe {
        RPG_ENGINE = Some(RpgWasmEngine {
            chunk_mgr,
            npcs,
            journal,
            pathfinder,
            arena,
            audio,
            player_pos,
            current_path: None,
            path_index: 0,
            in_combat: false,
            framebuffer,
            tick: 0,
        });
    }
}

/// Triggers quest acceptance in the RPG engine.
#[no_mangle]
pub extern "C" fn axiom_rpg_interact_quest(
    npc_id: u32,
    quest_id: u32,
    target_enemy_id: u32,
    req_kills: u32,
    reward_xp: u32,
) -> bool {
    let _ = npc_id;
    unsafe {
        if let Some(ref mut engine) = RPG_ENGINE {
            let quest = QuestRecord::new(
                quest_id,
                ObjectiveType::DefeatEnemies,
                target_enemy_id,
                req_kills,
                reward_xp,
                1,
                1,
                engine.tick,
            );
            engine.audio.emit_sound(SoundEvent {
                sound_id: 201,
                pos: engine.player_pos,
                base_volume: 800,
                max_distance: 20,
                category: SoundCategory::UI,
                priority: 8,
            });
            engine.journal.accept_quest(quest)
        } else {
            false
        }
    }
}

/// Plans an A* path from current player position to world tile (goal_x, goal_y).
#[no_mangle]
pub extern "C" fn axiom_rpg_plan_path(goal_x: i32, goal_y: i32) -> bool {
    unsafe {
        if let Some(ref mut engine) = RPG_ENGINE {
            let start_x = (engine.player_pos.x.to_int().clamp(0, 63)) as usize;
            let start_y = (engine.player_pos.y.to_int().clamp(0, 63)) as usize;
            let gx = (goal_x.clamp(0, 63)) as usize;
            let gy = (goal_y.clamp(0, 63)) as usize;

            let path = engine.pathfinder.find_path(start_x, start_y, gx, gy, |x, y| {
                let tile = engine.chunk_mgr.tile_at(x as i64, y as i64);
                tile.is_walkable()
            });

            if let Some(p) = path {
                engine.current_path = Some(p);
                engine.path_index = 0;
                true
            } else {
                engine.current_path = None;
                false
            }
        } else {
            false
        }
    }
}

/// Triggers an ATB combat encounter against an enemy mob.
#[no_mangle]
pub extern "C" fn axiom_rpg_trigger_encounter(enemy_id: u32) {
    unsafe {
        if let Some(ref mut engine) = RPG_ENGINE {
            engine.in_combat = true;
            let mut hero = AtbUnit::new(1, 1, Element::Fire, 500, 120, 25, 50);
            hero.atb_gauge = 500;
            engine.arena.spawn_unit(hero);

            let mut enemy = AtbUnit::new(enemy_id, 2, Element::Ice, 180, 40, 10, 30);
            enemy.apply_status(StatusKind::Slow, 30, 0);
            engine.arena.spawn_unit(enemy);

            engine.audio.emit_sound(SoundEvent {
                sound_id: 301,
                pos: engine.player_pos,
                base_volume: 900,
                max_distance: 40,
                category: SoundCategory::Combat,
                priority: 10,
            });
        }
    }
}

/// Advances 1 simulation tick for the RPG engine.
#[no_mangle]
pub extern "C" fn axiom_rpg_step(move_x: i32, move_y: i32) {
    unsafe {
        if let Some(ref mut engine) = RPG_ENGINE {
            engine.tick += 1;
            engine.npcs.update(engine.tick);

            // Waypoint traversal or manual input
            if let Some(ref path) = engine.current_path {
                if engine.path_index < path.len {
                    let (wx, wy) = path.waypoints[engine.path_index];
                    engine.player_pos = FixedVec2::from_ints(wx as i64, wy as i64);
                    engine.path_index += 1;
                    engine.chunk_mgr.update_player_pos(engine.player_pos, engine.tick);
                } else {
                    engine.current_path = None;
                }
            } else if move_x != 0 || move_y != 0 {
                engine.player_pos.x += Fixed::from_int(move_x as i64);
                engine.player_pos.y += Fixed::from_int(move_y as i64);
                engine.chunk_mgr.update_player_pos(engine.player_pos, engine.tick);
            }

            // ATB Combat
            if engine.in_combat {
                engine.arena.step();
                let enemy_alive = engine.arena.units.iter().any(|u| u.team == 2 && u.is_alive());
                if !enemy_alive {
                    engine.in_combat = false;
                    engine.journal.notify_enemy_defeated(999);
                    if let Some(q) = engine.journal.get_quest(100) {
                        if q.status == QuestStatus::ObjectiveComplete {
                            engine.journal.turn_in_quest(100, engine.tick);
                        }
                    }
                }
            }

            engine.audio.set_listener_pos(engine.player_pos);
            let _ = engine.audio.process_and_mix();
        }
    }
}

/// Renders the complete RPG scene with Chunks, A* Trail, Player, 100k Minimap, and Combat/Quest HUDs.
#[no_mangle]
pub extern "C" fn axiom_rpg_render(tile_size: u32) {
    unsafe {
        if let Some(ref mut engine) = RPG_ENGINE {
            engine.framebuffer.clear(0xFF0F172A);

            // 1. Render loaded chunks
            engine.framebuffer.render_chunks(
                &engine.chunk_mgr,
                engine.player_pos,
                tile_size as usize,
            );

            let half_w = (engine.framebuffer.width / 2) as i32;
            let half_h = (engine.framebuffer.height / 2) as i32;
            let cam_x = engine.player_pos.x.to_int() as i32;
            let cam_y = engine.player_pos.y.to_int() as i32;

            // 2. Render A* Path Trail
            if let Some(ref path) = engine.current_path {
                for i in engine.path_index..path.len {
                    let (wx, wy) = path.waypoints[i];
                    let sx = half_w + (wx as i32 - cam_x) * tile_size as i32;
                    let sy = half_h + (wy as i32 - cam_y) * tile_size as i32;
                    let offset = (tile_size as i32 / 2).saturating_sub(2);
                    engine.framebuffer.draw_rect(sx + offset, sy + offset, 4, 4, 0xFFFACC15); // Gold waypoint dot
                }
            }

            // 3. Render Player (Centered)
            engine.framebuffer.draw_rect(half_w - 5, half_h - 5, 10, 10, 0xFFFFFFFF);
            engine.framebuffer.draw_rect(half_w - 3, half_h - 3, 6, 6, 0xFF38BDF8);

            // 4. Render 100k NPC Minimap in top-right
            let mw = 160;
            let mh = 120;
            let mx = engine.framebuffer.width as i32 - mw as i32 - 15;
            let my = 15;
            engine.framebuffer.render_npc_minimap(&engine.npcs, mx, my, mw, mh, 8);

            // 5. Quest HUD Banner (Top-Left)
            let q_active = engine.journal.active_count;
            let q_done = engine.journal.completed_count;
            engine.framebuffer.draw_rect(15, 15, 170, 32, 0xFF1E293B);
            let pip_color = if q_done > 0 {
                0xFFFACC15 // Gold (Turned in)
            } else if q_active > 0 {
                0xFF22C55E // Green (Active)
            } else {
                0xFF64748B // Slate (None)
            };
            engine.framebuffer.draw_rect(22, 23, 16, 16, pip_color);

            // 6. Combat Battle HUD (Bottom-Center)
            if engine.in_combat {
                let box_w = 300;
                let box_h = 64;
                let bx = half_w - box_w / 2;
                let by = engine.framebuffer.height as i32 - box_h - 20;

                engine.framebuffer.draw_rect(bx, by, box_w, box_h, 0xFF0F172A);
                engine.framebuffer.draw_rect(bx + 1, by + 1, box_w - 2, box_h - 2, 0xFF1E293B);

                // Find Hero & Enemy
                let hero = engine.arena.units.iter().find(|u| u.team == 1 && u.is_alive());
                let enemy = engine.arena.units.iter().find(|u| u.team == 2 && u.is_alive());

                if let Some(h) = hero {
                    let hp_pct = ((h.hp as usize * 120) / h.max_hp.max(1) as usize) as i32;
                    let atb_pct = ((h.atb_gauge as usize * 120) / 1000) as i32;
                    engine.framebuffer.draw_rect(bx + 15, by + 16, 120, 10, 0xFF450A0A);
                    engine.framebuffer.draw_rect(bx + 15, by + 16, hp_pct, 10, 0xFF22C55E); // Green HP
                    engine.framebuffer.draw_rect(bx + 15, by + 32, atb_pct, 6, 0xFF06B6D4);  // Cyan ATB
                }

                if let Some(e) = enemy {
                    let hp_pct = ((e.hp as usize * 120) / e.max_hp.max(1) as usize) as i32;
                    let atb_pct = ((e.atb_gauge as usize * 120) / 1000) as i32;
                    engine.framebuffer.draw_rect(bx + 165, by + 16, 120, 10, 0xFF450A0A);
                    engine.framebuffer.draw_rect(bx + 165, by + 16, hp_pct, 10, 0xFFEF4444); // Red HP
                    engine.framebuffer.draw_rect(bx + 165, by + 32, atb_pct, 6, 0xFFF97316);  // Orange ATB
                }
            }
        }
    }
}

/// Returns raw pointer to RPG framebuffer.
#[no_mangle]
pub extern "C" fn axiom_rpg_get_framebuffer_ptr() -> *const u32 {
    unsafe {
        if let Some(ref engine) = RPG_ENGINE {
            engine.framebuffer.as_slice().as_ptr()
        } else {
            core::ptr::null()
        }
    }
}

/// Returns current RPG simulation tick.
#[no_mangle]
pub extern "C" fn axiom_rpg_get_tick() -> u64 {
    unsafe {
        if let Some(ref engine) = RPG_ENGINE {
            engine.tick
        } else {
            0
        }
    }
}

/// Returns whether RPG session is currently in combat mode.
#[no_mangle]
pub extern "C" fn axiom_rpg_is_in_combat() -> bool {
    unsafe {
        if let Some(ref engine) = RPG_ENGINE {
            engine.in_combat
        } else {
            false
        }
    }
}

/// Returns total earned XP.
#[no_mangle]
pub extern "C" fn axiom_rpg_get_xp() -> u32 {
    unsafe {
        if let Some(ref engine) = RPG_ENGINE {
            engine.journal.total_xp_earned
        } else {
            0
        }
    }
}

/// Returns active quest count.
#[no_mangle]
pub extern "C" fn axiom_rpg_get_active_quests() -> u32 {
    unsafe {
        if let Some(ref engine) = RPG_ENGINE {
            engine.journal.active_count as u32
        } else {
            0
        }
    }
}

/// Returns completed quest count.
#[no_mangle]
pub extern "C" fn axiom_rpg_get_completed_quests() -> u32 {
    unsafe {
        if let Some(ref engine) = RPG_ENGINE {
            engine.journal.completed_count as u32
        } else {
            0
        }
    }
}

/// Returns total combat damage dealt across all encounters.
#[no_mangle]
pub extern "C" fn axiom_rpg_get_combat_damage() -> u64 {
    unsafe {
        if let Some(ref engine) = RPG_ENGINE {
            engine.arena.total_damage_dealt
        } else {
            0
        }
    }
}

// =========================================================================
// Real-Time Multiplayer Rollback WebAssembly Bindings (T-030)
// =========================================================================

#[allow(static_mut_refs)]
struct NetcodeWasmEngine {
    session: RollbackSession<128, 32>,
    framebuffer: FrameBuffer,
}

#[allow(static_mut_refs)]
static mut NET_ENGINE: Option<NetcodeWasmEngine> = None;

/// Initializes a peer-to-peer multiplayer rollback session.
#[no_mangle]
pub extern "C" fn axiom_net_init(local_player_id: u32, width: u32, height: u32) {
    let mut initial_state = TickState::<32>::new();
    initial_state.spawn(1, FixedVec2::from_ints(-30, 0), FixedVec2::ZERO);
    initial_state.spawn(2, FixedVec2::from_ints(30, 0), FixedVec2::ZERO);

    let session = RollbackSession::<128, 32>::new(local_player_id as usize, initial_state);
    let framebuffer = FrameBuffer::new(width as usize, height as usize);

    unsafe {
        NET_ENGINE = Some(NetcodeWasmEngine {
            session,
            framebuffer,
        });
    }
}

/// Advances 1 local frame with zero input latency.
#[no_mangle]
pub extern "C" fn axiom_net_advance_frame(move_x: i32, move_y: i32, buttons: u32) -> u64 {
    unsafe {
        if let Some(ref mut engine) = NET_ENGINE {
            let input = InputState::new(move_x, move_y, buttons);
            let stats = engine.session.advance_frame(input);
            stats.current_frame
        } else {
            0
        }
    }
}

/// Delivers a remote input packet arrived from WebRTC DataChannel.
/// Returns true if a speculative rollback was triggered.
#[no_mangle]
pub extern "C" fn axiom_net_receive_remote(
    remote_player_id: u32,
    frame: u64,
    move_x: i32,
    move_y: i32,
    buttons: u32,
) -> bool {
    unsafe {
        if let Some(ref mut engine) = NET_ENGINE {
            let input = InputState::new(move_x, move_y, buttons);
            engine.session.receive_remote_input(remote_player_id as usize, frame, input)
        } else {
            false
        }
    }
}

/// Returns current frame count.
#[no_mangle]
pub extern "C" fn axiom_net_get_frame() -> u64 {
    unsafe {
        if let Some(ref engine) = NET_ENGINE {
            engine.session.current_frame
        } else {
            0
        }
    }
}

/// Returns confirmed frame boundary.
#[no_mangle]
pub extern "C" fn axiom_net_get_confirmed_frame() -> u64 {
    unsafe {
        if let Some(ref engine) = NET_ENGINE {
            engine.session.confirmed_frame
        } else {
            0
        }
    }
}

/// Returns total rollbacks triggered by prediction misalignments.
#[no_mangle]
pub extern "C" fn axiom_net_get_total_rollbacks() -> u64 {
    unsafe {
        if let Some(ref engine) = NET_ENGINE {
            engine.session.total_rollbacks
        } else {
            0
        }
    }
}

/// Returns total frames resimulated.
#[no_mangle]
pub extern "C" fn axiom_net_get_total_resimulated() -> u64 {
    unsafe {
        if let Some(ref engine) = NET_ENGINE {
            engine.session.total_frames_resimulated
        } else {
            0
        }
    }
}

/// Computes deterministic state hash of current world.
#[no_mangle]
pub extern "C" fn axiom_net_get_hash() -> u64 {
    unsafe {
        if let Some(ref engine) = NET_ENGINE {
            engine.session.compute_hash()
        } else {
            0
        }
    }
}

/// Renders the multiplayer arena, avatars, and rollback consensus HUD.
#[no_mangle]
pub extern "C" fn axiom_net_render() {
    unsafe {
        if let Some(ref mut engine) = NET_ENGINE {
            engine.framebuffer.clear(0xFF0F172A);

            let half_w = (engine.framebuffer.width / 2) as i32;
            let half_h = (engine.framebuffer.height / 2) as i32;

            // Draw center divider
            for y in 0..engine.framebuffer.height as i32 {
                if y % 8 < 4 {
                    engine.framebuffer.set_pixel(half_w, y, 0xFF334155);
                }
            }

            // Draw players
            for (idx, entity) in engine.session.current_state.entities.iter().enumerate() {
                if entity.is_active() {
                    let px = half_w + entity.pos.x.to_int() as i32 * 4;
                    let py = half_h + entity.pos.y.to_int() as i32 * 4;
                    let color = if idx == 0 {
                        0xFF38BDF8 // Player 0: Sky Blue
                    } else {
                        0xFFFB923C // Player 1: Bright Orange
                    };
                    engine.framebuffer.draw_rect(px - 8, py - 8, 16, 16, 0xFFFFFFFF);
                    engine.framebuffer.draw_rect(px - 6, py - 6, 12, 12, color);
                }
            }

            // Netcode Status Banner (Top)
            engine.framebuffer.draw_rect(20, 15, 320, 36, 0xFF1E293B);
            let hash_color = 0xFF4ADE80; // Green = Synced
            engine.framebuffer.draw_rect(28, 23, 20, 20, hash_color);
        }
    }
}

/// Returns raw pointer to multiplayer framebuffer memory.
#[no_mangle]
pub extern "C" fn axiom_net_get_framebuffer_ptr() -> *const u32 {
    unsafe {
        if let Some(ref engine) = NET_ENGINE {
            engine.framebuffer.as_slice().as_ptr()
        } else {
            core::ptr::null()
        }
    }
}

// =========================================================================
// Real-Time P2P Multiplayer Data-Driven Factory Rollback Bindings (T-052)
// =========================================================================

pub const COOP_W: usize = 32;
pub const COOP_H: usize = 32;
pub const COOP_FRAMES: usize = 32;
pub const COOP_PLAYERS: usize = 2;
pub const COOP_RECIPES: usize = 32;

#[allow(static_mut_refs)]
struct CoopFactoryWasmEngine {
    session: Box<GridRollbackSession<COOP_W, COOP_H, COOP_FRAMES, COOP_PLAYERS, COOP_RECIPES>>,
    framebuffer: FrameBuffer,
    local_player_id: u32,
    cursor_x: i32,
    cursor_y: i32,
    last_rollback_frame: u64,
}

impl CoopFactoryWasmEngine {
    pub fn render(&mut self, tile_size: u32) {
        let ts = (tile_size as i32).clamp(8, 64);
        let fb = &mut self.framebuffer;
        fb.clear(0xFF0A0F1D); // Deep dark industrial background

        let half_w = (fb.width / 2) as i32;
        let half_h = (fb.height / 2) as i32;

        // Camera offset centered around cursor
        let cam_x = self.cursor_x * ts;
        let cam_y = self.cursor_y * ts;
        let origin_x = half_w - cam_x;
        let origin_y = half_h - cam_y + 18; // offset for top HUD bar

        // 1. Draw 32x32 Spatial Tile Grid
        for y in 0..COOP_H {
            for x in 0..COOP_W {
                let sx = origin_x + (x as i32) * ts;
                let sy = origin_y + (y as i32) * ts;

                // Viewport boundary clipping
                if sx + ts < 0 || sx >= fb.width as i32 || sy + ts < 0 || sy >= fb.height as i32 {
                    continue;
                }

                // Grid cell background
                fb.draw_rect(sx, sy, ts, ts, 0xFF111827);
                fb.draw_rect(sx + 1, sy + 1, ts - 2, ts - 2, 0xFF1E293B);

                if let Some(cell) = self.session.current_grid.get(x, y) {
                    if !cell.is_empty() {
                        let inner = (ts - 4).max(2);
                        let ix = sx + 2;
                        let iy = sy + 2;

                        match cell.type_id {
                            1 => {
                                // Conveyor Belt (Slate Gray + direction arrow)
                                fb.draw_rect(ix, iy, inner, inner, 0xFF334155);
                                let dir = cell.custom_payload[1];
                                let mid_x = sx + ts / 2;
                                let mid_y = sy + ts / 2;
                                let arr_w = (ts / 4).max(2);
                                match dir {
                                    0 => fb.draw_rect(mid_x - arr_w / 2, sy + 3, arr_w, arr_w, 0xFF94A3B8),
                                    1 => fb.draw_rect(sx + ts - 3 - arr_w, mid_y - arr_w / 2, arr_w, arr_w, 0xFF94A3B8),
                                    2 => fb.draw_rect(mid_x - arr_w / 2, sy + ts - 3 - arr_w, arr_w, arr_w, 0xFF94A3B8),
                                    _ => fb.draw_rect(sx + 3, mid_y - arr_w / 2, arr_w, arr_w, 0xFF94A3B8),
                                }
                            }
                            2 => {
                                // Mining Drill (Amber / Gold machinery)
                                fb.draw_rect(ix, iy, inner, inner, 0xFFD97706);
                                fb.draw_rect(ix + 2, iy + 2, (inner - 4).max(1), (inner - 4).max(1), 0xFFF59E0B);
                                let spin = ((self.session.current_frame as i32) / 3) % (inner.max(2));
                                fb.draw_rect(ix + spin, iy + inner / 2 - 1, 2, 2, 0xFFFEF08A);
                            }
                            3 => {
                                // Smelter (Dark stone furnace + orange fire mouth)
                                fb.draw_rect(ix, iy, inner, inner, 0xFF3F3F46);
                                let fire_h = (inner / 2).max(1);
                                let fire_color = if (self.session.current_frame / 4) % 2 == 0 { 0xFFEA580C } else { 0xFFFBBF24 };
                                fb.draw_rect(ix + 2, iy + inner - fire_h - 1, inner - 4, fire_h, fire_color);
                            }
                            4 => {
                                // Chest (Brown crate + golden lock)
                                fb.draw_rect(ix, iy, inner, inner, 0xFF92400E);
                                fb.draw_rect(ix + 1, iy + 1, inner - 2, 2, 0xFFB45309);
                                fb.draw_rect(sx + ts / 2 - 1, sy + ts / 2 - 1, 3, 3, 0xFFFDE047);
                            }
                            5 => {
                                // Wall (Steel gray fortified block)
                                fb.draw_rect(sx + 1, sy + 1, ts - 2, ts - 2, 0xFF94A3B8);
                                fb.draw_rect(sx + 2, sy + 2, ts - 4, ts - 4, 0xFF64748B);
                            }
                            6 => {
                                // Turret (Deep cyan base + sky blue barrel)
                                fb.draw_rect(ix, iy, inner, inner, 0xFF0369A1);
                                fb.draw_rect(sx + ts / 2 - 2, sy + ts / 2 - 2, 5, 5, 0xFF38BDF8);
                            }
                            9 => {
                                // Base Heart (Rose / Crimson pulsating core)
                                fb.draw_rect(ix, iy, inner, inner, 0xFF9F1239);
                                let pulse = ((self.session.current_frame / 6) % 2) as i32;
                                fb.draw_rect(ix + 2 - pulse, iy + 2 - pulse, (inner - 4 + pulse * 2).max(1), (inner - 4 + pulse * 2).max(1), 0xFFF43F5E);
                            }
                            10 => {
                                // Iron Ore Vein (Metallic silver speckles)
                                fb.draw_rect(ix, iy, inner, inner, 0xFF475569);
                                fb.draw_rect(ix + 2, iy + 2, 3, 3, 0xFFCBD5E1);
                            }
                            11 => {
                                // Copper Ore Vein (Copper rust brown)
                                fb.draw_rect(ix, iy, inner, inner, 0xFF78350F);
                                fb.draw_rect(ix + 2, iy + 2, 3, 3, 0xFFFB923C);
                            }
                            _ => {
                                fb.draw_rect(ix, iy, inner, inner, 0xFF7C3AED);
                            }
                        }

                        // Ownership border highlight (P0 = Sky Blue, P1 = Vibrant Orange)
                        if cell.has_flag(tile_flags::PLAYER_PLACED) {
                            let owner = cell.custom_payload[0];
                            let border_color = if owner == 0 {
                                0xFF38BDF8 // Player 0: Sky Blue
                            } else {
                                0xFFFB923C // Player 1: Vibrant Orange
                            };
                            fb.draw_rect(sx, sy, ts, 1, border_color);
                            fb.draw_rect(sx, sy + ts - 1, ts, 1, border_color);
                            fb.draw_rect(sx, sy, 1, ts, border_color);
                            fb.draw_rect(sx + ts - 1, sy, 1, ts, border_color);
                        }
                    }
                }

                // Cursor border highlight
                if x == self.cursor_x as usize && y == self.cursor_y as usize {
                    fb.draw_rect(sx, sy, ts, 2, 0xFFFFFFFF);
                    fb.draw_rect(sx, sy + ts - 2, ts, 2, 0xFFFFFFFF);
                    fb.draw_rect(sx, sy, 2, ts, 0xFFFFFFFF);
                    fb.draw_rect(sx + ts - 2, sy, 2, ts, 0xFFFFFFFF);
                }
            }
        }

        // 2. Netcode Rollback HUD Banner (Top 36px)
        fb.draw_rect(0, 0, fb.width as i32, 36, 0xFF020617);
        fb.draw_rect(0, 36, fb.width as i32, 1, 0xFF1E293B);

        // Status pill: Green (Consensus) or Glowing Yellow (Rollback active)
        let is_rolling_back = self.session.current_frame.saturating_sub(self.last_rollback_frame) < 15;
        let status_color = if is_rolling_back { 0xFFEAB308 } else { 0xFF22C55E };
        fb.draw_rect(12, 10, 16, 16, status_color);

        // Local Player ID indicator
        let local_color = if self.local_player_id == 0 { 0xFF38BDF8 } else { 0xFFFB923C };
        fb.draw_rect(36, 10, 48, 16, local_color);
        fb.draw_rect(38, 12, 44, 12, 0xFF0F172A);

        // State Hash bitfield display
        let hash = self.session.state_hash();
        for b in 0..16 {
            let bit_color = if (hash >> (b * 4)) & 1 == 1 { 0xFF38BDF8 } else { 0xFF64748B };
            fb.draw_rect(96 + b * 4, 14, 3, 8, bit_color);
        }

        // Rollback activity bar
        let rollbacks = self.session.total_rollbacks;
        if rollbacks > 0 {
            let rb_w = ((rollbacks as i32) * 4).min(80);
            fb.draw_rect(180, 14, rb_w, 8, 0xFFF59E0B);
        }
    }
}

#[allow(static_mut_refs)]
static mut COOP_ENGINE: Option<CoopFactoryWasmEngine> = None;

#[inline(always)]
fn coop_engine() -> Option<&'static mut CoopFactoryWasmEngine> {
    unsafe { (*core::ptr::addr_of_mut!(COOP_ENGINE)).as_mut() }
}

/// Initializes a peer-to-peer multiplayer data-driven factory rollback session.
#[no_mangle]
pub extern "C" fn axiom_coop_init(local_player_id: u32, width: u32, height: u32) {
    let mut recipes = RecipeRegistry::<COOP_RECIPES>::new();

    // Data-Driven Factory Recipes:
    // Recipe 1: 2 Iron Ore (item 1) -> 1 Iron Ingot (item 2) in 3 ticks
    let mut r1 = RecipeDefinition::empty();
    r1.id = 1;
    r1.input_id = 1;
    r1.input_count = 2;
    r1.output_id = 2;
    r1.output_count = 1;
    r1.required_ticks = 3;
    recipes.register(r1);

    // Recipe 2: 1 Copper Ore (item 3) -> 1 Copper Wire (item 4) in 2 ticks
    let mut r2 = RecipeDefinition::empty();
    r2.id = 2;
    r2.input_id = 3;
    r2.input_count = 1;
    r2.output_id = 4;
    r2.output_count = 1;
    r2.required_ticks = 2;
    recipes.register(r2);

    // Recipe 3: 2 Iron Ingot (item 2) -> 1 Steel Plate (item 5) in 4 ticks
    let mut r3 = RecipeDefinition::empty();
    r3.id = 3;
    r3.input_id = 2;
    r3.input_count = 2;
    r3.output_id = 5;
    r3.output_count = 1;
    r3.required_ticks = 4;
    recipes.register(r3);

    let mut initial_grid = TileGrid::<COOP_W, COOP_H>::new();

    // Spawn natural ore deposits and base heart
    initial_grid.set(16, 16, TileCell::new(9, 2000)); // Base Heart at center

    // Iron deposit near (12, 10)
    for dy in 0..2 {
        for dx in 0..2 {
            initial_grid.set(11 + dx, 9 + dy, TileCell::new(10, 1000));
        }
    }

    // Copper deposit near (20, 22)
    for dy in 0..2 {
        for dx in 0..2 {
            initial_grid.set(19 + dx, 21 + dy, TileCell::new(11, 1000));
        }
    }

    let session = GridRollbackSession::new_boxed(local_player_id as usize, initial_grid, recipes);
    let framebuffer = FrameBuffer::new(width as usize, height as usize);

    unsafe {
        COOP_ENGINE = Some(CoopFactoryWasmEngine {
            session,
            framebuffer,
            local_player_id,
            cursor_x: 16,
            cursor_y: 16,
            last_rollback_frame: 0,
        });
    }
}

/// Advances 1 local frame with zero input latency, updating camera/cursor.
#[no_mangle]
pub extern "C" fn axiom_coop_step(dx: i32, dy: i32) -> u64 {
    if let Some(engine) = coop_engine() {
        engine.cursor_x = (engine.cursor_x + dx).clamp(0, COOP_W as i32 - 1);
        engine.cursor_y = (engine.cursor_y + dy).clamp(0, COOP_H as i32 - 1);
        let stats = engine.session.advance_frame(GridAction::NONE);
        stats.current_frame
    } else {
        0
    }
}

/// Places or demolishes a structure on the spatial grid with 0ms local lag.
#[no_mangle]
pub extern "C" fn axiom_coop_place(x: u16, y: u16, kind: u32, direction: u32, item: u32) -> u64 {
    if let Some(engine) = coop_engine() {
        let pid = engine.local_player_id as u16;
        let action = match kind {
            0 => GridAction::clear_tile(pid, x, y),
            1..=11 => {
                let hp = match kind {
                    5 => 500,  // Wall
                    6 => 250,  // Turret
                    9 => 2000, // Base Heart
                    _ => 300,
                };
                let mut act = GridAction::set_tile(pid, x, y, kind, hp);
                act.param1 = direction as u16;
                act.param2 = hp;
                act
            }
            _ => GridAction::NONE,
        };

        let stats = engine.session.advance_frame(action);

        // Optional item stocking (e.g. ammo chest or ore deposit)
        if (kind == 4 || kind == 2) && item != 0 {
            if let Some(cell) = engine.session.current_grid.get_mut(x as usize, y as usize) {
                cell.push_item(item as u16, 50);
            }
        }

        stats.current_frame
    } else {
        0
    }
}

/// Delivers a remote grid action arrived from WebRTC DataChannel.
/// Triggers speculative rollback and resimulation if mispredicted.
#[no_mangle]
pub extern "C" fn axiom_coop_receive_remote(
    remote_player_id: u32,
    frame: u64,
    kind: u16,
    x: u16,
    y: u16,
    type_id: u32,
    param1: u16,
    param2: u16,
) -> bool {
    if let Some(engine) = coop_engine() {
        let action = GridAction::new(kind, remote_player_id as u16, x, y, type_id, param1, param2);
        let rolled_back = engine.session.receive_remote_action(remote_player_id as usize, frame, action);
        if rolled_back {
            engine.last_rollback_frame = engine.session.current_frame;
        }
        rolled_back
    } else {
        false
    }
}

/// Renders the cooperative factory grid, structures, and GGPO rollback HUD.
#[no_mangle]
pub extern "C" fn axiom_coop_render(tile_size: u32) {
    if let Some(engine) = coop_engine() {
        engine.render(tile_size);
    }
}

/// Returns raw pointer to cooperative framebuffer memory.
#[no_mangle]
pub extern "C" fn axiom_coop_get_framebuffer_ptr() -> *const u32 {
    coop_engine().map(|e| e.framebuffer.as_slice().as_ptr()).unwrap_or(core::ptr::null())
}

/// Returns pixel count in cooperative framebuffer.
#[no_mangle]
pub extern "C" fn axiom_coop_get_framebuffer_len() -> u32 {
    coop_engine().map(|e| e.framebuffer.pixels.len() as u32).unwrap_or(0)
}

/// Returns current frame of the rollback session.
#[no_mangle]
pub extern "C" fn axiom_coop_get_tick() -> u64 {
    coop_engine().map(|e| e.session.current_frame).unwrap_or(0)
}

/// Returns confirmed frame boundary watermark.
#[no_mangle]
pub extern "C" fn axiom_coop_get_confirmed_tick() -> u64 {
    coop_engine().map(|e| e.session.confirmed_frame).unwrap_or(0)
}

/// Returns total rollbacks performed by the session.
#[no_mangle]
pub extern "C" fn axiom_coop_get_rollbacks() -> u64 {
    coop_engine().map(|e| e.session.total_rollbacks).unwrap_or(0)
}

/// Returns total resimulated frames executed during rollbacks.
#[no_mangle]
pub extern "C" fn axiom_coop_get_resimulated() -> u64 {
    coop_engine().map(|e| e.session.total_frames_resimulated).unwrap_or(0)
}

/// Returns deterministic 64-bit master state hash.
#[no_mangle]
pub extern "C" fn axiom_coop_get_hash() -> u64 {
    coop_engine().map(|e| e.session.state_hash()).unwrap_or(0)
}

/// Returns the frame number of the most recent rollback.
#[no_mangle]
pub extern "C" fn axiom_coop_get_last_rollback() -> u64 {
    coop_engine().map(|e| e.last_rollback_frame).unwrap_or(0)
}

/// Returns cursor X coordinate.
#[no_mangle]
pub extern "C" fn axiom_coop_get_cursor_x() -> i32 {
    coop_engine().map(|e| e.cursor_x).unwrap_or(16)
}

/// Returns cursor Y coordinate.
#[no_mangle]
pub extern "C" fn axiom_coop_get_cursor_y() -> i32 {
    coop_engine().map(|e| e.cursor_y).unwrap_or(16)
}

/// Sets cursor coordinates directly.
#[no_mangle]
pub extern "C" fn axiom_coop_set_cursor(x: i32, y: i32) {
    if let Some(engine) = coop_engine() {
        engine.cursor_x = x.clamp(0, COOP_W as i32 - 1);
        engine.cursor_y = y.clamp(0, COOP_H as i32 - 1);
    }
}

// =========================================================================
// Interactive Factory Base / Defence / Workforce WebAssembly Bindings (T-044)
// =========================================================================

use axiom_core::ChunkCoord;
use axiom_game::{
    generate_ore_veins, Chest, ColonyWorkforce, Direction, FactoryGrid, FactoryStructure,
    ItemType, StructureKind, WaveDirector, WorkerRole, CYCLE, MONSTER_TEAM, STAMINA_MAX,
    TURRET_AMMO_CAP,
};

/// Factory grid exposed to the web canvas: one 32x32 chunk (64 KiB of 64-byte cells).
pub const FACTORY_W: usize = 32;
pub const FACTORY_H: usize = 32;
const FACTORY_MONSTER_CAP: usize = 64;
const FACTORY_WORKER_CAP: usize = 64;
/// Colonists hired at init; they auto-staff drills, smelters and inserters as they are built.
const FACTORY_COLONISTS: u32 = 16;
const FACTORY_HEART_X: usize = 16;
const FACTORY_HEART_Y: usize = 16;
/// Ore units in a deposit placed by hand (`FACTORY_KIND_DEPOSIT`).
const FACTORY_DEPOSIT_AMOUNT: u32 = 500;
/// Items pre-stocked in a chest placed with a non-`None` item (ammo supply).
const FACTORY_CHEST_STOCK: u32 = 500;
const FACTORY_DRILL_SPEED: u16 = 25;
const FACTORY_BELT_SPEED: u16 = 50;
const FACTORY_INSERTER_SPEED: u16 = 50;
const FACTORY_SMELTER_SPEED: u16 = 25;
/// Largest tile size accepted by `axiom_factory_render` (keeps rect math in `i32`).
const FACTORY_MAX_TILE_SIZE: u32 = 256;

/// Structure codes accepted by [`axiom_factory_place`].
pub const FACTORY_KIND_REMOVE: u32 = 0;
pub const FACTORY_KIND_BELT: u32 = 1;
pub const FACTORY_KIND_DRILL: u32 = 2;
pub const FACTORY_KIND_SMELTER: u32 = 3;
pub const FACTORY_KIND_CHEST: u32 = 4;
pub const FACTORY_KIND_WALL: u32 = 5;
pub const FACTORY_KIND_TURRET: u32 = 6;
pub const FACTORY_KIND_INSERTER: u32 = 7;
pub const FACTORY_KIND_DEPOSIT: u32 = 8;
pub const FACTORY_KIND_BASE_HEART: u32 = 9;

/// Number of `u32` words written by [`axiom_factory_get_stats`].
pub const FACTORY_STATS_WORDS: u32 = 24;

/// Flat telemetry block copied out by [`axiom_factory_get_stats`].
/// Every field is a `u32` so JavaScript reads it as a `Uint32Array` of
/// [`FACTORY_STATS_WORDS`] words in declaration order.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FactoryStats {
    pub tick: u32,
    pub total_mined: u32,
    pub total_smelted: u32,
    pub total_stored: u32,
    pub items_on_belts: u32,
    pub wave_number: u32,
    pub monsters_alive: u32,
    pub total_spawned: u32,
    pub total_killed: u32,
    pub total_shots: u32,
    pub heart_hp: u32,
    pub heart_max_hp: u32,
    /// `1` once the base heart has been destroyed.
    pub base_fallen: u32,
    pub walls: u32,
    pub turrets: u32,
    pub turret_ammo: u32,
    pub structures_destroyed: u32,
    pub workers_hired: u32,
    pub workers_working: u32,
    pub workers_resting: u32,
    pub workers_idle: u32,
    pub total_hauled: u32,
    pub cursor_x: u32,
    pub cursor_y: u32,
}

const _: () = assert!(core::mem::size_of::<FactoryStats>() == FACTORY_STATS_WORDS as usize * 4);

struct FactoryWasmEngine {
    grid: FactoryGrid<FACTORY_W, FACTORY_H>,
    director: WaveDirector<FACTORY_MONSTER_CAP>,
    workforce: ColonyWorkforce<FACTORY_WORKER_CAP>,
    framebuffer: FrameBuffer,
    /// Build cursor / camera centre, in tile coordinates.
    cursor_x: i32,
    cursor_y: i32,
    tick: u64,
}

static mut FACTORY: Option<Box<FactoryWasmEngine>> = None;

/// Shared accessor for the factory engine static.
///
/// SAFETY: the WASM host is single-threaded and every export funnels through here,
/// mirroring the other engine statics in this crate.
#[inline(always)]
fn factory_engine() -> Option<&'static mut FactoryWasmEngine> {
    unsafe { (*core::ptr::addr_of_mut!(FACTORY)).as_deref_mut() }
}

#[inline(always)]
fn factory_item_from_code(code: u32) -> ItemType {
    match code {
        1 => ItemType::IronOre,
        2 => ItemType::CopperOre,
        3 => ItemType::Coal,
        4 => ItemType::Stone,
        10 => ItemType::IronPlate,
        11 => ItemType::CopperPlate,
        12 => ItemType::StoneBrick,
        _ => ItemType::None,
    }
}

#[inline(always)]
fn factory_direction_from_code(code: u32) -> Direction {
    match code {
        1 => Direction::East,
        2 => Direction::South,
        3 => Direction::West,
        _ => Direction::North,
    }
}

#[inline(always)]
fn factory_item_color(item: ItemType) -> u32 {
    match item {
        ItemType::None => 0x00000000,
        ItemType::IronOre => 0xFF94A3B8,
        ItemType::CopperOre => 0xFFEA580C,
        ItemType::Coal => 0xFF27272A,
        ItemType::Stone => 0xFFA8A29E,
        ItemType::IronPlate => 0xFFE2E8F0,
        ItemType::CopperPlate => 0xFFFB923C,
        ItemType::StoneBrick => 0xFFD6D3D1,
    }
}

/// Seeds finite ore patches from the chunk `(0, 0)` vein roll of `seed`,
/// leaving a clear 3x3 pad around the base heart.
fn factory_seed_deposits(grid: &mut FactoryGrid<FACTORY_W, FACTORY_H>, seed: u64) {
    let veins = generate_ore_veins::<FACTORY_W>(seed, ChunkCoord::new(0, 0));
    for v in veins.active() {
        let r = v.radius as i32;
        for dy in -r..=r {
            for dx in -r..=r {
                let dist = (dx.abs() + dy.abs()) as u32;
                if dist > v.radius as u32 {
                    continue;
                }
                let (x, y) = (v.cx as i32 + dx, v.cy as i32 + dy);
                if x < 0 || y < 0 || x as usize >= FACTORY_W || y as usize >= FACTORY_H {
                    continue;
                }
                if (x - FACTORY_HEART_X as i32).abs() <= 1 && (y - FACTORY_HEART_Y as i32).abs() <= 1 {
                    continue;
                }
                grid.place_deposit(x as usize, y as usize, v.item, v.amount_at(dist));
            }
        }
    }
}

/// Integer Bresenham line with per-pixel clipping (turret tracer rounds).
fn factory_draw_line(fb: &mut FrameBuffer, mut x0: i32, mut y0: i32, x1: i32, y1: i32, color: u32) {
    let dx = (x1 - x0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let dy = -(y1 - y0).abs();
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy;
    loop {
        fb.set_pixel(x0, y0, color);
        if x0 == x1 && y0 == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x0 += sx;
        }
        if e2 <= dx {
            err += dx;
            y0 += sy;
        }
    }
}

/// Two-pixel-high durability bar along the top edge of a tile (only when damaged).
#[inline]
fn factory_draw_hp_bar(fb: &mut FrameBuffer, sx: i32, sy: i32, ts: i32, hp: u16, max_hp: u16) {
    if max_hp == 0 || hp >= max_hp {
        return;
    }
    let w = (ts - 2).max(1);
    let fill = (w as i64 * hp as i64 / max_hp as i64) as i32;
    fb.draw_rect(sx + 1, sy + 1, w, 2, 0xFF450A0A);
    fb.draw_rect(sx + 1, sy + 1, fill, 2, 0xFF22C55E);
}

/// Small notch on the output edge of a tile, showing a machine's facing.
#[inline]
fn factory_draw_facing(fb: &mut FrameBuffer, sx: i32, sy: i32, ts: i32, dir: Direction, color: u32) {
    let t = (ts / 6).max(2);
    match dir {
        Direction::North => fb.draw_rect(sx + 1, sy, ts - 2, t, color),
        Direction::East => fb.draw_rect(sx + ts - t, sy + 1, t, ts - 2, color),
        Direction::South => fb.draw_rect(sx + 1, sy + ts - t, ts - 2, t, color),
        Direction::West => fb.draw_rect(sx, sy + 1, t, ts - 2, color),
    }
}

/// Rasterises one factory tile at screen position `(sx, sy)`.
/// `phase` is the simulation tick used as the animation clock (integer only).
fn factory_draw_cell(
    fb: &mut FrameBuffer,
    cell: &FactoryStructure,
    sx: i32,
    sy: i32,
    ts: i32,
    phase: i32,
) {
    let inner = (ts - 2).max(1);
    let pip = (ts / 3).max(2);
    let pip_off = (ts - pip) / 2;
    match cell.kind {
        StructureKind::Empty => {}
        StructureKind::Deposit(d) => {
            fb.draw_rect(sx + 1, sy + 1, inner, inner, factory_item_color(d.item));
            if d.remaining < 100 {
                // Nearly exhausted patch: dark core grows as the reserve shrinks.
                fb.draw_rect(sx + pip_off, sy + pip_off, pip, pip, 0xFF0B1220);
            }
        }
        StructureKind::Belt(b) => {
            fb.draw_rect(sx + 1, sy + 1, inner, inner, 0xFF334155);
            factory_draw_facing(fb, sx, sy, ts, b.direction, 0xFF64748B);
            if !b.item.is_none() {
                let is = (ts * 2 / 5).max(3);
                let travel = (ts - is).max(0);
                let off = (travel as i64 * b.progress.min(CYCLE) as i64 / CYCLE as i64) as i32;
                let mid = (ts - is) / 2;
                let (ix, iy) = match b.direction {
                    Direction::North => (sx + mid, sy + travel - off),
                    Direction::East => (sx + off, sy + mid),
                    Direction::South => (sx + mid, sy + off),
                    Direction::West => (sx + travel - off, sy + mid),
                };
                fb.draw_rect(ix, iy, is, is, factory_item_color(b.item));
            }
        }
        StructureKind::Drill(d) => {
            fb.draw_rect(sx + 1, sy + 1, inner, inner, 0xFFB45309);
            let core = if d.remaining == 0 { 0xFF7F1D1D } else { 0xFF78350F };
            fb.draw_rect(sx + 2, sy + 2, (ts - 4).max(1), (ts - 4).max(1), core);
            if !d.ore.is_none() {
                fb.draw_rect(sx + pip_off, sy + pip_off, pip, pip, factory_item_color(d.ore));
            }
            factory_draw_facing(fb, sx, sy, ts, d.direction, 0xFFFCD34D);
            let bar = (inner as i64 * d.progress.min(CYCLE) as i64 / CYCLE as i64) as i32;
            fb.draw_rect(sx + 1, sy + ts - 3, bar, 2, 0xFFFBBF24);
        }
        StructureKind::Inserter(i) => {
            fb.draw_rect(sx + 2, sy + 2, (ts - 4).max(1), (ts - 4).max(1), 0xFF6D28D9);
            let t = (ts / 5).max(2);
            let c = ts / 2;
            match i.direction {
                Direction::North => fb.draw_rect(sx + c - t / 2, sy, t, c, 0xFFC4B5FD),
                Direction::East => fb.draw_rect(sx + c, sy + c - t / 2, ts - c, t, 0xFFC4B5FD),
                Direction::South => fb.draw_rect(sx + c - t / 2, sy + c, t, ts - c, 0xFFC4B5FD),
                Direction::West => fb.draw_rect(sx, sy + c - t / 2, c, t, 0xFFC4B5FD),
            }
            if !i.held.is_none() {
                fb.draw_rect(sx + pip_off, sy + pip_off, pip, pip, factory_item_color(i.held));
            }
        }
        StructureKind::Smelter(s) => {
            fb.draw_rect(sx + 1, sy + 1, inner, inner, 0xFF3F3F46);
            let mouth = (ts / 2).max(2);
            let mx = sx + (ts - mouth) / 2;
            let my = sy + ts - mouth - 1;
            fb.draw_rect(mx, my, mouth, mouth, 0xFF7F1D1D);
            let burning = s.progress > 0 || s.input_count > 0;
            if burning {
                let fire = if (phase / 4) % 2 == 0 { 0xFFF97316 } else { 0xFFFACC15 };
                let f = (mouth - 2).max(1);
                fb.draw_rect(mx + 1, my + 1, f, f, fire);
                // Smoke puffs drift upward on the integer tick clock.
                let puff = (ts / 6).max(2);
                let rise = (phase % 8) * ts / 8;
                fb.draw_rect(sx + ts / 2 - puff / 2, sy - rise - puff, puff, puff, 0xFF9CA3AF);
                fb.draw_rect(sx + ts / 2 + puff, sy - rise - puff * 2 + ts / 4, puff, puff, 0xFF6B7280);
            }
            if s.output_count > 0 {
                let bar = (inner as i64 * s.output_count as i64 / axiom_game::SMELTER_OUTPUT_CAP as i64) as i32;
                fb.draw_rect(sx + ts - 3, sy + 1 + inner - bar, 2, bar, factory_item_color(s.output));
            }
        }
        StructureKind::Chest(c) => {
            fb.draw_rect(sx + 1, sy + 1, inner, inner, 0xFF92400E);
            fb.draw_rect(sx + 1, sy + 1, inner, (ts / 4).max(1), 0xFFD97706);
            if c.quantity > 0 {
                let h = (ts - 4).max(1);
                let fill = (h as i64 * c.quantity.min(100) as i64 / 100) as i32;
                fb.draw_rect(sx + 2, sy + 2 + h - fill, (ts - 4).max(1), fill, factory_item_color(c.item));
            }
        }
        StructureKind::Wall(w) => {
            fb.draw_rect(sx, sy, ts, ts, 0xFF9CA3AF);
            fb.draw_rect(sx, sy + ts / 2, ts, 1, 0xFF6B7280);
            fb.draw_rect(sx + ts / 2, sy, 1, ts / 2, 0xFF6B7280);
            fb.draw_rect(sx + ts / 4, sy + ts / 2, 1, ts - ts / 2, 0xFF6B7280);
            factory_draw_hp_bar(fb, sx, sy, ts, w.hp, w.max_hp);
        }
        StructureKind::Turret(t) => {
            fb.draw_rect(sx + 1, sy + 1, inner, inner, 0xFF14532D);
            fb.draw_rect(sx + 2, sy + 2, (ts - 4).max(1), (ts - 4).max(1), 0xFF16A34A);
            fb.draw_rect(sx + pip_off, sy + pip_off, pip, pip, 0xFF052E16);
            // Ammo gauge: up to five pips along the bottom edge.
            let pips = (t.ammo as i32 * 5 / axiom_game::TURRET_AMMO_CAP as i32).min(5);
            let pw = (ts / 7).max(1);
            for i in 0..pips {
                fb.draw_rect(sx + 2 + i * (pw + 1), sy + ts - pw - 2, pw, pw, 0xFFFDE047);
            }
            if t.ammo == 0 {
                fb.draw_rect(sx + 2, sy + ts - pw - 2, pw, pw, 0xFFEF4444);
            }
            factory_draw_hp_bar(fb, sx, sy, ts, t.hp, t.max_hp);
        }
        StructureKind::BaseHeart(h) => {
            fb.draw_rect(sx, sy, ts, ts, 0xFF881337);
            fb.draw_rect(sx + 1, sy + 1, inner, inner, 0xFFBE123C);
            let pulse = pip + (phase / 8) % 2;
            let po = (ts - pulse) / 2;
            fb.draw_rect(sx + po, sy + po, pulse, pulse, 0xFFFFE4E6);
            factory_draw_hp_bar(fb, sx, sy, ts, h.hp, h.max_hp);
        }
        StructureKind::FastBelt(b) => {
            fb.draw_rect(sx + 1, sy + 1, inner, inner, 0xFF1E293B);
            factory_draw_facing(fb, sx, sy, ts, b.direction, 0xFF38BDF8);
            if !b.item.is_none() {
                let is = (ts * 2 / 5).max(3);
                let travel = (ts - is).max(0);
                let off = (travel as i64 * b.progress.min(CYCLE) as i64 / CYCLE as i64) as i32;
                let mid = (ts - is) / 2;
                let (ix, iy) = match b.direction {
                    Direction::North => (sx + mid, sy + travel - off),
                    Direction::East => (sx + off, sy + mid),
                    Direction::South => (sx + mid, sy + off),
                    Direction::West => (sx + travel - off, sy + mid),
                };
                fb.draw_rect(ix, iy, is, is, factory_item_color(b.item));
            }
        }
        StructureKind::FilterInserter(i) => {
            fb.draw_rect(sx + 2, sy + 2, (ts - 4).max(1), (ts - 4).max(1), 0xFF8B5CF6);
            let t = (ts / 5).max(2);
            let c = ts / 2;
            match i.direction {
                Direction::North => fb.draw_rect(sx + c - t / 2, sy, t, c, 0xFFDDD6FE),
                Direction::East => fb.draw_rect(sx + c, sy + c - t / 2, ts - c, t, 0xFFDDD6FE),
                Direction::South => fb.draw_rect(sx + c - t / 2, sy + c, t, ts - c, 0xFFDDD6FE),
                Direction::West => fb.draw_rect(sx, sy + c - t / 2, c, t, 0xFFDDD6FE),
            }
            if !i.held.is_none() {
                fb.draw_rect(sx + pip_off, sy + pip_off, pip, pip, factory_item_color(i.held));
            }
            if !i.filter.is_none() {
                fb.draw_rect(sx + 1, sy + 1, 2, 2, factory_item_color(i.filter));
            }
        }
        StructureKind::LaserTurret(t) => {
            fb.draw_rect(sx + 1, sy + 1, inner, inner, 0xFF0C4A6E);
            fb.draw_rect(sx + 2, sy + 2, (ts - 4).max(1), (ts - 4).max(1), 0xFF0284C7);
            fb.draw_rect(sx + pip_off, sy + pip_off, pip, pip, 0xFF38BDF8);
            let bar = (inner as i32 * t.energy as i32 / t.max_energy.max(1) as i32).min(inner);
            fb.draw_rect(sx + 1, sy + ts - 3, bar, 2, 0xFF38BDF8);
            factory_draw_hp_bar(fb, sx, sy, ts, t.hp, t.max_hp);
        }
    }
}

impl FactoryWasmEngine {
    /// Releases any colonist posted on `(x, y)` so the tile can change structure.
    fn vacate_post(&mut self, x: usize, y: usize) {
        let here = ChunkCoord::new(0, 0);
        for _ in 0..FACTORY_WORKER_CAP {
            match self.workforce.find_post(here, x as u16, y as u16) {
                Some(slot) => {
                    if !self.workforce.unassign(slot, &mut self.grid) {
                        break;
                    }
                }
                None => break,
            }
        }
    }

    /// Posts the first idle colonist on the machine at `(x, y)` with `role`.
    fn auto_staff(&mut self, x: usize, y: usize, role: WorkerRole) -> bool {
        let slot = match self
            .workforce
            .workers
            .iter()
            .position(|w| w.is_hired() && !w.is_posted())
        {
            Some(s) => s,
            None => return false,
        };
        self.workforce.assign(slot, role, x, y, &mut self.grid)
    }

    fn place(&mut self, x: u32, y: u32, kind: u32, direction: u32, item_type: u32) -> bool {
        let (x, y) = (x as usize, y as usize);
        if x >= FACTORY_W || y >= FACTORY_H || kind > FACTORY_KIND_BASE_HEART {
            return false;
        }
        let item = factory_item_from_code(item_type);
        if kind == FACTORY_KIND_DEPOSIT && item.is_none() {
            return false;
        }
        // The objective cannot be built over or demolished; only re-planting it is allowed.
        if matches!(self.grid.cells[y][x].kind, StructureKind::BaseHeart(_))
            && kind != FACTORY_KIND_BASE_HEART
        {
            return false;
        }

        self.vacate_post(x, y);
        let dir = factory_direction_from_code(direction);
        match kind {
            FACTORY_KIND_REMOVE => {
                // A demolished drill hands its unmined reserve back to the ground.
                let restored = match self.grid.cells[y][x].kind {
                    StructureKind::Drill(d) if d.remaining > 0 => Some((d.ore, d.remaining)),
                    _ => None,
                };
                match restored {
                    Some((ore, amount)) => self.grid.place_deposit(x, y, ore, amount),
                    None => self.grid.cells[y][x] = FactoryStructure::empty(),
                }
            }
            FACTORY_KIND_BELT => self.grid.place_belt(x, y, dir, FACTORY_BELT_SPEED),
            FACTORY_KIND_DRILL => {
                self.grid.place_drill(x, y, dir, FACTORY_DRILL_SPEED);
                self.auto_staff(x, y, WorkerRole::Miner);
            }
            FACTORY_KIND_SMELTER => {
                self.grid.place_smelter(x, y, FACTORY_SMELTER_SPEED);
                self.auto_staff(x, y, WorkerRole::SmelterOperator);
            }
            FACTORY_KIND_CHEST => {
                self.grid.place_chest(x, y);
                if !item.is_none() {
                    self.grid.cells[y][x].kind = StructureKind::Chest(Chest {
                        item,
                        quantity: FACTORY_CHEST_STOCK,
                    });
                }
            }
            FACTORY_KIND_WALL => self.grid.place_wall(x, y),
            FACTORY_KIND_TURRET => {
                self.grid.place_turret(x, y);
                if let StructureKind::Turret(ref mut t) = self.grid.cells[y][x].kind {
                    t.ammo = TURRET_AMMO_CAP;
                    t.ammo_item = ItemType::IronPlate;
                }
            }
            FACTORY_KIND_INSERTER => {
                self.grid.place_inserter(x, y, dir, FACTORY_INSERTER_SPEED);
                self.auto_staff(x, y, WorkerRole::Technician);
            }
            FACTORY_KIND_DEPOSIT => self.grid.place_deposit(x, y, item, FACTORY_DEPOSIT_AMOUNT),
            _ => self.grid.place_base_heart(x, y),
        }
        true
    }

    fn step(&mut self, move_x: i32, move_y: i32) {
        self.tick += 1;
        self.cursor_x = self.cursor_x.saturating_add(move_x).clamp(0, FACTORY_W as i32 - 1);
        self.cursor_y = self.cursor_y.saturating_add(move_y).clamp(0, FACTORY_H as i32 - 1);
        // Fixed phase order: waves/turrets/monsters -> colonist buffs -> factory logistics.
        self.director.tick(&mut self.grid);
        self.workforce.tick(&mut self.grid);
        self.grid.step();
    }

    fn render(&mut self, tile_size: u32) {
        let ts = tile_size.clamp(1, FACTORY_MAX_TILE_SIZE) as i32;
        let fb = &mut self.framebuffer;
        let grid = &self.grid;
        let fb_w = fb.width as i32;
        let fb_h = fb.height as i32;
        let phase = (self.tick & 0x7FFF_FFFF) as i32;
        let now = self.director.current_tick as u32;

        fb.clear(0xFF0B1220);

        // Camera: the cursor tile sits at the screen centre.
        let ox = fb_w / 2 - self.cursor_x * ts - ts / 2;
        let oy = fb_h / 2 - self.cursor_y * ts - ts / 2;

        // 1. Floor + structures (row-major, one cache line per tile).
        for y in 0..FACTORY_H {
            let sy = oy + y as i32 * ts;
            if sy + ts <= 0 || sy >= fb_h {
                continue;
            }
            for x in 0..FACTORY_W {
                let sx = ox + x as i32 * ts;
                if sx + ts <= 0 || sx >= fb_w {
                    continue;
                }
                let floor = if (x + y) % 2 == 0 { 0xFF162032 } else { 0xFF131C2C };
                fb.draw_rect(sx, sy, ts, ts, floor);
                factory_draw_cell(fb, &grid.cells[y][x], sx, sy, ts, phase);
            }
        }

        // 2. Monsters from the wave director's arena slab.
        let ms = (ts * 3 / 5).max(2);
        let mo = (ts - ms) / 2;
        for u in self.director.arena.units.iter() {
            if !u.is_alive() || u.team != MONSTER_TEAM {
                continue;
            }
            let sx = ox + u.pos.x.to_int() as i32 * ts;
            let sy = oy + u.pos.y.to_int() as i32 * ts;
            if sx + ts <= 0 || sx >= fb_w || sy + ts <= 0 || sy >= fb_h {
                continue;
            }
            fb.draw_rect(sx + mo, sy + mo, ms, ms, 0xFF7F1D1D);
            fb.draw_rect(sx + mo + 1, sy + mo + 1, (ms - 2).max(1), (ms - 2).max(1), 0xFFDC2626);
            factory_draw_hp_bar(fb, sx, sy, ts, u.current_hp, u.max_hp);
        }

        // 3. Tracer rounds: a turret that fired this tick has its action stamp on the
        //    current tick and a freshly reset cooldown timer.
        if now > 0 {
            for y in 0..FACTORY_H {
                for x in 0..FACTORY_W {
                    let cell = &grid.cells[y][x];
                    let t = match cell.kind {
                        StructureKind::Turret(t) => t,
                        _ => continue,
                    };
                    let fired = cell.last_action_tick == now && t.cooldown > 0 && t.cooldown_timer == t.cooldown;
                    if !fired {
                        continue;
                    }
                    let cx = ox + x as i32 * ts + ts / 2;
                    let cy = oy + y as i32 * ts + ts / 2;
                    let flash = (ts / 2).max(2);
                    fb.draw_rect(cx - flash / 2, cy - flash / 2, flash, flash, 0xFFFDE047);

                    let range_sq = t.range as i64 * t.range as i64;
                    let mut best: Option<(i32, i32)> = None;
                    let mut best_d = i64::MAX;
                    for u in self.director.arena.units.iter() {
                        if !u.is_alive() || u.team != MONSTER_TEAM {
                            continue;
                        }
                        let (ux, uy) = (u.pos.x.to_int(), u.pos.y.to_int());
                        let (dx, dy) = (ux - x as i64, uy - y as i64);
                        let d = dx * dx + dy * dy;
                        if d <= range_sq && d < best_d {
                            best_d = d;
                            best = Some((ux as i32, uy as i32));
                        }
                    }
                    if let Some((ux, uy)) = best {
                        let tx = ox + ux * ts + ts / 2;
                        let ty = oy + uy * ts + ts / 2;
                        factory_draw_line(fb, cx, cy, tx, ty, 0xFFFDE047);
                    }
                }
            }
        }

        // 4. Build cursor outline on the camera tile.
        let cx = fb_w / 2 - ts / 2;
        let cy = fb_h / 2 - ts / 2;
        fb.draw_rect(cx, cy, ts, 1, 0xFFFFFFFF);
        fb.draw_rect(cx, cy + ts - 1, ts, 1, 0xFFFFFFFF);
        fb.draw_rect(cx, cy, 1, ts, 0xFFFFFFFF);
        fb.draw_rect(cx + ts - 1, cy, 1, ts, 0xFFFFFFFF);

        // 5. HUD banner: heart HP bar, wave pips (gold) and living monsters (red).
        let report = self.director.report(grid);
        fb.draw_rect(15, 15, 300, 40, 0xFF1E293B);
        fb.draw_rect(22, 22, 120, 10, 0xFF450A0A);
        let heart = if report.heart_max_hp > 0 {
            (120 * report.heart_hp as i32) / report.heart_max_hp as i32
        } else {
            0
        };
        fb.draw_rect(22, 22, heart, 10, 0xFFF43F5E);
        for i in 0..(report.wave_number as i32).min(16) {
            fb.draw_rect(22 + i * 8, 38, 6, 6, 0xFFFBBF24);
        }
        for i in 0..(report.monsters_alive as i32).min(16) {
            fb.draw_rect(160 + i * 8, 38, 6, 6, 0xFFDC2626);
        }
        if report.base_fallen {
            fb.draw_rect(15, 55, 300, 6, 0xFFDC2626);
        }
    }

    fn stats(&self) -> FactoryStats {
        let f = self.grid.report();
        let d = self.director.report(&self.grid);
        let w = self.workforce.report();
        FactoryStats {
            tick: self.tick as u32,
            total_mined: f.total_mined as u32,
            total_smelted: f.total_smelted as u32,
            total_stored: f.total_stored as u32,
            items_on_belts: f.items_on_belts,
            wave_number: d.wave_number as u32,
            monsters_alive: d.monsters_alive,
            total_spawned: d.total_spawned as u32,
            total_killed: d.total_killed as u32,
            total_shots: d.total_shots as u32,
            heart_hp: d.heart_hp as u32,
            heart_max_hp: d.heart_max_hp as u32,
            base_fallen: d.base_fallen as u32,
            walls: d.walls,
            turrets: d.turrets,
            turret_ammo: d.turret_ammo,
            structures_destroyed: d.structures_destroyed as u32,
            workers_hired: w.hired,
            workers_working: w.working,
            workers_resting: w.resting,
            workers_idle: w.idle,
            total_hauled: w.total_hauled as u32,
            cursor_x: self.cursor_x as u32,
            cursor_y: self.cursor_y as u32,
        }
    }
}

/// Initializes the interactive factory base: a 32x32 grid with seeded ore veins, the
/// base heart at the centre, a seeded wave director and 16 hired colonists.
#[no_mangle]
pub extern "C" fn axiom_factory_init(seed: u64, width: u32, height: u32) {
    let mut grid = FactoryGrid::<FACTORY_W, FACTORY_H>::new();
    factory_seed_deposits(&mut grid, seed);
    grid.place_base_heart(FACTORY_HEART_X, FACTORY_HEART_Y);

    let mut director = WaveDirector::<FACTORY_MONSTER_CAP>::new(seed);
    director.bind_chunk(ChunkCoord::new(0, 0));

    let mut workforce = ColonyWorkforce::<FACTORY_WORKER_CAP>::new();
    for id in 1..=FACTORY_COLONISTS {
        let _ = workforce.hire_raw(id, STAMINA_MAX);
    }

    let framebuffer = FrameBuffer::new(width as usize, height as usize);

    unsafe {
        FACTORY = Some(Box::new(FactoryWasmEngine {
            grid,
            director,
            workforce,
            framebuffer,
            cursor_x: FACTORY_HEART_X as i32,
            cursor_y: FACTORY_HEART_Y as i32,
            tick: 0,
        }));
    }
}

/// Moves the build cursor by `(move_x, move_y)` tiles (clamped to the grid) and advances
/// defence, workforce and factory logistics by one tick. Heap-free.
#[no_mangle]
pub extern "C" fn axiom_factory_step(move_x: i32, move_y: i32) {
    if let Some(engine) = factory_engine() {
        engine.step(move_x, move_y);
    }
}

/// Builds or demolishes a structure at tile `(x, y)`.
///
/// * `kind`: one of the `FACTORY_KIND_*` codes (0 remove, 1 belt, 2 drill, 3 smelter,
///   4 chest, 5 wall, 6 turret, 7 inserter, 8 deposit, 9 base heart).
/// * `direction`: 0 north, 1 east, 2 south, 3 west (belts, drills, inserters).
/// * `item_type`: `ItemType` code; ore for a deposit, or pre-stocked ammo for a chest.
///
/// Returns `false` for out-of-range tiles, unknown codes, a deposit without an item,
/// or an attempt to build over / demolish the base heart.
#[no_mangle]
pub extern "C" fn axiom_factory_place(x: u32, y: u32, kind: u32, direction: u32, item_type: u32) -> bool {
    match factory_engine() {
        Some(engine) => engine.place(x, y, kind, direction, item_type),
        None => false,
    }
}

/// Renders the factory grid, ore, belts with items, machines, defences, tracer rounds,
/// monsters and the HUD into the internal framebuffer, camera centred on the cursor.
#[no_mangle]
pub extern "C" fn axiom_factory_render(tile_size: u32) {
    if let Some(engine) = factory_engine() {
        engine.render(tile_size);
    }
}

/// Returns raw pointer to the factory ARGB framebuffer memory.
#[no_mangle]
pub extern "C" fn axiom_factory_get_framebuffer_ptr() -> *const u32 {
    match factory_engine() {
        Some(engine) => engine.framebuffer.as_slice().as_ptr(),
        None => core::ptr::null(),
    }
}

/// Returns pixel count of the factory framebuffer.
#[no_mangle]
pub extern "C" fn axiom_factory_get_framebuffer_len() -> u32 {
    match factory_engine() {
        Some(engine) => engine.framebuffer.pixels.len() as u32,
        None => 0,
    }
}

/// Returns current factory simulation tick.
#[no_mangle]
pub extern "C" fn axiom_factory_get_tick() -> u64 {
    match factory_engine() {
        Some(engine) => engine.tick,
        None => 0,
    }
}

/// Copies a [`FactoryStats`] block (`FACTORY_STATS_WORDS` x `u32`) to `out_stats_ptr`.
/// Returns the number of words written, or `0` if the pointer is null or the engine
/// has not been initialized.
#[no_mangle]
pub extern "C" fn axiom_factory_get_stats(out_stats_ptr: *mut u32) -> u32 {
    if out_stats_ptr.is_null() {
        return 0;
    }
    match factory_engine() {
        Some(engine) => {
            let stats = engine.stats();
            // SAFETY: the caller guarantees `out_stats_ptr` points at at least
            // FACTORY_STATS_WORDS writable u32 words; the write is unaligned-safe.
            unsafe { core::ptr::write_unaligned(out_stats_ptr as *mut FactoryStats, stats) };
            FACTORY_STATS_WORDS
        }
        None => 0,
    }
}

/// Convenience telemetry getters for browser JavaScript integration.
#[no_mangle]
pub extern "C" fn axiom_factory_get_wave() -> u32 {
    factory_engine().map(|e| e.stats().wave_number).unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn axiom_factory_get_monsters_alive() -> u32 {
    factory_engine().map(|e| e.stats().monsters_alive).unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn axiom_factory_get_heart_hp() -> u32 {
    factory_engine().map(|e| e.stats().heart_hp).unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn axiom_factory_get_heart_max_hp() -> u32 {
    factory_engine().map(|e| e.stats().heart_max_hp).unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn axiom_factory_get_total_smelted() -> u32 {
    factory_engine().map(|e| e.stats().total_smelted).unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn axiom_factory_get_cursor_x() -> i32 {
    factory_engine().map(|e| e.cursor_x).unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn axiom_factory_get_cursor_y() -> i32 {
    factory_engine().map(|e| e.cursor_y).unwrap_or(0)
}

// ============================================================================
// Track 2: Native Mecha Boxing Web Bridge
// ============================================================================

struct BoxingWasmEngine {
    engine: BoxingRingEngine,
    framebuffer: FrameBuffer,
    cutin_frames: u16,
    cutin_kind: CutinKind,
    feedback_timer: u16,
    feedback_msg: &'static str,
    feedback_color: u32,
}

static mut BOXING: Option<Box<BoxingWasmEngine>> = None;

#[inline(always)]
fn boxing_engine() -> Option<&'static mut BoxingWasmEngine> {
    unsafe { (*core::ptr::addr_of_mut!(BOXING)).as_deref_mut() }
}

/// Initializes the mecha boxing simulation and framebuffer.
#[no_mangle]
pub extern "C" fn axiom_boxing_init(seed: u64, width: u32, height: u32) {
    let engine = BoxingRingEngine::new(seed);
    let framebuffer = FrameBuffer::new(width as usize, height as usize);
    unsafe {
        BOXING = Some(Box::new(BoxingWasmEngine {
            engine,
            framebuffer,
            cutin_frames: 0,
            cutin_kind: CutinKind::OverheatMeltdown,
            feedback_timer: 0,
            feedback_msg: "FIGHT START! [A/D: SLIP, S: GUARD, J/K/L: PUNCH, SPACE: CUTIN]",
            feedback_color: 0xFFFBBF24,
        }));
    }
}

/// Advances the boxing simulation by 1 frame.
/// Input codes: 0: Idle, 1: Jab, 2: Cross, 3: Hook, 4: Guard, 5: SlipLeft, 6: SlipRight, 7: Finisher
/// Returns event code: 0: None, 1: PunchHit, 2: CounterHit, 3: Evaded, 4: Blocked, 5: PlayerHit, 6: OpponentDown, 7: PlayerDown
#[no_mangle]
pub extern "C" fn axiom_boxing_step(action_code: u32) -> u32 {
    let engine = match boxing_engine() {
        Some(e) => e,
        None => return 0,
    };

    let action = match action_code {
        1 => ActionKind::Jab,
        2 => ActionKind::Cross,
        3 => ActionKind::Hook,
        4 => ActionKind::Guard,
        5 => ActionKind::SlipLeft,
        6 => ActionKind::SlipRight,
        7 => {
            if engine.cutin_frames == 0 && engine.engine.player.heat <= engine.engine.player.max_heat {
                engine.cutin_frames = 40;
                engine.cutin_kind = CutinKind::OverheatMeltdown;
            }
            ActionKind::Finisher
        }
        _ => ActionKind::Idle,
    };

    let event = engine.engine.step(action);

    let event_code = match event {
        BoxingEvent::None => 0,
        BoxingEvent::PlayerPunched { is_counter, .. } => {
            if is_counter {
                engine.feedback_msg = "COUNTER CRITICAL! [150% DMG]";
                engine.feedback_color = 0xFFF97316;
                2
            } else {
                engine.feedback_msg = "PUNCH HIT!";
                engine.feedback_color = 0xFFFBBF24;
                1
            }
        }
        BoxingEvent::PlayerEvaded => {
            engine.feedback_msg = "SLIP EVADED! COUNTER NOW!";
            engine.feedback_color = 0xFF38BDF8;
            3
        }
        BoxingEvent::PlayerBlocked { .. } => {
            engine.feedback_msg = "BLOCKED! [80% DAMAGE ABSORB]";
            engine.feedback_color = 0xFF10B981;
            4
        }
        BoxingEvent::PlayerStruck { .. } => {
            engine.feedback_msg = "HEAVY IMPACT TAKEN!";
            engine.feedback_color = 0xFFEF4444;
            5
        }
        BoxingEvent::OpponentDowned => {
            engine.feedback_msg = "KNOCKOUT! ENEMY DOWN!";
            engine.feedback_color = 0xFFF59E0B;
            6
        }
        BoxingEvent::PlayerDowned => {
            engine.feedback_msg = "SYSTEM OVERLOAD! KNOCKED DOWN!";
            engine.feedback_color = 0xFFDC2626;
            7
        }
        BoxingEvent::DraftOpened => {
            engine.feedback_msg = "STAGE CLEARED! SELECT TACTICAL CARD [1, 2, OR 3]";
            engine.feedback_color = 0xFFFBBF24;
            8
        }
        BoxingEvent::CardDrafted { .. } => {
            engine.feedback_msg = "UPGRADE EQUIPPED! ADVANCING TO NEXT ARENA...";
            engine.feedback_color = 0xFF38BDF8;
            9
        }
        BoxingEvent::StageCleared { .. } => {
            engine.feedback_msg = "NEXT OPPONENT ENGAGING!";
            engine.feedback_color = 0xFFF97316;
            10
        }
    };

    if event_code != 0 {
        engine.feedback_timer = 60;
    } else if engine.feedback_timer > 0 {
        engine.feedback_timer -= 1;
    }

    if engine.cutin_frames > 0 {
        engine.cutin_frames -= 1;
    }

    event_code
}

/// Renders the boxing scene to the framebuffer.
#[no_mangle]
pub extern "C" fn axiom_boxing_render() {
    let engine = match boxing_engine() {
        Some(e) => e,
        None => return,
    };

    let fb = &mut engine.framebuffer;
    let w = fb.width as i32;
    let h = fb.height as i32;

    let s = engine.engine.camera_shake as i32;
    let shake_x = if s > 0 { (((engine.engine.tick as i32 & 0x7) - 4) * s) / 3 } else { 0 };
    let shake_y = if s > 0 { ((((engine.engine.tick as i32 >> 3) & 0x7) - 4) * s) / 3 } else { 0 };

    fb.clear(0xFF07090E);

    let ring_cx = w / 2 + shake_x;
    let ring_cy = h / 2 + 35 + shake_y;

    // Ring canvas floor (perspective trapezoid)
    for y in (ring_cy - 120)..(ring_cy + 120) {
        let dy = y - (ring_cy - 120);
        let half_span = 140 + (dy * 90) / 240;
        for x in (ring_cx - half_span)..(ring_cx + half_span) {
            let checker = ((x / 32) + (y / 16)) % 2 == 0;
            let col = if checker { 0xFF141923 } else { 0xFF181F2C };
            fb.set_pixel(x, y, col);
        }
        fb.set_pixel(ring_cx - half_span, y, 0xFF0A0D14);
        fb.set_pixel(ring_cx + half_span, y, 0xFF0A0D14);
    }

    // Ring Ropes
    for &ry in &[ring_cy - 70, ring_cy - 30, ring_cy + 10] {
        fb.draw_rect(ring_cx - 220, ry, 440, 3, 0xFFDC2626);
        fb.draw_rect(ring_cx - 220, ry + 1, 440, 1, 0xFFFCA5A5);
    }

    // Opponent Mecha
    let opp_base_x = ring_cx;
    let opp_base_y = ring_cy - 50;
    let is_telegraph = engine.engine.ai_phase == AiPhase::Telegraph;
    let is_hit = engine.engine.opponent.action.kind == ActionKind::HitStagger;
    let is_opp_down = engine.engine.opponent.action.kind == ActionKind::Down;

    let opp_y = if is_opp_down { opp_base_y + 40 } else { opp_base_y };
    let opp_tint = if is_hit {
        0xFFFFFFFF
    } else if is_telegraph {
        0xFFEF4444
    } else {
        engine.engine.opponent_kind.armor_tint()
    };

    fb.draw_rect(opp_base_x - 28, opp_y - 48, 56, 48, 0xFF0F172A);
    fb.draw_rect(opp_base_x - 25, opp_y - 45, 50, 42, opp_tint);
    fb.draw_rect(opp_base_x - 25, opp_y - 25, 50, 22, 0xFF1E3A8A);

    fb.draw_rect(opp_base_x - 16, opp_y - 74, 32, 24, 0xFF0F172A);
    fb.draw_rect(opp_base_x - 14, opp_y - 72, 28, 20, 0xFF64748B);

    let eye_col = if is_telegraph { 0xFFFF0000 } else { 0xFF00FFCC };
    fb.draw_rect(opp_base_x - 10, opp_y - 66, 20, 6, eye_col);

    if !is_opp_down {
        let l_punch = if engine.engine.opponent.action.kind == ActionKind::Jab { 20 } else { 0 };
        let r_punch = if engine.engine.opponent.action.kind == ActionKind::Cross || engine.engine.opponent.action.kind == ActionKind::Hook { 30 } else { 0 };

        fb.draw_rect(opp_base_x - 48, opp_y - 35 + l_punch, 20, 20, 0xFF0F172A);
        fb.draw_rect(opp_base_x - 46, opp_y - 33 + l_punch, 16, 16, 0xFFDC2626);

        fb.draw_rect(opp_base_x + 28, opp_y - 35 + r_punch, 20, 20, 0xFF0F172A);
        fb.draw_rect(opp_base_x + 30, opp_y - 33 + r_punch, 16, 16, 0xFFDC2626);
    }

    // Player Mecha
    let player_base_x = ring_cx + engine.engine.player.x_offset_px as i32;
    let player_base_y = ring_cy + 110;
    let is_guard = engine.engine.player.action.kind == ActionKind::Guard;
    let is_player_hit = engine.engine.player.action.kind == ActionKind::HitStagger;
    let is_player_down = engine.engine.player.action.kind == ActionKind::Down;

    let player_y = if is_player_down { player_base_y + 50 } else { player_base_y };
    let player_col = if is_player_hit { 0xFFFFFFFF } else { 0xFF1E293B };

    fb.draw_rect(player_base_x - 44, player_y - 64, 88, 70, 0xFF020617);
    fb.draw_rect(player_base_x - 40, player_y - 60, 80, 62, player_col);
    fb.draw_rect(player_base_x - 20, player_y - 50, 40, 40, 0xFF0F172A);

    let heat_ratio = (engine.engine.player.heat as i32 * 10) / (engine.engine.player.max_heat as i32).max(1);
    let heat_col = if heat_ratio > 7 { 0xFFEF4444 } else if heat_ratio > 3 { 0xFFF97316 } else { 0xFF06B6D4 };
    fb.draw_rect(player_base_x - 8, player_y - 38, 16, 16, heat_col);

    if !is_player_down {
        let punch_y = match engine.engine.player.action.kind {
            ActionKind::Jab => -45,
            ActionKind::Cross => -55,
            ActionKind::Hook => -50,
            ActionKind::Finisher => -70,
            _ => 0,
        };

        let (glove_l_x, glove_l_y) = if is_guard { (player_base_x - 16, player_y - 56) } else { (player_base_x - 52, player_y - 40 + punch_y) };
        let (glove_r_x, glove_r_y) = if is_guard { (player_base_x + 2, player_y - 56) } else { (player_base_x + 34, player_y - 40 + punch_y) };

        fb.draw_rect(glove_l_x, glove_l_y, 24, 24, 0xFF020617);
        fb.draw_rect(glove_l_x + 2, glove_l_y + 2, 20, 20, 0xFF2563EB);

        fb.draw_rect(glove_r_x, glove_r_y, 24, 24, 0xFF020617);
        fb.draw_rect(glove_r_x + 2, glove_r_y + 2, 20, 20, 0xFF2563EB);
    }

    // HUD Elements
    fb.draw_rect_blend(w / 2 - 180, 12, 360, 26, 0xDD0B0F19);
    fb.draw_rect(w / 2 - 176, 16, 352, 18, 0xFF1E293B);
    let opp_ratio = (engine.engine.opponent.hp as usize * 352) / (engine.engine.opponent.max_hp as usize).max(1);
    fb.draw_rect(w / 2 - 176, 16, opp_ratio as i32, 18, 0xFFDC2626);
    let opp_label = alloc::format!("OPPONENT: {} (STAGE {})", engine.engine.opponent_kind.name(), engine.engine.stage);
    fb.draw_text(w / 2 - 170, 21, &opp_label, 0xFFFFFFFF);

    if engine.feedback_timer > 0 {
        let msg_len = (engine.feedback_msg.len() as i32) * 4;
        let bx = (w - msg_len) / 2;
        fb.draw_rect_blend(bx - 12, 44, msg_len + 24, 18, 0xEE0B0F19);
        fb.draw_text(bx, 49, engine.feedback_msg, engine.feedback_color);
    }

    // Bottom Left Player HP & Heat Gauges
    let bottom_y = h - 90;
    fb.draw_rect_blend(16, bottom_y, 200, 80, 0xEE0B0F19);
    fb.draw_text(24, bottom_y + 8, "PLAYER INTEGRITY (HP)", 0xFFE2E8F0);
    fb.draw_rect(24, bottom_y + 18, 180, 12, 0xFF1E293B);
    let hp_ratio = (engine.engine.player.hp as usize * 180) / (engine.engine.player.max_hp as usize).max(1);
    fb.draw_rect(24, bottom_y + 18, hp_ratio as i32, 12, 0xFF10B981);

    fb.draw_text(24, bottom_y + 36, "CORE HEAT (RISK)", 0xFFF97316);
    fb.draw_rect(24, bottom_y + 46, 180, 10, 0xFF1E293B);
    let heat_ratio = (engine.engine.player.heat as usize * 180) / (engine.engine.player.max_heat as usize).max(1);
    let heat_bar_col = if engine.engine.player.heat > 80 { 0xFFDC2626 } else { 0xFFF97316 };
    fb.draw_rect(24, bottom_y + 46, heat_ratio as i32, 10, heat_bar_col);

    let score_str = alloc::format!("SCORE: {} | STAGE: {} | PWR: +{}", engine.engine.score, engine.engine.stage, engine.engine.bonus_power);
    fb.draw_text(24, bottom_y + 62, &score_str, 0xFFFBBF24);

    // Bottom Right Controls Hint
    fb.draw_rect_blend(w - 230, bottom_y, 214, 80, 0xEE0B0F19);
    fb.draw_text(w - 222, bottom_y + 8, "PUNCH-OUT CONTROLS", 0xFFFBBF24);
    fb.draw_text(w - 222, bottom_y + 22, "A / D : SLIP WEAVE", 0xFFE2E8F0);
    fb.draw_text(w - 222, bottom_y + 36, "S     : GUARD BLOCK", 0xFFE2E8F0);
    fb.draw_text(w - 222, bottom_y + 50, "J/K/L : JAB/CROSS/HOOK", 0xFF38BDF8);
    fb.draw_text(w - 222, bottom_y + 64, "SPACE : 2D ANIME CUTIN", 0xFFF43F5E);

    // 2D Anime Cut-in Overlay
    if engine.cutin_frames > 0 {
        let permille = (40 - engine.cutin_frames) as u32 * 1000 / 40;
        render_cutin_overlay(fb, engine.cutin_kind, permille, ring_cx, ring_cy);
    }

    // Tactical Overlays (Card Draft / Run Defeat)
    if engine.engine.fight_phase == FightPhase::Drafting {
        fb.draw_rect_blend(0, 0, w, h, 0xDD050811);
        fb.draw_rect_blend(w / 2 - 240, 45, 480, 44, 0xEE0F172A);
        fb.draw_rect(w / 2 - 238, 47, 476, 40, 0xFF334155);
        fb.draw_text_2x(w / 2 - 210, 52, "★ STAGE CLEARED! ★", 0xFFFBBF24);
        fb.draw_text(w / 2 - 160, 74, "SELECT 1 OF 3 TACTICAL REPAIR UPGRADES", 0xFFE2E8F0);

        let card_w = 190;
        let card_h = 240;
        let gap = 20;
        let start_x = (w - (card_w * 3 + gap * 2)) / 2;
        let start_y = 115;

        for i in 0..3 {
            let cx = start_x + (i as i32) * (card_w + gap);
            let cy = start_y;
            let card = engine.engine.draft_options[i];

            fb.draw_rect(cx, cy, card_w, card_h, 0xFF020617);
            fb.draw_rect(cx + 2, cy + 2, card_w - 4, card_h - 4, 0xFF1E293B);

            let (bar_col, text_col) = match i {
                0 => (0xFF0284C7, 0xFF38BDF8),
                1 => (0xFFD97706, 0xFFFBBF24),
                _ => (0xFF7C3AED, 0xFFC084FC),
            };
            fb.draw_rect(cx + 4, cy + 4, card_w - 8, 30, bar_col);
            let key_char = match i { 0 => 'J', 1 => 'K', _ => 'L' };
            let key_str = alloc::format!("[KEY {} / {}]", i + 1, key_char);
            fb.draw_text(cx + 10, cy + 12, &key_str, 0xFFFFFFFF);

            fb.draw_text(cx + 10, cy + 45, card.kind.name(), text_col);

            fb.draw_rect(cx + 8, cy + 65, card_w - 16, 50, 0xFF0F172A);
            let p_str = alloc::format!("POWER: +{}", card.power);
            let h_str = alloc::format!("HEAT:  +{}", card.heat);
            let g_str = alloc::format!("GUARD: +{}", card.guard);
            fb.draw_text(cx + 14, cy + 72, &p_str, 0xFFF87171);
            fb.draw_text(cx + 14, cy + 86, &h_str, 0xFFFB923C);
            fb.draw_text(cx + 14, cy + 100, &g_str, 0xFF34D399);

            fb.draw_rect(cx + 8, cy + 125, card_w - 16, 75, 0xFF0B0F19);
            fb.draw_text(cx + 12, cy + 135, card.kind.description(), 0xFFE2E8F0);

            fb.draw_rect(cx + 12, cy + 208, card_w - 24, 22, bar_col);
            let eq_str = alloc::format!("EQUIP CARD [{}]", i + 1);
            fb.draw_text(cx + 35, cy + 214, &eq_str, 0xFFFFFFFF);
        }

        fb.draw_rect_blend(w / 2 - 200, h - 55, 400, 28, 0xEE0F172A);
        fb.draw_text(w / 2 - 180, h - 46, "PRESS [1, 2, 3] OR [J, K, L] TO EQUIP & ENTER NEXT STAGE", 0xFF94A3B8);
    } else if engine.engine.fight_phase == FightPhase::Defeat {
        fb.draw_rect_blend(0, 0, w, h, 0xEE1E0505);
        fb.draw_rect_blend(w / 2 - 200, h / 2 - 50, 400, 100, 0xEE0B0F19);
        fb.draw_text_2x(w / 2 - 130, h / 2 - 35, "MECHA DOWN!", 0xFFEF4444);
        let final_str = alloc::format!("FINAL SCORE: {}", engine.engine.score);
        fb.draw_text(w / 2 - 100, h / 2 + 5, &final_str, 0xFFFBBF24);
        fb.draw_text(w / 2 - 140, h / 2 + 25, "PRESS SPACE TO REBOOT MECHA RUN", 0xFFE2E8F0);
    }
}

#[no_mangle]
pub extern "C" fn axiom_boxing_draft_card(choice_idx: u32) -> u32 {
    let engine = match boxing_engine() {
        Some(e) => e,
        None => return 0,
    };
    if engine.engine.draft_card(choice_idx as usize) {
        engine.feedback_msg = "UPGRADE EQUIPPED! ADVANCING TO NEXT ARENA...";
        engine.feedback_color = 0xFF38BDF8;
        engine.feedback_timer = 60;
        1
    } else {
        0
    }
}

#[no_mangle]
pub extern "C" fn axiom_boxing_get_phase() -> u32 {
    boxing_engine().map(|e| e.engine.fight_phase as u32).unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn axiom_boxing_get_stage() -> u32 {
    boxing_engine().map(|e| e.engine.stage as u32).unwrap_or(1)
}

#[no_mangle]
pub extern "C" fn axiom_boxing_get_bonus_power() -> u32 {
    boxing_engine().map(|e| e.engine.bonus_power as u32).unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn axiom_boxing_get_framebuffer_ptr() -> *const u32 {
    boxing_engine().map(|e| e.framebuffer.as_slice().as_ptr()).unwrap_or(core::ptr::null())
}

#[no_mangle]
pub extern "C" fn axiom_boxing_get_framebuffer_len() -> u32 {
    boxing_engine().map(|e| e.framebuffer.as_slice().len() as u32).unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn axiom_boxing_get_tick() -> u64 {
    boxing_engine().map(|e| e.engine.tick).unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn axiom_boxing_get_player_hp() -> u32 {
    boxing_engine().map(|e| e.engine.player.hp as u32).unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn axiom_boxing_get_player_heat() -> u32 {
    boxing_engine().map(|e| e.engine.player.heat as u32).unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn axiom_boxing_get_opponent_hp() -> u32 {
    boxing_engine().map(|e| e.engine.opponent.hp as u32).unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn axiom_boxing_get_score() -> u32 {
    boxing_engine().map(|e| e.engine.score).unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn axiom_boxing_is_ko() -> u32 {
    boxing_engine()
        .map(|e| {
            if e.engine.opponent.action.kind == ActionKind::Down
                || e.engine.player.action.kind == ActionKind::Down
            {
                1
            } else {
                0
            }
        })
        .unwrap_or(0)
}

// ============================================================================
// Track 3: WebGL/WASM 3D Cel-Shading Software Rasterizer Bridge
// ============================================================================

#[allow(static_mut_refs)]
struct Wasm3DEngine {
    rasterizer: Rasterizer3D,
    framebuffer: FrameBuffer,
    model: MechaModel3D,
    yaw_bam: u8,
    pitch_bam: u8,
    cam_dist_q16: i32,
    focal_length: i64,
    auto_rotate: bool,
    rim_lighting: bool,
    cel_palette_idx: u32,
    inverted_hull: bool,
    tick: u64,
}

static mut ENGINE_3D: Option<Box<Wasm3DEngine>> = None;
#[allow(static_mut_refs)]
static mut GLB_STAGING_BUFFER: Vec<u8> = Vec::new();

#[inline(always)]
fn wasm_3d_engine() -> Option<&'static mut Wasm3DEngine> {
    unsafe { (*core::ptr::addr_of_mut!(ENGINE_3D)).as_deref_mut() }
}

/// Allocates or resizes staging buffer in WASM memory for binary GLB transmission.
/// Returns pointer to the start of the buffer.
#[no_mangle]
#[allow(static_mut_refs)]
pub extern "C" fn axiom_3d_alloc_glb_buffer(len: u32) -> *mut u8 {
    unsafe {
        GLB_STAGING_BUFFER.clear();
        GLB_STAGING_BUFFER.resize(len as usize, 0);
        GLB_STAGING_BUFFER.as_mut_ptr()
    }
}

/// Parses the GLB binary currently residing in the staging buffer.
/// Returns 1 on success, 0 on failure.
#[no_mangle]
#[allow(static_mut_refs)]
pub extern "C" fn axiom_3d_parse_staged_glb() -> u32 {
    unsafe {
        if GLB_STAGING_BUFFER.len() < 12 {
            return 0;
        }
        match parse_glb(&GLB_STAGING_BUFFER) {
            Ok(model) => {
                if let Some(engine) = wasm_3d_engine() {
                    engine.model = model;
                    1
                } else {
                    0
                }
            }
            Err(_) => 0,
        }
    }
}

/// Initializes the 3D software rasterizer engine and loads procedural ELPIS mecha chassis.
#[no_mangle]
pub extern "C" fn axiom_3d_init(width: u32, height: u32) {
    let w = width.max(64) as usize;
    let h = height.max(64) as usize;
    let rasterizer = Rasterizer3D::new(w, h);
    let framebuffer = FrameBuffer::new(w, h);
    let model = MechaModel3D::procedural_chassis();

    unsafe {
        ENGINE_3D = Some(Box::new(Wasm3DEngine {
            rasterizer,
            framebuffer,
            model,
            yaw_bam: 32, // 45-degree default view
            pitch_bam: 0,
            cam_dist_q16: (2.15 * 65536.0) as i32, // 2.15m camera distance
            focal_length: 520,
            auto_rotate: true,
            rim_lighting: false,
            cel_palette_idx: 0, // Canon per-part materials
            inverted_hull: true,
            tick: 0,
        }));
    }
}

/// Ingests a Tripo/Blender GLB binary directly from WebAssembly memory.
/// Returns 1 on success, 0 on failure.
#[no_mangle]
pub extern "C" fn axiom_3d_load_glb(bytes_ptr: *const u8, len: u32) -> u32 {
    if bytes_ptr.is_null() || len < 12 {
        return 0;
    }
    let slice = unsafe { core::slice::from_raw_parts(bytes_ptr, len as usize) };
    match parse_glb(slice) {
        Ok(model) => {
            if let Some(engine) = wasm_3d_engine() {
                engine.model = model;
                1
            } else {
                0
            }
        }
        Err(_) => 0,
    }
}

/// Resets the 3D model to the procedural canon ELPIS chassis.
#[no_mangle]
pub extern "C" fn axiom_3d_reset_procedural() {
    if let Some(engine) = wasm_3d_engine() {
        engine.model = MechaModel3D::procedural_chassis();
    }
}

/// Advances 3D simulation / turntable rotation by 1 frame.
#[no_mangle]
pub extern "C" fn axiom_3d_step(yaw_delta: i32, pitch_delta: i32) {
    if let Some(engine) = wasm_3d_engine() {
        engine.tick += 1;
        if yaw_delta != 0 || pitch_delta != 0 {
            engine.auto_rotate = false;
            engine.yaw_bam = engine.yaw_bam.wrapping_add(yaw_delta as u8);
            engine.pitch_bam = (engine.pitch_bam as i32 + pitch_delta).clamp(-64, 64) as u8;
        } else if engine.auto_rotate {
            engine.yaw_bam = engine.yaw_bam.wrapping_add(1);
        }
    }
}

/// Configures 3D camera distance and focal length.
#[no_mangle]
pub extern "C" fn axiom_3d_set_cam(dist_q16: i32, focal_length: i64) {
    if let Some(engine) = wasm_3d_engine() {
        engine.cam_dist_q16 = dist_q16.clamp(65536, 65536 * 10);
        engine.focal_length = focal_length.clamp(100, 2000);
    }
}

/// Configures 3D rendering pipeline flags:
/// - auto_rotate: 0: disabled, 1: enabled
/// - rim_lighting: 0: disabled, 1: enabled
/// - palette_idx: 0: Canon per-part, 1: Golden Nova, 2: Gunmetal Steel, 3: Overheat Red
/// - inverted_hull: 0: disabled, 1: enabled
#[no_mangle]
pub extern "C" fn axiom_3d_set_options(
    auto_rotate: u32,
    rim_lighting: u32,
    palette_idx: u32,
    inverted_hull: u32,
) {
    if let Some(engine) = wasm_3d_engine() {
        engine.auto_rotate = auto_rotate != 0;
        engine.rim_lighting = rim_lighting != 0;
        engine.cel_palette_idx = palette_idx;
        engine.inverted_hull = inverted_hull != 0;
    }
}

/// Renders the 3D scene using pure CPU software rasterization to the internal framebuffer.
#[no_mangle]
pub extern "C" fn axiom_3d_render() {
    let engine = match wasm_3d_engine() {
        Some(e) => e,
        None => return,
    };

    let w = engine.framebuffer.width as i32;
    let h = engine.framebuffer.height as i32;

    // High-tech dark steel canvas
    engine.framebuffer.clear(0xFF0B0F19);

    // Draw radial turntable pedestal floor
    let ped_cy = h - (h / 4);
    let ped_cx = w / 2;
    for r in (30..180).step_by(25) {
        let ring_col = if r % 50 == 0 { 0xFF1E293B } else { 0xFF141C2B };
        for ang in 0..128 {
            let cos_val = cos_bams((ang * 2) as u8).raw();
            let sin_val = sin_bams((ang * 2) as u8).raw();
            let px = ped_cx + ((cos_val * r) >> 16) as i32;
            let py = ped_cy + ((sin_val * (r / 3)) >> 16) as i32;
            engine.framebuffer.set_pixel(px, py, ring_col);
        }
    }

    // Configure 3D rasterizer parameters
    engine.rasterizer.clear_depth();
    engine.rasterizer.cull_backfaces = false;
    engine.rasterizer.draw_outline = engine.inverted_hull;
    engine.rasterizer.rim_light_color = if engine.rim_lighting {
        Some(match engine.cel_palette_idx {
            1 => 0xFFFFD700, // Golden Nova
            2 => 0xFFFFFFFF, // Flash White
            3 => 0xFFEF4444, // Overheat Red
            _ => 0xFF38BDF8, // Cyan Beacon
        })
    } else {
        None
    };

    engine.rasterizer.custom_palette = match engine.cel_palette_idx {
        1 => Some(CelShadingPalette {
            highlight: 0xFFFFFBEB,
            base: 0xFFF59E0B,
            shadow: 0xFFB45309,
            outline: CelShadingPalette::INK,
        }),
        2 => Some(CelShadingPalette {
            highlight: 0xFFF8FAFC,
            base: 0xFF64748B,
            shadow: 0xFF1E293B,
            outline: CelShadingPalette::INK,
        }),
        3 => Some(CelShadingPalette {
            highlight: 0xFFFEE2E2,
            base: 0xFFDC2626,
            shadow: 0xFF7F1D1D,
            outline: CelShadingPalette::INK,
        }),
        _ => None, // Canon multi-material per part
    };

    // Transform: center mecha around origin, rotate by yaw & pitch
    let center_offset = Vec3Fixed::from_f32(0.0, -0.19, 0.0);
    let trans_mat = Mat4Fixed::from_translation(center_offset);
    let rot_y = Mat4Fixed::from_rotation_y(engine.yaw_bam);
    let rot_x = Mat4Fixed::from_rotation_x(engine.pitch_bam);
    let rot_mat = rot_x.mul_mat(&rot_y);
    let world_mat = rot_mat.mul_mat(&trans_mat);

    engine.rasterizer.render_model(
        &mut engine.framebuffer,
        &engine.model,
        &world_mat,
        Fixed::from_raw(engine.cam_dist_q16 as i64),
        engine.focal_length,
    );
}

#[no_mangle]
pub extern "C" fn axiom_3d_get_framebuffer_ptr() -> *const u32 {
    wasm_3d_engine()
        .map(|e| e.framebuffer.pixels.as_ptr())
        .unwrap_or(core::ptr::null())
}

#[no_mangle]
pub extern "C" fn axiom_3d_get_framebuffer_len() -> u32 {
    wasm_3d_engine()
        .map(|e| e.framebuffer.pixels.len() as u32)
        .unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn axiom_3d_get_triangles() -> u32 {
    wasm_3d_engine()
        .map(|e| e.model.total_triangles() as u32)
        .unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn axiom_3d_get_vertices() -> u32 {
    wasm_3d_engine()
        .map(|e| e.model.total_vertices() as u32)
        .unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn axiom_3d_get_yaw() -> u32 {
    wasm_3d_engine().map(|e| e.yaw_bam as u32).unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn axiom_3d_get_pitch() -> u32 {
    wasm_3d_engine().map(|e| e.pitch_bam as u32).unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn axiom_3d_get_tick() -> u64 {
    wasm_3d_engine().map(|e| e.tick).unwrap_or(0)
}

// ============================================================================
// Track 3.5: Mecha Custom Texture Painting & Decal Studio C-ABI Bridge
// ============================================================================

#[allow(static_mut_refs)]
static mut ACTIVE_TEXTURE: Option<TextureMap> = None;

#[inline(always)]
fn active_texture() -> Option<&'static mut TextureMap> {
    unsafe { (*core::ptr::addr_of_mut!(ACTIVE_TEXTURE)).as_mut() }
}

/// Initializes or resets the active texture painting canvas with power-of-two size (e.g. 128 or 256).
#[no_mangle]
#[allow(static_mut_refs)]
pub extern "C" fn axiom_texture_init(size: u32) {
    let size = size.clamp(16, 512).next_power_of_two();
    let mut tex = TextureMap::new(size, size);
    tex.clear(0xFF38BDF8); // Default Sky Blue base coat
    unsafe {
        ACTIVE_TEXTURE = Some(tex);
    }
}

/// Returns pointer to the raw 32-bit ARGB pixels of the active texture canvas.
#[no_mangle]
pub extern "C" fn axiom_texture_get_ptr() -> *const u32 {
    active_texture()
        .map(|t| t.pixels.as_ptr())
        .unwrap_or(core::ptr::null())
}

/// Returns the width/height (power of two) of the active texture canvas.
#[no_mangle]
pub extern "C" fn axiom_texture_get_size() -> u32 {
    active_texture().map(|t| t.width).unwrap_or(0)
}

/// Clears active texture with a solid ARGB color.
#[no_mangle]
pub extern "C" fn axiom_texture_clear(color: u32) {
    if let Some(tex) = active_texture() {
        tex.clear(color);
    }
}

/// Paints a circular brush stroke on the active texture.
#[no_mangle]
pub extern "C" fn axiom_texture_paint(cx: i32, cy: i32, radius: i32, color: u32, opacity: u32) {
    if let Some(tex) = active_texture() {
        tex.paint_brush(cx, cy, radius.max(1), color, opacity.min(255) as u8);
    }
}

/// Draws an interpolated brush stroke line between (x0, y0) and (x1, y1).
#[no_mangle]
pub extern "C" fn axiom_texture_line(
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
    color: u32,
    radius: i32,
    opacity: u32,
) {
    if let Some(tex) = active_texture() {
        tex.draw_line(x0, y0, x1, y1, color, radius.max(1), opacity.min(255) as u8);
    }
}

/// Executes a scanline flood fill on the active texture starting from (x, y).
#[no_mangle]
pub extern "C" fn axiom_texture_flood_fill(x: u32, y: u32, color: u32) {
    if let Some(tex) = active_texture() {
        tex.flood_fill(x, y, color);
    }
}

/// Stamps a procedural decal onto the active texture at (cx, cy) with scale (256 = 1.0x).
#[no_mangle]
pub extern "C" fn axiom_texture_stamp_decal(
    decal_idx: u32,
    cx: i32,
    cy: i32,
    scale_q8: u32,
) {
    let kind = match decal_idx {
        0 => DecalKind::Star,
        1 => DecalKind::HazardStripes,
        2 => DecalKind::SkullCrossbones,
        3 => DecalKind::Number01,
        4 => DecalKind::SponsorAxiom,
        5 => DecalKind::TargetReticle,
        _ => DecalKind::CheckeredFlag,
    };
    if let Some(tex) = active_texture() {
        let decal = TextureMap::generate_decal(kind, (tex.width / 2).max(32));
        tex.stamp_decal(&decal, cx, cy, scale_q8.clamp(32, 1024));
    }
}

/// Loads a procedural base preset onto the active texture:
/// 0: Hazard Stripes, 1: Industrial Metal Plating, 2: Military Camo.
#[no_mangle]
pub extern "C" fn axiom_texture_load_procedural(preset_idx: u32) {
    if let Some(tex) = active_texture() {
        let size = tex.width;
        let generated = match preset_idx {
            0 => TextureMap::procedural_hazard_stripes(size),
            1 => TextureMap::procedural_metal_plating(size),
            _ => TextureMap::procedural_camo(size),
        };
        *tex = generated;
    }
}

/// Binds the active texture canvas directly to the 3D software rasterizer for live turntable preview.
#[no_mangle]
pub extern "C" fn axiom_texture_apply_to_3d() -> u32 {
    if let (Some(engine), Some(tex)) = (wasm_3d_engine(), active_texture()) {
        engine.rasterizer.custom_texture = Some(tex.clone());
        1
    } else {
        0
    }
}

/// Detaches custom texture from the 3D rasterizer, restoring default cel-shading materials.
#[no_mangle]
pub extern "C" fn axiom_texture_remove_from_3d() {
    if let Some(engine) = wasm_3d_engine() {
        engine.rasterizer.custom_texture = None;
    }
}

// ============================================================================
// Track 4: P2P Multiplayer Boxing Rollback Netcode C-ABI Bridge
// ============================================================================

#[allow(static_mut_refs)]
struct NetboxWasmEngine {
    session: BoxingRollbackSession<64>,
    spectator: Option<SpectatorRelaySession<64>>,
    tournament: Option<TournamentEngine<4>>,
    director: Option<TournamentDirector>,
    telemetry: TournamentBroadcastTelemetry,
    framebuffer: FrameBuffer,
    wire_buffer: [u8; 64],
}

static mut NETBOX: Option<Box<NetboxWasmEngine>> = None;

#[inline(always)]
fn netbox_engine() -> Option<&'static mut NetboxWasmEngine> {
    unsafe { (*core::ptr::addr_of_mut!(NETBOX)).as_deref_mut() }
}

#[no_mangle]
pub extern "C" fn axiom_netbox_init(local_player_id: u32, seed: u64, width: u32, height: u32) {
    let initial_ring = BoxingRingEngine::new(seed);
    let session = BoxingRollbackSession::new(local_player_id as usize, initial_ring);
    let framebuffer = FrameBuffer::new(width as usize, height as usize);
    unsafe {
        NETBOX = Some(Box::new(NetboxWasmEngine {
            session,
            spectator: None,
            tournament: None,
            director: None,
            telemetry: TournamentBroadcastTelemetry::default(),
            framebuffer,
            wire_buffer: [0u8; 64],
        }));
    }
}

#[no_mangle]
pub extern "C" fn axiom_netbox_get_wire_buffer_ptr() -> *mut u8 {
    netbox_engine().map(|e| e.wire_buffer.as_mut_ptr()).unwrap_or(core::ptr::null_mut())
}

#[inline]
fn u32_to_action_kind(val: u32) -> ActionKind {
    match val {
        1 => ActionKind::Jab,
        2 => ActionKind::Cross,
        3 => ActionKind::Hook,
        4 => ActionKind::Guard,
        5 => ActionKind::SlipLeft,
        6 => ActionKind::SlipRight,
        7 => ActionKind::Finisher,
        _ => ActionKind::Idle,
    }
}

#[no_mangle]
pub extern "C" fn axiom_netbox_advance_frame(local_input: u32) -> u64 {
    let engine = match netbox_engine() {
        Some(e) => e,
        None => return 0,
    };
    let action = u32_to_action_kind(local_input);
    let stats = engine.session.advance_frame(action);
    stats.state_hash
}

#[no_mangle]
pub extern "C" fn axiom_netbox_receive_remote(remote_player_id: u32, frame: u64, input: u32) -> bool {
    let engine = match netbox_engine() {
        Some(e) => e,
        None => return false,
    };
    let action = u32_to_action_kind(input);
    engine.session.receive_remote_input(remote_player_id as usize, frame, action)
}

#[no_mangle]
pub extern "C" fn axiom_netbox_get_wire_packet_size() -> u32 {
    NetboxWirePacket::SIZE as u32
}

#[no_mangle]
pub extern "C" fn axiom_netbox_create_wire_packet(
    out_ptr: *mut u8,
    max_len: u32,
    local_input: u32,
    sequence: u32,
) -> u32 {
    let engine = match netbox_engine() {
        Some(e) => e,
        None => return 0,
    };
    if max_len < NetboxWirePacket::SIZE as u32 || out_ptr.is_null() {
        return 0;
    }
    let action = u32_to_action_kind(local_input);
    let pkt = engine.session.create_wire_packet(action, sequence as u16);
    let bytes = pkt.to_bytes();
    unsafe {
        core::ptr::copy_nonoverlapping(bytes.as_ptr(), out_ptr, NetboxWirePacket::SIZE);
    }
    NetboxWirePacket::SIZE as u32
}

#[no_mangle]
pub extern "C" fn axiom_netbox_receive_wire_packet(in_ptr: *const u8, len: u32) -> u32 {
    let engine = match netbox_engine() {
        Some(e) => e,
        None => return 0,
    };
    if in_ptr.is_null() || len < NetboxWirePacket::SIZE as u32 {
        return 0;
    }
    let slice = unsafe { core::slice::from_raw_parts(in_ptr, NetboxWirePacket::SIZE) };
    let pkt = match NetboxWirePacket::from_bytes(slice) {
        Some(p) => p,
        None => return 0,
    };
    let (rolled, rolled_count) = engine.session.receive_redundant_packet(&pkt);
    if rolled {
        rolled_count
    } else {
        0
    }
}

#[no_mangle]
pub extern "C" fn axiom_netbox_get_frame() -> u64 {
    netbox_engine().map(|e| e.session.current_frame).unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn axiom_netbox_get_confirmed_frame() -> u64 {
    netbox_engine().map(|e| e.session.confirmed_frame).unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn axiom_netbox_get_total_rollbacks() -> u64 {
    netbox_engine().map(|e| e.session.total_rollbacks).unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn axiom_netbox_get_total_resimulated() -> u64 {
    netbox_engine().map(|e| e.session.total_frames_resimulated).unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn axiom_netbox_get_hash() -> u64 {
    netbox_engine().map(|e| e.session.current_state.compute_hash()).unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn axiom_netbox_spectator_init(seed: u64) {
    if let Some(engine) = netbox_engine() {
        let ring = BoxingRingEngine::new(seed);
        engine.spectator = Some(SpectatorRelaySession::new(ring));
    }
}

#[no_mangle]
pub extern "C" fn axiom_netbox_spectator_receive_wire_packet(in_ptr: *const u8, len: u32) -> u32 {
    let engine = match netbox_engine() {
        Some(e) => e,
        None => return 0,
    };
    if in_ptr.is_null() || len < NetboxWirePacket::SIZE as u32 {
        return 0;
    }
    let slice = unsafe { core::slice::from_raw_parts(in_ptr, NetboxWirePacket::SIZE) };
    let pkt = match NetboxWirePacket::from_bytes(slice) {
        Some(p) => p,
        None => return 0,
    };
    if let Some(spec) = &mut engine.spectator {
        spec.receive_packet(pkt);
        1
    } else {
        0
    }
}

#[no_mangle]
pub extern "C" fn axiom_netbox_spectator_step() -> u32 {
    let engine = match netbox_engine() {
        Some(e) => e,
        None => return 0,
    };
    if let Some(spec) = &mut engine.spectator {
        if spec.step_frame().is_some() {
            1
        } else {
            0
        }
    } else {
        0
    }
}

#[no_mangle]
pub extern "C" fn axiom_netbox_spectator_get_frame() -> u64 {
    netbox_engine()
        .and_then(|e| e.spectator.as_ref())
        .map(|s| s.current_frame)
        .unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn axiom_netbox_spectator_get_hash() -> u64 {
    netbox_engine()
        .and_then(|e| e.spectator.as_ref())
        .map(|s| s.last_state_hash)
        .unwrap_or(0)
}

// --- Tournament Bracket WASM C-ABI ---

#[no_mangle]
pub extern "C" fn axiom_tournament_init_4(p0_id: u32, p1_id: u32, p2_id: u32, p3_id: u32) {
    if let Some(engine) = netbox_engine() {
        let names: [&[u8]; 4] = [b"ELPIS", b"TINKER", b"CIPHER", b"THESEUS"];
        let f0 = TournamentFighter::new(p0_id as u8, 1, names[(p0_id as usize) % 4]);
        let f1 = TournamentFighter::new(p1_id as u8, 4, names[(p1_id as usize) % 4]);
        let f2 = TournamentFighter::new(p2_id as u8, 2, names[(p2_id as usize) % 4]);
        let f3 = TournamentFighter::new(p3_id as u8, 3, names[(p3_id as usize) % 4]);
        engine.tournament = Some(TournamentEngine::new_4_bracket(f0, f1, f2, f3));
    }
}

#[no_mangle]
pub extern "C" fn axiom_tournament_get_match_status(match_id: u32) -> u32 {
    netbox_engine()
        .and_then(|e| e.tournament.as_ref())
        .and_then(|t| t.matches.get(match_id as usize))
        .map(|m| m.status as u32)
        .unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn axiom_tournament_get_match_fighter_a(match_id: u32) -> u32 {
    netbox_engine()
        .and_then(|e| e.tournament.as_ref())
        .and_then(|t| t.matches.get(match_id as usize))
        .and_then(|m| m.fighter_a.as_ref())
        .map(|f| f.id as u32)
        .unwrap_or(0xFF)
}

#[no_mangle]
pub extern "C" fn axiom_tournament_get_match_fighter_b(match_id: u32) -> u32 {
    netbox_engine()
        .and_then(|e| e.tournament.as_ref())
        .and_then(|t| t.matches.get(match_id as usize))
        .and_then(|m| m.fighter_b.as_ref())
        .map(|f| f.id as u32)
        .unwrap_or(0xFF)
}

#[no_mangle]
pub extern "C" fn axiom_tournament_get_match_winner(match_id: u32) -> u32 {
    netbox_engine()
        .and_then(|e| e.tournament.as_ref())
        .and_then(|t| t.matches.get(match_id as usize))
        .and_then(|m| m.winner.as_ref())
        .map(|w| w.id as u32)
        .unwrap_or(0xFF)
}

#[no_mangle]
pub extern "C" fn axiom_tournament_record_result(match_id: u32, score_a: u32, score_b: u32) -> u32 {
    if let Some(engine) = netbox_engine() {
        if let Some(t) = &mut engine.tournament {
            if let Ok(winner) = t.record_match_result(match_id as u8, score_a as u8, score_b as u8) {
                return winner.id as u32;
            }
        }
    }
    0xFF
}

#[no_mangle]
pub extern "C" fn axiom_tournament_get_champion() -> u32 {
    netbox_engine()
        .and_then(|e| e.tournament.as_ref())
        .and_then(|t| t.champion.as_ref())
        .map(|c| c.id as u32)
        .unwrap_or(0xFF)
}

#[no_mangle]
pub extern "C" fn axiom_tournament_get_hash() -> u64 {
    netbox_engine()
        .and_then(|e| e.tournament.as_ref())
        .map(|t| t.compute_hash())
        .unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn axiom_tournament_broadcast_init_4(p0_id: u32, p1_id: u32, p2_id: u32, p3_id: u32) {
    if let Some(engine) = netbox_engine() {
        let f0 = TournamentFighter::new(p0_id as u8, 1, b"ELPIS");
        let f1 = TournamentFighter::new(p1_id as u8, 4, b"TINKER");
        let f2 = TournamentFighter::new(p2_id as u8, 2, b"CIPHER");
        let f3 = TournamentFighter::new(p3_id as u8, 3, b"THESEUS");

        let director = TournamentDirector::new_4(f0, f1, f2, f3);
        engine.telemetry = director.telemetry();
        engine.director = Some(director);
    }
}

#[no_mangle]
pub extern "C" fn axiom_tournament_broadcast_step() -> u32 {
    if let Some(engine) = netbox_engine() {
        if let Some(d) = &mut engine.director {
            let ev = d.step();
            engine.telemetry = d.telemetry();
            return ev as u32;
        }
    }
    0
}

#[no_mangle]
pub extern "C" fn axiom_tournament_broadcast_get_telemetry_ptr() -> *const TournamentBroadcastTelemetry {
    if let Some(engine) = netbox_engine() {
        &engine.telemetry as *const _
    } else {
        core::ptr::null()
    }
}

#[no_mangle]
pub extern "C" fn axiom_tournament_broadcast_get_phase() -> u32 {
    netbox_engine()
        .and_then(|e| e.director.as_ref())
        .map(|d| d.phase as u32)
        .unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn axiom_tournament_broadcast_get_spectators() -> u32 {
    netbox_engine()
        .and_then(|e| e.director.as_ref())
        .map(|d| d.spectator_count)
        .unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn axiom_tournament_broadcast_set_spectators(count: u32) {
    if let Some(engine) = netbox_engine() {
        if let Some(d) = &mut engine.director {
            d.spectator_count = count;
            engine.telemetry.spectator_count = count;
        }
    }
}

#[no_mangle]
pub extern "C" fn axiom_tournament_broadcast_get_active_match() -> u32 {
    netbox_engine()
        .and_then(|e| e.director.as_ref())
        .map(|d| d.tournament.current_match_idx as u32)
        .unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn axiom_tournament_broadcast_get_active_frame() -> u32 {
    netbox_engine()
        .and_then(|e| e.director.as_ref())
        .map(|d| d.match_frame)
        .unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn axiom_tournament_broadcast_get_last_winner() -> u32 {
    netbox_engine()
        .and_then(|e| e.director.as_ref())
        .map(|d| d.last_winner_id as u32)
        .unwrap_or(0xFF)
}

#[no_mangle]
pub extern "C" fn axiom_netbox_get_p0_hp() -> u32 {
    netbox_engine().map(|e| e.session.current_state.player.hp as u32).unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn axiom_netbox_get_p1_hp() -> u32 {
    netbox_engine().map(|e| e.session.current_state.opponent.hp as u32).unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn axiom_netbox_get_p0_heat() -> u32 {
    netbox_engine().map(|e| e.session.current_state.player.heat as u32).unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn axiom_netbox_get_p1_heat() -> u32 {
    netbox_engine().map(|e| e.session.current_state.opponent.heat as u32).unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn axiom_netbox_render() {
    let engine = match netbox_engine() {
        Some(e) => e,
        None => return,
    };
    let fb = &mut engine.framebuffer;
    let w = fb.width as i32;
    let h = fb.height as i32;

    fb.clear(0xFF090D16);

    let ring_cx = w / 2;
    let ring_cy = h / 2 + 35;

    // Ring canvas floor
    for y in (ring_cy - 120)..(ring_cy + 120) {
        let dy = y - (ring_cy - 120);
        let half_span = 140 + (dy * 90) / 240;
        for x in (ring_cx - half_span)..(ring_cx + half_span) {
            let checker = ((x / 32) + (y / 16)) % 2 == 0;
            let col = if checker { 0xFF141923 } else { 0xFF181F2C };
            fb.set_pixel(x, y, col);
        }
        fb.set_pixel(ring_cx - half_span, y, 0xFF0A0D14);
        fb.set_pixel(ring_cx + half_span, y, 0xFF0A0D14);
    }

    // Ring Ropes
    for &ry in &[ring_cy - 70, ring_cy - 30, ring_cy + 10] {
        fb.draw_rect(ring_cx - 220, ry, 440, 3, 0xFFDC2626);
        fb.draw_rect(ring_cx - 220, ry + 1, 440, 1, 0xFFFCA5A5);
    }

    let ring = &engine.session.current_state;

    // P1 Mecha (Opponent / Blue Corner)
    let p1_x = ring_cx + ring.opponent.x_offset_px as i32;
    let p1_y = ring_cy - 50;
    let p1_col = if ring.opponent.action.kind == ActionKind::HitStagger { 0xFFFFFFFF } else { 0xFF38BDF8 };
    fb.draw_rect(p1_x - 24, p1_y - 40, 48, 40, 0xFF0F172A);
    fb.draw_rect(p1_x - 22, p1_y - 38, 44, 36, p1_col);
    fb.draw_rect(p1_x - 14, p1_y - 60, 28, 20, 0xFF64748B);
    fb.draw_rect(p1_x - 8, p1_y - 54, 16, 6, 0xFFFF0000);

    // P0 Mecha (Local / Red Corner)
    let p0_x = ring_cx + ring.player.x_offset_px as i32;
    let p0_y = ring_cy + 100;
    let p0_col = if ring.player.action.kind == ActionKind::HitStagger { 0xFFFFFFFF } else { 0xFFDC2626 };
    fb.draw_rect(p0_x - 30, p0_y - 50, 60, 50, 0xFF0F172A);
    fb.draw_rect(p0_x - 27, p0_y - 47, 54, 44, p0_col);
    fb.draw_rect(p0_x - 16, p0_y - 72, 32, 22, 0xFF475569);
    fb.draw_rect(p0_x - 10, p0_y - 66, 20, 6, 0xFF00FFCC);
}

#[no_mangle]
pub extern "C" fn axiom_netbox_get_framebuffer_ptr() -> *const u32 {
    netbox_engine().map(|e| e.framebuffer.pixels.as_ptr()).unwrap_or(core::ptr::null())
}

#[no_mangle]
pub extern "C" fn axiom_netbox_get_framebuffer_len() -> u32 {
    netbox_engine().map(|e| e.framebuffer.pixels.len() as u32).unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Zero-Float Procedural Audio Synthesis, 3D Spatial Panning & AudioWorklet C-ABI
// ---------------------------------------------------------------------------

#[no_mangle]
pub extern "C" fn axiom_audio_sample_rate() -> u32 {
    audio::SAMPLE_RATE
}

#[no_mangle]
pub extern "C" fn axiom_audio_synthesize(
    kind: u32,
    pitch_permill: u32,
    reverb_decay_permill: u32,
    pan_x: i32,
    pan_z: i32,
) -> u32 {
    let sound_kind = audio::SoundKind::from_u32(kind);
    audio::synthesize_to_static_buffer(
        sound_kind,
        pitch_permill as u16,
        reverb_decay_permill as u16,
        pan_x,
        pan_z,
    ) as u32
}

#[no_mangle]
pub extern "C" fn axiom_audio_get_interleaved_ptr() -> *const i16 {
    audio::get_stereo_buffer_ptr()
}

#[no_mangle]
pub extern "C" fn axiom_audio_get_frame_count() -> u32 {
    audio::get_stereo_frame_count() as u32
}

#[no_mangle]
pub extern "C" fn axiom_audio_worklet_play(
    kind: u32,
    pitch_permill: u32,
    reverb_decay_permill: u32,
    pan_x: i32,
    pan_z: i32,
) -> i32 {
    let mixer = audio::get_worklet_mixer();
    let sound_kind = audio::SoundKind::from_u32(kind);
    if mixer.play_sound(
        sound_kind,
        pitch_permill as u16,
        reverb_decay_permill as u16,
        pan_x,
        pan_z,
    ) {
        1
    } else {
        0
    }
}

#[no_mangle]
pub extern "C" fn axiom_audio_worklet_render_block() -> *const i16 {
    audio::render_worklet_block()
}

#[no_mangle]
pub extern "C" fn axiom_audio_worklet_active_voices() -> u32 {
    let mixer = audio::get_worklet_mixer();
    mixer.active_voice_count() as u32
}

#[no_mangle]
pub extern "C" fn axiom_audio_play_voxel_blast(x_norm: i32, z_norm: i32, radius: i32) -> i32 {
    let x_mm = x_norm.clamp(-1000, 1000);
    let z_mm = z_norm.clamp(-1000, 1000);
    let pitch = (1000 - (radius * 30)).clamp(600, 1200) as u32;
    axiom_audio_worklet_play(14, pitch, 850, x_mm, z_mm)
}

#[no_mangle]
pub extern "C" fn axiom_audio_play_debris_bounce(x_norm: i32, z_norm: i32, material: u32) -> i32 {
    let x_mm = x_norm.clamp(-1000, 1000);
    let z_mm = z_norm.clamp(-1000, 1000);
    let pitch = match material {
        2 => 1200, // Metal: higher pitch clank
        3 => 800,  // Molten slag: sizzle
        _ => 1000, // Stone: clatter
    };
    axiom_audio_worklet_play(15, pitch, 600, x_mm, z_mm)
}

// ---------------------------------------------------------------------------
// 3D Mecha Boxing Deterministic Replay Player (.axrp)
// ---------------------------------------------------------------------------

use axiom_core::replay::DeterministicReplay;

pub struct WasmReplayPlayer {
    pub replay: DeterministicReplay,
    pub inputs: Vec<(u8, u8)>,
    pub engine: BoxingRingEngine,
    pub current_frame: u32,
    pub p0_action: u8,
    pub p1_action: u8,
    pub state_hash: u64,
}

impl WasmReplayPlayer {
    pub fn new(replay: DeterministicReplay) -> Self {
        let inputs = replay.decompress_inputs();
        let engine = BoxingRingEngine::new(replay.header.match_seed);
        let state_hash = engine.compute_hash();
        Self {
            replay,
            inputs,
            engine,
            current_frame: 0,
            p0_action: 0,
            p1_action: 0,
            state_hash,
        }
    }

    pub fn reset(&mut self) {
        self.engine = BoxingRingEngine::new(self.replay.header.match_seed);
        self.current_frame = 0;
        self.p0_action = 0;
        self.p1_action = 0;
        self.state_hash = self.engine.compute_hash();
    }

    pub fn step(&mut self) -> u32 {
        if self.current_frame >= self.replay.header.total_frames {
            return self.current_frame;
        }

        let (p0, p1) = self.inputs.get(self.current_frame as usize).copied().unwrap_or((0, 0));
        self.p0_action = p0;
        self.p1_action = p1;

        let act0 = ActionKind::from_u8(p0);
        let act1 = ActionKind::from_u8(p1);
        self.engine.step_pvp(act0, act1);
        self.current_frame += 1;
        self.state_hash = self.engine.compute_hash();

        self.current_frame
    }

    pub fn seek(&mut self, target_frame: u32) -> u32 {
        let max_frame = self.replay.header.total_frames;
        let clamped_target = target_frame.min(max_frame);

        if clamped_target < self.current_frame {
            self.reset();
        }

        while self.current_frame < clamped_target {
            self.step();
        }

        self.current_frame
    }
}

static mut REPLAY_PLAYER: Option<WasmReplayPlayer> = None;

#[no_mangle]
pub extern "C" fn axiom_replay_load(bytes_ptr: *const u8, bytes_len: usize) -> u32 {
    if bytes_ptr.is_null() || bytes_len < 64 {
        return 0;
    }
    let slice = unsafe { core::slice::from_raw_parts(bytes_ptr, bytes_len) };
    match DeterministicReplay::deserialize(slice) {
        Ok(rep) => {
            let total = rep.header.total_frames;
            let player = WasmReplayPlayer::new(rep);
            unsafe {
                REPLAY_PLAYER = Some(player);
            }
            total
        }
        Err(_) => 0,
    }
}

#[no_mangle]
pub extern "C" fn axiom_replay_total_frames() -> u32 {
    unsafe {
        REPLAY_PLAYER.as_ref().map(|p| p.replay.header.total_frames).unwrap_or(0)
    }
}

#[no_mangle]
pub extern "C" fn axiom_replay_current_frame() -> u32 {
    unsafe {
        REPLAY_PLAYER.as_ref().map(|p| p.current_frame).unwrap_or(0)
    }
}

#[no_mangle]
pub extern "C" fn axiom_replay_match_seed() -> u64 {
    unsafe {
        REPLAY_PLAYER.as_ref().map(|p| p.replay.header.match_seed).unwrap_or(0)
    }
}

#[no_mangle]
pub extern "C" fn axiom_replay_winner_id() -> u8 {
    unsafe {
        REPLAY_PLAYER.as_ref().map(|p| p.replay.header.winner_id).unwrap_or(0xFF)
    }
}

#[no_mangle]
pub extern "C" fn axiom_replay_p0_score() -> u8 {
    unsafe {
        REPLAY_PLAYER.as_ref().map(|p| p.replay.header.final_score_p0).unwrap_or(0)
    }
}

#[no_mangle]
pub extern "C" fn axiom_replay_p1_score() -> u8 {
    unsafe {
        REPLAY_PLAYER.as_ref().map(|p| p.replay.header.final_score_p1).unwrap_or(0)
    }
}

#[no_mangle]
pub extern "C" fn axiom_replay_p0_hp() -> i32 {
    unsafe {
        REPLAY_PLAYER.as_ref().map(|p| p.engine.player.hp).unwrap_or(0)
    }
}

#[no_mangle]
pub extern "C" fn axiom_replay_p1_hp() -> i32 {
    unsafe {
        REPLAY_PLAYER.as_ref().map(|p| p.engine.opponent.hp).unwrap_or(0)
    }
}

#[no_mangle]
pub extern "C" fn axiom_replay_p0_action() -> u8 {
    unsafe {
        REPLAY_PLAYER.as_ref().map(|p| p.p0_action).unwrap_or(0)
    }
}

#[no_mangle]
pub extern "C" fn axiom_replay_p1_action() -> u8 {
    unsafe {
        REPLAY_PLAYER.as_ref().map(|p| p.p1_action).unwrap_or(0)
    }
}

#[no_mangle]
pub extern "C" fn axiom_replay_state_hash() -> u64 {
    unsafe {
        REPLAY_PLAYER.as_ref().map(|p| p.state_hash).unwrap_or(0)
    }
}

#[no_mangle]
pub extern "C" fn axiom_replay_is_finished() -> u32 {
    unsafe {
        REPLAY_PLAYER.as_ref()
            .map(|p| if p.current_frame >= p.replay.header.total_frames { 1 } else { 0 })
            .unwrap_or(1)
    }
}

#[no_mangle]
pub extern "C" fn axiom_replay_step() -> u32 {
    unsafe {
        if let Some(player) = REPLAY_PLAYER.as_mut() {
            player.step()
        } else {
            0
        }
    }
}

#[no_mangle]
pub extern "C" fn axiom_replay_seek(target_frame: u32) -> u32 {
    unsafe {
        if let Some(player) = REPLAY_PLAYER.as_mut() {
            player.seek(target_frame)
        } else {
            0
        }
    }
}

#[no_mangle]
pub extern "C" fn axiom_replay_reset() {
    unsafe {
        if let Some(player) = REPLAY_PLAYER.as_mut() {
            player.reset();
        }
    }
}

#[no_mangle]
pub extern "C" fn axiom_replay_free() {
    unsafe {
        REPLAY_PLAYER = None;
    }
}

// ============================================================================
// Track 5: Steamworks Multiplayer Lobby & Global ELO Leaderboard C-ABI Bridge
// ============================================================================

#[allow(static_mut_refs)]
static mut ACTIVE_LOBBY: Option<SteamLobby<MAX_LOBBY_MEMBERS>> = None;
#[allow(static_mut_refs)]
static mut GLOBAL_LEADERBOARD: Option<LeaderboardTable<64>> = None;

#[inline(always)]
#[allow(static_mut_refs)]
fn get_lobby() -> Option<&'static mut SteamLobby<MAX_LOBBY_MEMBERS>> {
    unsafe { (*core::ptr::addr_of_mut!(ACTIVE_LOBBY)).as_mut() }
}

#[inline(always)]
#[allow(static_mut_refs)]
fn get_leaderboard() -> &'static mut LeaderboardTable<64> {
    unsafe {
        if GLOBAL_LEADERBOARD.is_none() {
            let mut lb = LeaderboardTable::<64>::new();
            lb.find_or_create(76561198000000001, b"AxiomPrime");
            lb.find_or_create(76561198000000002, b"SteelVanguard");
            lb.find_or_create(76561198000000003, b"ShadowStrike");
            lb.find_or_create(76561198000000004, b"NeonGladiator");
            if let Some(idx) = lb.find_or_create(76561198000000001, b"AxiomPrime") {
                lb.entries[idx].rating = 2150;
                lb.entries[idx].matches_won = 42;
                lb.entries[idx].kos = 31;
                lb.entries[idx].flawless_victories = 12;
                lb.entries[idx].highest_win_streak = 15;
                lb.entries[idx].current_win_streak = 8;
            }
            if let Some(idx) = lb.find_or_create(76561198000000002, b"SteelVanguard") {
                lb.entries[idx].rating = 1820;
                lb.entries[idx].matches_won = 28;
                lb.entries[idx].kos = 19;
                lb.entries[idx].flawless_victories = 5;
            }
            if let Some(idx) = lb.find_or_create(76561198000000003, b"ShadowStrike") {
                lb.entries[idx].rating = 1540;
                lb.entries[idx].matches_won = 17;
                lb.entries[idx].kos = 11;
            }
            if let Some(idx) = lb.find_or_create(76561198000000004, b"NeonGladiator") {
                lb.entries[idx].rating = 1280;
                lb.entries[idx].matches_won = 8;
                lb.entries[idx].kos = 4;
            }
            lb.sort_by_rank();
            GLOBAL_LEADERBOARD = Some(lb);
        }
        (*core::ptr::addr_of_mut!(GLOBAL_LEADERBOARD)).as_mut().unwrap()
    }
}

#[inline(always)]
fn combine_u64(lo: u32, hi: u32) -> u64 {
    ((hi as u64) << 32) | (lo as u64)
}

#[no_mangle]
pub extern "C" fn axiom_lobby_create(
    host_id_lo: u32,
    host_id_hi: u32,
    name_ptr: *const u8,
    name_len: u32,
    code_ptr: *const u8,
) -> u32 {
    let host_id = combine_u64(host_id_lo, host_id_hi);
    let name_slice = if !name_ptr.is_null() && name_len > 0 {
        unsafe { core::slice::from_raw_parts(name_ptr, name_len as usize) }
    } else {
        b"Host"
    };

    let mut code_bytes = *b"AXM-0001";
    if !code_ptr.is_null() {
        let code_slice = unsafe { core::slice::from_raw_parts(code_ptr, 8) };
        code_bytes.copy_from_slice(code_slice);
    }

    unsafe {
        ACTIVE_LOBBY = Some(SteamLobby::new(host_id, name_slice, &code_bytes));
    }
    1
}

#[no_mangle]
pub extern "C" fn axiom_lobby_join(
    id_lo: u32,
    id_hi: u32,
    name_ptr: *const u8,
    name_len: u32,
    rating: u32,
    role: u32,
) -> u32 {
    let lobby = match get_lobby() {
        Some(l) => l,
        None => return 0,
    };
    let steam_id = combine_u64(id_lo, id_hi);
    let name_slice = if !name_ptr.is_null() && name_len > 0 {
        unsafe { core::slice::from_raw_parts(name_ptr, name_len as usize) }
    } else {
        b"Challenger"
    };

    let lobby_role = match role {
        0 => LobbyRole::Player1,
        1 => LobbyRole::Player2,
        _ => LobbyRole::Spectator,
    };

    if lobby.join(steam_id, name_slice, rating, lobby_role) {
        1
    } else {
        0
    }
}

#[no_mangle]
pub extern "C" fn axiom_lobby_leave(id_lo: u32, id_hi: u32) -> u32 {
    let lobby = match get_lobby() {
        Some(l) => l,
        None => return 0,
    };
    let steam_id = combine_u64(id_lo, id_hi);
    if lobby.leave(steam_id) {
        1
    } else {
        0
    }
}

#[no_mangle]
pub extern "C" fn axiom_lobby_set_ready(id_lo: u32, id_hi: u32, ready: u32) -> u32 {
    let lobby = match get_lobby() {
        Some(l) => l,
        None => return 0,
    };
    let steam_id = combine_u64(id_lo, id_hi);
    if lobby.set_ready(steam_id, ready != 0) {
        1
    } else {
        0
    }
}

#[no_mangle]
pub extern "C" fn axiom_lobby_get_state() -> u32 {
    get_lobby().map(|l| l.state as u32).unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn axiom_lobby_get_member_count() -> u32 {
    get_lobby().map(|l| l.member_count() as u32).unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn axiom_lobby_can_start() -> u32 {
    get_lobby().map(|l| if l.can_start_match() { 1 } else { 0 }).unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn axiom_lobby_get_room_code(out_buf: *mut u8) -> u32 {
    let lobby = match get_lobby() {
        Some(l) => l,
        None => return 0,
    };
    if out_buf.is_null() {
        return 0;
    }
    unsafe {
        core::ptr::copy_nonoverlapping(lobby.room_code.as_ptr(), out_buf, 8);
    }
    8
}

#[no_mangle]
pub extern "C" fn axiom_lobby_get_member_info(index: u32, out_stats: *mut u32) -> u32 {
    let lobby = match get_lobby() {
        Some(l) => l,
        None => return 0,
    };
    let idx = index as usize;
    if idx >= lobby.members.len() || out_stats.is_null() {
        return 0;
    }

    if let Some(m) = &lobby.members[idx] {
        let lo = (m.steam_id & 0xFFFF_FFFF) as u32;
        let hi = (m.steam_id >> 32) as u32;
        unsafe {
            *out_stats.add(0) = lo;
            *out_stats.add(1) = hi;
            *out_stats.add(2) = m.rating;
            *out_stats.add(3) = m.role as u32;
            *out_stats.add(4) = if m.ready { 1 } else { 0 };
        }
        5
    } else {
        0
    }
}

#[no_mangle]
pub extern "C" fn axiom_lobby_get_member_name(index: u32, out_buf: *mut u8, max_len: u32) -> u32 {
    let lobby = match get_lobby() {
        Some(l) => l,
        None => return 0,
    };
    let idx = index as usize;
    if idx >= lobby.members.len() || out_buf.is_null() || max_len == 0 {
        return 0;
    }

    if let Some(m) = &lobby.members[idx] {
        let name_bytes = m.name_str().as_bytes();
        let copy_len = name_bytes.len().min(max_len as usize);
        unsafe {
            core::ptr::copy_nonoverlapping(name_bytes.as_ptr(), out_buf, copy_len);
        }
        copy_len as u32
    } else {
        0
    }
}

#[no_mangle]
pub extern "C" fn axiom_leaderboard_register(
    id_lo: u32,
    id_hi: u32,
    name_ptr: *const u8,
    name_len: u32,
) -> u32 {
    let lb = get_leaderboard();
    let steam_id = combine_u64(id_lo, id_hi);
    let name_slice = if !name_ptr.is_null() && name_len > 0 {
        unsafe { core::slice::from_raw_parts(name_ptr, name_len as usize) }
    } else {
        b"Gladiator"
    };

    if lb.find_or_create(steam_id, name_slice).is_some() {
        lb.sort_by_rank();
        1
    } else {
        0
    }
}

#[no_mangle]
pub extern "C" fn axiom_leaderboard_record_match(
    winner_lo: u32,
    winner_hi: u32,
    loser_lo: u32,
    loser_hi: u32,
    is_ko: u32,
    is_flawless: u32,
) -> u32 {
    let lb = get_leaderboard();
    let winner_id = combine_u64(winner_lo, winner_hi);
    let loser_id = combine_u64(loser_lo, loser_hi);

    if lb.record_match(winner_id, loser_id, is_ko != 0, is_flawless != 0) {
        1
    } else {
        0
    }
}

#[no_mangle]
pub extern "C" fn axiom_leaderboard_get_count() -> u32 {
    get_leaderboard().len as u32
}

#[no_mangle]
pub extern "C" fn axiom_leaderboard_get_entry(rank_idx: u32, out_stats: *mut u32) -> u32 {
    let lb = get_leaderboard();
    let idx = rank_idx as usize;
    if idx >= lb.len || out_stats.is_null() {
        return 0;
    }
    let e = &lb.entries[idx];
    let lo = (e.steam_id & 0xFFFF_FFFF) as u32;
    let hi = (e.steam_id >> 32) as u32;
    unsafe {
        *out_stats.add(0) = lo;
        *out_stats.add(1) = hi;
        *out_stats.add(2) = e.rating;
        *out_stats.add(3) = e.matches_won;
        *out_stats.add(4) = e.matches_lost;
        *out_stats.add(5) = e.kos;
        *out_stats.add(6) = e.flawless_victories;
        *out_stats.add(7) = e.current_win_streak;
        *out_stats.add(8) = e.highest_win_streak;
    }
    9
}

#[no_mangle]
pub extern "C" fn axiom_leaderboard_get_name(rank_idx: u32, out_buf: *mut u8, max_len: u32) -> u32 {
    let lb = get_leaderboard();
    let idx = rank_idx as usize;
    if idx >= lb.len || out_buf.is_null() || max_len == 0 {
        return 0;
    }
    let name_bytes = lb.entries[idx].name_str().as_bytes();
    let copy_len = name_bytes.len().min(max_len as usize);
    unsafe {
        core::ptr::copy_nonoverlapping(name_bytes.as_ptr(), out_buf, copy_len);
    }
    copy_len as u32
}

#[no_mangle]
pub extern "C" fn axiom_leaderboard_get_tier(rank_idx: u32, out_buf: *mut u8, max_len: u32) -> u32 {
    let lb = get_leaderboard();
    let idx = rank_idx as usize;
    if idx >= lb.len || out_buf.is_null() || max_len == 0 {
        return 0;
    }
    let tier_bytes = lb.entries[idx].tier_name().as_bytes();
    let copy_len = tier_bytes.len().min(max_len as usize);
    unsafe {
        core::ptr::copy_nonoverlapping(tier_bytes.as_ptr(), out_buf, copy_len);
    }
    copy_len as u32
}

#[no_mangle]
pub extern "C" fn axiom_leaderboard_get_rank(id_lo: u32, id_hi: u32) -> u32 {
    let lb = get_leaderboard();
    let steam_id = combine_u64(id_lo, id_hi);
    lb.rank_of(steam_id).map(|r| r as u32).unwrap_or(0)
}

// ---------------------------------------------------------------------------
// 3D Destructible Voxel Terrain Studio & Real-time Benchmark Engine (T-128)
// ---------------------------------------------------------------------------

struct WasmVoxelEngine {
    volume: VoxelVolume16,
    framebuffer: FrameBuffer,
    rasterizer: Rasterizer3D,
    extracted_mesh: Option<PartMesh>,
    yaw_bam: u8,
    pitch_bam: u8,
    cam_dist: Fixed,
    focal_length: i64,
    last_triangles: u32,
    preset_id: u32,
    staged_axvm: Vec<u8>,
    shards: VoxelShardPool<128>,
    collapse: DynamicCollapseSystem<16>,
    thermal: ThermalStressField16,
    fluid: VoxelFluidField16,
}

#[allow(static_mut_refs)]
static mut VOXEL_ENGINE: Option<WasmVoxelEngine> = None;

#[cfg(test)]
fn voxel_engine_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    match LOCK.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

#[no_mangle]
pub extern "C" fn axiom_voxel_init(width: u32, height: u32, preset_id: u32, seed: u32) {
    let preset = match preset_id {
        0 => VoxelPreset::FlatArena,
        1 => VoxelPreset::CrateredArena,
        2 => VoxelPreset::FortifiedBunker,
        3 => VoxelPreset::MonolithRuins,
        4 => VoxelPreset::VolcanicVent,
        _ => VoxelPreset::CrateredArena,
    };
    let volume = generate_voxel_preset(preset, seed as u64);
    let mesh = extract_voxel_mesh("voxel_scene", &volume, Fixed::from_raw(2048));
    let tris = mesh.triangle_count() as u32;

    let mut rasterizer = Rasterizer3D::new(width as usize, height as usize);
    rasterizer.light_dir = Vec3Fixed::new(
        Fixed::from_raw(32768),
        Fixed::from_raw(49152),
        Fixed::from_raw(-32768),
    );

    let mut shards = VoxelShardPool::new(seed as u64);
    shards.floor_y_q16 = 0;
    let mut collapse = DynamicCollapseSystem::new(seed as u64);
    collapse.floor_y_q16 = 0;

    let engine = WasmVoxelEngine {
        volume,
        framebuffer: FrameBuffer::new(width as usize, height as usize),
        rasterizer,
        extracted_mesh: Some(mesh),
        yaw_bam: 24,
        pitch_bam: 236,
        cam_dist: Fixed::from_raw(262144), // 4.0m
        focal_length: 220,
        last_triangles: tris,
        preset_id,
        staged_axvm: Vec::new(),
        shards,
        collapse,
        thermal: ThermalStressField16::new(),
        fluid: VoxelFluidField16::new(),
    };

    unsafe {
        VOXEL_ENGINE = Some(engine);
    }
}

#[no_mangle]
pub extern "C" fn axiom_voxel_carve_sphere(
    cx: i32,
    cy: i32,
    cz: i32,
    radius: i32,
    fill_material: u32,
) -> u32 {
    let eng = match unsafe { VOXEL_ENGINE.as_mut() } {
        Some(e) => e,
        None => return 0,
    };
    let carved = eng.volume.carve_sphere(
        cx as i16,
        cy as i16,
        cz as i16,
        radius as i16,
        fill_material as u8,
    );
    if carved > 0 {
        let origin = [
            -16384 + (cx * 2048),
            cy * 2048,
            -16384 + (cz * 2048),
        ];
        eng.shards.spawn_burst(origin, (fill_material as u8).max(1), (carved as usize).min(16), 1200);
        let _ = eng.collapse.detect_and_collapse(
            &mut eng.volume,
            0,
            [-16384, 0, -16384],
            2048,
        );
    }
    let mesh = extract_voxel_mesh("voxel_scene", &eng.volume, Fixed::from_raw(2048));
    eng.last_triangles = mesh.triangle_count() as u32;
    eng.extracted_mesh = Some(mesh);
    carved
}

#[no_mangle]
pub extern "C" fn axiom_voxel_carve_crater(cx: i32, cz: i32, depth: i32, radius: i32) -> u32 {
    let eng = match unsafe { VOXEL_ENGINE.as_mut() } {
        Some(e) => e,
        None => return 0,
    };
    let carved = eng.volume.carve_crater_with_shards(
        cx as i16,
        cz as i16,
        depth as i16,
        radius as i16,
        &mut eng.shards,
        [-16384, 0, -16384],
        2048,
    );
    let _collapsed = eng.collapse.detect_and_collapse(
        &mut eng.volume,
        0,
        [-16384, 0, -16384],
        2048,
    );
    let mesh = extract_voxel_mesh("voxel_scene", &eng.volume, Fixed::from_raw(2048));
    eng.last_triangles = mesh.triangle_count() as u32;
    eng.extracted_mesh = Some(mesh);
    carved
}

#[no_mangle]
pub extern "C" fn axiom_voxel_step(yaw_delta: i32, pitch_delta: i32) -> u32 {
    let eng = match unsafe { VOXEL_ENGINE.as_mut() } {
        Some(e) => e,
        None => return 0,
    };
    if yaw_delta != 0 {
        eng.yaw_bam = (eng.yaw_bam as i32 + yaw_delta).rem_euclid(256) as u8;
    }
    if pitch_delta != 0 {
        eng.pitch_bam = (eng.pitch_bam as i32 + pitch_delta).clamp(180, 255) as u8;
    }

    let (c_bounces, c_shatters) = eng.collapse.step(Some(&mut eng.shards));
    let shard_bounces = eng.shards.step();

    (c_bounces as u32 & 0xFF) | (((c_shatters as u32) & 0xFF) << 8) | (((shard_bounces as u32) & 0xFF) << 16)
}

#[no_mangle]
pub extern "C" fn axiom_voxel_render() -> u32 {
    let eng = match unsafe { VOXEL_ENGINE.as_mut() } {
        Some(e) => e,
        None => return 0,
    };

    eng.framebuffer.clear(0xFF0F172A); // Dark slate background
    eng.rasterizer.clear_depth();

    let rot_y = Mat4Fixed::from_rotation_y(eng.yaw_bam);
    let rot_x = Mat4Fixed::from_rotation_x(eng.pitch_bam);
    let trans = Mat4Fixed::from_translation(Vec3Fixed::from_raw(0, 0, 15000));
    let world_mat = trans.mul_mat(&rot_x.mul_mat(&rot_y));

    if let Some(ref mesh) = eng.extracted_mesh {
        eng.rasterizer.render_part(
            &mut eng.framebuffer,
            mesh,
            &world_mat,
            eng.cam_dist,
            eng.focal_length,
        );
    }

    render_dynamic_collapse_chunks(
        &mut eng.framebuffer,
        &mut eng.rasterizer.depth_buffer,
        &eng.collapse,
        &world_mat,
        eng.cam_dist,
        eng.focal_length,
    );

    render_voxel_shards(
        &mut eng.framebuffer,
        &mut eng.rasterizer.depth_buffer,
        &eng.shards,
        &world_mat,
        eng.cam_dist,
        eng.focal_length,
    );

    let voxel_scale = Fixed::from_raw(2048);
    let world_origin = Vec3Fixed::new(
        Fixed::from_raw(-16384),
        Fixed::ZERO,
        Fixed::from_raw(-16384),
    );

    render_thermal_glow(
        &mut eng.framebuffer,
        &mut eng.rasterizer.depth_buffer,
        &world_mat,
        &eng.thermal,
        voxel_scale,
        world_origin,
    );

    render_voxel_fluid(
        &mut eng.framebuffer,
        &mut eng.rasterizer.depth_buffer,
        &world_mat,
        &eng.fluid,
        voxel_scale,
        world_origin,
    );

    eng.last_triangles
}

#[no_mangle]
pub extern "C" fn axiom_voxel_render_raymarch(tile_scale: u32, cast_shadows: u32) -> u32 {
    let eng = match unsafe { VOXEL_ENGINE.as_mut() } {
        Some(e) => e,
        None => return 0,
    };

    eng.framebuffer.clear(0xFF0F172A); // Dark slate background
    eng.rasterizer.clear_depth();

    let rot_y = Mat4Fixed::from_rotation_y(eng.yaw_bam);
    let rot_x = Mat4Fixed::from_rotation_x(eng.pitch_bam);
    let trans = Mat4Fixed::from_translation(Vec3Fixed::from_raw(0, 0, 15000));
    let world_mat = trans.mul_mat(&rot_x.mul_mat(&rot_y));

    let voxel_scale = Fixed::from_raw(2048);
    let world_origin = Vec3Fixed::new(
        Fixed::from_raw(-16384),
        Fixed::ZERO,
        Fixed::from_raw(-16384),
    );

    let config = VoxelRaymarchRenderConfig {
        max_steps: 48,
        cast_shadows: cast_shadows != 0,
        light_dir: [577, 577, -577],
        tile_scale: tile_scale.clamp(1, 4) as u8,
    };

    let fragments = render_voxel_raymarch(
        &mut eng.framebuffer,
        &mut eng.rasterizer.depth_buffer,
        &eng.volume,
        &world_mat,
        &config,
        world_origin,
        voxel_scale,
    );

    fragments as u32
}

#[no_mangle]
pub extern "C" fn axiom_voxel_get_framebuffer_ptr() -> *const u32 {
    unsafe {
        VOXEL_ENGINE
            .as_ref()
            .map(|e| e.framebuffer.pixels.as_ptr())
            .unwrap_or(core::ptr::null())
    }
}

#[no_mangle]
pub extern "C" fn axiom_voxel_get_framebuffer_len() -> u32 {
    unsafe {
        VOXEL_ENGINE
            .as_ref()
            .map(|e| e.framebuffer.pixels.len() as u32)
            .unwrap_or(0)
    }
}

#[no_mangle]
pub extern "C" fn axiom_voxel_get_solid_count() -> u32 {
    unsafe {
        VOXEL_ENGINE
            .as_ref()
            .map(|e| e.volume.total_solid)
            .unwrap_or(0)
    }
}

#[no_mangle]
pub extern "C" fn axiom_voxel_get_damaged_count() -> u32 {
    unsafe {
        VOXEL_ENGINE
            .as_ref()
            .map(|e| e.volume.damaged_count)
            .unwrap_or(0)
    }
}

#[no_mangle]
pub extern "C" fn axiom_voxel_get_triangles() -> u32 {
    unsafe {
        VOXEL_ENGINE
            .as_ref()
            .map(|e| e.last_triangles)
            .unwrap_or(0)
    }
}

#[no_mangle]
pub extern "C" fn axiom_voxel_benchmark(iterations: u32) -> u32 {
    let eng = match unsafe { VOXEL_ENGINE.as_mut() } {
        Some(e) => e,
        None => return 0,
    };

    let mut test_vol = eng.volume;
    let mut total_tris = 0u32;

    for i in 0..iterations {
        let cx = ((i * 3) % 12 + 2) as i16;
        let cz = ((i * 5) % 12 + 2) as i16;
        test_vol.carve_crater(cx, cz, 3, 4);
        let m = extract_voxel_mesh("bench", &test_vol, Fixed::from_raw(2048));
        total_tris = total_tris.wrapping_add(m.triangle_count() as u32);
    }

    total_tris
}

#[no_mangle]
pub extern "C" fn axiom_voxel_alloc_axvm_buffer(len: u32) -> *mut u8 {
    let eng = match unsafe { VOXEL_ENGINE.as_mut() } {
        Some(e) => e,
        None => return core::ptr::null_mut(),
    };
    eng.staged_axvm.resize(len as usize, 0);
    eng.staged_axvm.as_mut_ptr()
}

#[no_mangle]
pub extern "C" fn axiom_voxel_load_staged_axvm(len: u32) -> i32 {
    let eng = match unsafe { VOXEL_ENGINE.as_mut() } {
        Some(e) => e,
        None => return 0,
    };
    if len as usize > eng.staged_axvm.len() {
        return 0;
    }
    match deserialize_axvm(&eng.staged_axvm[..len as usize]) {
        Ok((header, volume)) => {
            eng.volume = volume;
            eng.preset_id = header.preset_id as u32;
            eng.shards.active_count = 0;
            eng.collapse.active_count = 0;
            let mesh = extract_voxel_mesh("loaded_axvm", &eng.volume, Fixed::from_raw(2048));
            eng.last_triangles = mesh.triangle_count() as u32;
            eng.extracted_mesh = Some(mesh);
            1
        }
        Err(_) => 0,
    }
}

#[no_mangle]
pub extern "C" fn axiom_voxel_get_shard_count() -> u32 {
    unsafe {
        VOXEL_ENGINE
            .as_ref()
            .map(|e| e.shards.active_count as u32)
            .unwrap_or(0)
    }
}

#[no_mangle]
pub extern "C" fn axiom_voxel_get_collapsed_chunk_count() -> u32 {
    unsafe {
        VOXEL_ENGINE
            .as_ref()
            .map(|e| e.collapse.active_count as u32)
            .unwrap_or(0)
    }
}

#[no_mangle]
pub extern "C" fn axiom_voxel_trigger_collapse(anchor_y: i32) -> u32 {
    let eng = match unsafe { VOXEL_ENGINE.as_mut() } {
        Some(e) => e,
        None => return 0,
    };
    let collapsed = eng.collapse.detect_and_collapse(
        &mut eng.volume,
        anchor_y as i16,
        [-16384, 0, -16384],
        2048,
    );
    if collapsed > 0 {
        let mesh = extract_voxel_mesh("voxel_scene", &eng.volume, Fixed::from_raw(2048));
        eng.last_triangles = mesh.triangle_count() as u32;
        eng.extracted_mesh = Some(mesh);
    }
    collapsed as u32
}

#[no_mangle]
pub extern "C" fn axiom_voxel_spawn_shard_storm(count: u32) -> u32 {
    let eng = match unsafe { VOXEL_ENGINE.as_mut() } {
        Some(e) => e,
        None => return 0,
    };
    let center = [0, 24576, 0];
    let spawned = (count as usize).min(128);
    eng.shards.spawn_burst(center, 1, spawned, 2400);
    eng.shards.active_count as u32
}

#[no_mangle]
pub extern "C" fn axiom_voxel_set_gravity(gravity_q16: i32) {
    if let Some(eng) = unsafe { VOXEL_ENGINE.as_mut() } {
        eng.shards.gravity_q16 = gravity_q16;
        eng.collapse.gravity_q16 = gravity_q16;
    }
}

#[no_mangle]
pub extern "C" fn axiom_voxel_set_drag(drag_permille: u32) {
    if let Some(eng) = unsafe { VOXEL_ENGINE.as_mut() } {
        let drag = (drag_permille as u16).clamp(800, 1000);
        eng.shards.drag_permille = drag;
        eng.collapse.drag_permille = drag;
    }
}

#[no_mangle]
pub extern "C" fn axiom_voxel_reset_physics() {
    if let Some(eng) = unsafe { VOXEL_ENGINE.as_mut() } {
        eng.shards = VoxelShardPool::new(12345);
        eng.shards.floor_y_q16 = 0;
        eng.collapse = DynamicCollapseSystem::new(12345);
        eng.collapse.floor_y_q16 = 0;
    }
}

#[no_mangle]
pub extern "C" fn axiom_voxel_detonate_meteor(cx: i32, cz: i32, radius: i32, shards: u32) -> u32 {
    let eng = match unsafe { VOXEL_ENGINE.as_mut() } {
        Some(e) => e,
        None => return 0,
    };
    let depth = (radius + 1).min(6);
    let carved = eng.volume.carve_crater_with_shards(
        cx as i16,
        cz as i16,
        depth as i16,
        radius as i16,
        &mut eng.shards,
        [-16384, 0, -16384],
        2048,
    );
    let impact_pos = [
        -16384 + (cx * 2048),
        24576,
        -16384 + (cz * 2048),
    ];
    eng.shards.spawn_burst(impact_pos, 3, (shards as usize).min(128), 3000);

    let _ = eng.collapse.detect_and_collapse(
        &mut eng.volume,
        0,
        [-16384, 0, -16384],
        2048,
    );

    let mesh = extract_voxel_mesh("voxel_scene", &eng.volume, Fixed::from_raw(2048));
    eng.last_triangles = mesh.triangle_count() as u32;
    eng.extracted_mesh = Some(mesh);

    carved
}

#[no_mangle]
pub extern "C" fn axiom_voxel_bench_collapse(iterations: u32, out_stats: *mut u32) -> u32 {
    let eng = match unsafe { VOXEL_ENGINE.as_mut() } {
        Some(e) => e,
        None => return 0,
    };

    let mut volume = generate_voxel_preset(VoxelPreset::MonolithRuins, 42);
    let mut collapse = DynamicCollapseSystem::<16>::new(42);
    let mut shards = VoxelShardPool::<128>::new(42);
    collapse.floor_y_q16 = 0;
    shards.floor_y_q16 = 0;

    let mut total_collapsed = 0u32;
    let mut total_shattered = 0u32;
    let mut total_rendered = 0u32;

    let view_mat = Mat4Fixed::IDENTITY;
    let cam_dist = Fixed::from_raw(262144);
    let focal = 220;

    for step_idx in 0..iterations {
        if step_idx % 5 == 0 {
            let cx = ((step_idx * 3) % 10 + 3) as i16;
            let cz = ((step_idx * 7) % 10 + 3) as i16;
            volume.carve_crater_with_shards(cx, cz, 2, 3, &mut shards, [-16384, 0, -16384], 2048);
            let c = collapse.detect_and_collapse(&mut volume, 0, [-16384, 0, -16384], 2048);
            total_collapsed = total_collapsed.saturating_add(c as u32);
        }
        let (_b, sh) = collapse.step(Some(&mut shards));
        total_shattered = total_shattered.saturating_add(sh);
        shards.step();

        eng.rasterizer.clear_depth();
        let rc = render_dynamic_collapse_chunks(
            &mut eng.framebuffer,
            &mut eng.rasterizer.depth_buffer,
            &collapse,
            &view_mat,
            cam_dist,
            focal,
        );
        let rs = render_voxel_shards(
            &mut eng.framebuffer,
            &mut eng.rasterizer.depth_buffer,
            &shards,
            &view_mat,
            cam_dist,
            focal,
        );
        total_rendered = total_rendered.saturating_add((rc + rs) as u32);
    }

    let consensus_hash = collapse.compute_consensus_hash() ^ shards.compute_consensus_hash();

    if !out_stats.is_null() {
        unsafe {
            *out_stats.add(0) = total_collapsed;
            *out_stats.add(1) = total_shattered;
            *out_stats.add(2) = shards.active_count as u32;
            *out_stats.add(3) = (consensus_hash & 0xFFFF_FFFF) as u32;
            *out_stats.add(4) = ((consensus_hash >> 32) & 0xFFFF_FFFF) as u32;
            *out_stats.add(5) = total_rendered;
        }
    }

    iterations
}

#[no_mangle]
pub extern "C" fn axiom_voxel_export_axvm(out_ptr: *mut u8, max_len: u32) -> u32 {
    let eng = match unsafe { VOXEL_ENGINE.as_ref() } {
        Some(e) => e,
        None => return 0,
    };
    if out_ptr.is_null() || max_len < 4160 {
        return 0;
    }
    let bytes = serialize_axvm("WASM Export", eng.preset_id as u16, &eng.volume, 1729700000);
    let copy_len = bytes.len().min(max_len as usize);
    unsafe {
        core::ptr::copy_nonoverlapping(bytes.as_ptr(), out_ptr, copy_len);
    }
    copy_len as u32
}

#[no_mangle]
pub extern "C" fn axiom_voxel_apply_thermal_pulse(
    cx: i32,
    cy: i32,
    cz: i32,
    radius: i32,
    peak_temp: u32,
    peak_stress: u32,
) -> u32 {
    let eng = match unsafe { VOXEL_ENGINE.as_mut() } {
        Some(e) => e,
        None => return 0,
    };

    let cx_u = cx.clamp(0, 15) as usize;
    let cy_u = cy.clamp(0, 15) as usize;
    let cz_u = cz.clamp(0, 15) as usize;
    let r_u = radius.clamp(1, 15) as usize;

    eng.thermal.inject_thermal_pulse(
        cx_u,
        cy_u,
        cz_u,
        r_u,
        peak_temp.min(1000) as u16,
        peak_stress.min(1000) as u16,
    );

    let res = eng.thermal.step_thermal_and_collapse(
        &mut eng.volume,
        &mut eng.collapse,
        50, // 5% default cooling
        0,
        [-16384, 0, -16384],
        2048,
    );

    if res.phase_transitions > 0 || res.fractures_triggered > 0 || res.collapsed_chunks > 0 {
        let mesh = extract_voxel_mesh("voxel_scene", &eng.volume, Fixed::from_raw(2048));
        eng.last_triangles = mesh.triangle_count() as u32;
        eng.extracted_mesh = Some(mesh);
    }

    eng.thermal.max_temp() as u32
}

#[no_mangle]
pub extern "C" fn axiom_voxel_step_thermal(cooling_permille: u32) -> u32 {
    let eng = match unsafe { VOXEL_ENGINE.as_mut() } {
        Some(e) => e,
        None => return 0,
    };

    let res = eng.thermal.step_thermal_and_collapse(
        &mut eng.volume,
        &mut eng.collapse,
        cooling_permille.min(1000) as u16,
        0,
        [-16384, 0, -16384],
        2048,
    );

    if res.phase_transitions > 0 || res.fractures_triggered > 0 || res.collapsed_chunks > 0 {
        let mesh = extract_voxel_mesh("voxel_scene", &eng.volume, Fixed::from_raw(2048));
        eng.last_triangles = mesh.triangle_count() as u32;
        eng.extracted_mesh = Some(mesh);
    }

    (res.phase_transitions.min(255) as u32)
        | ((res.fractures_triggered.min(255) as u32) << 8)
        | (((res.collapsed_chunks as u32).min(255)) << 16)
}

#[no_mangle]
pub extern "C" fn axiom_voxel_get_max_temperature() -> u32 {
    unsafe {
        VOXEL_ENGINE
            .as_ref()
            .map(|e| e.thermal.max_temp() as u32)
            .unwrap_or(0)
    }
}

#[no_mangle]
pub extern "C" fn axiom_voxel_get_thermal_transitions() -> u32 {
    unsafe {
        VOXEL_ENGINE
            .as_ref()
            .map(|e| e.thermal.phase_transitions)
            .unwrap_or(0)
    }
}

#[no_mangle]
pub extern "C" fn axiom_voxel_get_thermal_fractures() -> u32 {
    unsafe {
        VOXEL_ENGINE
            .as_ref()
            .map(|e| e.thermal.fractures_triggered)
            .unwrap_or(0)
    }
}

#[no_mangle]
pub extern "C" fn axiom_voxel_reset_thermal() {
    if let Some(eng) = unsafe { VOXEL_ENGINE.as_mut() } {
        eng.thermal = ThermalStressField16::new();
    }
}

#[no_mangle]
pub extern "C" fn axiom_voxel_inject_fluid(x: i32, y: i32, z: i32, amount: u32) -> u32 {
    if let Some(eng) = unsafe { VOXEL_ENGINE.as_mut() } {
        if x >= 0 && x < 16 && y >= 0 && y < 16 && z >= 0 && z < 16 {
            eng.fluid.inject_fluid(
                &mut eng.volume,
                x as usize,
                y as usize,
                z as usize,
                amount.min(255) as u8,
            );
            let mesh = extract_voxel_mesh("voxel_scene", &eng.volume, Fixed::from_raw(2048));
            eng.last_triangles = mesh.triangle_count() as u32;
            eng.extracted_mesh = Some(mesh);
            return eng.fluid.total_fluid_units;
        }
    }
    0
}

#[no_mangle]
pub extern "C" fn axiom_voxel_step_fluid(infiltration_permille: u32) -> u32 {
    if let Some(eng) = unsafe { VOXEL_ENGINE.as_mut() } {
        let inf = infiltration_permille.min(1000) as u16;
        let res = eng.fluid.step_fluid(&mut eng.volume, Some(&mut eng.thermal), inf);
        let mesh = extract_voxel_mesh("voxel_scene", &eng.volume, Fixed::from_raw(2048));
        eng.last_triangles = mesh.triangle_count() as u32;
        eng.extracted_mesh = Some(mesh);
        return (res.units_moved as u32) | ((res.units_evaporated as u32) << 16);
    }
    0
}

#[no_mangle]
pub extern "C" fn axiom_voxel_get_fluid_units() -> u32 {
    unsafe {
        VOXEL_ENGINE
            .as_ref()
            .map(|e| e.fluid.total_fluid_units)
            .unwrap_or(0)
    }
}

#[no_mangle]
pub extern "C" fn axiom_voxel_get_infiltrated_units() -> u32 {
    unsafe {
        VOXEL_ENGINE
            .as_ref()
            .map(|e| e.fluid.total_infiltrated)
            .unwrap_or(0)
    }
}

#[no_mangle]
pub extern "C" fn axiom_voxel_get_active_fluid_cells() -> u32 {
    unsafe {
        VOXEL_ENGINE
            .as_ref()
            .map(|e| e.fluid.active_fluid_cells as u32)
            .unwrap_or(0)
    }
}

#[no_mangle]
pub extern "C" fn axiom_voxel_get_steam_explosions() -> u32 {
    unsafe {
        VOXEL_ENGINE
            .as_ref()
            .map(|e| e.fluid.steam_explosions as u32)
            .unwrap_or(0)
    }
}

#[no_mangle]
pub extern "C" fn axiom_voxel_reset_fluid() {
    if let Some(eng) = unsafe { VOXEL_ENGINE.as_mut() } {
        eng.fluid = VoxelFluidField16::new();
        for y in 0..16 {
            for z in 0..16 {
                for x in 0..16 {
                    if eng.volume.get(x, y, z) == axiom_core::voxel_destruct::VOXEL_WATER {
                        eng.volume.set(x, y, z, axiom_core::voxel_destruct::VOXEL_AIR);
                    }
                }
            }
        }
        let mesh = extract_voxel_mesh("voxel_scene", &eng.volume, Fixed::from_raw(2048));
        eng.last_triangles = mesh.triangle_count() as u32;
        eng.extracted_mesh = Some(mesh);
    }
}

#[no_mangle]
pub extern "C" fn axiom_voxel_bench_extreme(steps: u32, out_stats: *mut u32) -> u32 {
    let eng = match unsafe { VOXEL_ENGINE.as_mut() } {
        Some(e) => e,
        None => return 0,
    };

    let iterations = steps.clamp(1, 10_000);
    let mut total_phase_transitions = 0u32;
    let mut total_shattered = 0u32;

    for step in 0..iterations {
        if step % 50 == 0 {
            let cx = ((step * 7) % 16) as usize;
            let cz = ((step * 11) % 16) as usize;
            let peak_t = 800 + ((step % 200) as u16);
            let peak_s = 750 + ((step % 250) as u16);
            eng.thermal.inject_thermal_pulse(cx, 4, cz, 2, peak_t, peak_s);
        }

        let res = eng.thermal.step_thermal_and_collapse_advanced(
            &mut eng.volume,
            &mut eng.collapse,
            15,
            0,
            [-16384, 0, -16384],
            2048,
        );
        total_phase_transitions += res.phase_transitions;

        let (_c, sh) = eng.collapse.step(Some(&mut eng.shards));
        total_shattered += sh;
        eng.shards.step();
    }

    let mesh = extract_voxel_mesh("voxel_scene", &eng.volume, Fixed::from_raw(2048));
    eng.last_triangles = mesh.triangle_count() as u32;
    eng.extracted_mesh = Some(mesh);

    if !out_stats.is_null() {
        let mut hash: u64 = 0xCBF2_9CE4_8422_2325;
        hash ^= eng.thermal.compute_consensus_hash();
        hash = hash.wrapping_mul(0x100_0000_01B3);
        hash ^= eng.collapse.compute_consensus_hash();
        hash = hash.wrapping_mul(0x100_0000_01B3);
        hash ^= eng.shards.compute_consensus_hash();
        hash = hash.wrapping_mul(0x100_0000_01B3);

        unsafe {
            *out_stats.add(0) = iterations;
            *out_stats.add(1) = total_phase_transitions;
            *out_stats.add(2) = eng.collapse.total_collapsed as u32;
            *out_stats.add(3) = total_shattered;
            *out_stats.add(4) = eng.shards.active_count as u32;
            *out_stats.add(5) = (hash & 0xFFFF_FFFF) as u32;
            *out_stats.add(6) = (hash >> 32) as u32;
        }
    }

    iterations
}

#[no_mangle]
pub extern "C" fn axiom_voxel_bench_fluid(steps: u32, out_stats: *mut u32) -> u32 {
    let eng = match unsafe { VOXEL_ENGINE.as_mut() } {
        Some(e) => e,
        None => return 0,
    };

    let iterations = steps.clamp(1, 10_000);
    let mut total_quenched = 0u32;
    let mut total_relaxed = 0u32;
    let mut total_phase_transitions = 0u32;

    for step in 0..iterations {
        if step % 50 == 0 {
            let cx = ((step * 7) % 16) as usize;
            let cz = ((step * 11) % 16) as usize;
            let peak_t = 800 + ((step % 200) as u16);
            let peak_s = 700 + ((step % 250) as u16);
            eng.thermal.inject_thermal_pulse(cx, 2, cz, 2, peak_t, peak_s);
        }

        if step % 25 == 0 {
            let fx = ((step * 7) % 16) as usize;
            let fz = ((step * 11) % 16) as usize;
            eng.fluid.inject_fluid(&mut eng.volume, fx, 2, fz, 220);
        }

        let (q_temp, r_stress) = eng.fluid.relax_thermal_and_stress(&mut eng.thermal);
        total_quenched += q_temp;
        total_relaxed += r_stress;

        let _ = eng.fluid.step_fluid(&mut eng.volume, Some(&mut eng.thermal), 25);

        let res = eng.thermal.step_thermal_and_collapse_advanced(
            &mut eng.volume,
            &mut eng.collapse,
            15,
            1,
            [-16384, 0, -16384],
            2048,
        );
        total_phase_transitions += res.phase_transitions;

        let (_c, _sh) = eng.collapse.step(Some(&mut eng.shards));
        eng.shards.step();
    }

    let mesh = extract_voxel_mesh("voxel_scene", &eng.volume, Fixed::from_raw(2048));
    eng.last_triangles = mesh.triangle_count() as u32;
    eng.extracted_mesh = Some(mesh);

    if !out_stats.is_null() {
        let mut hash: u64 = 0xCBF2_9CE4_8422_2325;
        hash ^= eng.fluid.compute_consensus_hash();
        hash = hash.wrapping_mul(0x100_0000_01B3);
        hash ^= eng.thermal.compute_consensus_hash();
        hash = hash.wrapping_mul(0x100_0000_01B3);
        hash ^= eng.collapse.compute_consensus_hash();
        hash = hash.wrapping_mul(0x100_0000_01B3);
        hash ^= eng.shards.compute_consensus_hash();
        hash = hash.wrapping_mul(0x100_0000_01B3);

        unsafe {
            *out_stats.add(0) = iterations;
            *out_stats.add(1) = eng.fluid.total_infiltrated;
            *out_stats.add(2) = total_quenched;
            *out_stats.add(3) = total_relaxed;
            *out_stats.add(4) = total_phase_transitions;
            *out_stats.add(5) = (hash & 0xFFFF_FFFF) as u32;
            *out_stats.add(6) = (hash >> 32) as u32;
        }
    }

    iterations
}

#[no_mangle]
pub extern "C" fn axiom_voxel_bench_simd_fluid(steps: u32, out_stats: *mut u32) -> u32 {
    use axiom_core::voxel_fluid_simd::SimdFluidEngine;

    let eng = match unsafe { VOXEL_ENGINE.as_mut() } {
        Some(e) => e,
        None => return 0,
    };

    let iterations = steps.clamp(1, 10_000);
    let mut simd_eng = SimdFluidEngine::new();
    let mut total_moved = 0u32;
    let mut _total_quenched = 0u32;
    let mut _total_relaxed = 0u32;

    for step in 0..iterations {
        if step % 50 == 0 {
            let cx = ((step * 7) % 16) as usize;
            let cz = ((step * 11) % 16) as usize;
            let peak_t = 800 + ((step % 200) as u16);
            let peak_s = 700 + ((step % 250) as u16);
            eng.thermal.inject_thermal_pulse(cx, 2, cz, 2, peak_t, peak_s);
        }

        if step % 25 == 0 {
            let fx = ((step * 7) % 16) as usize;
            let fz = ((step * 11) % 16) as usize;
            eng.fluid.inject_fluid(&mut eng.volume, fx, 2, fz, 220);
        }

        let (q_temp, r_stress) = eng.fluid.relax_thermal_and_stress(&mut eng.thermal);
        _total_quenched += q_temp;
        _total_relaxed += r_stress;

        let res = simd_eng.step_fluid_simd(&mut eng.fluid, &mut eng.volume, Some(&mut eng.thermal), 25);
        total_moved += res.units_moved;
    }

    let mesh = extract_voxel_mesh("voxel_scene", &eng.volume, Fixed::from_raw(2048));
    eng.last_triangles = mesh.triangle_count() as u32;
    eng.extracted_mesh = Some(mesh);

    let hash = eng.fluid.compute_consensus_hash();

    if !out_stats.is_null() {
        unsafe {
            *out_stats.add(0) = iterations;
            *out_stats.add(1) = total_moved;
            *out_stats.add(2) = simd_eng.sleep_cycles_saved;
            *out_stats.add(3) = simd_eng.active_slices as u32;
            *out_stats.add(4) = eng.fluid.total_fluid_units;
            *out_stats.add(5) = (hash & 0xFFFF_FFFF) as u32;
            *out_stats.add(6) = (hash >> 32) as u32;
        }
    }

    iterations
}

#[no_mangle]
pub extern "C" fn axiom_voxel_bench_raymarch(
    frames: u32,
    tile_scale: u32,
    cast_shadows: u32,
    out_stats: *mut u32,
) -> u32 {
    let eng = match unsafe { VOXEL_ENGINE.as_mut() } {
        Some(e) => e,
        None => return 0,
    };

    let iterations = frames.clamp(1, 1000);
    let mut total_fragments = 0u64;
    let mut hash = 0xCBF1_CCE4_80CB_9716u64;

    let rot_y = Mat4Fixed::from_rotation_y(eng.yaw_bam);
    let rot_x = Mat4Fixed::from_rotation_x(eng.pitch_bam);
    let trans = Mat4Fixed::from_translation(Vec3Fixed::from_raw(0, 0, 15000));
    let world_mat = trans.mul_mat(&rot_x.mul_mat(&rot_y));

    let voxel_scale = Fixed::from_raw(2048);
    let world_origin = Vec3Fixed::new(
        Fixed::from_raw(-16384),
        Fixed::ZERO,
        Fixed::from_raw(-16384),
    );

    let config = VoxelRaymarchRenderConfig {
        max_steps: 48,
        cast_shadows: cast_shadows != 0,
        light_dir: [577, 577, -577],
        tile_scale: tile_scale.clamp(1, 4) as u8,
    };

    for frame in 0..iterations {
        eng.framebuffer.clear(0xFF0F172A);
        eng.rasterizer.clear_depth();

        let fragments = render_voxel_raymarch(
            &mut eng.framebuffer,
            &mut eng.rasterizer.depth_buffer,
            &eng.volume,
            &world_mat,
            &config,
            world_origin,
            voxel_scale,
        );

        total_fragments += fragments as u64;

        let sample_idx = (eng.framebuffer.height / 2) * eng.framebuffer.width + (eng.framebuffer.width / 2);
        let sample_val = eng.framebuffer.pixels.get(sample_idx).copied().unwrap_or(0);
        hash ^= sample_val as u64;
        hash = hash.wrapping_mul(0x100_0000_01B3u64);
        hash ^= (fragments as u64).wrapping_add(frame as u64);
        hash = hash.wrapping_mul(0x100_0000_01B3u64);
    }

    if !out_stats.is_null() {
        unsafe {
            *out_stats.add(0) = iterations;
            *out_stats.add(1) = (total_fragments & 0xFFFF_FFFF) as u32;
            *out_stats.add(2) = (total_fragments / iterations as u64) as u32;
            *out_stats.add(3) = eng.volume.total_solid as u32;
            *out_stats.add(4) = config.tile_scale as u32;
            *out_stats.add(5) = (hash & 0xFFFF_FFFF) as u32;
            *out_stats.add(6) = (hash >> 32) as u32;
        }
    }

    iterations
}

#[no_mangle]
pub extern "C" fn axiom_net_bench_rollback(
    frames: u32,
    loss_permille: u32,
    latency_frames: u32,
    out_stats: *mut u32,
) -> u32 {
    use axiom_net::{DistributedRollbackSession, NetworkLossSimulator};

    let iters = frames.clamp(10, 500);
    let mut peers: alloc::vec::Vec<DistributedRollbackSession<64>> = (0..16)
        .map(|id| DistributedRollbackSession::<64>::with_expected_peers(id, 0xFFFF))
        .collect();

    let mut sim = NetworkLossSimulator::new(loss_permille.min(400), latency_frames.clamp(1, 8), 99991);

    let sim_func = |frame: u32, inputs: &[u16; 16]| -> u64 {
        let mut h: u64 = 0xCBF2_9CE4_8422_2325 ^ (frame as u64);
        for (p, &inp) in inputs.iter().enumerate() {
            h ^= ((inp as u64) | ((p as u64) << 16)).wrapping_mul(0x100_0000_01B3);
        }
        h
    };

    let mut total_packets = 0u32;

    for f in 0..iters {
        for src in 0..16 {
            let inp = ((src as u16 + 1) * 37) ^ (f as u16);
            let pkt = peers[src].submit_local_input(inp);
            peers[src].advance_frame(sim_func);

            for dst in 0..16 {
                if src != dst {
                    sim.send_to(dst, pkt, f);
                    total_packets += 1;
                }
            }
        }

        let arrivals = sim.poll_deliveries(f);
        for (dst, pkt) in arrivals {
            if let Ok(Some(from_frame)) = peers[dst].receive_remote_packet(&pkt) {
                peers[dst].rollback_and_resimulate(from_frame, sim_func);
            }
            let _ = peers[dst].verify_remote_consensus(&pkt);
        }
    }

    // Flush redundant packets
    for f in iters..(iters + 12) {
        for src in 0..16 {
            let pkt = peers[src].submit_local_input(0);
            peers[src].advance_frame(sim_func);
            for dst in 0..16 {
                if src != dst {
                    sim.send_to(dst, pkt, f);
                    total_packets += 1;
                }
            }
        }
        let arrivals = sim.poll_deliveries(f);
        for (dst, pkt) in arrivals {
            if let Ok(Some(from_frame)) = peers[dst].receive_remote_packet(&pkt) {
                peers[dst].rollback_and_resimulate(from_frame, sim_func);
            }
            let _ = peers[dst].verify_remote_consensus(&pkt);
        }
    }

    // Drain remaining
    for f in (iters + 12)..(iters + 25) {
        let arrivals = sim.poll_deliveries(f);
        for (dst, pkt) in arrivals {
            if let Ok(Some(from_frame)) = peers[dst].receive_remote_packet(&pkt) {
                peers[dst].rollback_and_resimulate(from_frame, sim_func);
            }
            let _ = peers[dst].verify_remote_consensus(&pkt);
        }
    }

    let mut total_resim = 0u32;
    let mut total_rollbacks = 0u32;
    let mut total_desyncs = 0u32;
    for p in &peers {
        total_resim += p.telemetry.total_resimulated_frames as u32;
        total_rollbacks += p.telemetry.total_rollbacks_triggered as u32;
        total_desyncs += p.telemetry.total_desyncs as u32;
    }

    let check_frame = iters.saturating_sub(1);
    let slot = (check_frame as usize) % 64;
    let consensus_hash = peers[0].state_hashes[slot];
    let mut matched_peers = 0u32;
    for p in 0..16 {
        if peers[p].state_hashes[slot] == consensus_hash {
            matched_peers += 1;
        }
    }

    if !out_stats.is_null() {
        unsafe {
            *out_stats.add(0) = iters;
            *out_stats.add(1) = total_resim;
            *out_stats.add(2) = total_rollbacks;
            *out_stats.add(3) = total_desyncs;
            *out_stats.add(4) = matched_peers;
            *out_stats.add(5) = (consensus_hash & 0xFFFF_FFFF) as u32;
            *out_stats.add(6) = (consensus_hash >> 32) as u32;
            *out_stats.add(7) = total_packets;
        }
    }

    iters
}

#[no_mangle]
pub extern "C" fn axiom_net_bench_nat_traversal(
    peer_count: u32,
    loss_permille: u32,
    out_stats: *mut u32,
) -> u32 {
    use axiom_net::{NatCandidate64, NatTraversalSession, StunPacket64};

    let num_peers = peer_count.clamp(2, 16) as usize;
    let loss = loss_permille.min(500);

    let mut sessions: alloc::vec::Vec<NatTraversalSession<16>> = (0..num_peers)
        .map(|id| {
            let host_ip = [192, 168, 1, 10 + id as u8];
            let host_port = 7000 + (id as u16) * 10;
            let cand = NatCandidate64::new_ipv4_host(host_ip, host_port, 2_130_706_431 - (id as u32) * 1000);
            NatTraversalSession::<16>::new(id as u8, cand)
        })
        .collect();

    // Exchange public STUN (server-reflexive) candidate descriptors across all peers
    for i in 0..num_peers {
        for j in 0..num_peers {
            if i != j {
                let srflx_ip = [203, 0, 113, 10 + j as u8];
                let srflx_port = 17000 + (j as u16) * 10;
                let cand = NatCandidate64::new_ipv4_srflx(srflx_ip, srflx_port, 1_694_498_815 - (j as u32) * 1000);
                sessions[i].add_peer_candidate(j, cand);
            }
        }
    }

    let mut rng: u32 = 0x1234_5678 ^ (num_peers as u32).wrapping_mul(31337);
    let mut rounds = 0u32;
    let max_rounds = 40u32;

    while rounds < max_rounds {
        rounds += 1;
        let now_micros = rounds * 10_000;

        // Check if all peers have reached full connectivity
        let mut all_done = true;
        for i in 0..num_peers {
            if !sessions[i].all_peers_connected() {
                all_done = false;
                break;
            }
        }
        if all_done {
            break;
        }

        // 1. Send probes for disconnected peers
        let mut forward_probes: alloc::vec::Vec<(usize, usize, StunPacket64)> = alloc::vec::Vec::new();
        for i in 0..num_peers {
            for j in 0..num_peers {
                if i != j && !sessions[i].is_peer_connected(j) {
                    let ping = sessions[i].create_probe_ping(j as u8, rounds as u16, now_micros);
                    rng = rng.wrapping_mul(1664525).wrapping_add(1013904223);
                    if (rng % 1000) >= loss {
                        forward_probes.push((i, j, ping));
                    }
                }
            }
        }

        // 2. Process incoming pings and generate return pongs
        let mut return_pongs: alloc::vec::Vec<(usize, usize, StunPacket64)> = alloc::vec::Vec::new();
        for (src, dst, ping) in forward_probes {
            if let Some(pong) = sessions[dst].process_stun_packet(&ping, now_micros + 4_000) {
                rng = rng.wrapping_mul(1664525).wrapping_add(1013904223);
                if (rng % 1000) >= loss {
                    return_pongs.push((dst, src, pong));
                }
            }
        }

        // 3. Process incoming pongs
        for (_dst, src, pong) in return_pongs {
            sessions[src].process_stun_packet(&pong, now_micros + 8_000);
        }
    }

    let mut total_probes_sent = 0u32;
    let mut total_probes_recv = 0u32;
    let mut connected_peers_count = 0u32;
    let mut total_rtt_micros = 0u64;
    let mut total_pairs = 0u32;

    let mut consensus_hash: u64 = 0xCBF2_9CE4_8422_2325;

    for (id, s) in sessions.iter().enumerate() {
        total_probes_sent += s.total_probes_sent;
        total_probes_recv += s.total_probes_received;
        if s.all_peers_connected() {
            connected_peers_count += 1;
        }

        consensus_hash ^= (s.connected_mask as u64) | ((id as u64) << 16);
        consensus_hash = consensus_hash.wrapping_mul(0x100_0000_01B3);

        for j in 0..num_peers {
            if (id != j) && s.is_peer_connected(j) {
                total_pairs += 1;
                let rtt = s.peer_candidates[j].rtt_micros;
                total_rtt_micros += rtt as u64;
                consensus_hash ^= (rtt as u64).wrapping_mul(0x100_0000_01B3);
            }
        }
    }

    let avg_rtt = if total_pairs > 0 {
        (total_rtt_micros / total_pairs as u64) as u32
    } else {
        0
    };

    if !out_stats.is_null() {
        unsafe {
            *out_stats.add(0) = num_peers as u32;
            *out_stats.add(1) = rounds;
            *out_stats.add(2) = total_probes_sent;
            *out_stats.add(3) = total_probes_recv;
            *out_stats.add(4) = connected_peers_count;
            *out_stats.add(5) = (consensus_hash & 0xFFFF_FFFF) as u32;
            *out_stats.add(6) = (consensus_hash >> 32) as u32;
            *out_stats.add(7) = avg_rtt;
        }
    }

    rounds
}

#[no_mangle]
pub extern "C" fn axiom_job_bench_scheduler(
    job_count: u32,
    workers: u32,
    out_stats: *mut u32,
) -> u32 {
    use axiom_core::job_system::{JobContext, JobPriority, JobStatus, JobSystem};
    use axiom_core::worker_pool::WorkerPoolCoordinator;

    let target_jobs = (job_count as usize).clamp(16, 240);
    let worker_count = (workers as usize).clamp(1, 8);

    let mut js = Box::new(JobSystem::<256>::new());
    let mut coordinator = Box::new(WorkerPoolCoordinator::<8, 256>::new(worker_count));

    fn bench_payload_job(ctx: JobContext) {
        let mut x = ctx.param;
        for _ in 0..10 {
            x = x.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        }
    }

    // 1. Allocate root parent job
    let parent_handle = js.allocate_job(
        bench_payload_job,
        9999,
        0xFEED,
        JobPriority::Critical,
    ).unwrap();

    // 2. Allocate child jobs and bind to parent
    for i in 0..target_jobs {
        let child = js.allocate_job(
            bench_payload_job,
            (i as u64) + 1,
            (i as u64) * 31,
            JobPriority::Normal,
        ).unwrap();
        js.set_parent(child, parent_handle);

        // Imbalance: push 80% of jobs exclusively onto worker 0 to trigger heavy work stealing
        let target_worker = if i % 5 == 0 {
            (i / 5) % worker_count
        } else {
            0
        };
        js.descriptors[child.slot()].status = JobStatus::Ready as u8;
        coordinator.push_local(target_worker, child);
    }

    // Submit parent (waits until all children complete)
    js.submit(parent_handle);

    // 3. Drain workers cooperatively via Chase-Lev work stealing
    let executed_stolen = coordinator.drain_all(&mut js);

    // 4. Drain any remaining ready jobs (including root parent after children completed)
    let executed_parent = js.run_all(0);
    let total_executed = executed_stolen + executed_parent;

    let total_stolen = coordinator.total_jobs_stolen() as u32;
    let total_attempts = coordinator.telemetry.iter().take(worker_count)
        .map(|t| t.steal_attempts.load(core::sync::atomic::Ordering::Relaxed))
        .sum::<u64>() as u32;
    let consensus_hash = js.compute_consensus_hash();

    if !out_stats.is_null() {
        unsafe {
            *out_stats.add(0) = total_executed as u32;
            *out_stats.add(1) = worker_count as u32;
            *out_stats.add(2) = total_stolen;
            *out_stats.add(3) = total_attempts;
            *out_stats.add(4) = (consensus_hash & 0xFFFF_FFFF) as u32;
            *out_stats.add(5) = (consensus_hash >> 32) as u32;
            *out_stats.add(6) = 256; // max capacity
            *out_stats.add(7) = (target_jobs + 1) as u32;
        }
    }

    total_executed as u32
}

#[no_mangle]
pub extern "C" fn axiom_event_bench_bus(
    event_count: u32,
    workers: u32,
    out_stats: *mut u32,
) -> u32 {
    use axiom_core::event_bus::{
        channel, dispatch_events_with_jobs, DeterministicEventBus,
    };
    use axiom_core::job_system::JobSystem;
    use axiom_core::worker_pool::WorkerPoolCoordinator;

    let target_events = (event_count as usize).clamp(16, 240);
    let worker_count = (workers as usize).clamp(1, 8);

    let mut bus = Box::new(DeterministicEventBus::<256>::new());
    let mut js = Box::new(JobSystem::<256>::new());
    let mut coordinator = Box::new(WorkerPoolCoordinator::<8, 256>::new(worker_count));

    #[derive(Clone, Copy, PartialEq, Eq)]
    #[repr(C)]
    struct WasmEventPayload {
        pub value: u32,
        pub source: u32,
        pub data: [u8; 16],
    }

    // 1. Emit target_events into the bus across multiple channels
    for i in 0..target_events {
        let ch = match i % 4 {
            0 => channel::COMBAT,
            1 => channel::PHYSICS,
            2 => channel::AUDIO,
            _ => channel::VFX | channel::COMBAT,
        };
        let p = WasmEventPayload {
            value: (i as u32) * 17,
            source: i as u32,
            data: [(i as u8).wrapping_mul(7); 16],
        };
        bus.emit(100 + (i as u32), (i % 8) as u16 + 1, ch, (i as u32) + 1, 0, &p);
    }

    let emitted_count = bus.count;

    // 2. Count combat-matching events via for_each_matching
    let mut combat_matches = 0u32;
    bus.for_each_matching(channel::COMBAT, |_ev| {
        combat_matches += 1;
    });

    // 3. Confirm up to sequence 70% of emitted events and test rollback
    let confirm_seq = (emitted_count * 7) / 10;
    bus.confirm(confirm_seq as u32);

    let pre_rollback = bus.count;
    bus.rollback_to(confirm_seq as u32);
    let post_rollback = bus.count;
    let rewound_count = (pre_rollback.saturating_sub(post_rollback)) as u32;

    // 4. Parallel dispatch remaining events across worker pool
    let dispatched = dispatch_events_with_jobs(
        &bus,
        channel::ALL,
        worker_count,
        &mut js,
        &mut coordinator,
    );

    let consensus_hash = bus.compute_consensus_hash();

    if !out_stats.is_null() {
        unsafe {
            *out_stats.add(0) = emitted_count as u32;
            *out_stats.add(1) = post_rollback as u32;
            *out_stats.add(2) = combat_matches;
            *out_stats.add(3) = (consensus_hash & 0xFFFF_FFFF) as u32;
            *out_stats.add(4) = (consensus_hash >> 32) as u32;
            *out_stats.add(5) = dispatched as u32;
            *out_stats.add(6) = rewound_count;
            *out_stats.add(7) = 256; // Ring buffer capacity
        }
    }

    emitted_count as u32
}

#[no_mangle]
pub extern "C" fn axiom_spatial_bench_broadphase(
    entity_count: u32,
    iterations: u32,
    cell_size_mm: i32,
    out_stats: *mut u32,
) -> u32 {
    use axiom_core::event_bus::DeterministicEventBus;
    use axiom_core::job_system::JobSystem;
    use axiom_core::spatial_grid::{
        dispatch_broadphase_with_jobs, emit_collision_pairs_to_event_bus, AabbSlab64,
        CollisionPair64, SpatialHashGrid, COLLISION_FLAG_DYNAMIC,
    };
    use axiom_core::worker_pool::WorkerPoolCoordinator;

    let count = (entity_count as usize).clamp(4, 128);
    let iters = (iterations as usize).clamp(1, 20);
    let cell_size = if cell_size_mm <= 0 { 1000 } else { cell_size_mm };

    let mut slabs = [AabbSlab64::default(); 128];
    for i in 0..count {
        // Clustered distribution centered in 3D arena
        let cx = ((i as i32 * 37) % 2000) - 1000;
        let cy = ((i as i32 * 53) % 2000) - 1000;
        let cz = ((i as i32 * 71) % 1000) - 500;
        let half_ext = 250; // 500mm bounding box

        slabs[i] = AabbSlab64::new(
            (i + 1) as u32,
            1,
            if i % 2 == 0 { 0x01 } else { 0x03 },
            COLLISION_FLAG_DYNAMIC,
            (cx - half_ext, cy - half_ext, cz - half_ext),
            (cx + half_ext, cy + half_ext, cz + half_ext),
        );
    }

    let mut grid = Box::new(SpatialHashGrid::<1024, 14>::new(cell_size));
    let mut bus = Box::new(DeterministicEventBus::<256>::new());
    let mut js = Box::new(JobSystem::<256>::new());
    let mut coordinator = Box::new(WorkerPoolCoordinator::<8, 256>::new(4));

    let mut pairs_buf = [CollisionPair64::default(); 256];
    let mut total_pairs = 0usize;
    let mut total_overflow = 0usize;
    let mut dispatched_jobs = 0usize;

    for iter in 0..iters {
        grid.clear();
        for i in 0..count {
            if grid.insert(&slabs[i]) == 0 {
                total_overflow += 1;
            }
        }

        let pairs_count = grid.query_all_collision_pairs(&slabs, &mut pairs_buf);
        total_pairs = pairs_count;

        emit_collision_pairs_to_event_bus(
            &pairs_buf[0..pairs_count],
            200 + (iter as u32),
            &mut bus,
        );

        dispatched_jobs = dispatch_broadphase_with_jobs(
            &slabs,
            4,
            &mut js,
            &mut coordinator,
        );
    }

    let grid_hash = grid.compute_consensus_hash();
    let bus_hash = bus.compute_consensus_hash();
    let combined_hash = grid_hash ^ (bus_hash.rotate_left(17));

    if !out_stats.is_null() {
        unsafe {
            *out_stats.add(0) = count as u32;
            *out_stats.add(1) = total_pairs as u32;
            *out_stats.add(2) = total_overflow as u32;
            *out_stats.add(3) = (combined_hash & 0xFFFF_FFFF) as u32;
            *out_stats.add(4) = (combined_hash >> 32) as u32;
            *out_stats.add(5) = dispatched_jobs as u32;
            *out_stats.add(6) = cell_size as u32;
            *out_stats.add(7) = 1024; // Table capacity
        }
    }

    total_pairs as u32
}

#[no_mangle]
pub extern "C" fn axiom_ccd_bench_hitbox(
    hitbox_count: u32,
    iterations: u32,
    velocity_scale_pct: u32,
    out_stats: *mut u32,
) -> u32 {
    use axiom_core::ccd::{
        compute_ccd_consensus_hash, dispatch_ccd_hitboxes_with_jobs, emit_ccd_hits_to_event_bus,
        swept_hitbox_vs_hurtbox, HitType, HitboxSlab64, SweptAabbResult64, HITBOX_FLAG_ACTIVE,
        HITBOX_FLAG_SWEPT_CCD,
    };
    use axiom_core::event_bus::DeterministicEventBus;
    use axiom_core::job_system::JobSystem;
    use axiom_core::worker_pool::WorkerPoolCoordinator;

    let count = (hitbox_count as usize).clamp(4, 64);
    let iters = (iterations as usize).clamp(1, 20);
    let scale = if velocity_scale_pct == 0 { 100 } else { velocity_scale_pct };
    let base_disp_x = (500 * (scale as i32)) / 100; // 500 mm/tick @ 100% = 30 m/s

    // Generate N moving hitbox slabs
    let mut slabs = [HitboxSlab64::empty(); 64];
    for i in 0..count {
        let origin_x = ((i as i32 * 43) % 1500) - 750;
        let origin_y = ((i as i32 * 61) % 1500) - 750;
        let origin_z = 800 + ((i as i32 * 29) % 400);

        slabs[i] = HitboxSlab64::from_extents_and_disp(
            (i + 1) as u32,
            (i as u16) + 1,
            0xFFFF,
            (origin_x - 100, origin_y - 100, origin_z - 100),
            (origin_x + 100, origin_y + 100, origin_z + 100),
            (base_disp_x, 0, 0),
            50 + (i as u16 % 50),
            1500,
            HitType::Mid,
            HITBOX_FLAG_ACTIVE | HITBOX_FLAG_SWEPT_CCD,
        );
    }

    let mut bus = Box::new(DeterministicEventBus::<256>::new());
    let mut js = Box::new(JobSystem::<256>::new());
    let mut coordinator = Box::new(WorkerPoolCoordinator::<8, 256>::new(4));

    let mut results_buf = [SweptAabbResult64::empty(); 64];
    let mut total_hits = 0usize;
    let mut earliest_toi = u32::MAX;
    let mut dispatched_jobs = 0usize;

    for iter in 0..iters {
        // Multi-threaded job system partition dispatch
        dispatched_jobs = dispatch_ccd_hitboxes_with_jobs(
            &slabs,
            4,
            &mut js,
            &mut coordinator,
        );
        coordinator.drain_all(&mut js);

        // Evaluate swept hitboxes against simulated defender hurtboxes
        let mut iter_hits = 0usize;
        for i in 0..count {
            let hb = &slabs[i];
            let origin_x = ((i as i32 * 43) % 1500) - 750;
            let origin_y = ((i as i32 * 61) % 1500) - 750;
            let origin_z = 800 + ((i as i32 * 29) % 400);

            // Position defender ahead in punch path to guarantee sub-frame swept TOI
            let defender_x = origin_x + 250;
            let defender_y = origin_y;
            let defender_z = origin_z;
            let hurtbox_min = (defender_x - 100, defender_y - 100, defender_z - 100);
            let hurtbox_max = (defender_x + 100, defender_y + 100, defender_z + 100);
            let hurtbox_disp = (0, 0, 0);

            if let Some(res) = swept_hitbox_vs_hurtbox(hb, hurtbox_min, hurtbox_max, hurtbox_disp, (i + 100) as u32) {
                if iter_hits < results_buf.len() {
                    if res.toi < earliest_toi {
                        earliest_toi = res.toi;
                    }
                    results_buf[iter_hits] = res;
                    iter_hits += 1;
                }
            }
        }
        total_hits = iter_hits;

        // Emit detected hits to deterministic event bus
        emit_ccd_hits_to_event_bus(
            &results_buf[..iter_hits],
            300 + (iter as u32),
            &mut bus,
        );
    }

    let ccd_hash = compute_ccd_consensus_hash(&results_buf[..total_hits]);
    let bus_hash = bus.compute_consensus_hash();
    let combined_hash = ccd_hash ^ (bus_hash.rotate_left(19));

    if !out_stats.is_null() {
        unsafe {
            *out_stats.add(0) = count as u32;
            *out_stats.add(1) = total_hits as u32;
            *out_stats.add(2) = if earliest_toi == u32::MAX { 0 } else { earliest_toi };
            *out_stats.add(3) = (combined_hash & 0xFFFF_FFFF) as u32;
            *out_stats.add(4) = (combined_hash >> 32) as u32;
            *out_stats.add(5) = dispatched_jobs as u32;
            *out_stats.add(6) = base_disp_x.abs() as u32; // mm/tick displacement
            *out_stats.add(7) = bus.count as u32;
        }
    }

    total_hits as u32
}

#[no_mangle]
pub extern "C" fn axiom_ragdoll_bench_pbd(
    bone_count: u32,
    iterations: u32,
    sever_target_idx: u32,
    out_stats: *mut u32,
) -> u32 {
    use axiom_core::ragdoll::*;
    use axiom_core::event_bus::DeterministicEventBus;
    use axiom_core::job_system::JobSystem;
    use axiom_core::worker_pool::WorkerPoolCoordinator;

    let count = (bone_count as usize).clamp(4, 16);
    let iters = (iterations as usize).clamp(1, 20);

    let mut engine = Box::new(RagdollEngine::<16, 16>::new());
    let mut bus = Box::new(DeterministicEventBus::<256>::new());
    let mut js = Box::new(JobSystem::<256>::new());
    let mut coordinator = Box::new(WorkerPoolCoordinator::<8, 256>::new(4));

    // Construct modular mecha skeleton hierarchy
    // Root bone at origin (0, 1500, 0)
    let b0 = RagdollBoneSlab64::new(0, 0, (0, 1500, 0), 0, 100, 200); // Fixed anchor
    engine.add_bone(b0);

    for i in 1..count {
        let parent = (i - 1) as u8;
        let pos = ((i as i32) * 200, 1500, 0);
        let b = RagdollBoneSlab64::new(i as u8, parent, pos, 65536, 50, 200);
        engine.add_bone(b);

        let j = RagdollJointSlab64::new(
            parent,
            i as u8,
            if i % 2 == 0 { RagdollJointType::Hinge } else { RagdollJointType::BallSocket },
            200,
            50_000,
        );
        engine.add_joint(j);
    }

    // Closed-form Two-Bone IK reach correction for right arm/punch
    let p_shoulder = (0, 1500, 0);
    let p_target = (350, 1650, 50);
    let pole_vector = (0, 2000, 0);
    let elbow_pos = solve_two_bone_ik(p_shoulder, 200, 200, p_target, pole_vector);
    if count >= 3 {
        engine.bones[1].pos_x = elbow_pos.0;
        engine.bones[1].pos_y = elbow_pos.1;
        engine.bones[1].pos_z = elbow_pos.2;
        engine.bones[2].pos_x = p_target.0;
        engine.bones[2].pos_y = p_target.1;
        engine.bones[2].pos_z = p_target.2;
    }

    let mut dispatched_jobs = 0usize;
    let mut severances = 0u32;

    for iter in 0..iters {
        // Multi-threaded job system partition dispatch
        dispatched_jobs = dispatch_ragdoll_with_jobs(
            &engine,
            4,
            &mut js,
            &mut coordinator,
        );
        coordinator.drain_all(&mut js);

        // Perform PBD simulation step with gravity and floor collision
        engine.step_ragdoll(-10, 4, 0, 5000);

        // Sever designated target joint mid-simulation if specified
        if sever_target_idx > 0 && (sever_target_idx as usize) < engine.active_joints_count && iter == iters / 2 {
            if engine.sever_joint(sever_target_idx as usize) {
                severances += 1;
            }
        }

        // Emit impact & severance report to deterministic event bus
        let mut reports = [RagdollImpactReport::default(); 16];
        let mut rep_count = 0;
        for i in 0..engine.active_bones_count {
            let b = &engine.bones[i];
            if b.is_active() {
                reports[rep_count] = RagdollImpactReport {
                    entity_id: 1,
                    bone_id: b.bone_id,
                    joint_id: if i > 0 { (i - 1) as u8 } else { 0 },
                    is_severed: if b.is_severed() { 1 } else { 0 },
                    flags: b.flags,
                    impact_force: 10_000 + (b.pos_y.abs() as u32 * 5),
                    hit_x: b.pos_x,
                    hit_y: b.pos_y,
                    hit_z: b.pos_z,
                    impulse_x: b.vel_x,
                    impulse_y: b.vel_y,
                    impulse_z: b.vel_z,
                };
                rep_count += 1;
            }
        }

        emit_ragdoll_impacts_to_event_bus(&reports[..rep_count], 400 + iter as u32, &mut bus);
    }

    let ragdoll_hash = engine.compute_consensus_hash();
    let bus_hash = bus.compute_consensus_hash();
    let combined_hash = ragdoll_hash ^ (bus_hash.rotate_left(23));

    if !out_stats.is_null() {
        unsafe {
            *out_stats.add(0) = engine.active_bones_count as u32;
            *out_stats.add(1) = engine.active_joints_count as u32;
            *out_stats.add(2) = (engine.active_joints_count * iters * 4) as u32; // Constraints relaxed
            *out_stats.add(3) = (combined_hash & 0xFFFF_FFFF) as u32;
            *out_stats.add(4) = (combined_hash >> 32) as u32;
            *out_stats.add(5) = dispatched_jobs as u32;
            *out_stats.add(6) = severances;
            *out_stats.add(7) = bus.count as u32;
        }
    }

    engine.active_bones_count as u32
}

#[no_mangle]
pub extern "C" fn axiom_terrain_bench_crater(
    grid_dim: u32,
    crater_count: u32,
    impact_energy_j: u32,
    out_stats: *mut u32,
) -> u32 {
    use axiom_core::deformable_mesh::*;
    use axiom_core::event_bus::DeterministicEventBus;
    use axiom_core::job_system::JobSystem;
    use axiom_core::worker_pool::WorkerPoolCoordinator;

    let dim = (grid_dim as usize).clamp(9, 33);
    let craters_to_apply = (crater_count as usize).clamp(1, 16);
    let energy = if impact_energy_j > 0 { impact_energy_j as u64 } else { 50_000 };

    let mut grid = Box::new(DeformableMeshGrid::<33, 1089, 32>::new_flat(-8000, -8000, 0, 500));
    let mut bus = Box::new(DeterministicEventBus::<256>::new());
    let mut js = Box::new(JobSystem::<256>::new());
    let mut coordinator = Box::new(WorkerPoolCoordinator::<8, 256>::new(4));

    let mut total_deformed_vertices = 0usize;
    let mut dispatched_jobs = 0usize;

    for i in 0..craters_to_apply {
        // Multi-threaded job system partition dispatch
        dispatched_jobs = dispatch_terrain_deform_with_jobs(
            &grid,
            4,
            &mut js,
            &mut coordinator,
        );
        coordinator.drain_all(&mut js);

        let cx = ((i as i32) % 3 - 1) * 1500;
        let cz = ((i as i32) / 3 - 1) * 1500;

        let crater = CraterSlab64::from_kinetic_impact(
            cx, 0, cz,
            energy + (i as u64) * 10_000,
            2_000,
            12_000,
            TERRAIN_MAT_DIRT,
            (i + 1) as u32,
            100 + (i as u32),
        );

        let deformed = grid.apply_crater_displacement(&crater);
        total_deformed_vertices += deformed;

        let report = TerrainCraterReport::from_crater(&crater);
        emit_crater_to_event_bus(&report, 100 + (i as u32), &mut bus);
    }

    let mesh_hash = grid.compute_consensus_hash();
    let bus_hash = bus.compute_consensus_hash();
    let combined_hash = mesh_hash ^ (bus_hash.rotate_left(17));

    if !out_stats.is_null() {
        unsafe {
            *out_stats.add(0) = (dim * dim) as u32;                 // Total vertices in grid
            *out_stats.add(1) = grid.crater_count as u32;            // Craters applied
            *out_stats.add(2) = total_deformed_vertices as u32;      // Vertices displaced
            *out_stats.add(3) = (combined_hash & 0xFFFF_FFFF) as u32;
            *out_stats.add(4) = (combined_hash >> 32) as u32;
            *out_stats.add(5) = dispatched_jobs as u32;              // Worker partition jobs
            *out_stats.add(6) = grid.min_elevation_mm.unsigned_abs(); // Excavation bowl depth in mm
            *out_stats.add(7) = bus.count as u32;                    // Event bus crater events
        }
    }

    total_deformed_vertices as u32
}

#[no_mangle]
pub extern "C" fn axiom_audio_bench_destruction(
    voice_count: u32,
    material_id: u32,
    impact_energy_j: u32,
    out_stats: *mut u32,
) -> u32 {
    use axiom_core::destruction_audio::*;
    use axiom_core::event_bus::DeterministicEventBus;
    use axiom_core::job_system::JobSystem;
    use axiom_core::worker_pool::WorkerPoolCoordinator;

    let voices_to_spawn = (voice_count as usize).clamp(1, 16);
    let mat = match material_id {
        0 => AUDIO_MAT_DIRT,
        1 => AUDIO_MAT_STONE,
        2 => AUDIO_MAT_METAL,
        3 => AUDIO_MAT_MOLTEN,
        _ => AUDIO_MAT_SAND,
    };
    let energy = if impact_energy_j > 0 { impact_energy_j } else { 50_000 };

    let mut engine = Box::new(DestructionAudioEngine::<16, 4>::default());
    let mut bus = Box::new(DeterministicEventBus::<256>::new());
    let mut js = Box::new(JobSystem::<256>::new());
    let mut coordinator = Box::new(WorkerPoolCoordinator::<8, 256>::new(4));

    for i in 0..voices_to_spawn {
        let px = ((i as i32) % 3 - 1) * 2000;
        let pz = ((i as i32) / 3 - 1) * 2000;

        let voice = DestructionVoiceSlab64::from_impact(
            px, 0, pz,
            energy + (i as u32) * 5_000,
            mat,
            (i + 1) as u32,
            100 + (i as u32),
        );

        engine.spawn_destruction_voice(voice);

        let report = DestructionAudioReport::from_voice(&voice);
        emit_destruction_audio_to_event_bus(&report, 100 + (i as u32), &mut bus);
    }

    // Multi-threaded job system partition dispatch
    let dispatched_jobs = dispatch_destruction_audio_with_jobs(
        &engine,
        4,
        &mut js,
        &mut coordinator,
    );
    coordinator.drain_all(&mut js);

    // Synthesize 1 simulation tick frame (800 stereo samples ~ 16.6ms @ 48kHz)
    let mut stereo_frame = [(0i16, 0i16); 800];
    let active_synthesized = engine.step_synthesis(&mut stereo_frame);

    // Compute peak amplitude and energy across synthesized stereo buffer
    let mut peak_amplitude: u32 = 0;
    for (l, r) in &stereo_frame {
        let abs_l = l.unsigned_abs() as u32;
        let abs_r = r.unsigned_abs() as u32;
        peak_amplitude = peak_amplitude.max(abs_l).max(abs_r);
    }

    let audio_hash = engine.compute_consensus_hash();
    let bus_hash = bus.compute_consensus_hash();
    let combined_hash = audio_hash ^ (bus_hash.rotate_left(17));

    if !out_stats.is_null() {
        unsafe {
            *out_stats.add(0) = engine.voices.len() as u32;              // Total voice slabs (16)
            *out_stats.add(1) = engine.total_voices_spawned as u32;       // Spawned voices
            *out_stats.add(2) = active_synthesized as u32;                // Active synthesized voices
            *out_stats.add(3) = (combined_hash & 0xFFFF_FFFF) as u32;    // Low 32-bit consensus hash
            *out_stats.add(4) = (combined_hash >> 32) as u32;            // High 32-bit consensus hash
            *out_stats.add(5) = dispatched_jobs as u32;                  // Worker partition jobs
            *out_stats.add(6) = peak_amplitude;                          // Peak PCM amplitude (0..32767)
            *out_stats.add(7) = bus.count as u32;                        // Event bus audio events
        }
    }

    peak_amplitude
}

#[no_mangle]
pub extern "C" fn axiom_debris_bench_burst(
    particle_count: u32,
    material_id: u32,
    impulse_energy_j: u32,
    out_stats: *mut u32,
) -> u32 {
    use axiom_core::debris_particle::*;
    use axiom_core::event_bus::DeterministicEventBus;
    use axiom_core::job_system::JobSystem;
    use axiom_core::worker_pool::WorkerPoolCoordinator;

    let count_to_spawn = (particle_count as usize).clamp(4, 128);
    let mat = match material_id {
        0 => DEBRIS_MAT_DIRT,
        1 => DEBRIS_MAT_STONE,
        2 => DEBRIS_MAT_METAL,
        3 => DEBRIS_MAT_MOLTEN,
        _ => DEBRIS_MAT_SAND,
    };
    let energy = if impulse_energy_j > 0 { impulse_energy_j } else { 60_000 };

    let mut pool = Box::new(DebrisParticlePool::<128>::new(12345, 0, 3000));
    let mut bus = Box::new(DeterministicEventBus::<256>::new());
    let mut js = Box::new(JobSystem::<256>::new());
    let mut coordinator = Box::new(WorkerPoolCoordinator::<8, 256>::new(4));

    let spawned = pool.scatter_debris_burst(
        0, 1000, 0,
        count_to_spawn,
        2000, 8000,
        mat,
        1,
        energy,
    );

    let report = DebrisBurstReport::from_burst(
        (0, 1000, 0),
        spawned as u16,
        2000,
        8000,
        16384,
        mat,
        1,
        energy,
    );
    emit_debris_burst_to_event_bus(&report, 100, &mut bus);

    // Multi-threaded job system partition dispatch
    let dispatched_jobs = dispatch_debris_with_jobs(
        &pool,
        4,
        &mut js,
        &mut coordinator,
    );
    coordinator.drain_all(&mut js);

    // Step ballistic simulation across 10 ticks
    let mut contacts = [ParticleContactSlab64::empty(); 16];
    let mut total_contacts = 0;
    for tick in 1..=10 {
        total_contacts += pool.step_debris_simulation(&mut contacts, 100 + tick);
    }

    let debris_hash = pool.compute_consensus_hash();
    let bus_hash = bus.compute_consensus_hash();
    let combined_hash = debris_hash ^ (bus_hash.rotate_left(19));

    if !out_stats.is_null() {
        unsafe {
            *out_stats.add(0) = pool.particles.len() as u32;             // Total particle slabs (128)
            *out_stats.add(1) = pool.total_spawned as u32;               // Spawned particles
            *out_stats.add(2) = pool.active_count() as u32;              // Active particles
            *out_stats.add(3) = (combined_hash & 0xFFFF_FFFF) as u32;    // Low 32-bit consensus hash
            *out_stats.add(4) = (combined_hash >> 32) as u32;            // High 32-bit consensus hash
            *out_stats.add(5) = dispatched_jobs as u32;                  // Worker partition jobs
            *out_stats.add(6) = total_contacts as u32;                   // Surface/boundary contacts
            *out_stats.add(7) = pool.total_bounces as u32;               // Total bounces
        }
    }

    spawned as u32
}

#[no_mangle]
pub extern "C" fn axiom_shockwave_bench_burst(
    wave_count: u32,
    dust_count: u32,
    impulse_energy_j: u32,
    out_stats: *mut u32,
) -> u32 {
    use axiom_core::shockwave::*;
    use axiom_core::event_bus::DeterministicEventBus;
    use axiom_core::job_system::JobSystem;
    use axiom_core::worker_pool::WorkerPoolCoordinator;

    let waves_to_spawn = (wave_count as usize).clamp(1, 32);
    let dust_to_spawn = (dust_count as usize).clamp(1, 32);
    let energy = if impulse_energy_j > 0 { impulse_energy_j } else { 100_000 };

    let mut engine = Box::new(VolumetricShockwaveEngine::<32, 32>::new());
    let mut bus = Box::new(DeterministicEventBus::<256>::new());
    let mut js = Box::new(JobSystem::<256>::new());
    let mut coordinator = Box::new(WorkerPoolCoordinator::<8, 256>::new(4));

    let mut spawned_waves = 0;
    for i in 0..waves_to_spawn {
        let offset = (i as i32) * 100;
        let shock_type = if i % 2 == 0 { SHOCKWAVE_TYPE_SPHERICAL } else { SHOCKWAVE_TYPE_TOROIDAL };
        if engine.trigger_shockwave(
            (offset, 500, offset),
            100,
            75_000,
            340_000,
            150,
            energy,
            (i + 1) as u32,
            1,
            shock_type,
            100,
        ).is_some() {
            spawned_waves += 1;
        }
    }

    let mut spawned_dust = 0;
    for i in 0..dust_to_spawn {
        let offset = (i as i32) * 80;
        if engine.trigger_dust_cloud(
            (offset, 0, offset),
            150,
            4000,
            120_000,
            200,
            1,
            (i + 1) as u32,
            60,
            100,
        ).is_some() {
            spawned_dust += 1;
        }
    }

    let report = ShockwaveReport::from_explosion(
        (0, 500, 0),
        3500,
        340_000,
        energy,
        1,
        1,
        200,
    );
    emit_shockwave_to_event_bus(&report, 100, &mut bus);

    // Multi-threaded job system partition dispatch
    let dispatched_jobs = dispatch_shockwave_with_jobs(
        &engine,
        4,
        &mut js,
        &mut coordinator,
    );
    coordinator.drain_all(&mut js);

    // Step volumetric shockwave & dust simulation across 10 ticks
    for tick in 1..=10 {
        engine.step_simulation(100 + tick);
    }

    let wave_hash = engine.compute_consensus_hash();
    let bus_hash = bus.compute_consensus_hash();
    let combined_hash = wave_hash ^ (bus_hash.rotate_left(23));
    let transmittance = engine.sample_fog_transmittance((500, 200, 500));

    if !out_stats.is_null() {
        unsafe {
            *out_stats.add(0) = 32;                                       // Capacity shockwaves (32)
            *out_stats.add(1) = engine.active_shockwaves_count() as u32;  // Active shockwaves
            *out_stats.add(2) = engine.active_dust_count() as u32;        // Active dust clouds
            *out_stats.add(3) = (combined_hash & 0xFFFF_FFFF) as u32;     // Low 32-bit consensus hash
            *out_stats.add(4) = (combined_hash >> 32) as u32;             // High 32-bit consensus hash
            *out_stats.add(5) = dispatched_jobs as u32;                   // Worker partition jobs
            *out_stats.add(6) = transmittance as u32;                     // Fog transmittance at sample point (Q16.16)
            *out_stats.add(7) = spawned_dust as u32;                      // Spawned dust emitters
        }
    }

    spawned_waves as u32
}

#[no_mangle]
pub extern "C" fn axiom_xpbd_bench_burst(
    bone_count: u32,
    substeps: u32,
    impulse_energy_j: u32,
    out_stats: *mut u32,
) -> u32 {
    use axiom_core::xpbd_ragdoll::*;
    use axiom_core::event_bus::DeterministicEventBus;
    use axiom_core::job_system::JobSystem;
    use axiom_core::worker_pool::WorkerPoolCoordinator;

    let bones_to_spawn = (bone_count as usize).clamp(2, 16);
    let subs = if substeps > 0 { substeps as usize } else { 4 };
    let energy = if impulse_energy_j > 0 { impulse_energy_j } else { 85_000 };

    let mut engine = Box::new(XpbdRagdollEngine::<16, 16>::new());
    let mut bus = Box::new(DeterministicEventBus::<256>::new());
    let mut js = Box::new(JobSystem::<256>::new());
    let mut coordinator = Box::new(WorkerPoolCoordinator::<8, 256>::new(4));

    // Spawn mecha chain
    let mut spawned_bones = 0;
    for i in 0..bones_to_spawn {
        let parent = if i == 0 { 255 } else { (i - 1) as u8 };
        let inv_mass = if i == 0 { 0 } else { 65536 }; // Bone 0 is anchor
        let pos_x = (i as i32) * 250;
        let pos_y = 2000;
        let pos_z = 0;
        if engine.add_bone(XpbdBoneSlab64::new(
            i as u8,
            parent,
            (pos_x, pos_y, pos_z),
            inv_mass,
            75,
            250,
        )).is_some() {
            spawned_bones += 1;
        }

        if i > 0 {
            // Add distance constraint with 5% yield strain and 20% rupture strain
            engine.add_constraint(XpbdConstraintSlab64::new_distance(
                (i - 1) as u8,
                i as u8,
                250,
                0, // Rigid XPBD
                3277,
                13107,
            ));
        }
    }

    // Apply high impact impulse to distal bone to trigger plastic strain or rupture
    if spawned_bones > 1 {
        let last_idx = spawned_bones - 1;
        let impulse_mms = (energy / 50).min(5000) as i32;
        engine.bones[last_idx].vel_x = impulse_mms;
        engine.bones[last_idx].vel_y = -(impulse_mms / 2);
    }

    // Multi-threaded job system partition dispatch
    let dispatched_jobs = dispatch_xpbd_with_jobs(
        &engine,
        4,
        &mut js,
        &mut coordinator,
    );
    coordinator.drain_all(&mut js);

    // Step XPBD ragdoll simulation across 10 ticks
    for _tick in 1..=10 {
        engine.step_xpbd(-9810, subs, 0, 5000);
    }

    // Emit event report if plastic or severance happened
    let is_severed = engine.total_severances > 0;
    let report = XpbdEventReport {
        entity_id: 1,
        bone_a: 0,
        bone_b: (spawned_bones - 1) as u8,
        constraint_id: 0,
        is_severed: if is_severed { 1 } else { 0 },
        strain_q16: engine.constraints[0].current_strain_q16,
        tension_force_n: energy,
        rupture_pos_xyz: [engine.bones[spawned_bones - 1].pos_x, engine.bones[spawned_bones - 1].pos_y, 0],
        impulse_xyz: [engine.bones[spawned_bones - 1].vel_x, engine.bones[spawned_bones - 1].vel_y, 0],
    };
    emit_xpbd_to_event_bus(&report, 100, &mut bus);

    let xpbd_hash = engine.compute_consensus_hash();
    let bus_hash = bus.compute_consensus_hash();
    let combined_hash = xpbd_hash ^ (bus_hash.rotate_left(29));

    if !out_stats.is_null() {
        unsafe {
            *out_stats.add(0) = 16;                                      // Capacity bone slabs (16)
            *out_stats.add(1) = engine.active_bones_count as u32;        // Active bones count
            *out_stats.add(2) = engine.active_constraints_count as u32;  // Active constraints count
            *out_stats.add(3) = (combined_hash & 0xFFFF_FFFF) as u32;    // Low 32-bit consensus hash
            *out_stats.add(4) = (combined_hash >> 32) as u32;            // High 32-bit consensus hash
            *out_stats.add(5) = dispatched_jobs as u32;                  // Worker partition jobs
            *out_stats.add(6) = engine.total_plastic_events;             // Total plastic yield events
            *out_stats.add(7) = engine.total_severances;                 // Total severed constraints
        }
    }

    spawned_bones as u32
}

#[no_mangle]
pub extern "C" fn axiom_soft_body_bench_burst(
    cluster_count: u32,
    particles_per_cluster: u32,
    deform_force_n: u32,
    out_stats: *mut u32,
) -> u32 {
    use axiom_core::soft_body::*;
    use axiom_core::event_bus::DeterministicEventBus;
    use axiom_core::job_system::JobSystem;
    use axiom_core::worker_pool::WorkerPoolCoordinator;

    let clusters_to_spawn = (cluster_count as usize).clamp(1, 8);
    let pts_per_cl = (particles_per_cluster as usize).clamp(2, 8);
    let force = if deform_force_n > 0 { deform_force_n } else { 50_000 };

    let mut engine = Box::new(SoftBodyEngine::<64, 8>::new());
    let mut bus = Box::new(DeterministicEventBus::<256>::new());
    let mut js = Box::new(JobSystem::<256>::new());
    let mut coordinator = Box::new(WorkerPoolCoordinator::<8, 256>::new(4));

    let offsets_8 = [
        (-100, -100, -100), (100, -100, -100), (-100, 100, -100), (100, 100, -100),
        (-100, -100, 100), (100, -100, 100), (-100, 100, 100), (100, 100, 100),
    ];

    let mut total_spawned_particles = 0;
    for c in 0..clusters_to_spawn {
        let center = (c as i32 * 600 - ((clusters_to_spawn as i32 - 1) * 300), 1500, 0);
        let start_offset = total_spawned_particles as u16;

        let cluster = SoftBodyClusterSlab64::new(
            center,
            start_offset,
            pts_per_cl as u16,
            32768, // 0.5 shape stiffness
            32768, // 0.5 volume stiffness
            1_000_000,
            c as u8,
        );
        engine.add_cluster(cluster);

        for p in 0..pts_per_cl {
            let off = offsets_8[p % 8];
            let pos = (center.0 + off.0, center.1 + off.1, center.2 + off.2);
            let mut part = SoftBodyParticleSlab64::new(
                pos,
                off,
                65536,
                25,
                c as u16,
                SOFT_MAT_RUBBER,
            );
            // Apply deformation impulse to last particle in each cluster
            if p == pts_per_cl - 1 {
                part.vel_x = (force / 10).min(5000) as i32;
                part.vel_y = -((force / 20).min(2500) as i32);
            }
            if engine.add_particle(part).is_some() {
                total_spawned_particles += 1;
            }
        }
    }

    // Parallel JobSystem dispatch
    let dispatched_jobs = dispatch_soft_body_with_jobs(
        &engine,
        4,
        &mut js,
        &mut coordinator,
    );
    coordinator.drain_all(&mut js);

    // Step simulation for 10 ticks
    for _ in 0..10 {
        engine.step_simulation(-9810, 0);
    }

    // Emit representative deformation event
    let report = SoftBodyEventReport {
        entity_id: 1,
        cluster_id: 0,
        flags: if engine.total_ruptures > 0 { 1 } else { 0 },
        max_strain_q16: engine.particles[0].stress_q16 as u32,
        volume_ratio_q16: 65536,
        centroid_pos_xyz: [engine.clusters[0].center_x, engine.clusters[0].center_y, engine.clusters[0].center_z],
        deform_impulse_xyz: [engine.particles[pts_per_cl - 1].vel_x, engine.particles[pts_per_cl - 1].vel_y, 0],
    };
    emit_soft_body_to_event_bus(&report, 100, &mut bus);

    let sb_hash = engine.compute_consensus_hash();
    let bus_hash = bus.compute_consensus_hash();
    let combined_hash = sb_hash ^ (bus_hash.rotate_left(29));

    if !out_stats.is_null() {
        unsafe {
            *out_stats.add(0) = 8;                                        // Capacity clusters (8)
            *out_stats.add(1) = engine.active_clusters_count as u32;      // Active clusters count
            *out_stats.add(2) = engine.active_particles_count as u32;     // Active particles count
            *out_stats.add(3) = (combined_hash & 0xFFFF_FFFF) as u32;     // Low 32-bit consensus hash
            *out_stats.add(4) = (combined_hash >> 32) as u32;             // High 32-bit consensus hash
            *out_stats.add(5) = dispatched_jobs as u32;                   // Worker partition jobs
            *out_stats.add(6) = engine.total_deformations;                // Total deformation events
            *out_stats.add(7) = engine.total_ruptures;                    // Total ruptured particles
        }
    }

    total_spawned_particles as u32
}

#[no_mangle]
pub extern "C" fn axiom_actuator_bench_burst(
    actuator_count: u32,
    chamber_count: u32,
    target_pressure_kpa: u32,
    out_stats: *mut u32,
) -> u32 {
    use axiom_core::actuator::*;
    use axiom_core::event_bus::DeterministicEventBus;
    use axiom_core::job_system::JobSystem;
    use axiom_core::worker_pool::WorkerPoolCoordinator;

    let acts_to_spawn = (actuator_count as usize).clamp(1, 16);
    let chambers_to_spawn = (chamber_count as usize).clamp(1, 4);
    let pressure = if target_pressure_kpa > 0 { target_pressure_kpa } else { 15_000 };

    let mut engine = Box::new(MechaActuatorEngine::<16, 4>::new());
    let mut bus = Box::new(DeterministicEventBus::<256>::new());
    let mut js = Box::new(JobSystem::<256>::new());
    let mut coordinator = Box::new(WorkerPoolCoordinator::<8, 256>::new(4));

    for ch in 0..chambers_to_spawn {
        let chamber = HydraulicChamberSlab64::new(
            ch as u8,
            1,
            200_000,
            pressure as i32,
            1_500_000,
        );
        engine.add_chamber(chamber);
    }

    let mut total_spawned_actuators = 0;
    for i in 0..acts_to_spawn {
        let ch_idx = (i % chambers_to_spawn) as u8;
        let mut act = if i % 2 == 0 {
            ActuatorSlab64::new_hydraulic(
                1,
                2,
                (0, 0, 0),
                (0, 500 + (i as i32 * 30), 0),
                100,
                200,
                40_000,
            )
        } else {
            ActuatorSlab64::new_mckibben_pam(
                2,
                3,
                (0, 0, 0),
                (0, 500 + (i as i32 * 30), 0),
                100,
                1500,
            )
        };
        act.chamber_idx = ch_idx;
        act.entity_id = 1;
        if engine.add_actuator(act).is_some() {
            total_spawned_actuators += 1;
        }
    }

    // Parallel JobSystem dispatch
    let dispatched_jobs = dispatch_actuators_with_jobs(
        &engine,
        4,
        &mut js,
        &mut coordinator,
    );
    coordinator.drain_all(&mut js);

    // Step simulation for 10 ticks
    for _ in 0..10 {
        engine.step_simulation();
    }

    // Emit representative actuator event
    let report = ActuatorEventReport {
        entity_id: 1,
        actuator_id: 0,
        chamber_id: 0,
        flags: if engine.total_bursts > 0 { ACTUATOR_FLAG_BURST } else { 0 },
        actuator_type: ACTUATOR_TYPE_HYDRAULIC,
        pressure_kpa: engine.actuators[0].pressure_kpa,
        thrust_n: ((engine.actuators[0].pressure_kpa as i64 * engine.actuators[0].piston_area_mm2 as i64) / 1000) as i32,
        current_len_mm: engine.actuators[0].current_len_mm,
        target_len_mm: engine.actuators[0].target_len_mm,
        pos_x: engine.actuators[0].anchor_b_x,
        pos_y: engine.actuators[0].anchor_b_y,
        pos_z: engine.actuators[0].anchor_b_z,
        impulse_m_ns: 2000,
        _reserved: [0; 4],
    };
    emit_actuator_to_event_bus(&report, 100, &mut bus);

    let act_hash = engine.compute_consensus_hash();
    let bus_hash = bus.compute_consensus_hash();
    let combined_hash = act_hash ^ (bus_hash.rotate_left(31));

    if !out_stats.is_null() {
        unsafe {
            *out_stats.add(0) = 16;                                      // Capacity actuators (16)
            *out_stats.add(1) = engine.active_actuators_count as u32;    // Active actuators count
            *out_stats.add(2) = engine.active_chambers_count as u32;     // Active chambers count
            *out_stats.add(3) = (combined_hash & 0xFFFF_FFFF) as u32;    // Low 32-bit consensus hash
            *out_stats.add(4) = (combined_hash >> 32) as u32;            // High 32-bit consensus hash
            *out_stats.add(5) = dispatched_jobs as u32;                  // Worker partition jobs
            *out_stats.add(6) = engine.peak_thrust_n;                    // Peak mechanical thrust N
            *out_stats.add(7) = engine.total_bursts;                     // Total bursts
        }
    }

    total_spawned_actuators as u32
}

#[no_mangle]
pub extern "C" fn axiom_aero_bench_burst(
    body_count: u32,
    cell_count: u32,
    test_speed_mm_s: u32,
    out_stats: *mut u32,
) -> u32 {
    use axiom_core::aerodynamics::*;
    use axiom_core::event_bus::DeterministicEventBus;
    use axiom_core::job_system::JobSystem;
    use axiom_core::worker_pool::WorkerPoolCoordinator;

    let bodies_to_spawn = (body_count as usize).clamp(1, 16);
    let cells_to_spawn = (cell_count as usize).clamp(1, 4);
    let speed = if test_speed_mm_s > 0 { test_speed_mm_s as i32 } else { 25_000 };

    let mut engine = Box::new(MechaAerodynamicsEngine::<16, 4>::new());
    let mut bus = Box::new(DeterministicEventBus::<256>::new());
    let mut js = Box::new(JobSystem::<256>::new());
    let mut coordinator = Box::new(WorkerPoolCoordinator::<8, 256>::new(4));

    for c in 0..cells_to_spawn {
        let cell = AirflowGridSlab64::new(c as u8, [c as i32 * 1000, 0, 0]);
        engine.register_cell(cell);
    }

    let mut total_spawned_bodies = 0;
    for i in 0..bodies_to_spawn {
        let b_type = match i % 4 {
            0 => AERO_BODY_CYLINDER,
            1 => AERO_BODY_SPHERE,
            2 => AERO_BODY_PLATE,
            _ => AERO_BODY_AIRFOIL,
        };
        let mut body = AeroBodySlab64::new(
            1,
            b_type,
            40_000 + (i as u32 * 2000),
            25_000 + (i as u32 * 1000),
            100 + (i as u16 * 10),
            52428,
        );
        body.vel_xyz = [speed + (i as i32 * 1000), 0, 0];
        body.cell_idx = (i % cells_to_spawn) as u8;

        if engine.register_body(body).is_some() {
            total_spawned_bodies += 1;
        }
    }

    // Parallel JobSystem dispatch
    let dispatched_jobs = dispatch_aerodynamics_with_jobs(
        &engine,
        4,
        &mut js,
        &mut coordinator,
    );
    coordinator.drain_all(&mut js);

    // Step simulation for 10 ticks
    let mut total_lock_ins = 0;
    for _ in 0..10 {
        let (_eval, lock_in) = engine.step_simulation();
        total_lock_ins += lock_in;
    }

    // Emit representative aerodynamic event
    if engine.active_bodies_count > 0 {
        let report = AeroEventReport {
            pos_xyz: engine.bodies[0].pos_xyz,
            rel_vel_xyz: engine.bodies[0].vel_xyz,
            drag_force_n: engine.bodies[0].drag_force_n,
            viv_force_n: engine.bodies[0].viv_force_n,
            shedding_freq_q8: engine.bodies[0].shedding_freq_q8,
            entity_id: engine.bodies[0].entity_id,
            body_id: 0,
            flags: engine.bodies[0].flags,
            body_type: engine.bodies[0].body_type,
            reserved: [0; 2],
        };
        emit_aero_to_event_bus(&report, 100, &mut bus);
    }

    let aero_hash = engine.compute_consensus_hash();
    let bus_hash = bus.compute_consensus_hash();
    let combined_hash = aero_hash ^ (bus_hash.rotate_left(31));

    if !out_stats.is_null() {
        unsafe {
            *out_stats.add(0) = 16;                                         // Capacity bodies (16)
            *out_stats.add(1) = engine.active_bodies_count as u32;          // Active bodies count
            *out_stats.add(2) = engine.active_cells_count as u32;           // Active cells count
            *out_stats.add(3) = (combined_hash & 0xFFFF_FFFF) as u32;       // Low 32-bit consensus hash
            *out_stats.add(4) = (combined_hash >> 32) as u32;               // High 32-bit consensus hash
            *out_stats.add(5) = dispatched_jobs as u32;                     // Worker partition jobs
            *out_stats.add(6) = engine.bodies[0].drag_force_n.unsigned_abs(); // Drag force N
            *out_stats.add(7) = total_lock_ins as u32;                      // Total lock-in occurrences
        }
    }

    total_spawned_bodies as u32
}

#[no_mangle]
pub extern "C" fn axiom_turbulence_bench_burst(
    cell_count: u32,
    shock_count: u32,
    flow_speed_mm_s: u32,
    out_stats: *mut u32,
) -> u32 {
    use axiom_core::turbulence::*;
    use axiom_core::event_bus::DeterministicEventBus;
    use axiom_core::job_system::JobSystem;
    use axiom_core::worker_pool::WorkerPoolCoordinator;

    let cells_to_spawn = (cell_count as usize).clamp(1, 16);
    let shocks_to_spawn = (shock_count as usize).clamp(1, 4);
    let speed = if flow_speed_mm_s > 0 { flow_speed_mm_s } else { 510_000 }; // 1.5 Mach default

    let mut engine = Box::new(MechaTurbulenceEngine::<16, 4>::new());
    let mut bus = Box::new(DeterministicEventBus::<256>::new());
    let mut js = Box::new(JobSystem::<256>::new());
    let mut coordinator = Box::new(WorkerPoolCoordinator::<8, 256>::new(4));

    for c in 0..cells_to_spawn {
        let cell = TurbulenceCellSlab64::new(
            c as u8,
            [c as i32 * 500, 0, 0],
            [1200 + c as i32 * 300, 1800, 0],
        );
        engine.register_cell(cell);
    }

    let mut total_spawned_shocks = 0;
    for s in 0..shocks_to_spawn {
        let mut shock = ShockBoundarySlab64::new(
            (s + 1) as u8,
            [0, s as i32 * 1000, 1000],
            [0, 65536, 0], // +Y normal in Q16
            12 + (s as u16 * 2),
            3500 + (s as u32 * 500),
        );
        shock.mach_q16 = ((speed as u64 * 65536) / SPEED_OF_SOUND_MMS as u64) as u32;
        if engine.register_shock(shock).is_some() {
            total_spawned_shocks += 1;
        }
    }

    // Parallel JobSystem dispatch
    let dispatched_jobs = dispatch_turbulence_with_jobs(
        &engine,
        4,
        &mut js,
        &mut coordinator,
    );
    coordinator.drain_all(&mut js);

    // Step simulation for 10 ticks
    let mut total_separated = 0;
    for _ in 0..10 {
        let (_eval, separated) = engine.step_simulation(32768); // 0.5 confinement epsilon
        total_separated += separated;
    }

    // Emit representative turbulence event
    if engine.active_cells_count > 0 {
        let report = TurbulenceEventReport {
            pos_xyz: engine.cells[0].center_xyz,
            vector_xyz: engine.cells[0].vorticity_xyz,
            impulse_or_force_n: 250,
            tke_or_pressure_ratio_q16: engine.cells[0].tke_q16,
            mach_or_freq_q8: 3840,
            bl_thickness_or_cell_id: engine.cells[0].cell_id as u16,
            emitter_id: engine.cells[0].cell_id,
            target_id: 0,
            flags: engine.cells[0].flags,
            event_kind: TURB_EVENT_KIND_VORTEX,
        };
        emit_turbulence_to_event_bus(&report, 100, &mut bus);
    }

    if engine.active_shocks_count > 0 {
        let report = TurbulenceEventReport {
            pos_xyz: engine.shocks[0].origin_xyz,
            vector_xyz: engine.shocks[0].normal_xyz,
            impulse_or_force_n: engine.shocks[0].peak_shock_impulse_n as i32,
            tke_or_pressure_ratio_q16: engine.shocks[0].pressure_jump_ratio_q16,
            mach_or_freq_q8: (engine.shocks[0].mach_q16 / 256) as u16,
            bl_thickness_or_cell_id: engine.shocks[0].bl_thickness_mm,
            emitter_id: engine.shocks[0].emitter_id,
            target_id: 1,
            flags: engine.shocks[0].flags,
            event_kind: TURB_EVENT_KIND_SHOCK_BL,
        };
        emit_turbulence_to_event_bus(&report, 101, &mut bus);
    }

    let turb_hash = engine.compute_consensus_hash();
    let bus_hash = bus.compute_consensus_hash();
    let combined_hash = turb_hash ^ (bus_hash.rotate_left(31));

    if !out_stats.is_null() {
        unsafe {
            *out_stats.add(0) = 16;                                      // Capacity cells (16)
            *out_stats.add(1) = engine.active_cells_count as u32;        // Active cells count
            *out_stats.add(2) = engine.active_shocks_count as u32;       // Active shocks count
            *out_stats.add(3) = (combined_hash & 0xFFFF_FFFF) as u32;    // Low 32-bit consensus hash
            *out_stats.add(4) = (combined_hash >> 32) as u32;            // High 32-bit consensus hash
            *out_stats.add(5) = dispatched_jobs as u32;                  // Worker partition jobs
            *out_stats.add(6) = engine.shocks[0].pressure_jump_ratio_q16;// P2/P1 Q16
            *out_stats.add(7) = total_separated as u32;                  // Total SBLI separations
        }
    }

    total_spawned_shocks as u32
}

#[no_mangle]
pub extern "C" fn axiom_radiation_bench_burst(
    cell_count: u32,
    beam_count: u32,
    beam_energy_mj: u32,
    out_stats: *mut u32,
) -> u32 {
    use axiom_core::radiation::*;
    use axiom_core::event_bus::DeterministicEventBus;
    use axiom_core::job_system::JobSystem;
    use axiom_core::worker_pool::WorkerPoolCoordinator;

    let cells_to_spawn = (cell_count as usize).clamp(1, 16);
    let beams_to_spawn = (beam_count as usize).clamp(1, 4);
    let energy = if beam_energy_mj > 0 { beam_energy_mj } else { 1_000_000 }; // 1 kJ default

    let mut engine = Box::new(MechaRadiationEngine::<16, 4>::new());
    let mut bus = Box::new(DeterministicEventBus::<256>::new());
    let mut js = Box::new(JobSystem::<256>::new());
    let mut coordinator = Box::new(WorkerPoolCoordinator::<8, 256>::new(4));

    for c in 0..cells_to_spawn {
        let cell = RadiationCellSlab64::new(
            c as u8,
            [c as i32 * 500, 0, 0],
            1000 + (c as u32 * 250),
        );
        engine.register_cell(cell);
    }

    let mut total_spawned_beams = 0;
    for b in 0..beams_to_spawn {
        let beam = ParticleBeamSlab64::new(
            1,
            (b + 2) as u8,
            [0, 0, 0],
            [0, 0, 32768],
            energy + (b as u32 * 500_000),
            1064,
            2,
        );
        if engine.register_beam(beam).is_some() {
            total_spawned_beams += 1;
        }
    }

    // Parallel JobSystem dispatch
    let dispatched_jobs = dispatch_radiation_with_jobs(
        &engine,
        4,
        &mut js,
        &mut coordinator,
    );
    coordinator.drain_all(&mut js);

    // Step simulation for 10 ticks
    let mut total_penetrations = 0;
    for _ in 0..10 {
        let (_eval, penetrating) = engine.step_simulation(0);
        total_penetrations += penetrating;
    }

    // Emit representative radiation event
    if engine.active_cells_count > 0 {
        let report = RadiationEventReport {
            pos_xyz: engine.cells[0].center_xyz,
            dir_xyz: [0, 0, 32768],
            flux_or_energy_mj: engine.cells[0].emission_power_w,
            optical_depth_or_temp: engine.cells[0].temperature_k,
            penetration_depth_mm: 0,
            wavelength_or_cell_id: engine.cells[0].cell_id as u16,
            emitter_id: engine.cells[0].cell_id,
            target_id: 0,
            flags: engine.cells[0].flags,
            event_kind: RAD_EVENT_KIND_BURST,
        };
        emit_radiation_to_event_bus(&report, 100, &mut bus);
    }

    if engine.active_beams_count > 0 {
        let report = RadiationEventReport {
            pos_xyz: engine.beams[0].origin_xyz,
            dir_xyz: engine.beams[0].dir_xyz,
            flux_or_energy_mj: engine.beams[0].beam_energy_mj,
            optical_depth_or_temp: 65536,
            penetration_depth_mm: engine.beams[0].penetration_depth_mm,
            wavelength_or_cell_id: engine.beams[0].wavelength_nm,
            emitter_id: engine.beams[0].emitter_id,
            target_id: engine.beams[0].target_id,
            flags: engine.beams[0].flags,
            event_kind: RAD_EVENT_KIND_BEAM,
        };
        emit_radiation_to_event_bus(&report, 101, &mut bus);
    }

    let rad_hash = engine.compute_consensus_hash();
    let bus_hash = bus.compute_consensus_hash();
    let combined_hash = rad_hash ^ (bus_hash.rotate_left(31));

    if !out_stats.is_null() {
        unsafe {
            *out_stats.add(0) = 16;                                      // Capacity cells (16)
            *out_stats.add(1) = engine.active_cells_count as u32;        // Active cells count
            *out_stats.add(2) = engine.active_beams_count as u32;        // Active beams count
            *out_stats.add(3) = (combined_hash & 0xFFFF_FFFF) as u32;    // Low 32-bit consensus hash
            *out_stats.add(4) = (combined_hash >> 32) as u32;            // High 32-bit consensus hash
            *out_stats.add(5) = dispatched_jobs as u32;                  // Worker partition jobs
            *out_stats.add(6) = engine.beams[0].penetration_depth_mm as u32; // Penetration depth in mm
            *out_stats.add(7) = total_penetrations as u32;               // Total breakthrough penetrations
        }
    }

    total_spawned_beams as u32
}

#[no_mangle]
pub extern "C" fn axiom_plasma_bench_burst(
    cell_count: u32,
    field_count: u32,
    current_density_ma: u32,
    out_stats: *mut u32,
) -> u32 {
    use axiom_core::plasma::*;
    use axiom_core::event_bus::DeterministicEventBus;
    use axiom_core::job_system::JobSystem;
    use axiom_core::worker_pool::WorkerPoolCoordinator;

    let cells_to_spawn = (cell_count as usize).clamp(1, 16);
    let fields_to_spawn = (field_count as usize).clamp(1, 4);
    let current_ma = if current_density_ma > 0 { current_density_ma } else { 15_000 };

    let mut engine = Box::new(MechaPlasmaEngine::<16, 4>::new());
    let mut bus = Box::new(DeterministicEventBus::<256>::new());
    let mut js = Box::new(JobSystem::<256>::new());
    let mut coordinator = Box::new(WorkerPoolCoordinator::<8, 256>::new(4));

    for c in 0..cells_to_spawn {
        let cell = PlasmaCellSlab64::new(
            c as u8,
            [c as i32 * 400, 0, 0],
            [current_ma as i32, (c as i32 * 500), 0],
            10_000 + (c as u32 * 1_000),
        );
        engine.register_cell(cell);
    }

    let mut total_spawned_fields = 0;
    for f in 0..fields_to_spawn {
        let field = MagneticFieldSlab64::new(
            (f + 1) as u8,
            [0, 0, f as i32 * 200],
            [0, 0, 500 + (f as i32 * 200)],
            1500,
        );
        if engine.register_field(field).is_some() {
            total_spawned_fields += 1;
        }
    }

    // Parallel JobSystem dispatch
    let dispatched_jobs = dispatch_plasma_with_jobs(
        &engine,
        4,
        &mut js,
        &mut coordinator,
    );
    coordinator.drain_all(&mut js);

    // Step simulation for 10 ticks
    let mut total_discharges = 0;
    for _ in 0..10 {
        let (discharges, _fields) = engine.step_simulation(101, 1200);
        total_discharges += discharges;
    }

    // Emit representative plasma event
    if engine.active_cells_count > 0 {
        let report = PlasmaEventReport {
            pos_xyz: engine.cells[0].center_xyz,
            force_or_b_xyz: [100, 200, 0],
            pressure_or_voltage: engine.cells[0].magnetic_pressure_kpa,
            temp_or_alfven: engine.cells[0].electron_temp_k,
            current_or_flux: current_ma as u16,
            beta_or_cell_id: engine.cells[0].cell_id as u16,
            emitter_id: engine.cells[0].cell_id,
            target_id: 0,
            flags: engine.cells[0].flags,
            event_kind: PLASMA_EVENT_KIND_DISCHARGE,
        };
        emit_plasma_to_event_bus(&report, 100, &mut bus);
    }

    if engine.active_fields_count > 0 {
        let report = PlasmaEventReport {
            pos_xyz: engine.fields[0].origin_xyz,
            force_or_b_xyz: engine.fields[0].b_vector_xyz,
            pressure_or_voltage: engine.fields[0].induced_emf_mv.abs() as u32,
            temp_or_alfven: engine.fields[0].alfven_velocity_mms,
            current_or_flux: engine.fields[0].b_flux_total_mwb as u16,
            beta_or_cell_id: engine.fields[0].emitter_id as u16,
            emitter_id: engine.fields[0].emitter_id,
            target_id: engine.fields[0].target_id,
            flags: engine.fields[0].flags | MAG_FLAG_PULSED_EMP,
            event_kind: PLASMA_EVENT_KIND_EMP,
        };
        emit_plasma_to_event_bus(&report, 101, &mut bus);
    }

    let plasma_hash = engine.compute_consensus_hash();
    let bus_hash = bus.compute_consensus_hash();
    let combined_hash = plasma_hash ^ (bus_hash.rotate_left(31));

    if !out_stats.is_null() {
        unsafe {
            *out_stats.add(0) = 16;                                      // Capacity cells (16)
            *out_stats.add(1) = engine.active_cells_count as u32;        // Active cells count
            *out_stats.add(2) = engine.active_fields_count as u32;       // Active fields count
            *out_stats.add(3) = (combined_hash & 0xFFFF_FFFF) as u32;    // Low 32-bit consensus hash
            *out_stats.add(4) = (combined_hash >> 32) as u32;            // High 32-bit consensus hash
            *out_stats.add(5) = dispatched_jobs as u32;                  // Worker partition jobs
            *out_stats.add(6) = engine.cells[0].magnetic_pressure_kpa;   // Magnetic pressure in kPa
            *out_stats.add(7) = total_discharges as u32;                 // Total discharge steps
        }
    }

    total_spawned_fields as u32
}

#[no_mangle]
pub extern "C" fn axiom_gpu_bench_compute(
    width: u32,
    height: u32,
    backend_id: u32,
    out_stats: *mut u32,
) -> u32 {
    use axiom_render::gpu_backend::{GpuBackendKind, VoxelComputeDispatch64};

    let backend = match backend_id {
        1 => GpuBackendKind::Vulkan,
        2 => GpuBackendKind::DirectX12,
        3 => GpuBackendKind::Metal,
        4 => GpuBackendKind::WebGpu,
        _ => GpuBackendKind::SoftwareCpu,
    };

    let mut dispatch = VoxelComputeDispatch64 {
        backend,
        ..Default::default()
    };

    let us = dispatch.dispatch(width.clamp(64, 4096) as u16, height.clamp(64, 4096) as u16);
    let total_workgroups = (dispatch.dispatch_x as u32)
        * (dispatch.dispatch_y as u32)
        * (dispatch.dispatch_z as u32);

    if !out_stats.is_null() {
        unsafe {
            *out_stats.add(0) = dispatch.dispatch_x as u32;
            *out_stats.add(1) = dispatch.dispatch_y as u32;
            *out_stats.add(2) = dispatch.dispatch_z as u32;
            *out_stats.add(3) = us;
            *out_stats.add(4) = total_workgroups;
            *out_stats.add(5) = dispatch.total_dispatches as u32;
        }
    }

    us
}

#[no_mangle]
pub extern "C" fn axiom_gpu_get_raymarch_wgsl_len() -> u32 {
    VOXEL_RAYMARCH_WGSL.len() as u32
}

#[no_mangle]
pub extern "C" fn axiom_gpu_get_raymarch_wgsl(out_ptr: *mut u8, max_len: u32) -> u32 {
    if out_ptr.is_null() || max_len == 0 {
        return 0;
    }
    let bytes = VOXEL_RAYMARCH_WGSL.as_bytes();
    let to_copy = (bytes.len() as u32).min(max_len);
    unsafe {
        core::ptr::copy_nonoverlapping(bytes.as_ptr(), out_ptr, to_copy as usize);
    }
    to_copy
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wasm_replay_player() {
        let mut replay = DeterministicReplay::new(42, 0, 1);
        for frame in 0..120 {
            let p0 = if frame % 20 < 5 { 4 } else { 0 }; // Jab
            let p1 = if frame % 30 < 10 { 3 } else { 0 }; // Guard
            replay.record_frame(p0, p1, (frame as u64).wrapping_mul(99991));
        }
        replay.finalize(0, 10, 5);
        let serialized = replay.serialize();

        // Load into WASM replay player
        let total = axiom_replay_load(serialized.as_ptr(), serialized.len());
        assert_eq!(total, 120);
        assert_eq!(axiom_replay_total_frames(), 120);
        assert_eq!(axiom_replay_current_frame(), 0);
        assert_eq!(axiom_replay_match_seed(), 42);
        assert_eq!(axiom_replay_winner_id(), 0);
        assert_eq!(axiom_replay_is_finished(), 0);

        // Step 10 frames
        for _ in 0..10 {
            axiom_replay_step();
        }
        assert_eq!(axiom_replay_current_frame(), 10);
        assert!(axiom_replay_state_hash() > 0);

        // Seek to frame 60
        assert_eq!(axiom_replay_seek(60), 60);
        assert_eq!(axiom_replay_current_frame(), 60);

        // Reset
        axiom_replay_reset();
        assert_eq!(axiom_replay_current_frame(), 0);

        axiom_replay_free();
        assert_eq!(axiom_replay_total_frames(), 0);
    }

    #[test]
    fn test_audio_synthesis_and_worklet() {
        assert_eq!(axiom_audio_sample_rate(), 22050);
        // Synthesize jab: pitch 1000, reverb 0, center
        let frames = axiom_audio_synthesize(1, 1000, 0, 0, 1000);
        assert!(frames > 0);
        assert_eq!(axiom_audio_get_frame_count(), frames);
        let ptr = axiom_audio_get_interleaved_ptr();
        assert!(!ptr.is_null());

        // Test worklet streaming
        assert_eq!(axiom_audio_worklet_play(2, 1000, 300, -300, 800), 1);
        assert!(axiom_audio_worklet_active_voices() > 0);
        let block_ptr = axiom_audio_worklet_render_block();
        assert!(!block_ptr.is_null());
    }

    static ENGINE_3D_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    fn engine_3d_lock() -> std::sync::MutexGuard<'static, ()> {
        ENGINE_3D_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    #[test]
    fn test_wasm_3d_lifecycle_and_render() {
        let _g = engine_3d_lock();
        axiom_3d_init(320, 240);
        assert_eq!(axiom_3d_get_tick(), 0);
        assert!(axiom_3d_get_triangles() > 0, "procedural chassis must have triangles");
        assert!(axiom_3d_get_vertices() > 0, "procedural chassis must have vertices");
        assert_eq!(axiom_3d_get_framebuffer_len(), 320 * 240);
        assert_eq!(axiom_3d_get_yaw(), 32);

        // Advance 10 ticks in auto rotate
        for _ in 0..10 {
            axiom_3d_step(0, 0);
        }
        assert_eq!(axiom_3d_get_tick(), 10);
        assert_eq!(axiom_3d_get_yaw(), 42);

        // Manual drag step
        axiom_3d_step(16, 5);
        assert_eq!(axiom_3d_get_yaw(), 58);
        assert_eq!(axiom_3d_get_pitch(), 5);

        // Render frame
        axiom_3d_render();
        let ptr = axiom_3d_get_framebuffer_ptr();
        assert!(!ptr.is_null());

        let fb_slice = unsafe { core::slice::from_raw_parts(ptr, (320 * 240) as usize) };
        // Check that pixels have been rendered (not all clear color)
        let non_bg_count = fb_slice.iter().filter(|&&p| p != 0xFF0B0F19).count();
        assert!(non_bg_count > 100, "rendered mecha must paint visible pixels: count={}", non_bg_count);

        // Test options change: golden nova, rim light, inverted hull
        axiom_3d_set_options(1, 1, 1, 1);
        axiom_3d_step(0, 0);
        axiom_3d_render();

        // Reset to procedural
        axiom_3d_reset_procedural();
        assert!(axiom_3d_get_triangles() > 0);

        // Test GLB staging buffer allocation
        let stage_ptr = axiom_3d_alloc_glb_buffer(100);
        assert!(!stage_ptr.is_null());
        // Invalid GLB in buffer returns 0
        assert_eq!(axiom_3d_parse_staged_glb(), 0);
    }

    #[test]
    fn test_netbox_wasm_lifecycle() {
        axiom_netbox_init(0, 1001, 320, 240);
        assert_eq!(axiom_netbox_get_frame(), 0);
        assert_eq!(axiom_netbox_get_p0_hp(), 100);
        assert_eq!(axiom_netbox_get_p1_hp(), 120);

        // Advance 5 frames locally
        for _ in 0..5 {
            axiom_netbox_advance_frame(0);
        }
        assert_eq!(axiom_netbox_get_frame(), 5);

        // Receive delayed remote packet from frame 2 (P1 cross)
        let rolled = axiom_netbox_receive_remote(1, 2, 2);
        assert!(rolled, "delayed remote strike must trigger rollback");
        assert_eq!(axiom_netbox_get_total_rollbacks(), 1);

        axiom_netbox_render();
        assert!(!axiom_netbox_get_framebuffer_ptr().is_null());
        assert_eq!(axiom_netbox_get_framebuffer_len(), 320 * 240);

        // Test WebRTC wire packet generation and reception
        assert_eq!(axiom_netbox_get_wire_packet_size(), 18);
        let mut wire_buf = [0u8; 18];
        let written = axiom_netbox_create_wire_packet(wire_buf.as_mut_ptr(), 18, 1, 1);
        assert_eq!(written, 18);
        assert_eq!(wire_buf[0], 0x58); // Magic 'X'

        // Peer 1 receives Peer 0's wire packet
        axiom_netbox_init(1, 1001, 320, 240);
        let rolled_cnt = axiom_netbox_receive_wire_packet(wire_buf.as_ptr(), 18);
        assert_eq!(rolled_cnt, 0); // P1 at frame 0, received frame 5, confirmed without rollback
    }

    #[test]
    fn test_boxing_wasm_lifecycle_and_exports() {
        axiom_boxing_init(42, 640, 480);
        assert_eq!(axiom_boxing_get_tick(), 0);
        assert_eq!(axiom_boxing_get_player_hp(), 100);
        assert_eq!(axiom_boxing_get_opponent_hp(), 120);
        assert_eq!(axiom_boxing_get_stage(), 1);
        assert_eq!(axiom_boxing_get_phase(), 0);
        assert_eq!(axiom_boxing_get_framebuffer_len(), 640 * 480);

        // Step jab and render
        let _ev = axiom_boxing_step(1); // Jab
        axiom_boxing_render();
        assert_eq!(axiom_boxing_get_tick(), 1);
        let ptr = axiom_boxing_get_framebuffer_ptr();
        assert!(!ptr.is_null());
        assert_eq!(axiom_boxing_is_ko(), 0);

        // Test draft card export
        if let Some(engine) = boxing_engine() {
            engine.engine.fight_phase = FightPhase::Drafting;
        }
        assert_eq!(axiom_boxing_get_phase(), 1);
        assert_eq!(axiom_boxing_draft_card(0), 1);
        assert_eq!(axiom_boxing_get_stage(), 2);
        assert_eq!(axiom_boxing_get_phase(), 0);
    }

    #[test]
    fn test_wasm_lifecycle_and_exports() {
        axiom_wasm_init(12345, 640, 480);
        assert_eq!(axiom_wasm_get_tick(), 0);
        assert_eq!(axiom_wasm_get_blue_alive(), 50);
        assert_eq!(axiom_wasm_get_red_alive(), 50);
        assert_eq!(axiom_wasm_get_framebuffer_len(), 640 * 480);

        // Step 10 ticks and render
        for _ in 0..10 {
            axiom_wasm_step();
        }
        axiom_wasm_render();

        assert_eq!(axiom_wasm_get_tick(), 10);
        let ptr = axiom_wasm_get_framebuffer_ptr();
        assert!(!ptr.is_null());
    }

    #[test]
    fn test_openworld_wasm_bindings() {
        // Spawn 1,000 NPCs for quick test
        axiom_openworld_init(999, 640, 480, 1000);
        assert_eq!(axiom_openworld_get_tick(), 0);
        assert_eq!(axiom_openworld_get_npc_count(), 1000);

        for _ in 0..5 {
            axiom_openworld_step(1, 0); // Move East
        }
        axiom_openworld_render(16);

        assert_eq!(axiom_openworld_get_tick(), 5);
        let ptr = axiom_openworld_get_framebuffer_ptr();
        assert!(!ptr.is_null());
    }

    #[test]
    fn test_rpg_wasm_bindings() {
        axiom_rpg_init(0x1234_5678_9ABC_DEF0, 640, 480, 1000);
        assert_eq!(axiom_rpg_get_tick(), 0);
        assert_eq!(axiom_rpg_get_active_quests(), 0);
        assert_eq!(axiom_rpg_get_completed_quests(), 0);
        assert_eq!(axiom_rpg_get_xp(), 0);
        assert!(!axiom_rpg_is_in_combat());

        // Accept Quest
        assert!(axiom_rpg_interact_quest(1, 100, 999, 1, 500));
        assert_eq!(axiom_rpg_get_active_quests(), 1);

        // Plan Path to nearest walkable tile
        let mut gx = 5;
        let gy = 5;
        while !axiom_rpg_plan_path(gx, gy) && gx < 30 {
            gx += 1;
        }
        assert!(axiom_rpg_plan_path(gx, gy));

        // Trigger Combat Encounter
        axiom_rpg_trigger_encounter(999);
        assert!(axiom_rpg_is_in_combat());

        // Step 30 ticks
        for _ in 0..30 {
            axiom_rpg_step(0, 0);
        }

        assert_eq!(axiom_rpg_get_tick(), 30);
        assert!(!axiom_rpg_is_in_combat());
        assert_eq!(axiom_rpg_get_active_quests(), 0);
        assert_eq!(axiom_rpg_get_completed_quests(), 1);
        assert_eq!(axiom_rpg_get_xp(), 500);
        assert!(axiom_rpg_get_combat_damage() > 0);

        // Render scene
        axiom_rpg_render(20);
        let ptr = axiom_rpg_get_framebuffer_ptr();
        assert!(!ptr.is_null());
    }

    #[test]
    fn test_netcode_wasm_bindings() {
        axiom_net_init(0, 640, 480);
        assert_eq!(axiom_net_get_frame(), 0);
        assert_eq!(axiom_net_get_total_rollbacks(), 0);

        // Advance 10 frames locally
        for _ in 0..10 {
            axiom_net_advance_frame(1, 0, 0);
        }
        assert_eq!(axiom_net_get_frame(), 10);

        // Late remote input arriving for frame 2 (triggering rollback!)
        let rolled_back = axiom_net_receive_remote(1, 2, 0, 1, 0);
        assert!(rolled_back, "Remote input contradicting prediction must trigger speculative rollback");
        assert!(axiom_net_get_total_rollbacks() > 0);
        assert_eq!(axiom_net_get_frame(), 10); // Frame preserved at 10

        axiom_net_render();
        let ptr = axiom_net_get_framebuffer_ptr();
        assert!(!ptr.is_null());
    }

    #[test]
    fn test_coop_factory_rollback_wasm_bindings() {
        axiom_coop_init(0, 640, 480);
        assert_eq!(axiom_coop_get_tick(), 0);
        assert_eq!(axiom_coop_get_rollbacks(), 0);

        // Player 0 places a Mining Drill at (12, 10)
        let f1 = axiom_coop_place(12, 10, 2, 1, 0);
        assert_eq!(f1, 1);
        assert_eq!(axiom_coop_get_tick(), 1);

        // Step 5 frames forward locally
        for _ in 0..5 {
            axiom_coop_step(1, 0);
        }
        assert_eq!(axiom_coop_get_tick(), 6);

        // Late remote action arriving from Player 1 for frame 2 (Wall at (14, 14))
        let rolled_back = axiom_coop_receive_remote(1, 2, grid_action_kind::SET_TILE, 14, 14, 5, 0, 500);
        assert!(rolled_back, "Delayed remote building placement must trigger speculative rollback");
        assert!(axiom_coop_get_rollbacks() > 0);
        assert_eq!(axiom_coop_get_tick(), 6); // Frame preserved at 6

        // Render verification
        axiom_coop_render(14);
        let ptr = axiom_coop_get_framebuffer_ptr();
        assert!(!ptr.is_null());
        assert_eq!(axiom_coop_get_framebuffer_len(), 640 * 480);
        assert_ne!(axiom_coop_get_hash(), 0);
    }

    // ---------------------------------------------------------------------
    // T-044: factory / defence / workforce bindings
    // ---------------------------------------------------------------------

    /// The factory bindings share one static; serialize the tests that touch it.
    static FACTORY_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn factory_lock() -> std::sync::MutexGuard<'static, ()> {
        FACTORY_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn factory_stats() -> FactoryStats {
        let mut s = FactoryStats::default();
        let n = axiom_factory_get_stats(&mut s as *mut FactoryStats as *mut u32);
        assert_eq!(n, FACTORY_STATS_WORDS);
        s
    }

    fn factory_cell(x: usize, y: usize) -> StructureKind {
        factory_engine().expect("factory initialized").grid.cells[y][x].kind
    }

    fn factory_pixels() -> Vec<u32> {
        let len = axiom_factory_get_framebuffer_len() as usize;
        let ptr = axiom_factory_get_framebuffer_ptr();
        assert!(!ptr.is_null());
        unsafe { core::slice::from_raw_parts(ptr, len) }.to_vec()
    }

    /// Four turrets around the heart, each fed by an iron-plate chest, walls N/S.
    fn build_fortress() {
        for &(tx, ty, cx, cy) in &[(14, 14, 14, 13), (18, 14, 18, 13), (14, 18, 14, 19), (18, 18, 18, 19)] {
            assert!(axiom_factory_place(tx, ty, FACTORY_KIND_TURRET, 0, 0));
            assert!(axiom_factory_place(cx, cy, FACTORY_KIND_CHEST, 0, 10));
        }
        for x in 12..=20 {
            assert!(axiom_factory_place(x, 12, FACTORY_KIND_WALL, 0, 0));
            assert!(axiom_factory_place(x, 20, FACTORY_KIND_WALL, 0, 0));
        }
    }

    /// deposit+drill -> 3 belts -> inserter -> smelter -> inserter -> chest, along row `y`.
    fn build_production_line(y: u32) {
        assert!(axiom_factory_place(2, y, FACTORY_KIND_DEPOSIT, 0, 1));
        assert!(axiom_factory_place(2, y, FACTORY_KIND_DRILL, 1, 0));
        for x in 3..=5 {
            assert!(axiom_factory_place(x, y, FACTORY_KIND_BELT, 1, 0));
        }
        assert!(axiom_factory_place(6, y, FACTORY_KIND_INSERTER, 1, 0));
        assert!(axiom_factory_place(7, y, FACTORY_KIND_SMELTER, 0, 0));
        assert!(axiom_factory_place(8, y, FACTORY_KIND_INSERTER, 1, 0));
        assert!(axiom_factory_place(9, y, FACTORY_KIND_CHEST, 0, 0));
    }

    #[test]
    fn test_factory_init_exports_and_stats() {
        let _g = factory_lock();
        axiom_factory_init(7, 320, 240);
        assert_eq!(axiom_factory_get_tick(), 0);
        assert_eq!(axiom_factory_get_framebuffer_len(), 320 * 240);
        assert!(!axiom_factory_get_framebuffer_ptr().is_null());

        let s = factory_stats();
        assert_eq!(s.tick, 0);
        assert_eq!(s.heart_hp, axiom_game::BASE_HEART_HP as u32);
        assert_eq!(s.heart_max_hp, axiom_game::BASE_HEART_HP as u32);
        assert_eq!(s.base_fallen, 0);
        assert_eq!(s.wave_number, 0);
        assert_eq!(s.monsters_alive, 0);
        assert_eq!(s.workers_hired, FACTORY_COLONISTS);
        assert_eq!(s.workers_idle, FACTORY_COLONISTS);
        assert_eq!((s.cursor_x, s.cursor_y), (16, 16));
        assert!(matches!(factory_cell(16, 16), StructureKind::BaseHeart(_)));

        let engine = factory_engine().unwrap();
        let deposits = engine
            .grid
            .cells
            .iter()
            .flatten()
            .filter(|c| matches!(c.kind, StructureKind::Deposit(_)))
            .count();
        assert!(deposits > 0, "seeded ore veins must leave deposits on the grid");
        for y in 15..=17 {
            for x in 15..=17 {
                assert!(!matches!(engine.grid.cells[y][x].kind, StructureKind::Deposit(_)));
            }
        }
        assert_eq!(axiom_factory_get_stats(core::ptr::null_mut()), 0);
    }

    #[test]
    fn test_factory_place_codes_map_to_structures() {
        let _g = factory_lock();
        axiom_factory_init(7, 64, 64);

        assert!(axiom_factory_place(1, 1, FACTORY_KIND_BELT, 1, 0));
        assert!(matches!(factory_cell(1, 1), StructureKind::Belt(b) if b.direction == Direction::East));
        assert!(axiom_factory_place(1, 2, FACTORY_KIND_SMELTER, 0, 0));
        assert!(matches!(factory_cell(1, 2), StructureKind::Smelter(_)));
        assert!(axiom_factory_place(1, 3, FACTORY_KIND_CHEST, 0, 0));
        assert!(matches!(factory_cell(1, 3), StructureKind::Chest(c) if c.quantity == 0));
        assert!(axiom_factory_place(1, 4, FACTORY_KIND_CHEST, 0, 10));
        assert!(matches!(
            factory_cell(1, 4),
            StructureKind::Chest(c) if c.item == ItemType::IronPlate && c.quantity == FACTORY_CHEST_STOCK
        ));
        assert!(axiom_factory_place(1, 5, FACTORY_KIND_WALL, 0, 0));
        assert!(matches!(factory_cell(1, 5), StructureKind::Wall(w) if w.hp == axiom_game::WALL_HP));
        assert!(axiom_factory_place(1, 6, FACTORY_KIND_TURRET, 0, 0));
        assert!(matches!(factory_cell(1, 6), StructureKind::Turret(t) if t.ammo == TURRET_AMMO_CAP));
        assert!(axiom_factory_place(1, 7, FACTORY_KIND_INSERTER, 3, 0));
        assert!(matches!(factory_cell(1, 7), StructureKind::Inserter(i) if i.direction == Direction::West));
        assert!(axiom_factory_place(1, 8, FACTORY_KIND_BASE_HEART, 0, 0));
        assert!(matches!(factory_cell(1, 8), StructureKind::BaseHeart(_)));

        // Deposit then drill: the drill absorbs the reserve and gets a Miner colonist.
        assert!(axiom_factory_place(5, 5, FACTORY_KIND_DEPOSIT, 0, 2));
        assert!(matches!(
            factory_cell(5, 5),
            StructureKind::Deposit(d) if d.item == ItemType::CopperOre && d.remaining == FACTORY_DEPOSIT_AMOUNT
        ));
        assert!(axiom_factory_place(5, 5, FACTORY_KIND_DRILL, 2, 0));
        assert!(matches!(
            factory_cell(5, 5),
            StructureKind::Drill(d) if d.ore == ItemType::CopperOre
                && d.remaining == FACTORY_DEPOSIT_AMOUNT
                && d.direction == Direction::South
        ));
        {
            let wf = &factory_engine().unwrap().workforce;
            let slot = wf.find_post(ChunkCoord::new(0, 0), 5, 5).expect("drill auto-staffed");
            assert_eq!(wf.workers[slot].role, WorkerRole::Miner);
        }
        // Demolishing the drill frees the colonist and restores the ore patch.
        assert!(axiom_factory_place(5, 5, FACTORY_KIND_REMOVE, 0, 0));
        assert!(matches!(
            factory_cell(5, 5),
            StructureKind::Deposit(d) if d.item == ItemType::CopperOre && d.remaining == FACTORY_DEPOSIT_AMOUNT
        ));
        assert!(factory_engine().unwrap().workforce.find_post(ChunkCoord::new(0, 0), 5, 5).is_none());
        assert!(axiom_factory_place(1, 2, FACTORY_KIND_REMOVE, 0, 0));
        assert!(axiom_factory_place(1, 7, FACTORY_KIND_REMOVE, 0, 0));
        assert_eq!(factory_stats().workers_idle, FACTORY_COLONISTS);
        assert!(axiom_factory_place(1, 1, FACTORY_KIND_REMOVE, 0, 0));
        assert!(matches!(factory_cell(1, 1), StructureKind::Empty));

        // Rejections.
        assert!(!axiom_factory_place(FACTORY_W as u32, 0, FACTORY_KIND_BELT, 0, 0));
        assert!(!axiom_factory_place(0, FACTORY_H as u32, FACTORY_KIND_BELT, 0, 0));
        assert!(!axiom_factory_place(0, 0, 42, 0, 0));
        assert!(!axiom_factory_place(0, 0, FACTORY_KIND_DEPOSIT, 0, 0));
        assert!(!axiom_factory_place(16, 16, FACTORY_KIND_BELT, 0, 0));
        assert!(!axiom_factory_place(16, 16, FACTORY_KIND_REMOVE, 0, 0));
        assert!(matches!(factory_cell(16, 16), StructureKind::BaseHeart(_)));
    }

    #[test]
    fn test_factory_step_moves_cursor_with_clamping() {
        let _g = factory_lock();
        axiom_factory_init(7, 64, 64);
        axiom_factory_step(-100, -100);
        let s = factory_stats();
        assert_eq!((s.cursor_x, s.cursor_y), (0, 0));
        axiom_factory_step(5, 3);
        let s = factory_stats();
        assert_eq!((s.cursor_x, s.cursor_y), (5, 3));
        axiom_factory_step(i32::MAX, i32::MAX);
        let s = factory_stats();
        assert_eq!((s.cursor_x, s.cursor_y), (FACTORY_W as u32 - 1, FACTORY_H as u32 - 1));
        assert_eq!(axiom_factory_get_tick(), 3);
        assert_eq!(s.tick, 3);
    }

    #[test]
    fn test_factory_production_line_smelts_plates() {
        let _g = factory_lock();
        axiom_factory_init(7, 64, 64);
        build_production_line(4);
        let before = factory_stats();
        assert!(before.workers_working + before.workers_resting + before.workers_idle == FACTORY_COLONISTS);

        for _ in 0..300 {
            axiom_factory_step(0, 0);
        }
        let s = factory_stats();
        assert_eq!(s.tick, 300);
        assert!(s.total_mined > 0, "drill must mine ore: {s:?}");
        assert!(s.total_smelted > 0, "smelter must produce plates: {s:?}");
        assert!(s.total_stored > 0, "plates must reach the chest: {s:?}");
        assert!(matches!(
            factory_cell(9, 4),
            StructureKind::Chest(c) if c.item == ItemType::IronPlate && c.quantity as u32 == s.total_stored
        ));
        assert_eq!(s.workers_hired, FACTORY_COLONISTS);
        assert!(s.workers_working > 0 || s.workers_resting > 0, "staffed machines: {s:?}");
    }

    #[test]
    fn test_factory_defense_turrets_repel_waves() {
        let _g = factory_lock();
        axiom_factory_init(7, 64, 64);
        build_fortress();
        let s0 = factory_stats();
        assert_eq!(s0.turrets, 4);
        assert_eq!(s0.walls, 18);

        for _ in 0..120 {
            axiom_factory_step(0, 0);
        }
        let s = factory_stats();
        assert!(s.wave_number >= 2, "{s:?}");
        assert!(s.total_spawned >= 10, "{s:?}");
        assert!(s.total_shots > 0, "turrets must fire: {s:?}");
        assert!(s.total_killed > 0, "turrets must score kills: {s:?}");
        assert!(s.turret_ammo > 0, "chests must auto-supply turrets: {s:?}");
        assert_eq!(s.base_fallen, 0);
        assert_eq!(s.heart_hp, s.heart_max_hp, "fortified heart takes no damage: {s:?}");
        assert_eq!(s.turrets, 4);
    }

    #[test]
    fn test_factory_render_scene_and_clipping() {
        let _g = factory_lock();
        axiom_factory_init(7, 640, 480);
        build_fortress();
        build_production_line(4);

        // Heart core sits under the cursor at the screen centre.
        axiom_factory_render(12);
        let px = factory_pixels();
        assert_eq!(px[240 * 640 + 320], 0xFFFFE4E6);
        assert!(px.contains(&0xFFBE123C), "heart body");
        assert!(px.contains(&0xFF16A34A), "turret body");
        assert!(px.contains(&0xFF9CA3AF), "wall");
        assert!(px.contains(&0xFFB45309), "drill");
        assert!(px.contains(&0xFF334155), "belt");
        assert!(px.contains(&0xFF3F3F46), "smelter");
        assert!(px.contains(&0xFF92400E), "chest");

        // Run until a turret fires; that frame must show a tracer and a monster.
        let mut fired_frame = false;
        for _ in 0..200 {
            axiom_factory_step(0, 0);
            let s = factory_stats();
            if s.total_shots > 0 {
                axiom_factory_render(12);
                let px = factory_pixels();
                assert!(px.contains(&0xFFFDE047), "tracer / muzzle flash on firing tick");
                fired_frame = true;
                break;
            }
        }
        assert!(fired_frame, "no turret fired within 200 ticks");
        let s = factory_stats();
        assert!(s.monsters_alive > 0 || s.total_killed > 0);

        // Advance until the production line has ore moving and smelter active.
        let mut smelt_seen = false;
        for _ in 0..150 {
            axiom_factory_step(0, 0);
            axiom_factory_render(12);
            let px = factory_pixels();
            if px.contains(&0xFFF97316) || px.contains(&0xFFFACC15) {
                smelt_seen = true;
                break;
            }
        }
        assert!(smelt_seen, "smelter fire visible during production cycle");
        let px = factory_pixels();
        assert!(px.contains(&0xFF94A3B8) || px.contains(&0xFFCBD5E1), "iron ore or plates on belt/chest");

        // Degenerate viewports and tile sizes must clip, never panic.
        axiom_factory_init(7, 8, 8);
        build_fortress();
        axiom_factory_render(0);
        axiom_factory_render(64);
        axiom_factory_render(u32::MAX);
        axiom_factory_step(-16, -16);
        axiom_factory_render(1);
        axiom_factory_step(31, 31);
        axiom_factory_render(256);
        assert_eq!(axiom_factory_get_framebuffer_len(), 64);
    }

    #[test]
    fn test_factory_100_tick_run_is_bit_identical() {
        let _g = factory_lock();
        let run = || {
            axiom_factory_init(0xA11C_E5ED_F00D_4444, 320, 240);
            build_fortress();
            build_production_line(4);
            for i in 0..100 {
                axiom_factory_step((i % 3) as i32 - 1, 0);
                if i == 50 {
                    assert!(axiom_factory_place(20, 20, FACTORY_KIND_TURRET, 0, 0));
                }
            }
            axiom_factory_render(10);
            let stats = factory_stats();
            let hash = factory_engine().unwrap().grid.compute_hash()
                ^ factory_engine().unwrap().director.compute_hash().rotate_left(17)
                ^ factory_engine().unwrap().workforce.compute_hash().rotate_left(41);
            (stats, hash, factory_pixels())
        };
        let (s1, h1, p1) = run();
        let (s2, h2, p2) = run();
        assert_eq!(s1, s2);
        assert_eq!(h1, h2);
        assert_eq!(p1, p2);
        assert!(s1.total_shots > 0 && s1.total_smelted > 0, "{s1:?}");
    }

    #[test]
    fn test_texture_painting_and_3d_binding_wasm_exports() {
        let _g = engine_3d_lock();
        axiom_texture_init(64);
        assert_eq!(axiom_texture_get_size(), 64);
        assert!(!axiom_texture_get_ptr().is_null());

        axiom_texture_clear(0xFF1E293B);
        axiom_texture_paint(32, 32, 4, 0xFFFF0000, 255);
        axiom_texture_line(0, 0, 10, 0, 0xFF00FF00, 1, 255);
        axiom_texture_stamp_decal(0, 32, 32, 256); // Star

        axiom_3d_init(128, 128);
        assert_eq!(axiom_texture_apply_to_3d(), 1);
        axiom_3d_render();
        assert!(!axiom_3d_get_framebuffer_ptr().is_null());

        axiom_texture_remove_from_3d();
    }

    #[test]
    fn test_lobby_and_leaderboard_wasm_bindings() {
        // Leaderboard test
        assert!(axiom_leaderboard_get_count() >= 4);

        // Register two players
        let name_a = b"CyberViper";
        let reg_a = axiom_leaderboard_register(9991, 0, name_a.as_ptr(), name_a.len() as u32);
        assert_eq!(reg_a, 1);
        let name_b = b"IronFang";
        let reg_b = axiom_leaderboard_register(9992, 0, name_b.as_ptr(), name_b.len() as u32);
        assert_eq!(reg_b, 1);
        let count = axiom_leaderboard_get_count();
        assert!(count >= 6);

        // Record match: CyberViper (9991) beats IronFang (9992)
        let rec = axiom_leaderboard_record_match(9991, 0, 9992, 0, 1, 1);
        assert_eq!(rec, 1);

        let rank = axiom_leaderboard_get_rank(9991, 0);
        assert!(rank > 0);

        let mut stats = [0u32; 9];
        let n = axiom_leaderboard_get_entry(0, stats.as_mut_ptr());
        assert_eq!(n, 9);
        assert!(stats[2] > 0); // Rating > 0

        let mut name_buf = [0u8; 32];
        let name_len = axiom_leaderboard_get_name(0, name_buf.as_mut_ptr(), 32);
        assert!(name_len > 0);

        let mut tier_buf = [0u8; 32];
        let tier_len = axiom_leaderboard_get_tier(0, tier_buf.as_mut_ptr(), 32);
        assert!(tier_len > 0);

        // Lobby test
        let host_name = b"HostGamer";
        let code = b"AXM-TEST";
        let created = axiom_lobby_create(1001, 0, host_name.as_ptr(), host_name.len() as u32, code.as_ptr());
        assert_eq!(created, 1);
        assert_eq!(axiom_lobby_get_member_count(), 1);
        assert_eq!(axiom_lobby_get_state(), 0); // WaitingForPlayers
        assert_eq!(axiom_lobby_can_start(), 0);

        let mut room_code = [0u8; 8];
        axiom_lobby_get_room_code(room_code.as_mut_ptr());
        assert_eq!(&room_code, b"AXM-TEST");

        // Join challenger as Player2
        let ch_name = b"ChallengerGamer";
        let joined = axiom_lobby_join(1002, 0, ch_name.as_ptr(), ch_name.len() as u32, 1250, 1);
        assert_eq!(joined, 1);
        assert_eq!(axiom_lobby_get_member_count(), 2);
        assert_eq!(axiom_lobby_can_start(), 0);

        // Ready
        axiom_lobby_set_ready(1002, 0, 1);
        assert_eq!(axiom_lobby_can_start(), 1);
        assert_eq!(axiom_lobby_get_state(), 1); // ReadyToStart

        // Member info
        let mut m_stats = [0u32; 5];
        let info_len = axiom_lobby_get_member_info(1, m_stats.as_mut_ptr());
        assert_eq!(info_len, 5);
        assert_eq!(m_stats[0], 1002);
        assert_eq!(m_stats[2], 1250);
        assert_eq!(m_stats[3], 1); // Player2
        assert_eq!(m_stats[4], 1); // Ready
    }

    #[test]
    fn test_voxel_destruction_wasm_bindings() {
        let _g = voxel_engine_lock();

        // 1. Initialize Bunker
        axiom_voxel_init(128, 128, 2, 777);
        assert!(!axiom_voxel_get_framebuffer_ptr().is_null());
        assert_eq!(axiom_voxel_get_framebuffer_len(), 128 * 128);
        let solid_init = axiom_voxel_get_solid_count();
        assert!(solid_init > 0);
        let tris_init = axiom_voxel_get_triangles();
        assert!(tris_init > 0);

        // 2. Render initial frame
        let rendered_tris = axiom_voxel_render();
        assert_eq!(rendered_tris, tris_init);

        // 3. Step camera rotation
        axiom_voxel_step(16, -10);

        // 4. Carve sphere blast into bunker floor
        let carved = axiom_voxel_carve_sphere(8, 3, 8, 3, 0);
        assert!(carved > 0);
        assert!(axiom_voxel_get_solid_count() < solid_init);
        assert_eq!(axiom_voxel_get_damaged_count(), carved);
        let shards_after_sphere = axiom_voxel_get_shard_count();
        assert!(shards_after_sphere > 0, "Carving should spawn kinetic shards into the pool");

        // 4.1 Carve crater with ballistic shards & collapse detection
        let crater_carved = axiom_voxel_carve_crater(8, 8, 4, 5);
        assert!(crater_carved > 0);
        let shards_after_crater = axiom_voxel_get_shard_count();
        assert!(shards_after_crater >= shards_after_sphere);

        // 4.2 Spawn shard storm
        let spawned_storm = axiom_voxel_spawn_shard_storm(24);
        assert!(spawned_storm >= 24);
        assert!(axiom_voxel_get_shard_count() >= 24);

        // 4.3 Trigger structural collapse detection
        let _collapsed = axiom_voxel_trigger_collapse(0);
        let _chunks = axiom_voxel_get_collapsed_chunk_count();

        // 4.4 Step physics and verify simulation advance
        let step_flags = axiom_voxel_step(0, 0);
        let _ = step_flags;

        // 4.5 Render scene with Reversed-Z depth buffer containing mesh, chunks, shards
        let post_tris = axiom_voxel_render();
        assert!(post_tris > 0);

        // 4.6 Meteor Detonation and Physics Controls (T-138)
        axiom_voxel_set_gravity(240);
        axiom_voxel_set_drag(970);
        let meteor_carved = axiom_voxel_detonate_meteor(4, 12, 3, 32);
        assert!(meteor_carved > 0);
        assert!(axiom_voxel_get_shard_count() > 0);

        // 4.7 Dynamic Collapse 10-Iteration Benchmark
        let mut collapse_stats = [0u32; 6];
        let bench_ticks = axiom_voxel_bench_collapse(10, collapse_stats.as_mut_ptr());
        assert_eq!(bench_ticks, 10);
        assert!(collapse_stats[5] > 0, "Benchmark should render dynamic elements");

        // 4.8 Reset Physics
        axiom_voxel_reset_physics();
        assert_eq!(axiom_voxel_get_shard_count(), 0);
        assert_eq!(axiom_voxel_get_collapsed_chunk_count(), 0);

        // 5. Run benchmark
        let bench_result = axiom_voxel_benchmark(5);
        assert!(bench_result > 0);

        // 6. Export .axvm
        let mut export_buf = [0u8; 4160];
        let exported_len = axiom_voxel_export_axvm(export_buf.as_mut_ptr(), 4160);
        assert_eq!(exported_len, 4160);

        // 7. Load staged .axvm
        let alloc_ptr = axiom_voxel_alloc_axvm_buffer(4160);
        assert!(!alloc_ptr.is_null());
        unsafe {
            core::ptr::copy_nonoverlapping(export_buf.as_ptr(), alloc_ptr, 4160);
        }
        let load_ok = axiom_voxel_load_staged_axvm(4160);
        assert_eq!(load_ok, 1);
    }

    #[test]
    fn test_wasm_voxel_thermal_stress_and_collapse_bindings() {
        let _g = voxel_engine_lock();

        // 1. Init Bunker Scene
        axiom_voxel_init(128, 128, 2, 888);
        assert_eq!(axiom_voxel_get_max_temperature(), 20); // Room ambient
        assert_eq!(axiom_voxel_get_thermal_transitions(), 0);

        // 2. Inject 950°C high-energy thermal pulse into bunker center
        let max_t = axiom_voxel_apply_thermal_pulse(8, 4, 8, 3, 950, 850);
        assert!(max_t >= 400, "Diffused temperature must remain significantly elevated");
        assert!(axiom_voxel_get_max_temperature() >= 400);

        // 3. Step thermal diffusion with cooling
        let packed_step = axiom_voxel_step_thermal(40);
        let transitions = packed_step & 0xFF;
        let fractures = (packed_step >> 8) & 0xFF;
        assert!(transitions > 0 || fractures > 0 || axiom_voxel_get_max_temperature() > 20);

        // 4. Render with thermal glow
        let rendered_tris = axiom_voxel_render();
        assert!(rendered_tris > 0);

        // 5. Reset thermal
        axiom_voxel_reset_thermal();
        assert_eq!(axiom_voxel_get_max_temperature(), 20);

        // 6. Extreme Physics Benchmark
        let mut extreme_stats = [0u32; 7];
        let bench_ticks = axiom_voxel_bench_extreme(50, extreme_stats.as_mut_ptr());
        assert_eq!(bench_ticks, 50);
        assert_eq!(extreme_stats[0], 50);
        assert!(extreme_stats[5] != 0 || extreme_stats[6] != 0, "Consensus hash must be non-zero");
    }

    #[test]
    fn test_wasm_voxel_fluid_dynamics_bindings() {
        let _g = voxel_engine_lock();

        // 1. Init Crater Scene
        axiom_voxel_init(128, 128, 1, 999);
        assert_eq!(axiom_voxel_get_fluid_units(), 0);
        assert_eq!(axiom_voxel_get_infiltrated_units(), 0);
        assert_eq!(axiom_voxel_get_active_fluid_cells(), 0);

        // 2. Inject fluid into crater
        let total_units = axiom_voxel_inject_fluid(8, 8, 8, 200);
        assert_eq!(total_units, 200);
        assert_eq!(axiom_voxel_get_fluid_units(), 200);
        assert_eq!(axiom_voxel_get_active_fluid_cells(), 1);

        // 3. Step fluid simulation
        let step_res = axiom_voxel_step_fluid(10);
        let _moved = step_res & 0xFFFF;
        let _evaporated = (step_res >> 16) & 0xFFFF;
        assert!(axiom_voxel_get_fluid_units() > 0);

        // 4. Render with fluid rasterizer
        let rendered_tris = axiom_voxel_render();
        assert!(rendered_tris > 0);

        // 5. Reset fluid
        axiom_voxel_reset_fluid();
        assert_eq!(axiom_voxel_get_fluid_units(), 0);
        assert_eq!(axiom_voxel_get_active_fluid_cells(), 0);

        // 6. SIMD / Checkerboard Fluid Benchmark
        let mut simd_stats = [0u32; 7];
        let bench_ticks = axiom_voxel_bench_simd_fluid(50, simd_stats.as_mut_ptr());
        assert_eq!(bench_ticks, 50);
        assert_eq!(simd_stats[0], 50);
        assert!(simd_stats[5] != 0 || simd_stats[6] != 0, "SIMD fluid hash must be non-zero");
    }

    #[test]
    fn test_wasm_net_and_gpu_compute_bindings() {
        // 1. Rollback Netcode Benchmark (20 frames, 15% loss, 4-frame latency)
        let mut net_stats = [0u32; 8];
        let ticks = axiom_net_bench_rollback(20, 150, 4, net_stats.as_mut_ptr());
        assert_eq!(ticks, 20);
        assert_eq!(net_stats[0], 20); // iters
        assert_eq!(net_stats[3], 0);  // desyncs == 0
        assert_eq!(net_stats[4], 16); // 16/16 peers agreed!
        assert!(net_stats[5] != 0 || net_stats[6] != 0, "Consensus hash must be non-zero");

        // 2. GPU Compute Dispatch (WebGPU backend)
        let mut gpu_stats = [0u32; 6];
        let us = axiom_gpu_bench_compute(704, 544, 4, gpu_stats.as_mut_ptr());
        assert!(us > 0);
        assert_eq!(gpu_stats[0], 88); // 704 / 8
        assert_eq!(gpu_stats[1], 68); // 544 / 8
        assert_eq!(gpu_stats[2], 1);
        assert_eq!(gpu_stats[3], 320); // 320 us WebGPU compute
        assert_eq!(gpu_stats[4], 88 * 68);

        // 3. WebGPU WGSL Source Retrieval
        let wgsl_len = axiom_gpu_get_raymarch_wgsl_len();
        assert!(wgsl_len > 0);
        let mut wgsl_buf = vec![0u8; wgsl_len as usize];
        let copied = axiom_gpu_get_raymarch_wgsl(wgsl_buf.as_mut_ptr(), wgsl_len);
        assert_eq!(copied, wgsl_len);
        let wgsl_str = core::str::from_utf8(&wgsl_buf).unwrap();
        assert!(wgsl_str.contains("@compute @workgroup_size(8, 8, 1)"));

        // 4. Software Voxel Raymarch Rendering
        let _g = voxel_engine_lock();
        axiom_voxel_init(64, 64, 0, 12345);
        let fragments = axiom_voxel_render_raymarch(1, 1);
        assert!(fragments > 0, "Raymarcher must produce fragments for solid preset");

        // 5. Voxel Raymarch Benchmark (20 frames, 2x tile scale)
        let mut bench_stats = [0u32; 7];
        let bench_frames = axiom_voxel_bench_raymarch(20, 2, 1, bench_stats.as_mut_ptr());
        assert_eq!(bench_frames, 20);
        assert_eq!(bench_stats[0], 20);
        assert!(bench_stats[1] > 0, "Total fragments > 0");
        assert!(bench_stats[5] != 0 || bench_stats[6] != 0, "Consensus hash must be non-zero");

        // 6. NAT Traversal Benchmark (16 peers, 15% loss)
        let mut nat_stats = [0u32; 8];
        let nat_rounds = axiom_net_bench_nat_traversal(16, 150, nat_stats.as_mut_ptr());
        assert!(nat_rounds > 0);
        assert_eq!(nat_stats[0], 16); // 16 peers
        assert_eq!(nat_stats[4], 16); // All 16 peers connected!
        assert!(nat_stats[2] > 0, "Probes sent > 0");
        assert!(nat_stats[3] > 0, "Probes recv > 0");
        assert!(nat_stats[5] != 0 || nat_stats[6] != 0, "NAT consensus hash must be non-zero");

        // 7. Async Job System & Work-Stealing Benchmark (50 jobs, 4 workers)
        let mut job_stats = [0u32; 8];
        let total_exec = axiom_job_bench_scheduler(50, 4, job_stats.as_mut_ptr());
        assert_eq!(total_exec, 51); // 50 children + 1 root parent
        assert_eq!(job_stats[0], 51);
        assert_eq!(job_stats[1], 4);  // 4 workers
        assert!(job_stats[2] > 0, "Work-stealing events must be > 0");
        assert!(job_stats[4] != 0 || job_stats[5] != 0, "Job consensus hash must be non-zero");

        // 8. Deterministic Event Bus & Component Query Benchmark (100 events, 4 workers)
        let mut event_stats = [0u32; 8];
        let total_emitted = axiom_event_bench_bus(100, 4, event_stats.as_mut_ptr());
        assert_eq!(total_emitted, 100);
        assert_eq!(event_stats[0], 100);
        assert_eq!(event_stats[1], 70); // 70 confirmed remaining after rollback of 30
        assert!(event_stats[2] > 0, "Combat matches > 0");
        assert!(event_stats[3] != 0 || event_stats[4] != 0, "Event consensus hash non-zero");
        assert_eq!(event_stats[5], 4, "4 parallel dispatch jobs");
        assert_eq!(event_stats[6], 30, "30 events rewound");

        // 9. Spatial Hashing & Broadphase Collision Acceleration Benchmark (64 entities, 10 iters)
        let mut spatial_stats = [0u32; 8];
        let total_pairs = axiom_spatial_bench_broadphase(64, 10, 1000, spatial_stats.as_mut_ptr());
        assert!(total_pairs > 0, "Expected collision pairs in clustered layout");
        assert_eq!(spatial_stats[0], 64);
        assert_eq!(spatial_stats[1], total_pairs);
        assert!(spatial_stats[3] != 0 || spatial_stats[4] != 0, "Spatial consensus hash non-zero");
        assert_eq!(spatial_stats[5], 4, "4 parallel dispatch broadphase jobs");
        assert_eq!(spatial_stats[6], 1000, "1000mm cell size");
        assert_eq!(spatial_stats[7], 1024, "1024 table capacity");

        // 10. Continuous Collision Detection (CCD) Hitbox Benchmark (16 hitboxes, 5 iters, 100% velocity)
        let mut ccd_stats = [0u32; 8];
        let ccd_hits = axiom_ccd_bench_hitbox(16, 5, 100, ccd_stats.as_mut_ptr());
        assert!(ccd_hits > 0, "CCD benchmark must detect continuous collision hits");
        assert_eq!(ccd_stats[0], 16);
        assert_eq!(ccd_stats[1], ccd_hits);
        assert!(ccd_stats[2] > 0, "Earliest TOI must be > 0");
        assert!(ccd_stats[3] != 0 || ccd_stats[4] != 0, "CCD consensus hash must be non-zero");
        assert_eq!(ccd_stats[5], 4, "4 parallel dispatch CCD jobs");
        assert_eq!(ccd_stats[6], 500, "500 mm/tick displacement");
        assert!(ccd_stats[7] > 0, "Event bus must contain emitted CCD hit events");

        // 11. Modular Mecha Ragdoll & Two-Bone IK Benchmark (8 bones, 5 iters, sever joint 3)
        let mut ragdoll_stats = [0u32; 8];
        let bones_simulated = axiom_ragdoll_bench_pbd(8, 5, 3, ragdoll_stats.as_mut_ptr());
        assert_eq!(bones_simulated, 8);
        assert_eq!(ragdoll_stats[0], 8, "8 active bones");
        assert_eq!(ragdoll_stats[1], 7, "7 joints");
        assert!(ragdoll_stats[2] > 0, "Constraints relaxed must be > 0");
        assert!(ragdoll_stats[3] != 0 || ragdoll_stats[4] != 0, "Consensus hash non-zero");
        assert_eq!(ragdoll_stats[5], 4, "4 parallel dispatch jobs");
        assert_eq!(ragdoll_stats[6], 1, "1 limb severed");
        assert!(ragdoll_stats[7] > 0, "Event bus must contain emitted ragdoll impacts");

        // 12. Deformable Terrain Mesh & Crater Tessellation Benchmark (33x33 grid, 3 craters, 60kJ energy)
        let mut terrain_stats = [0u32; 8];
        let displaced_vertices = axiom_terrain_bench_crater(33, 3, 60_000, terrain_stats.as_mut_ptr());
        assert!(displaced_vertices > 0, "Deformable terrain benchmark must displace vertices");
        assert_eq!(terrain_stats[0], 1089, "1089 grid vertices (33x33)");
        assert_eq!(terrain_stats[1], 3, "3 craters applied");
        assert_eq!(terrain_stats[2], displaced_vertices);
        assert!(terrain_stats[3] != 0 || terrain_stats[4] != 0, "Mesh consensus hash non-zero");
        assert_eq!(terrain_stats[5], 4, "4 parallel dispatch jobs");
        assert!(terrain_stats[6] > 0, "Excavation bowl depth > 0");
        assert_eq!(terrain_stats[7], 3, "Event bus must contain 3 crater events");

        // 13. Procedural Destruction Audio & Surface Reverb Benchmark (4 voices, Metal material, 75kJ energy)
        let mut audio_stats = [0u32; 8];
        let peak_amplitude = axiom_audio_bench_destruction(4, 2, 75_000, audio_stats.as_mut_ptr());
        assert!(peak_amplitude > 0, "Peak PCM amplitude must be non-zero");
        assert_eq!(audio_stats[0], 16, "16 voice slabs capacity");
        assert_eq!(audio_stats[1], 4, "4 voices spawned");
        assert_eq!(audio_stats[2], 4, "4 voices actively synthesized");
        assert!(audio_stats[3] != 0 || audio_stats[4] != 0, "Audio consensus hash non-zero");
        assert_eq!(audio_stats[5], 4, "4 parallel dispatch jobs");
        assert_eq!(audio_stats[6], peak_amplitude);
        assert_eq!(audio_stats[7], 4, "Event bus must contain 4 audio destruction events");

        // 14. Physics-Based Debris Particle Scattering & Collision Impulse Benchmark (32 particles, Stone material, 60kJ energy)
        let mut debris_stats = [0u32; 8];
        let spawned_particles = axiom_debris_bench_burst(32, 1, 60_000, debris_stats.as_mut_ptr());
        assert_eq!(spawned_particles, 32, "Debris benchmark must spawn 32 particles");
        assert_eq!(debris_stats[0], 128, "128 particle slabs capacity");
        assert_eq!(debris_stats[1], 32, "32 particles spawned");
        assert!(debris_stats[2] > 0, "Active particles remaining after 10 ticks");
        assert!(debris_stats[3] != 0 || debris_stats[4] != 0, "Debris consensus hash non-zero");
        assert_eq!(debris_stats[5], 4, "4 parallel dispatch jobs");
        assert_eq!(debris_stats[7], debris_stats[6], "Bounces match contacts on floor/boundary");

        // 15. Volumetric Shockwave & Procedural Dust Storm Benchmark (8 shockwaves, 16 dust clouds, 120kJ energy)
        let mut shock_stats = [0u32; 8];
        let spawned_waves = axiom_shockwave_bench_burst(8, 16, 120_000, shock_stats.as_mut_ptr());
        assert_eq!(spawned_waves, 8, "Shockwave benchmark must spawn 8 waves");
        assert_eq!(shock_stats[0], 32, "32 shockwave slabs capacity");
        assert_eq!(shock_stats[1], 8, "8 shockwaves active after 10 ticks");
        assert_eq!(shock_stats[2], 16, "16 dust clouds active after 10 ticks");
        assert!(shock_stats[3] != 0 || shock_stats[4] != 0, "Shockwave consensus hash non-zero");
        assert_eq!(shock_stats[5], 4, "4 parallel dispatch jobs");
        assert!(shock_stats[6] <= 65536, "Fog transmittance must be valid Q16.16 (0..=65536)");
        assert_eq!(shock_stats[7], 16, "16 dust emitters spawned");

        // 16. XPBD Mecha Ragdoll & Elastic Strain Limits Benchmark (8 bones, 4 substeps, 90kJ energy)
        let mut xpbd_stats = [0u32; 8];
        let spawned_bones = axiom_xpbd_bench_burst(8, 4, 90_000, xpbd_stats.as_mut_ptr());
        assert_eq!(spawned_bones, 8, "XPBD benchmark must spawn 8 bones");
        assert_eq!(xpbd_stats[0], 16, "16 bone slabs capacity");
        assert_eq!(xpbd_stats[1], 8, "8 bones active after 10 ticks");
        assert_eq!(xpbd_stats[2], 7, "7 constraints active");
        assert!(xpbd_stats[3] != 0 || xpbd_stats[4] != 0, "XPBD consensus hash non-zero");
        assert_eq!(xpbd_stats[5], 4, "4 parallel dispatch jobs");

        // 17. 3D Shape Matching Soft Body & Reversible Volume Conservation Benchmark (4 clusters, 4 particles/cluster, 60kN force)
        let mut sb_stats = [0u32; 8];
        let spawned_pts = axiom_soft_body_bench_burst(4, 4, 60_000, sb_stats.as_mut_ptr());
        assert_eq!(spawned_pts, 16, "Soft body benchmark must spawn 16 particles (4 clusters * 4 particles)");
        assert_eq!(sb_stats[0], 8, "8 cluster slabs capacity");
        assert_eq!(sb_stats[1], 4, "4 clusters active");
        assert_eq!(sb_stats[2], 16, "16 particles active");
        assert!(sb_stats[3] != 0 || sb_stats[4] != 0, "Soft body consensus hash non-zero");
        assert_eq!(sb_stats[5], 4, "4 parallel dispatch jobs");

        // 18. Procedural Mecha Actuator & Artificial Muscle Benchmark (8 actuators, 2 chambers, 15MPa)
        let mut act_stats = [0u32; 8];
        let spawned_acts = axiom_actuator_bench_burst(8, 2, 15_000, act_stats.as_mut_ptr());
        assert_eq!(spawned_acts, 8, "Actuator benchmark must spawn 8 actuators");
        assert_eq!(act_stats[0], 16, "16 actuator slabs capacity");
        assert_eq!(act_stats[1], 8, "8 actuators active");
        assert_eq!(act_stats[2], 2, "2 chambers active");
        assert!(act_stats[3] != 0 || act_stats[4] != 0, "Actuator consensus hash non-zero");
        assert_eq!(act_stats[5], 4, "4 parallel dispatch jobs");

        // 19. Procedural Aerodynamics & Vortex-Induced Vibration Benchmark (8 bodies, 2 cells, 25 m/s)
        let mut aero_stats = [0u32; 8];
        let spawned_bodies = axiom_aero_bench_burst(8, 2, 25_000, aero_stats.as_mut_ptr());
        assert_eq!(spawned_bodies, 8, "Aero benchmark must spawn 8 bodies");
        assert_eq!(aero_stats[0], 16, "16 aero body slabs capacity");
        assert_eq!(aero_stats[1], 8, "8 aero bodies active");
        assert_eq!(aero_stats[2], 2, "2 airflow grid cells active");
        assert!(aero_stats[3] != 0 || aero_stats[4] != 0, "Aero consensus hash non-zero");
        assert_eq!(aero_stats[5], 4, "4 parallel dispatch jobs");

        // 20. Atmospheric Turbulence & Shock Boundary Layer Separation (SBLI) Benchmark (8 cells, 2 shocks, 1.5 Mach)
        let mut turb_stats = [0u32; 8];
        let spawned_shocks = axiom_turbulence_bench_burst(8, 2, 510_000, turb_stats.as_mut_ptr());
        assert_eq!(spawned_shocks, 2, "Turbulence benchmark must spawn 2 shocks");
        assert_eq!(turb_stats[0], 16, "16 turbulence cell slabs capacity");
        assert_eq!(turb_stats[1], 8, "8 turbulence cells active");
        assert_eq!(turb_stats[2], 2, "2 shock boundaries active");
        assert!(turb_stats[3] != 0 || turb_stats[4] != 0, "Turbulence consensus hash non-zero");
        assert_eq!(turb_stats[5], 4, "4 parallel dispatch jobs");
        assert!(turb_stats[6] >= 78643, "P2/P1 ratio must exceed SBLI detachment threshold (1.20 in Q16)");

        // 21. Radiative Transfer & Relativistic Particle Beam Benchmark (8 cells, 2 beams, 1 kJ pulse)
        let mut rad_stats = [0u32; 8];
        let spawned_beams = axiom_radiation_bench_burst(8, 2, 1_000_000, rad_stats.as_mut_ptr());
        assert_eq!(spawned_beams, 2, "Radiation benchmark must spawn 2 beams");
        assert_eq!(rad_stats[0], 16, "16 radiation cell slabs capacity");
        assert_eq!(rad_stats[1], 8, "8 radiation cells active");
        assert_eq!(rad_stats[2], 2, "2 particle beams active");
        assert!(rad_stats[3] != 0 || rad_stats[4] != 0, "Radiation consensus hash non-zero");
        assert_eq!(rad_stats[5], 4, "4 parallel dispatch jobs");
        assert!(rad_stats[6] > 0, "Penetration depth must be positive");

        // 22. Plasma MHD & Electromagnetic Induction Benchmark (8 cells, 2 fields, 15000 mA/mm2)
        let mut plasma_stats = [0u32; 8];
        let spawned_fields = axiom_plasma_bench_burst(8, 2, 15_000, plasma_stats.as_mut_ptr());
        assert_eq!(spawned_fields, 2, "Plasma benchmark must spawn 2 fields");
        assert_eq!(plasma_stats[0], 16, "16 plasma cell slabs capacity");
        assert_eq!(plasma_stats[1], 8, "8 plasma cells active");
        assert_eq!(plasma_stats[2], 2, "2 magnetic fields active");
        assert!(plasma_stats[3] != 0 || plasma_stats[4] != 0, "Plasma consensus hash non-zero");
        assert_eq!(plasma_stats[5], 4, "4 parallel dispatch jobs");
        assert!(plasma_stats[6] > 0, "Magnetic pressure must be positive");
    }
}
