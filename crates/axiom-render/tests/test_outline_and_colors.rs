use axiom_core::Fixed;
use axiom_render::math3d::Mat4Fixed;
use axiom_render::mesh3d::parse_glb;
use axiom_render::raster3d::{CelShadingPalette, Rasterizer3D};
use axiom_render::FrameBuffer;
use std::fs;
use std::path::Path;

#[test]
fn test_canon_color_grammar_palette_mapping() {
    assert_eq!(CelShadingPalette::INK, 0xFF2B222A);

    let eye_pal = CelShadingPalette::for_part("hero_eye");
    assert_eq!(eye_pal.base, 0xFFFF660A, "Eye must be Canon Orange");
    assert_eq!(eye_pal.outline, CelShadingPalette::INK);

    let glove_pal = CelShadingPalette::for_part("hero_glove_L");
    assert_eq!(glove_pal.base, 0xFF8A2B17, "Glove must be Canon RED");

    let torso_pal = CelShadingPalette::for_part("hero_torso");
    assert_eq!(torso_pal.base, 0xFF7F772B, "Torso must be Canon OLIVE");

    let chassis_pal = CelShadingPalette::for_part("hero_pelvis");
    assert_eq!(chassis_pal.base, 0xFF6F4430, "Chassis must be Canon Brown");
}

#[test]
fn test_inverted_hull_outline_rasterization() {
    let hero_glb_path = Path::new(r"C:\Dreamhub\Game\steel-the-show\assets\robot3d\hero\hero_a.glb");
    if !hero_glb_path.exists() {
        return;
    }

    let bytes = fs::read(hero_glb_path).expect("Failed to read GLB");
    let model = parse_glb(&bytes).expect("Failed to parse GLB");

    let width = 640;
    let height = 480;
    let mut fb = FrameBuffer::new(width, height);
    fb.clear(0xFF0F172A);

    let mut rasterizer = Rasterizer3D::new(width, height);
    rasterizer.draw_outline = true;
    rasterizer.outline_width = Fixed::from_raw(524); // ~0.008m
    rasterizer.outline_color = CelShadingPalette::INK;

    let world_mat = Mat4Fixed::IDENTITY;
    let cam_dist = Fixed::from_int(2);
    rasterizer.render_model(&mut fb, &model, &world_mat, cam_dist, 500);

    // Verify that INK pixels were drawn
    let ink_pixels = fb.pixels.iter().filter(|&&p| p == CelShadingPalette::INK).count();
    assert!(
        ink_pixels > 500,
        "Inverted hull outline must draw >500 ink outline pixels, drew {}",
        ink_pixels
    );
}
