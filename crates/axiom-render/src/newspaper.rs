//! Newspaper Photogravure and Front-Page 3D Snapshot System.
//!
//! Provides 64-byte L1 cache-aligned configurations and deterministic 0-Float
//! vintage newsprint filters (Sepia Halftone, Manga Monochrome, Vibrant Cel)
//! for rendering in-game press archives and championship extras.

use crate::framebuffer::FrameBuffer;

/// Vintage newsprint photo gravure filter modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum PhotoFilter {
    #[default]
    SepiaNewsprint = 0,
    HighContrastMonochrome = 1,
    VibrantCel = 2,
}

/// 64-Byte L1 cache-aligned newspaper photo frame configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C, align(64))]
pub struct NewspaperGravureConfig {
    pub x: i32,                  // 4 bytes
    pub y: i32,                  // 4 bytes
    pub width: u32,              // 4 bytes
    pub height: u32,             // 4 bytes
    pub border_color: u32,       // 4 bytes
    pub caption_color: u32,      // 4 bytes
    pub filter: PhotoFilter,     // 1 byte
    pub draw_rivets: bool,       // 1 byte
    pub _pad: [u8; 38],          // 38 bytes -> exactly 64 bytes
}

impl Default for NewspaperGravureConfig {
    fn default() -> Self {
        Self {
            x: 0,
            y: 0,
            width: 140,
            height: 120,
            border_color: 0xFFA06432,  // Canon Brass
            caption_color: 0xFFD4C8B8, // Worn Paper Bone
            filter: PhotoFilter::SepiaNewsprint,
            draw_rivets: true,
            _pad: [0; 38],
        }
    }
}

/// Applies a 0-Float vintage newsprint filter to a framebuffer in-place.
pub fn apply_newsprint_filter(fb: &mut FrameBuffer, filter: PhotoFilter) {
    match filter {
        PhotoFilter::SepiaNewsprint => {
            for y in 0..fb.height {
                let row = y * fb.width;
                let scanline_dim = y % 2 == 1;
                for x in 0..fb.width {
                    let p = fb.pixels[row + x];
                    // Skip transparent/black background and replace with aged paper tone
                    if p == 0xFF000000 || (p >> 24) == 0 {
                        fb.pixels[row + x] = if scanline_dim { 0xFF241D17 } else { 0xFF2B221B };
                        continue;
                    }
                    let r = ((p >> 16) & 0xFF) as u32;
                    let g = ((p >> 8) & 0xFF) as u32;
                    let b = (p & 0xFF) as u32;
                    let gray = (r * 77 + g * 151 + b * 28) >> 8;

                    let factor = if scanline_dim { 220 } else { 255 };
                    let sr = ((((gray * 230) >> 8) + 26) * factor) >> 8;
                    let sg = ((((gray * 190) >> 8) + 18) * factor) >> 8;
                    let sb = ((((gray * 135) >> 8) + 12) * factor) >> 8;

                    let cr = sr.min(255);
                    let cg = sg.min(255);
                    let cb = sb.min(255);

                    fb.pixels[row + x] = 0xFF000000 | (cr << 16) | (cg << 8) | cb;
                }
            }
        }
        PhotoFilter::HighContrastMonochrome => {
            for y in 0..fb.height {
                let row = y * fb.width;
                for x in 0..fb.width {
                    let p = fb.pixels[row + x];
                    if p == 0xFF000000 || (p >> 24) == 0 {
                        fb.pixels[row + x] = 0xFF14171E;
                        continue;
                    }
                    let r = ((p >> 16) & 0xFF) as u32;
                    let g = ((p >> 8) & 0xFF) as u32;
                    let b = (p & 0xFF) as u32;
                    let gray = (r * 77 + g * 151 + b * 28) >> 8;

                    // 4-tier posterized newspaper gravure ink
                    let ink = if gray < 45 {
                        0x18
                    } else if gray < 110 {
                        0x55
                    } else if gray < 185 {
                        0xAA
                    } else {
                        0xEE
                    };
                    fb.pixels[row + x] = 0xFF000000 | (ink << 16) | (ink << 8) | ink;
                }
            }
        }
        PhotoFilter::VibrantCel => {
            // Retain original multi-material cel-shaded colors, only set background
            for y in 0..fb.height {
                let row = y * fb.width;
                for x in 0..fb.width {
                    let p = fb.pixels[row + x];
                    if p == 0xFF000000 || (p >> 24) == 0 {
                        fb.pixels[row + x] = 0xFF1E242B;
                    }
                }
            }
        }
    }
}

/// Renders a decorative brass newspaper photo frame with corner rivets and caption banner.
pub fn draw_newspaper_photo_frame(
    fb: &mut FrameBuffer,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    caption: &str,
    border_color: u32,
    caption_color: u32,
    draw_rivets: bool,
) {
    // Outer border (2px)
    fb.draw_rect(x, y, w, 2, border_color);
    fb.draw_rect(x, y + h - 2, w, 2, border_color);
    fb.draw_rect(x, y, 2, h, border_color);
    fb.draw_rect(x + w - 2, y, 2, h, border_color);

    // Inner shadow line (1px)
    fb.draw_rect(x + 2, y + 2, w - 4, 1, 0xFF14171E);
    fb.draw_rect(x + 2, y + h - 3, w - 4, 1, 0xFF14171E);
    fb.draw_rect(x + 2, y + 2, 1, h - 4, 0xFF14171E);
    fb.draw_rect(x + w - 3, y + 2, 1, h - 4, 0xFF14171E);

    // Brass corner rivets (2x2)
    if draw_rivets {
        fb.draw_rect(x + 3, y + 3, 2, 2, 0xFFD97706);
        fb.draw_rect(x + w - 5, y + 3, 2, 2, 0xFFD97706);
        fb.draw_rect(x + 3, y + h - 5, 2, 2, 0xFFD97706);
        fb.draw_rect(x + w - 5, y + h - 5, 2, 2, 0xFFD97706);
    }

    // Caption banner if present
    if !caption.is_empty() {
        let cap_h = 16;
        let cap_y = y + h + 2;
        fb.draw_rect(x, cap_y, w, cap_h, 0xFF14171E);
        fb.draw_rect(x, cap_y + cap_h - 1, w, 1, border_color);
        fb.draw_text(x + 4, cap_y + 4, caption, caption_color);
    }
}
