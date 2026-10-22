//! 2D Anime Cut-in Renderer Tests (T-075).

use axiom_render::cutin::{render_cutin_overlay, CutinKind};
use axiom_render::FrameBuffer;

#[test]
fn test_2d_cutin_render() {
    let mut fb = FrameBuffer::new(704, 544);
    fb.clear(0xFF000000);

    // Render cut-in overlay
    render_cutin_overlay(&mut fb, CutinKind::OverheatMeltdown, 500, 352, 272);

    // Top and bottom black bars must be plotted
    assert_eq!(fb.pixels[10 * 704 + 10], 0xFF050508);
    assert_eq!(fb.pixels[(544 - 10) * 704 + 10], 0xFF050508);

    // Center band must contain fiery color or white highlight or ink outline
    let center_pixel = fb.pixels[272 * 704 + 352];
    assert!(
        center_pixel == 0xFFFFFFFF || center_pixel == 0xFFD44018 || center_pixel == 0xFF101015 || center_pixel == 0xFFFFF040,
        "Center pixel was {:#X}",
        center_pixel
    );
}
