//! 0-Float L1-Cache Friendly Texture Mapping and Integer UV Sampler.
//!
//! Provides deterministic integer-based texture buffers with nearest-neighbor
//! and bilinear filtering, wrapping modes (Repeat, Clamp), and color modulation.

/// Standard decal templates for mecha custom detailing.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecalKind {
    Star = 0,
    HazardStripes = 1,
    SkullCrossbones = 2,
    Number01 = 3,
    SponsorAxiom = 4,
    TargetReticle = 5,
    CheckeredFlag = 6,
}

impl DecalKind {
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Star => "Star Badge",
            Self::HazardStripes => "Hazard Stripes",
            Self::SkullCrossbones => "Skull Stencil",
            Self::Number01 => "Racing 01",
            Self::SponsorAxiom => "AXIOM Sponsor",
            Self::TargetReticle => "Target Reticle",
            Self::CheckeredFlag => "Checkered Flag",
        }
    }
}

/// Texture wrap mode for UV coordinates outside [0..65536].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextureWrapMode {
    #[default]
    Repeat,
    Clamp,
}

/// 32-bit ARGB texture map with power-of-two dimensions for bitwise coordinate wrapping.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextureMap {
    pub width: u32,
    pub height: u32,
    width_mask: u32,
    height_mask: u32,
    width_shift: u32,
    pub pixels: Vec<u32>,
}

impl TextureMap {
    /// Creates a new texture buffer with power-of-two dimensions (e.g. 32, 64, 128, 256).
    /// If non-power-of-two is passed, dimensions are rounded down to nearest power-of-two (min 1).
    pub fn new(mut width: u32, mut height: u32) -> Self {
        width = width.next_power_of_two();
        if width == 0 {
            width = 1;
        }
        height = height.next_power_of_two();
        if height == 0 {
            height = 1;
        }

        let width_mask = width - 1;
        let height_mask = height - 1;
        let width_shift = width.trailing_zeros();
        let total_pixels = (width * height) as usize;

        Self {
            width,
            height,
            width_mask,
            height_mask,
            width_shift,
            pixels: vec![0xFFFFFFFF; total_pixels],
        }
    }

    /// Creates a texture initialized with existing pixel data.
    pub fn from_pixels(width: u32, height: u32, pixels: Vec<u32>) -> Option<Self> {
        if !width.is_power_of_two() || !height.is_power_of_two() {
            return None;
        }
        if pixels.len() != (width * height) as usize {
            return None;
        }

        let width_mask = width - 1;
        let height_mask = height - 1;
        let width_shift = width.trailing_zeros();

        Some(Self {
            width,
            height,
            width_mask,
            height_mask,
            width_shift,
            pixels,
        })
    }

    /// Generates a procedural hazard stripes texture (black and canon yellow-amber).
    pub fn procedural_hazard_stripes(size: u32) -> Self {
        let mut tex = Self::new(size, size);
        for y in 0..tex.height {
            for x in 0..tex.width {
                let stripe = ((x + y) / 8) % 2 == 0;
                let color = if stripe { 0xFFFD9F07 } else { 0xFF141923 };
                tex.set_pixel(x, y, color);
            }
        }
        tex
    }

    /// Generates a procedural industrial metal plate texture with rivets and seams.
    pub fn procedural_metal_plating(size: u32) -> Self {
        let mut tex = Self::new(size, size);
        for y in 0..tex.height {
            for x in 0..tex.width {
                let border = x == 0 || y == 0 || x == tex.width - 1 || y == tex.height - 1;
                let rivet = (x == 4 || x == tex.width - 5) && (y == 4 || y == tex.height - 5);
                let color = if rivet {
                    0xFFD97706 // Brass rivet
                } else if border {
                    0xFF2B222A // Manga ink seam
                } else {
                    0xFF5A5148 // Worn iron base
                };
                tex.set_pixel(x, y, color);
            }
        }
        tex
    }

