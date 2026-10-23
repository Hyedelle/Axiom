use axiom_render::newspaper::{
    apply_newsprint_filter, draw_newspaper_photo_frame, NewspaperGravureConfig, PhotoFilter,
};
use axiom_render::FrameBuffer;

#[test]
fn test_newspaper_gravure_config_alignment() {
    assert_eq!(
        std::mem::size_of::<NewspaperGravureConfig>(),
        64,
        "NewspaperGravureConfig must be 64 bytes"
    );
    assert_eq!(
        std::mem::align_of::<NewspaperGravureConfig>(),
        64,
        "NewspaperGravureConfig must be 64-byte aligned"
    );
}

#[test]
fn test_newsprint_filter_sepia_and_monochrome() {
    let mut fb = FrameBuffer::new(32, 32);
    // Draw some colored pixels
    fb.draw_rect(0, 0, 16, 16, 0xFFFF0000); // Red
    fb.draw_rect(16, 0, 16, 16, 0xFF00FF00); // Green
    fb.draw_rect(0, 16, 16, 16, 0xFF0000FF); // Blue
    fb.draw_rect(16, 16, 16, 16, 0xFF000000); // Background black

    // Test Sepia filter
    let mut sepia_fb = fb.clone();
    apply_newsprint_filter(&mut sepia_fb, PhotoFilter::SepiaNewsprint);

    // Pixels should be converted to sepia tones (R > G > B)
    let p_red = sepia_fb.pixels[0];
    let r = (p_red >> 16) & 0xFF;
    let g = (p_red >> 8) & 0xFF;
    let b = p_red & 0xFF;
    assert!(r >= g && g >= b, "Sepia tone must satisfy R >= G >= B");

    // Background black should be converted to vintage paper tone
    let p_bg = sepia_fb.pixels[16 * 32 + 16];
    assert_ne!(p_bg, 0xFF000000, "Background should receive vintage paper tone");

    // Test Monochrome filter
    let mut mono_fb = fb.clone();
    apply_newsprint_filter(&mut mono_fb, PhotoFilter::HighContrastMonochrome);
    let p_mono = mono_fb.pixels[0];
    let mr = (p_mono >> 16) & 0xFF;
    let mg = (p_mono >> 8) & 0xFF;
    let mb = p_mono & 0xFF;
    assert_eq!(mr, mg, "Monochrome R and G must match");
    assert_eq!(mg, mb, "Monochrome G and B must match");
}

#[test]
fn test_framebuffer_blit_and_border() {
    let mut canvas = FrameBuffer::new(64, 64);
    let mut sub = FrameBuffer::new(16, 16);
    sub.clear(0xFFFFCC00);

    // Blit sub into canvas at (10, 10)
    canvas.blit(&sub, 10, 10);
    assert_eq!(canvas.pixels[10 * 64 + 10], 0xFFFFCC00);

    // Draw photo frame border around it
    draw_newspaper_photo_frame(&mut canvas, 8, 8, 20, 20, "CAPTURE", 0xFFA06432, 0xFFFFFFFF, true);
    // Outer border pixel
    assert_eq!(canvas.pixels[8 * 64 + 8], 0xFFA06432);
}
