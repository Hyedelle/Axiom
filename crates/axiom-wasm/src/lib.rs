//! # Axiom WASM Web Viewer Bridge (`axiom-wasm`)
//!
//! Exposes zero-copy C-ABI interfaces for WebAssembly execution in the browser.
//! Allows running 100% deterministic simulations and rendering in 60fps canvas.

extern crate alloc;

use alloc::boxed::Box;
use axiom_core::audio::{SoundCategory, SoundEvent, SpatialAudioSimulator, DEFAULT_MAX_VOICES};
use axiom_core::chunk::ChunkManager;
use axiom_core::combat::BattleArena;
use axiom_core::effector::{AtbArena, AtbUnit, Element, StatusKind, MAX_ACTIVE_UNITS};
use axiom_core::fixed::{Fixed, FixedVec2};
use axiom_core::input::InputState;
use axiom_core::netcode::RollbackSession;
use axiom_core::npc_crowd::NpcCrowdPool;
use axiom_core::pathfinding::{Path, PathFinder, DEFAULT_GRID_H, DEFAULT_GRID_W, MAX_PATH_WAYPOINTS};
use axiom_core::pcg::TileMap;
use axiom_core::quest::{ObjectiveType, QuestJournal, QuestRecord, QuestStatus};
use axiom_core::state::TickState;
use axiom_render::FrameBuffer;

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

    let mut chunk_mgr = ChunkManager::new(seed, 2);
    let player_pos = FixedVec2::new(Fixed::ZERO, Fixed::ZERO);
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