    /// Generates a military camouflage texture adhering to canon olive/ink tones.
    pub fn procedural_camo(size: u32) -> Self {
        let mut tex = Self::new(size, size);
        for y in 0..tex.height {
            for x in 0..tex.width {
                let hash = ((x * 37 + y * 73) ^ ((x >> 2) * 19 + (y >> 2) * 53)) & 0xFF;
                let color = if hash < 70 {
                    0xFF7F772B // Olive
                } else if hash < 140 {
                    0xFF5A5148 // Iron
                } else if hash < 200 {
                    0xFF3D3730 // Dark mud
                } else {
                    0xFF2B222A // Ink
                };
                tex.set_pixel(x, y, color);
            }
        }
        tex
    }

    #[inline(always)]
    pub fn get_pixel(&self, x: u32, y: u32) -> u32 {
        let idx = ((y & self.height_mask) << self.width_shift) | (x & self.width_mask);
        self.pixels[idx as usize]
    }

    #[inline(always)]
    pub fn set_pixel(&mut self, x: u32, y: u32, color: u32) {
        let idx = ((y & self.height_mask) << self.width_shift) | (x & self.width_mask);
        self.pixels[idx as usize] = color;
    }

    /// Samples texture at fixed-point UV coordinates (where 0 = 0.0, 65536 = 1.0).
    /// Pure integer, zero floating point operations.
    #[inline(always)]
    pub fn sample_nearest(&self, u: i32, v: i32, wrap: TextureWrapMode) -> u32 {
        let (u_norm, v_norm) = match wrap {
            TextureWrapMode::Repeat => (
                u.rem_euclid(65536),
                v.rem_euclid(65536),
            ),
            TextureWrapMode::Clamp => (
                u.clamp(0, 65535),
                v.clamp(0, 65535),
            ),
        };

        let tx = ((u_norm as u64 * self.width as u64) >> 16) as u32;
        let ty = ((v_norm as u64 * self.height as u64) >> 16) as u32;

        self.get_pixel(tx, ty)
    }

    /// Samples texture with integer bilinear filtering.
    #[inline]
    pub fn sample_bilinear(&self, u: i32, v: i32, wrap: TextureWrapMode) -> u32 {
        let (u_norm, v_norm) = match wrap {
            TextureWrapMode::Repeat => (
                u.rem_euclid(65536),
                v.rem_euclid(65536),
            ),
            TextureWrapMode::Clamp => (
                u.clamp(0, 65535),
                v.clamp(0, 65535),
            ),
        };

        // Scaled coordinates in 8.8 fixed point relative to pixel grid
        let fx = (u_norm as u64 * self.width as u64) >> 8; // width * 256
        let fy = (v_norm as u64 * self.height as u64) >> 8;

        let x0 = (fx >> 8) as u32;
        let y0 = (fy >> 8) as u32;
        let x1 = (x0 + 1) & self.width_mask;
        let y1 = (y0 + 1) & self.height_mask;

        let wx = (fx & 0xFF) as u32;
        let wy = (fy & 0xFF) as u32;
        let inv_wx = 256 - wx;
        let inv_wy = 256 - wy;

        let c00 = self.get_pixel(x0, y0);
        let c10 = self.get_pixel(x1, y0);
        let c01 = self.get_pixel(x0, y1);
        let c11 = self.get_pixel(x1, y1);

        blend_4_colors(c00, c10, c01, c11, inv_wx, wx, inv_wy, wy)
    }

    /// Clears the entire texture buffer with a solid ARGB color.
    pub fn clear(&mut self, color: u32) {
        self.pixels.fill(color);
    }

    /// Paints a circular brush stroke at (cx, cy) with integer radius and alpha opacity.
    pub fn paint_brush(&mut self, cx: i32, cy: i32, radius: i32, color: u32, opacity: u8) {
        let r_sq = radius * radius;
        let x_start = (cx - radius).max(0);
        let x_end = (cx + radius).min(self.width as i32 - 1);
        let y_start = (cy - radius).max(0);
        let y_end = (cy + radius).min(self.height as i32 - 1);

        for y in y_start..=y_end {
            let dy = y - cy;
            for x in x_start..=x_end {
                let dx = x - cx;
                if dx * dx + dy * dy <= r_sq {
                    let base = self.get_pixel(x as u32, y as u32);
                    let blended = blend_alpha(base, color, opacity);
                    self.set_pixel(x as u32, y as u32, blended);
                }
            }
        }
    }

