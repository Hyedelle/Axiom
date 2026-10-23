use axiom_core::Fixed;
use axiom_render::kinematics3d::{evaluate_forward_kinematics, get_part_matrix, MechaPose3D};
use axiom_render::math3d::Mat4Fixed;
use axiom_render::mesh3d::parse_glb;
use axiom_render::raster3d::Rasterizer3D;
use axiom_render::FrameBuffer;
use std::fs;
use std::path::Path;

#[test]
fn test_forward_kinematics_poses() {
    // 1. Jab pose test
    let jab_pose = MechaPose3D::jab(400);
    let table = evaluate_forward_kinematics(&jab_pose, false);

    let glove_l_mat = get_part_matrix(&table, "hero_glove_L");
    // Glove L should be translated forward along Z (m[2][3] > 0)
    assert!(
        glove_l_mat.m[2][3].raw() > 0,
        "Jab must extend left glove forward along Z, got {}",
        glove_l_mat.m[2][3].raw()
    );

    // 2. Cross pose test
    let cross_pose = MechaPose3D::cross(450);
    let table_cross = evaluate_forward_kinematics(&cross_pose, false);
    let glove_r_mat = get_part_matrix(&table_cross, "hero_glove_R");
    assert!(
        glove_r_mat.m[2][3].raw() > 0,
        "Cross must extend right glove forward along Z"
    );

    // 3. Guard pose test
    let guard_pose = MechaPose3D::guard();
    let table_guard = evaluate_forward_kinematics(&guard_pose, false);
    let glove_l_guard = get_part_matrix(&table_guard, "hero_glove_L");
    assert!(
        glove_l_guard.m[1][3].raw() > 0,
        "Guard must raise gloves upwards along Y"
    );
}

#[test]
fn test_render_posed_mecha() {
    let hero_glb_path = Path::new(r"C:\Dreamhub\Game\steel-the-show\assets\robot3d\hero\hero_a.glb");
    if !hero_glb_path.exists() {
        return;
    }

    let bytes = fs::read(hero_glb_path).expect("Failed to read hero_a.glb");
    let model = parse_glb(&bytes).expect("Failed to parse hero_a.glb");

    let width = 640;
    let height = 480;
    let mut fb = FrameBuffer::new(width, height);
    fb.clear(0xFF0F172A);

    let mut rasterizer = Rasterizer3D::new(width, height);
    let jab_pose = MechaPose3D::jab(400);
    let transforms = evaluate_forward_kinematics(&jab_pose, false);

    let world_mat = Mat4Fixed::IDENTITY;
    let cam_dist = Fixed::from_int(2);
    rasterizer.render_model_posed(&mut fb, &model, &transforms, &world_mat, cam_dist, 500);

    let drawn = fb.pixels.iter().filter(|&&p| p != 0xFF0F172A).count();
    assert!(drawn > 5000, "Should have rendered >5000 pixels of posed mecha, got {}", drawn);
}
