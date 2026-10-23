//! Headless Dual 3D Mecha Ring Combat Capture Tool for Axiom Engine.
//!
//! Validates simultaneous forward kinematics 3D rendering of BOTH Player (ELPIS)
//! and Opponent (DUMMY_v2) inside the combat ring sharing a single Reversed-Z depth buffer.

use axiom_game::boxing::ActionKind;
use axiom_render::FrameBuffer;
use std::fs::{self, File};
use std::io::{BufWriter, Write};

fn save_ppm(fb: &FrameBuffer, path: &str) -> std::io::Result<()> {
    let file = File::create(path)?;
    let mut writer = BufWriter::new(file);

    writeln!(writer, "P6")?;
    writeln!(writer, "{} {}", fb.width, fb.height)?;
    writeln!(writer, "255")?;

    for &pixel in &fb.pixels {
        let r = ((pixel >> 16) & 0xFF) as u8;
        let g = ((pixel >> 8) & 0xFF) as u8;
        let b = (pixel & 0xFF) as u8;
        writer.write_all(&[r, g, b])?;
    }
    writer.flush()?;
    Ok(())
}

fn main() {
    println!("[Axiom 3D Dual] Initializing Dual 3D Mecha Combat View...");
    let mut boxing = axiom_cli::boxing_view::BoxingView::new(42);

    let width = 704;
    let height = 544;
    let mut fb = FrameBuffer::new(width, height);
    fs::create_dir_all("tmp").ok();

    // 1. Idle frame: Both mechas facing each other
    println!("[Axiom 3D Dual] Rendering Frame 1: Stance Neutral Idle...");
    boxing.render(&mut fb);
    let ppm1 = "tmp/dual_combat_idle_3d.ppm";
    save_ppm(&fb, ppm1).expect("Failed to write PPM");
    println!("  -> Saved {}", ppm1);

    // 2. Combat Action frame: Player throws Jab punch forward, opponent hit
    println!("[Axiom 3D Dual] Rendering Frame 2: Player Jab Punch Strike...");
    boxing.engine.player.action.kind = ActionKind::Jab;
    boxing.engine.opponent.action.kind = ActionKind::HitStagger;
    boxing.render(&mut fb);
    let ppm2 = "tmp/dual_combat_punch_3d.ppm";
    save_ppm(&fb, ppm2).expect("Failed to write PPM");
    println!("  -> Saved {}", ppm2);

    println!("[Axiom 3D Dual] Dual 3D Mecha Capture Sequence Complete!");
}
