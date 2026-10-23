//! Unit & Integration Tests for 3D Custom Textures, UV Mapping, and Workshop Modding Pipeline.

use axiom_core::Fixed;
use axiom_render::math3d::{Mat4Fixed, Vec3Fixed};
use axiom_render::mesh3d::{MechaModel3D, PartMesh};
use axiom_render::modding::{WorkshopManager, WorkshopModManifest, WorkshopModPackage};
use axiom_render::raster3d::Rasterizer3D;
use axiom_render::texture::{blend_decal, modulate_color, TextureMap, TextureWrapMode};
use axiom_render::FrameBuffer;

#[test]
fn test_texture_map_creation_and_sampling() {
    let mut tex = TextureMap::new(64, 64);
    assert_eq!(tex.width, 64);
    assert_eq!(tex.height, 64);

    // Set distinctive corners
    tex.set_pixel(0, 0, 0xFFFF0000); // Red
    tex.set_pixel(63, 0, 0xFF00FF00); // Green
    tex.set_pixel(0, 63, 0xFF0000FF); // Blue
    tex.set_pixel(63, 63, 0xFFFFFF00); // Yellow

    assert_eq!(tex.get_pixel(0, 0), 0xFFFF0000);
    assert_eq!(tex.get_pixel(63, 0), 0xFF00FF00);

    // Sample UV at (0, 0)
    let s00 = tex.sample_nearest(0, 0, TextureWrapMode::Clamp);
    assert_eq!(s00, 0xFFFF0000);

    // Sample UV with Repeat mode wrapping
    let s_repeat = tex.sample_nearest(65536 * 2, 65536 * 3, TextureWrapMode::Repeat);
    assert_eq!(s_repeat, 0xFFFF0000);

    // Sample Clamp
    let s_clamp = tex.sample_nearest(65536 * 10, 65536 * 10, TextureWrapMode::Clamp);
    assert_eq!(s_clamp, 0xFFFFFF00);
}

#[test]
fn test_procedural_textures_generation() {
    let hazard = TextureMap::procedural_hazard_stripes(64);
    assert_eq!(hazard.width, 64);
    assert_eq!(hazard.height, 64);
    assert!(hazard.pixels.iter().any(|&c| c == 0xFFFD9F07));
    assert!(hazard.pixels.iter().any(|&c| c == 0xFF141923));

    let metal = TextureMap::procedural_metal_plating(64);
    assert_eq!(metal.width, 64);
    assert!(metal.pixels.iter().any(|&c| c == 0xFFD97706)); // Brass rivets

    let camo = TextureMap::procedural_camo(64);
    assert_eq!(camo.width, 64);
    assert!(camo.pixels.iter().any(|&c| c == 0xFF7F772B)); // Olive
}

#[test]
fn test_integer_color_modulation_and_decal_blend() {
    let white = 0xFFFFFFFF;
    let half_lit = modulate_color(white, 128); // 50%
    assert_eq!(half_lit, 0xFF808080);

    let full_lit = modulate_color(0xFF5A5148, 256);
    assert_eq!(full_lit, 0xFF5A5148);

    // Decal blend: 100% opaque texture replaces base
    let base = 0xFF5BB8A0;
    let opaque_tex = 0xFFFF0000;
    assert_eq!(blend_decal(base, opaque_tex), 0xFFFF0000);

    // Decal blend: 0% alpha leaves base untouched
    let transparent_tex = 0x00FF0000;
    assert_eq!(blend_decal(base, transparent_tex), base);
}

#[test]
fn test_textured_pineda_rasterizer_execution() {
    let mut fb = FrameBuffer::new(128, 128);
    let mut rasterizer = Rasterizer3D::new(128, 128);
    let texture = TextureMap::procedural_hazard_stripes(32);

    // Draw a single textured triangle spanning (20,20) to (100,20) to (60,100)
    rasterizer.custom_texture = Some(texture);
    rasterizer.rasterize_textured_pineda_8x8(
        &mut fb,
        20, 20, 1000, 0, 0,
        100, 20, 1000, 65536, 0,
        60, 100, 1000, 32768, 65536,
        0xFF5BB8A0,
    );

    // Center pixel should be drawn and contain texture stripe color
    let center_color = fb.pixels[50 * 128 + 60];
    assert!(
        center_color == 0xFFFD9F07 || center_color == 0xFF141923,
        "Center pixel was 0x{:08X}",
        center_color
    );
    assert!(rasterizer.depth_buffer[50 * 128 + 60] > i32::MIN);
}

