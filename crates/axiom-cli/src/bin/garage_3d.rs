//! Headless 3D Mecha Garage Part Inspection Capture Tool for Axiom Engine.
//!
//! Visualizes modular mecha parts customization, 360° turntable inspection,
//! and part-specific highlights for ELPIS (`hero_a.glb`).

use axiom_core::Fixed;
use axiom_render::math3d::{Mat4Fixed, Vec3Fixed};
use axiom_render::mesh3d::parse_glb;
use axiom_render::raster3d::{CelShadingPalette, Rasterizer3D};
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

    println!("[Axiom 3D Garage] Ingesting hero_a.glb for Garage Inspection...");
    let bytes = fs::read(glb_path).expect("Failed to read GLB");
    let model = parse_glb(&bytes).expect("Failed to parse GLB");

    let width = 704;
    let height = 544;
    let mut fb = FrameBuffer::new(width, height);
    let mut rasterizer = Rasterizer3D::new(width, height);

    fs::create_dir_all("tmp").ok();

    // Inspection Modes:
    let inspect_modes = [
        ("all", "FULL MECHA CHASSIS", "hero_pelvis", 32u8),
        ("core", "CORE BOILER TORSO", "hero_torso", 24u8),
        ("arm", "HYDRAULIC CRUSH ARM", "hero_arm_L", 48u8),
        ("visor", "SENSORY OPTIC VISOR", "hero_head", 16u8),
    ];

    let center_offset = Vec3Fixed::from_f32(0.0, -0.19, 0.0);
    let trans_mat = Mat4Fixed::from_translation(center_offset);
    let cam_dist = Fixed::from_raw((2.1 * 65536.0) as i64);
    let focal_length = 520;

    for (slug, title, target_part, bam) in &inspect_modes {
        // High-tech hangar background
        fb.clear(0xFF070B14);
        rasterizer.clear_depth();

        // 1. Grid Floor
        for gy in (height / 2 + 100)..height {
            let row = gy as usize * width;
            let checker = (gy / 16) % 2 == 0;
            for gx in 0..width {
                if checker && (gx % 32 == 0) {
                    fb.pixels[row + gx] = 0xFF1E293B;
                }
            }
        }

        // 2. 3D Model Render
        let rot_mat = Mat4Fixed::from_rotation_y(*bam);
        let world_mat = rot_mat.mul_mat(&trans_mat);

        // Highlight selected target part
        for part in &model.parts {
            if part.name.contains(target_part) {
                // Golden amber highlight for inspected part
                rasterizer.custom_palette = Some(CelShadingPalette {
                    highlight: 0xFFFEF08A,
                    base: 0xFFEAB308,
                    shadow: 0xFF854D0E,
                    outline: 0xFF0F172A,
                });
            } else {
                rasterizer.custom_palette = None;
            }

            // Single part render
            let mut single_model = axiom_render::mesh3d::MechaModel3D::default();
            single_model.parts.push(part.clone());
            rasterizer.render_model(&mut fb, &single_model, &world_mat, cam_dist, focal_length);
        }

        // 3. High-Tech Garage HUD Overlay
        // Top Header
        fb.draw_rect_blend(20, 20, width as i32 - 40, 48, 0xEE0F172A);
        fb.draw_rect(24, 24, 4, 40, 0xFF38BDF8); // Cyan vertical accent
        fb.draw_text_2x(36, 26, "ELPIS WORKSHOP [3D GARAGE]", 0xFFE2E8F0);
        fb.draw_text(38, 48, "MODULAR ROBOT ASSEMBLY & 360 TURNTABLE INSPECTOR", 0xFF94A3B8);

        // Left Part Telemetry Card
        fb.draw_rect_blend(20, 80, 240, 160, 0xEE0B0F19);
        fb.draw_rect(20, 80, 240, 24, 0xFF1E293B);
        fb.draw_text(30, 87, "INSPECTED COMPONENT", 0xFF38BDF8);

        fb.draw_text_2x(30, 112, title, 0xFFFBBF24);
        fb.draw_text(30, 138, &format!("ACTIVE NODE : {}", target_part), 0xFFE2E8F0);
        fb.draw_text(30, 154, "GRADE       : MK-II TITAN", 0xFF34D399);
        fb.draw_text(30, 170, "MATERIAL    : REINFORCED SLATE", 0xFF94A3B8);
        fb.draw_text(30, 186, "WEIGHT CLASS: 85 KG", 0xFFE2E8F0);
        fb.draw_text(30, 202, "HEAT EFF    : 94.2% OPTIMAL", 0xFFF97316);

        // Right Stats & Controls Card
        fb.draw_rect_blend(width as i32 - 240, 80, 220, 140, 0xEE0B0F19);
        fb.draw_text(width as i32 - 230, 90, "GEOMETRY TELEMETRY", 0xFF38BDF8);
        fb.draw_text(width as i32 - 230, 112, &format!("TOTAL TRIS : {}", model.total_triangles()), 0xFFE2E8F0);
        fb.draw_text(width as i32 - 230, 128, &format!("TOTAL VERTS: {}", model.total_vertices()), 0xFFE2E8F0);
        fb.draw_text(width as i32 - 230, 144, &format!("NODES COUNT: {} PARTS", model.parts.len()), 0xFFE2E8F0);
        fb.draw_text(width as i32 - 230, 160, &format!("YAW ANGLE  : {} DEG", (*bam as u32 * 360) / 256), 0xFFFBBF24);
        fb.draw_text(width as i32 - 230, 176, "RASTERIZER : PINEDA 8x8 L1", 0xFF34D399);

        // Bottom Footer Controls
        fb.draw_rect_blend(20, height as i32 - 50, width as i32 - 40, 32, 0xEE0F172A);
        fb.draw_text(35, height as i32 - 40, "[TAB: VIEWPORT]  [1: CORE]  [2: ARMS]  [3: LEGS]  [4: VISOR]  [MOUSE: 360 ORBIT]", 0xFFE2E8F0);

        let ppm_path = format!("tmp/garage_inspect_{}.ppm", slug);
        save_ppm(&fb, &ppm_path).expect("Failed to write PPM");
        println!("[Axiom 3D Garage] Rendered {}: -> {}", title, ppm_path);
    }

    println!("[Axiom 3D Garage] Inspection sequence successfully completed!");
}
