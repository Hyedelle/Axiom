//! Headless In-Game 3D Boxing Combat Capture Tool for Axiom Engine.
//!
//! Validates the live integration of Juan Pineda 8x8 Tile Binning 3D Mecha Rasterizer
//! inside the actual `BoxingView` combat ring with ropes, HUD, and opponent.

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
    println!("[Axiom 3D] Initializing BoxingView with 3D Mecha Pipeline...");
    let boxing = axiom_cli::boxing_view::BoxingView::new(42);

    let width = 704;
    let height = 544;
    let mut fb = FrameBuffer::new(width, height);

    println!("[Axiom 3D] Rendering full combat arena with 3D player mecha...");
    boxing.render(&mut fb);

    fs::create_dir_all("tmp").ok();
    let ppm_path = "tmp/boxing_combat_3d.ppm";
    save_ppm(&fb, ppm_path).expect("Failed to write PPM");
    println!("[Axiom 3D] Combat frame saved -> {}", ppm_path);
}
