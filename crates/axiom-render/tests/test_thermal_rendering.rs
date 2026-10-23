//! Test suite for 3D Thermal Stress Field & Incandescent Emissive Rasterizer.

use axiom_core::Fixed;
use axiom_core::thermal_stress::{ThermalStressField16, TEMP_AMBIENT};
use axiom_render::framebuffer::FrameBuffer;
use axiom_render::math3d::{Mat4Fixed, Vec3Fixed};
use axiom_render::thermal_render::{render_thermal_glow, sample_thermal_glow_color};

#[test]
fn test_thermal_glow_color_sampling() {
    // Ambient / cold returns None
    assert_eq!(sample_thermal_glow_color(TEMP_AMBIENT), None);
    assert_eq!(sample_thermal_glow_color(250), None);

    // Warm temperature returns amber/orange glow
    let warm_glow = sample_thermal_glow_color(500);
    assert!(warm_glow.is_some());
    let warm_c = warm_glow.unwrap();
    let wr = (warm_c >> 16) & 0xFF;
    let wg = (warm_c >> 8) & 0xFF;
    assert!(wr >= 200, "Warm glow should have high red component");
    assert!(wg > 0, "Warm glow should have green component");

    // Incandescent temperature returns white-hot emission
    let incand_glow = sample_thermal_glow_color(850);
    assert!(incand_glow.is_some());
    let incand_c = incand_glow.unwrap();
    let ir = (incand_c >> 16) & 0xFF;
    let ig = (incand_c >> 8) & 0xFF;
    let ib = incand_c & 0xFF;
    assert_eq!(ir, 255);
    assert!(ig >= 240);
    assert!(ib >= 138, "Incandescent glow should have significant blue component for white-hot look");
}

#[test]
fn test_render_thermal_glow_rasterization() {
    let mut fb = FrameBuffer::new(320, 240);
    let mut depth_buffer = vec![i32::MIN; 320 * 240];

    let mut thermal = ThermalStressField16::new();
    thermal.inject_thermal_pulse(8, 8, 8, 3, 900, 700);

    // View translation pushing scene forward down +Z
    let vp = Mat4Fixed::from_translation(Vec3Fixed::new(Fixed::ZERO, Fixed::ZERO, Fixed::from_int(15)));

    let rendered = render_thermal_glow(
        &mut fb,
        &mut depth_buffer,
        &vp,
        &thermal,
        Fixed::from_int(1),
        Vec3Fixed::ZERO,
    );

    assert!(rendered > 0, "High temperature voxels must be rasterized as thermal glow");

    // Center pixel of framebuffer should have glowing color
    let center_idx = 120 * 320 + 160;
    let center_pixel = fb.pixels[center_idx];
    let cr = (center_pixel >> 16) & 0xFF;
    assert!(cr > 0, "Thermal emissive pixels must be written to framebuffer");
}
