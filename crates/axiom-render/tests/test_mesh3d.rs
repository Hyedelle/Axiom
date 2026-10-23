use axiom_core::Fixed;
use axiom_render::math3d::{cos_bams, sin_bams, Mat4Fixed, Vec3Fixed};
use axiom_render::mesh3d::parse_glb;
use axiom_render::raster3d::Rasterizer3D;
use axiom_render::FrameBuffer;
use std::fs;
use std::path::Path;

#[test]
fn test_fixed_point_3d_math_transforms() {
    let v = Vec3Fixed::from_ints(1, 0, 0);

    // 90 degrees around Y axis (BAM = 64)
    let rot_y_90 = Mat4Fixed::from_rotation_y(64);
    let v_rot = rot_y_90.transform_point(v);

    // After 90 deg rotation around Y, (1, 0, 0) -> (0, 0, -1) in standard right-handed space
    // Let's verify coordinates are close to (0, 0, -1) in fixed point
    assert!(v_rot.x.raw().abs() < 500, "X should be ~0, got {}", v_rot.x.raw());
    assert_eq!(v_rot.y, Fixed::ZERO);
    assert!((v_rot.z.raw() + 65536).abs() < 500, "Z should be ~-1, got {}", v_rot.z.raw());

    // Identity check
    let id = Mat4Fixed::IDENTITY;
    assert_eq!(id.transform_point(v), v);

    // Translation check
    let trans = Mat4Fixed::from_translation(Vec3Fixed::from_ints(2, 3, 4));
    let v_trans = trans.transform_point(v);
    assert_eq!(v_trans, Vec3Fixed::from_ints(3, 3, 4));
}

#[test]
fn test_sine_cosine_bams_determinism() {
    // 0 deg
    assert_eq!(sin_bams(0).raw(), 0);
    assert!((cos_bams(0).raw() - 65536).abs() < 500);

    // 90 deg (BAM 64)
    assert!((sin_bams(64).raw() - 65536).abs() < 500);
    assert!(cos_bams(64).raw().abs() < 500);

    // 180 deg (BAM 128)
    assert!(sin_bams(128).raw().abs() < 500);
    assert!((cos_bams(128).raw() + 65536).abs() < 500);
}

#[test]
fn test_load_hero_a_glb_and_rasterize() {
    let hero_glb_path = Path::new(r"C:\Dreamhub\Game\steel-the-show\assets\robot3d\hero\hero_a.glb");
    if !hero_glb_path.exists() {
        eprintln!("hero_a.glb not found at path, skipping disk test");
        return;
    }

    let bytes = fs::read(hero_glb_path).expect("Failed to read hero_a.glb");
    let model = parse_glb(&bytes).expect("Failed to parse hero_a.glb");

    // Verify 12 parts extracted from Blender
    assert_eq!(model.parts.len(), 12, "Should have 12 mecha parts");
    assert!(model.total_triangles() > 20000, "Should have >20k triangles");
    assert!(model.total_vertices() > 10000, "Should have >10k vertices");

    // Verify key nodes
    assert!(model.find_part("hero_torso").is_some());
    assert!(model.find_part("hero_pelvis").is_some());
    assert!(model.find_part("hero_head").is_some());
    assert!(model.find_part("hero_glove_R").is_some());

    // Rasterize onto FrameBuffer
    let mut fb = FrameBuffer::new(640, 480);
    fb.clear(0xFF10141C); // Dark industrial background

    let mut rasterizer = Rasterizer3D::new(640, 480);
    let world_mat = Mat4Fixed::from_rotation_y(32); // 45 degree turntable angle
    let cam_dist = Fixed::from_int(2); // 2 meters camera distance
    let focal_length = 500; // Focal length in pixels

    rasterizer.render_model(&mut fb, &model, &world_mat, cam_dist, focal_length);

    // Verify that pixels were drawn (not all background)
    let non_bg_count = fb.pixels.iter().filter(|&&p| p != 0xFF10141C).count();
    assert!(
        non_bg_count > 5000,
        "Rasterizer should have drawn at least 5000 mecha pixels, drew {}",
        non_bg_count
    );
}
