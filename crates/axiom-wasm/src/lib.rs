//! # Axiom WASM Web Viewer Bridge (`axiom-wasm`)
//!
//! Exposes zero-copy C-ABI interfaces for WebAssembly execution in the browser.
//! Allows running 100% deterministic simulations and rendering in 60fps canvas.

use axiom_core::combat::BattleArena;
use axiom_core::fixed::FixedVec2;
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

/// Advances simulation by 1 discrete tick.
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
}
