//! Headless Turntable 3D Capture Tool for Axiom Engine.
//!
//! Renders `hero_a.glb` mecha across multiple turntable angles (0°, 45°, 90°, 180°)
//! using Axiom's Software 3D Cel-shading Rasterizer and outputs PPM frames for validation.

use axiom_core::Fixed;
use axiom_render::math3d::{Mat4Fixed, Vec3Fixed};
use axiom_render::mesh3d::parse_glb;
use axiom_render::raster3d::Rasterizer3D;
use axiom_render::FrameBuffer;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::Path;

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
    let glb_path = Path::new(r"C:\Dreamhub\Game\steel-the-show\assets\robot3d\hero\hero_a.glb");
    if !glb_path.exists() {
        eprintln!("Error: hero_a.glb not found at {:?}", glb_path);
        std::process::exit(1);
    }

    println!("[Axiom 3D] Ingesting hero_a.glb...");
    let bytes = fs::read(glb_path).expect("Failed to read GLB file");
    let model = parse_glb(&bytes).expect("Failed to parse GLB model");

    println!(
        "[Axiom 3D] Model loaded successfully: {} parts, {} vertices, {} triangles",
        model.parts.len(),
        model.total_vertices(),
        model.total_triangles()
    );

    let width = 640;
    let height = 480;
    let mut fb = FrameBuffer::new(width, height);
    let mut rasterizer = Rasterizer3D::new(width, height);
    rasterizer.cull_backfaces = false; // Double-sided for thin mecha panels

    // Turntable angles (BAM: 0=0°, 32=45°, 64=90°, 128=180°)
    let angles = [
        ("front", 0u8, "0° Front View"),
        ("three_quarter", 32u8, "45° Dynamic Perspective"),
        ("profile", 64u8, "90° Side Profile"),
        ("back", 128u8, "180° Rear Exhaust View"),
    ];

    fs::create_dir_all("tmp").ok();

    // Model vertical centering translation: shift down by 0.19m so mecha center is at origin
    let center_offset = Vec3Fixed::from_f32(0.0, -0.19, 0.0);
    let trans_mat = Mat4Fixed::from_translation(center_offset);

    let cam_dist = Fixed::from_raw((2.15 * 65536.0) as i64); // 2.15m camera distance
    let focal_length = 520; // 520px focal length

    for (name, bam, desc) in &angles {
        // Background: high-tech dark steel gradient
        fb.clear(0xFF0F172A);
        rasterizer.clear_depth();

        // Rotate around Y-axis, then apply centering translation
        let rot_mat = Mat4Fixed::from_rotation_y(*bam);
        let world_mat = rot_mat.mul_mat(&trans_mat);

        rasterizer.render_model(&mut fb, &model, &world_mat, cam_dist, focal_length);

        let ppm_path = format!("tmp/turntable_{}.ppm", name);
        save_ppm(&fb, &ppm_path).expect("Failed to write PPM");
        println!("[Axiom 3D] Rendered {}: {} -> {}", desc, name, ppm_path);
    }

    println!("[Axiom 3D] Turntable render sequence complete!");
}