    /// Draws a line using Bresenham algorithm with brush thickness.
    pub fn draw_line(&mut self, mut x0: i32, mut y0: i32, x1: i32, y1: i32, color: u32, radius: i32, opacity: u8) {
        let dx = (x1 - x0).abs();
        let sx = if x0 < x1 { 1 } else { -1 };
        let dy = -(y1 - y0).abs();
        let sy = if y0 < y1 { 1 } else { -1 };
        let mut err = dx + dy;

        loop {
            self.paint_brush(x0, y0, radius, color, opacity);
            if x0 == x1 && y0 == y1 {
                break;
            }
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                x0 += sx;
            }
            if e2 <= dx {
                err += dx;
                y0 += sy;
            }
        }
    }

    /// Performs deterministic scanline flood fill bounded to texture dimensions.
    pub fn flood_fill(&mut self, start_x: u32, start_y: u32, fill_color: u32) {
        if start_x >= self.width || start_y >= self.height {
            return;
        }
        let target_color = self.get_pixel(start_x, start_y);
        if target_color == fill_color {
            return;
        }

        let mut stack = Vec::with_capacity(512);
        self.set_pixel(start_x, start_y, fill_color);
        stack.push((start_x, start_y));

        while let Some((x, y)) = stack.pop() {
            if x > 0 && self.get_pixel(x - 1, y) == target_color {
                self.set_pixel(x - 1, y, fill_color);
                stack.push((x - 1, y));
            }
            if x + 1 < self.width && self.get_pixel(x + 1, y) == target_color {
                self.set_pixel(x + 1, y, fill_color);
                stack.push((x + 1, y));
            }
            if y > 0 && self.get_pixel(x, y - 1) == target_color {
                self.set_pixel(x, y - 1, fill_color);
                stack.push((x, y - 1));
            }
            if y + 1 < self.height && self.get_pixel(x, y + 1) == target_color {
                self.set_pixel(x, y + 1, fill_color);
                stack.push((x, y + 1));
            }
        }
    }

    /// Stamps an alpha-blended decal texture onto this texture map with fixed-point scale (256 = 1.0x).
    pub fn stamp_decal(&mut self, decal: &TextureMap, center_x: i32, center_y: i32, scale_q8: u32) {
        if decal.width == 0 || decal.height == 0 || scale_q8 == 0 {
            return;
        }

        let scaled_w = ((decal.width as u64 * scale_q8 as u64) >> 8) as i32;
        let scaled_h = ((decal.height as u64 * scale_q8 as u64) >> 8) as i32;
        let half_w = scaled_w / 2;
        let half_h = scaled_h / 2;

        let x_start = (center_x - half_w).max(0);
        let x_end = (center_x + half_w).min(self.width as i32);
        let y_start = (center_y - half_h).max(0);
        let y_end = (center_y + half_h).min(self.height as i32);

        for dst_y in y_start..y_end {
            let rel_y = dst_y - (center_y - half_h);
            let v = ((rel_y as i64 * 65536) / scaled_h.max(1) as i64) as i32;

            for dst_x in x_start..x_end {
                let rel_x = dst_x - (center_x - half_w);
                let u = ((rel_x as i64 * 65536) / scaled_w.max(1) as i64) as i32;

                let decal_pixel = decal.sample_nearest(u, v, TextureWrapMode::Clamp);
                let base_pixel = self.get_pixel(dst_x as u32, dst_y as u32);
                let blended = blend_decal(base_pixel, decal_pixel);
                self.set_pixel(dst_x as u32, dst_y as u32, blended);
            }
        }
    }

    /// Generates a procedural decal stamp texture of the requested kind.
    pub fn generate_decal(kind: DecalKind, size: u32) -> Self {
        let size = size.clamp(16, 256).next_power_of_two();
        let mut decal = Self::new(size, size);
        decal.clear(0x00000000); // Fully transparent
        let s = size as i32;
        let half = s / 2;

        match kind {
            DecalKind::Star => {
                for y in 0..s {
                    for x in 0..s {
                        let dx = (x - half).abs();
                        let dy = (y - half).abs();
                        let dist_manhattan = dx + dy;
                        if dist_manhattan <= half * 3 / 4
                            && (dx <= half / 4 || dy <= half / 4 || dist_manhattan <= half / 2)
                        {
                            decal.set_pixel(x as u32, y as u32, 0xFFFBBF24); // Gold Star
                        }
                    }
                }
            }
            DecalKind::HazardStripes => {
                let tile = (s / 8).max(1);
                for y in 0..s {
                    for x in 0..s {
                        let stripe = ((x + y) / tile) % 2 == 0;
                        let col = if stripe { 0xFFF59E0B } else { 0xFF1E293B };
                        decal.set_pixel(x as u32, y as u32, col);
                    }
                }
            }
            DecalKind::SkullCrossbones => {
                for y in 0..s {
                    for x in 0..s {
                        let dx = x - half;
                        let dy = y - (half - s / 8);
                        // Skull cranium
                        if dx * dx + dy * dy <= (s / 3) * (s / 3) {
                            decal.set_pixel(x as u32, y as u32, 0xFFE2E8F0);
                        }
                        // Eye sockets (cutout)
                        let eye_dx1 = x - (half - s / 7);
                        let eye_dx2 = x - (half + s / 7);
                        let eye_dy = y - (half - s / 10);
                        if eye_dx1 * eye_dx1 + eye_dy * eye_dy <= (s / 12) * (s / 12)
                            || eye_dx2 * eye_dx2 + eye_dy * eye_dy <= (s / 12) * (s / 12)
                        {
                            decal.set_pixel(x as u32, y as u32, 0x00000000);
                        }
                        // Teeth / chin
                        let bx = (x - half).abs();
                        let by = y - (half + s / 6);
                        if by >= 0 && by < s / 6 && bx < s / 4 {
                            decal.set_pixel(x as u32, y as u32, 0xFFCBD5E1);
                        }
                    }
                }
            }
            DecalKind::Number01 => {
                for y in (s / 5)..(s * 4 / 5) {
                    for x in (s / 6)..(s * 5 / 6) {
                        let is_zero = x < half - 2
                            && (x == s / 6 || x == half - 4 || y == s / 5 || y == s * 4 / 5 - 1);
                        let is_one = x >= half + 4 && x <= half + 8;
                        if is_zero || is_one {
                            decal.set_pixel(x as u32, y as u32, 0xFF38BDF8); // Cyan
                        }
                    }
                }
            }
            DecalKind::SponsorAxiom => {
                let bar_h = s / 3;
                let bar_y0 = half - bar_h / 2;
                for y in bar_y0..(bar_y0 + bar_h) {
                    for x in (s / 8)..(s * 7 / 8) {
                        decal.set_pixel(x as u32, y as u32, 0xFFEA580C); // Axiom Fire Orange
                    }
                }
                for y in (bar_y0 + 2)..(bar_y0 + bar_h - 2) {
                    for x in (s / 8 + 2)..(s * 7 / 8 - 2) {
                        decal.set_pixel(x as u32, y as u32, 0xFFFFFFFF);
                    }
                }
            }
            DecalKind::TargetReticle => {
                let r_outer = (s * 3) / 8;
                let r_inner = s / 5;
                for y in 0..s {
                    for x in 0..s {
                        let dx = x - half;
                        let dy = y - half;
                        let dist_sq = dx * dx + dy * dy;
                        let is_outer_ring = dist_sq >= (r_outer - 2) * (r_outer - 2) && dist_sq <= r_outer * r_outer;
                        let is_inner_ring = dist_sq >= (r_inner - 2) * (r_inner - 2) && dist_sq <= r_inner * r_inner;
                        let is_crosshair = (dx.abs() <= 1 && dy.abs() <= r_outer + 4)
                            || (dy.abs() <= 1 && dx.abs() <= r_outer + 4);
                        if is_outer_ring || is_inner_ring || is_crosshair {
                            decal.set_pixel(x as u32, y as u32, 0xFFEF4444); // Laser Red
                        }
                    }
                }
            }
            DecalKind::CheckeredFlag => {
                let tile = (s / 4).max(1);
                for y in 0..s {
                    for x in 0..s {
                        let is_black = ((x / tile) + (y / tile)) % 2 == 0;
                        let col = if is_black { 0xFF0F172A } else { 0xFFF8FAFC };
                        decal.set_pixel(x as u32, y as u32, col);
                    }
                }
            }
        }

        decal
    }
}

