//! 2D Anime/Comic Cut-in rasterizer for Axiom Render.
//!
//! Renders cinematic comic book visual effects (slanted action bands, radial speed lines,
//! high-contrast monochrome inversion, and bold comic impact text) onto a FrameBuffer.

use crate::framebuffer::FrameBuffer;

/// Kinds of 2D comic cut-in sequences.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CutinKind {
    StraightPierce = 0,
    TitanMirror = 1,
    OverheatMeltdown = 2,
    Knockout = 3,
    ClimaxNova = 4,
    CounterStrike = 5,
    BossEncounter = 6,
}

/// Renders a full 2D anime cut-in overlay onto the framebuffer.
/// `progress` is in 0..100 permille (0 to 1000).
pub fn render_cutin_overlay(
    fb: &mut FrameBuffer,
    kind: CutinKind,
    permille: u32,
    hit_x: i32,
    hit_y: i32,
) {
    let w = fb.width as i32;
    let h = fb.height as i32;

    // 1. Cinematic Black Bars (Top and Bottom, 48px)
    let bar_h = 48;
    for y in 0..bar_h {
        for x in 0..w {
            fb.set_pixel(x, y, 0xFF050508);
            fb.set_pixel(x, h - 1 - y, 0xFF050508);
        }
    }

    // 2. High-Contrast Slanted Comic Band (-8 degree slope)
    // Band center sweeps across based on permille
    let band_center_y = h / 2;
    let band_half_thickness = 70;
    let band_color = match kind {
        CutinKind::StraightPierce => 0xFF2E6CB5,   // Steel Blue
        CutinKind::TitanMirror => 0xFF228B58,      // Emerald Green
        CutinKind::OverheatMeltdown => 0xFFD44018,  // Fiery Orange
        CutinKind::Knockout => 0xFFF0B020,          // Golden Yellow
        CutinKind::ClimaxNova => 0xFF9333EA,        // Supernova Magenta / Purple
        CutinKind::CounterStrike => 0xFFDC2626,     // Crimson Red
        CutinKind::BossEncounter => 0xFFD97706,     // Warning Amber
    };

    let sweep_offset = ((permille as i32 - 500) * 12) / 10;

    for y in (band_center_y - band_half_thickness)..(band_center_y + band_half_thickness) {
        if y < bar_h || y >= h - bar_h {
            continue;
        }
        for x in 0..w {
            let slant_y = y + (x - w / 2) / 8 + sweep_offset / 10;
            let dist = (slant_y - band_center_y).abs();
            if dist < band_half_thickness {
                if dist > band_half_thickness - 4 {
                    // Dark ink outline of the band
                    fb.set_pixel(x, y, 0xFF101015);
                } else if dist < 12 {
                    // Inner bright highlight streak
                    fb.set_pixel(x, y, 0xFFFFFFFF);
                } else {
                    fb.set_pixel(x, y, band_color);
                }
            }
        }
    }

    // 3. Radial Speed Lines (Spokes centered on impact point)
    let cx = hit_x.clamp(64, w - 64);
    let cy = hit_y.clamp(64, h - 64);
    let line_count = 32;

    for i in 0..line_count {
        if (i + (permille / 80) as usize) % 3 != 0 {
            continue;
        }
        // Integer angle ray tracing
        let angle_idx = i * 360 / line_count;
        let dx = match (angle_idx / 45) % 8 {
            0 | 7 => 3,
            1 | 2 => 1,
            3 | 4 => -3,
            _ => -1,
        };
        let dy = match (angle_idx / 45) % 8 {
            1 | 6 => 3,
            2 | 5 => 1,
            0 | 3 => -1,
            _ => -3,
        };

        let mut px = cx + dx * 8;
        let mut py = cy + dy * 8;
        for _ in 0..60 {
            if px >= 0 && px < w && py >= bar_h && py < h - bar_h {
                fb.set_pixel(px, py, 0xFFFFFF);
            }
            px += dx * 2;
            py += dy * 2;
        }
    }

    // 4. Large Bold Impact Text ("FINISHER!" or "CRITICAL!")
    let label = match kind {
        CutinKind::StraightPierce => "PIERCE FINISHER!",
        CutinKind::TitanMirror => "COUNTER IMPACT!",
        CutinKind::OverheatMeltdown => "OVERHEAT MELTDOWN!",
        CutinKind::Knockout => "K N O C K O U T !!",
        CutinKind::ClimaxNova => "SUPERNOVA FINISHER!",
        CutinKind::CounterStrike => "CROSS COUNTER!",
        CutinKind::BossEncounter => "BOSS CONTENDER!",
    };

    let text_x = (w - (label.len() as i32 * 8 * 2)) / 2;
    let text_y = band_center_y - 8;
    fb.draw_text_2x(text_x + 2, text_y + 2, label, 0x0A0A10); // 2x scaled drop shadow
    fb.draw_text_2x(text_x, text_y, label, 0xFFF040);          // Bright yellow font
}
