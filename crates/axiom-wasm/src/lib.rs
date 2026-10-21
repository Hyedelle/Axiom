//! # Axiom WASM Web Viewer Bridge (`axiom-wasm`)
//!
//! Exposes zero-copy C-ABI interfaces for WebAssembly execution in the browser.
//! Allows running 100% deterministic simulations and rendering in 60fps canvas.

extern crate alloc;

use alloc::boxed::Box;
use axiom_core::chunk::ChunkManager;
use axiom_core::combat::BattleArena;
use axiom_core::fixed::{Fixed, FixedVec2};
use axiom_core::npc_crowd::NpcCrowdPool;
use axiom_core::pcg::TileMap;
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
}
