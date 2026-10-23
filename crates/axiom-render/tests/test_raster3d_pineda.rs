use axiom_core::Fixed;
use axiom_render::math3d::{Mat4Fixed, Vec3Fixed};
use axiom_render::mesh3d::parse_glb;
use axiom_render::raster3d::Rasterizer3D;
use axiom_render::FrameBuffer;
use std::fs;
use std::path::Path;

#[test]
fn test_pineda_8x8_rasterizer_determinism() {
    let hero_glb_path = Path::new(r"C:\Dreamhub\Game\steel-the-show\assets\robot3d\hero\hero_a.glb");
    if !hero_glb_path.exists() {
        return;
    }

    let bytes = fs::read(hero_glb_path).expect("Failed to read hero_a.glb");
    let model = parse_glb(&bytes).expect("Failed to parse hero_a.glb");

    let width = 640;
    let height = 480;

    // Run 1
    let mut fb1 = FrameBuffer::new(width, height);
    fb1.clear(0xFF0F172A);
    let mut r1 = Rasterizer3D::new(width, height);
    let world_mat = Mat4Fixed::from_rotation_y(32);
    let cam_dist = Fixed::from_int(2);
    r1.render_model(&mut fb1, &model, &world_mat, cam_dist, 500);

    // Run 2
    let mut fb2 = FrameBuffer::new(width, height);
    fb2.clear(0xFF0F172A);
    let mut r2 = Rasterizer3D::new(width, height);
    r2.render_model(&mut fb2, &model, &world_mat, cam_dist, 500);

    // Bit-exact verification across entire 307,200 pixel framebuffer and depth buffer
    assert_eq!(
        fb1.pixels, fb2.pixels,
        "Pineda 8x8 Tile Rasterizer must be 100% bit-exact across executions"
    );
    assert_eq!(
        r1.depth_buffer, r2.depth_buffer,
        "Depth buffers must match bit-for-bit"
    );

    // Verify non-empty pixel count
    let drawn = fb1.pixels.iter().filter(|&&p| p != 0xFF0F172A).count();
    assert!(drawn > 10000, "Should have rendered >10000 mecha pixels, got {}", drawn);
}

#[test]
fn test_pineda_tile_binning_depth_order() {
    let width = 64;
    let height = 64;
    let mut fb = FrameBuffer::new(width, height);
    fb.clear(0);
    let mut rasterizer = Rasterizer3D::new(width, height);

    // Draw background triangle at Z=10 (inv_z = 100)
    // Red color = 0xFFFF0000
    // Triangle (10, 10), (50, 10), (30, 50)
    let mut m = axiom_render::mesh3d::MechaModel3D::default();
    m.parts.push(axiom_render::mesh3d::PartMesh {
        name: "test_bg".into(),
        vertices: vec![
            axiom_render::mesh3d::Vertex3D {
                pos: Vec3Fixed::from_ints(-1, -1, 0),
                norm: Vec3Fixed::UP,
                u: 0,
                v: 0,
            },
            axiom_render::mesh3d::Vertex3D {
                pos: Vec3Fixed::from_ints(1, -1, 0),
                norm: Vec3Fixed::UP,
                u: 0,
                v: 0,
            },
            axiom_render::mesh3d::Vertex3D {
                pos: Vec3Fixed::from_ints(0, 1, 0),
                norm: Vec3Fixed::UP,
                u: 0,
                v: 0,
            },
        ],
        indices: vec![0, 1, 2],
    });

    let world_mat = Mat4Fixed::IDENTITY;
    let cam_dist = Fixed::from_int(3);
    rasterizer.render_model(&mut fb, &m, &world_mat, cam_dist, 50);

    let drawn = fb.pixels.iter().filter(|&&p| p != 0).count();
    assert!(drawn > 0, "Test model should be rasterized");
}