#[test]
fn test_workshop_mod_manifest_and_part_injection() {
    let manifest_json = r#"{
        "mod_id": "titan_fist_mk2",
        "title": "Titan Hydraulic Knuckle",
        "author": "Hephaestus",
        "version": "1.2.0",
        "target_slot": "Arm",
        "target_mesh_name": "hero_glove_R",
        "atk_bonus": 15,
        "def_bonus": 5,
        "heat_bonus": 4,
        "weight_bonus": 10,
        "tags": ["Hydraulic", "Heavy", "Champion"]
    }"#;

    let manifest: WorkshopModManifest = serde_json::from_str(manifest_json).expect("valid manifest");
    assert_eq!(manifest.mod_id, "titan_fist_mk2");
    assert_eq!(manifest.atk_bonus, 15);

    // Create a custom mod mesh
    let custom_mesh = PartMesh::create_box(
        "hero_glove_R",
        Vec3Fixed::from_f32(-0.2, -0.2, -0.2),
        Vec3Fixed::from_f32(0.2, 0.2, 0.2),
    );
    let custom_tex = TextureMap::procedural_metal_plating(32);

    let pkg = WorkshopModPackage::from_mesh(manifest, custom_mesh.clone(), Some(custom_tex));

    // Base procedural model
    let mut model = MechaModel3D::procedural_chassis();
    let initial_parts_count = model.parts.len();

    // Inject mod part
    let replaced = pkg.inject_into_model(&mut model);
    assert!(replaced, "hero_glove_R should have been replaced");
    assert_eq!(model.parts.len(), initial_parts_count);

    // Verify injected mesh is present
    let modded_part = model.parts.iter().find(|p| p.name == "hero_glove_R").unwrap();
    assert_eq!(modded_part.triangle_count(), custom_mesh.triangle_count());

    // Restore fallback
    let fallback = PartMesh::create_box(
        "hero_glove_R",
        Vec3Fixed::from_f32(-0.1, -0.1, -0.1),
        Vec3Fixed::from_f32(0.1, 0.1, 0.1),
    );
    let restored = pkg.restore_original_in_model(&mut model, fallback);
    assert!(restored);
}

#[test]
fn test_render_model_with_custom_texture() {
    let mut fb = FrameBuffer::new(160, 120);
    let mut rasterizer = Rasterizer3D::new(160, 120);
    rasterizer.custom_texture = Some(TextureMap::procedural_camo(64));

    let model = MechaModel3D::procedural_chassis();
    rasterizer.render_model(
        &mut fb,
        &model,
        &Mat4Fixed::IDENTITY,
        Fixed::from_raw(163840), // 2.5 in 16.16 fixed point
        160,
    );

    // Framebuffer must have drawn pixels and depth buffer populated
    let filled_pixels = fb.pixels.iter().filter(|&&c| c != 0).count();
    assert!(filled_pixels > 200, "Rendered pixels count: {}", filled_pixels);
}

#[test]
fn test_workshop_manager_lifecycle_and_hot_swap() {
    let temp_dir = std::env::temp_dir().join(format!(
        "axiom_workshop_test_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_micros()
    ));
    let _ = std::fs::create_dir_all(&temp_dir);

    let mut manager = WorkshopManager::new(&temp_dir);
    assert_eq!(manager.scan_mods().unwrap(), 0);

    // Create a template mod
    let manifest = WorkshopModManifest {
        mod_id: "mod_turbo_core".to_string(),
        title: "Turbo Overclock Core".to_string(),
        author: "TechGuild".to_string(),
        version: "2.0.0".to_string(),
        target_slot: "Core".to_string(),
        target_mesh_name: "hero_torso".to_string(),
        atk_bonus: 20,
        def_bonus: 10,
        heat_bonus: 15,
        weight_bonus: -5,
        tags: vec!["Overclock".to_string(), "Core".to_string()],
    };
    let mod_path = WorkshopManager::create_template_mod(&temp_dir, &manifest).expect("template created");
    assert!(mod_path.exists());
    assert!(mod_path.join("mod.json").exists());

    // Scan mods again
    let count = manager.scan_mods().unwrap();
    assert_eq!(count, 1);
    assert_eq!(manager.installed_mods[0].manifest.title, "Turbo Overclock Core");

    // Hot-swap into procedural chassis
    let mut chassis = MechaModel3D::procedural_chassis();
    let replaced = manager.hot_swap_reload(&mut chassis).unwrap();
    assert_eq!(replaced, 1);

    // Toggle mod off
    assert!(!manager.toggle_mod("mod_turbo_core"));
    let mut chassis2 = MechaModel3D::procedural_chassis();
    let replaced2 = manager.apply_active_mods(&mut chassis2);
    assert_eq!(replaced2, 0);

    // Clean up
    let _ = std::fs::remove_dir_all(&temp_dir);
}
