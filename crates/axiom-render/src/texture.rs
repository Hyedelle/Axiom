//! 0-Float L1-Cache Friendly Texture Mapping and Integer UV Sampler.
//!
//! Provides deterministic integer-based texture buffers with nearest-neighbor
//! and bilinear filtering, wrapping modes (Repeat, Clamp), and color modulation.

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