/// Blends overlay color onto base color with integer opacity (0..255).
#[inline(always)]
pub fn blend_alpha(base: u32, overlay: u32, opacity: u8) -> u32 {
    let overlay_a = (((overlay >> 24) & 0xFF) * opacity as u32) / 255;
    if overlay_a == 0 {
        return base;
    }
    if overlay_a >= 255 {
        return overlay | 0xFF000000;
    }
    let inv_a = 255 - overlay_a;
    let r = (((overlay >> 16) & 0xFF) * overlay_a + ((base >> 16) & 0xFF) * inv_a) / 255;
    let g = (((overlay >> 8) & 0xFF) * overlay_a + ((base >> 8) & 0xFF) * inv_a) / 255;
    let b = ((overlay & 0xFF) * overlay_a + (base & 0xFF) * inv_a) / 255;
    0xFF000000 | (r << 16) | (g << 8) | b
}

/// Blends 4 colors with 8-bit bilinear weights (sum = 256 * 256 = 65536).
#[inline(always)]
fn blend_4_colors(
    c00: u32,
    c10: u32,
    c01: u32,
    c11: u32,
    iwx: u32,
    wx: u32,
    iwy: u32,
    wy: u32,
) -> u32 {
    let w00 = iwx * iwy;
    let w10 = wx * iwy;
    let w01 = iwx * wy;
    let w11 = wx * wy;

    let a = (((c00 >> 24) & 0xFF) * w00
        + ((c10 >> 24) & 0xFF) * w10
        + ((c01 >> 24) & 0xFF) * w01
        + ((c11 >> 24) & 0xFF) * w11)
        >> 16;

    let r = (((c00 >> 16) & 0xFF) * w00
        + ((c10 >> 16) & 0xFF) * w10
        + ((c01 >> 16) & 0xFF) * w01
        + ((c11 >> 16) & 0xFF) * w11)
        >> 16;

    let g = (((c00 >> 8) & 0xFF) * w00
        + ((c10 >> 8) & 0xFF) * w10
        + ((c01 >> 8) & 0xFF) * w01
        + ((c11 >> 8) & 0xFF) * w11)
        >> 16;

    let b = ((c00 & 0xFF) * w00
        + (c10 & 0xFF) * w10
        + (c01 & 0xFF) * w01
        + (c11 & 0xFF) * w11)
        >> 16;

    (a << 24) | (r << 16) | (g << 8) | b
}

