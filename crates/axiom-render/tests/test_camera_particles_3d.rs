use axiom_core::Fixed;
use axiom_render::camera3d::Camera3D;
use axiom_render::math3d::{Mat4Fixed, Vec3Fixed};
use axiom_render::particles3d::ParticleSystem3D;
use axiom_render::FrameBuffer;

#[test]
fn test_camera_punch_zoom_and_kinematics() {
    let mut cam = Camera3D::default();
    let initial_dist = cam.effective_distance();

    // Punch zoom trigger
    cam.trigger_punch_zoom(Fixed::from_int(1));
    assert!(cam.effective_distance() < initial_dist, "Punch zoom must reduce distance");

    // Kinematic spring decay
    for _ in 0..30 {
        cam.step_kinematics();
    }
    assert_eq!(
        cam.effective_distance(),
        initial_dist,
        "Camera must return to initial distance after spring decay"
    );
}

#[test]
fn test_particles_spawn_decay_and_render() {
    let mut ps = ParticleSystem3D::<64>::new(42);
    assert_eq!(ps.active_count, 0);

    // Spawn sparks
    ps.spawn_sparks(Vec3Fixed::ZERO, 10);
    assert_eq!(ps.active_count, 10);

    // Spawn steam
    ps.spawn_steam(Vec3Fixed::ZERO, 5);
    assert_eq!(ps.active_count, 15);

    // Render into framebuffer
    let mut fb = FrameBuffer::new(128, 128);
    let mut depth_buffer = vec![i32::MIN; 128 * 128];
    let world_mat = Mat4Fixed::IDENTITY;
    let cam_dist = Fixed::from_int(2);

    ps.render(&mut fb, &mut depth_buffer, &world_mat, cam_dist, 100);

    let drawn = fb.pixels.iter().filter(|&&p| p != 0xFF000000).count();
    assert!(drawn > 0, "Active particles must be rasterized to framebuffer");

    // Kinematic decay over 30 ticks
    for _ in 0..30 {
        ps.step_kinematics();
    }
    assert_eq!(ps.active_count, 0, "All particles must expire after their lifetime");
}

#[test]
fn test_debris_and_ground_bounce() {
    let mut ps = ParticleSystem3D::<32>::new(1337);
    ps.floor_y = Fixed::from_raw(-15000); // Floor close below spawn

    // Spawn 5 pieces of armor debris from (0, 0, 0)
    ps.spawn_debris(Vec3Fixed::ZERO, 5, 0xFF8A2B17);
    assert_eq!(ps.active_count, 5);

    // Downward trajectory under gravity and bounce
    let mut bounced = false;
    for _ in 0..25 {
        ps.step_kinematics();
        for p in &ps.particles {
            if p.is_active {
                // Must never sink below floor
                assert!(p.pos.y >= ps.floor_y, "Particle must not sink below floor");
                if p.pos.y == ps.floor_y && p.vel.y > Fixed::ZERO {
                    bounced = true;
                }
            }
        }
    }
    assert!(bounced, "Debris particle must bounce upward off the floor");
}