/// Modulates texture color with cel-shading light multiplier (0..256 where 256 = 1.0).
#[inline(always)]
pub fn modulate_color(tex_color: u32, light_multiplier: u32) -> u32 {
    let a = (tex_color >> 24) & 0xFF;
    let r = ((((tex_color >> 16) & 0xFF) * light_multiplier + 128) >> 8).min(255);
    let g = ((((tex_color >> 8) & 0xFF) * light_multiplier + 128) >> 8).min(255);
    let b = (((tex_color & 0xFF) * light_multiplier + 128) >> 8).min(255);

    (a << 24) | (r << 16) | (g << 8) | b
}

/// Blends texture decal color onto a base cel-shading color using texture alpha (0..255).
#[inline(always)]
pub fn blend_decal(base_color: u32, tex_color: u32) -> u32 {
    let alpha = (tex_color >> 24) & 0xFF;
    if alpha == 0 {
        return base_color;
    }
    if alpha >= 255 {
        return tex_color;
    }

    let inv_alpha = 255 - alpha;

    let r = (((tex_color >> 16) & 0xFF) * alpha + ((base_color >> 16) & 0xFF) * inv_alpha) / 255;
    let g = (((tex_color >> 8) & 0xFF) * alpha + ((base_color >> 8) & 0xFF) * inv_alpha) / 255;
    let b = ((tex_color & 0xFF) * alpha + (base_color & 0xFF) * inv_alpha) / 255;

    0xFF000000 | (r << 16) | (g << 8) | b
}
