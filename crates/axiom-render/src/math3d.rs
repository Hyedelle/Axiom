//! 3D Fixed-Point Mathematics for Deterministic Mecha Rendering.
//!
//! Provides `Vec3Fixed`, `Mat4Fixed`, and deterministic trigonometric lookup tables
//! adhering to Axiom's zero-float constitutional principles.

use axiom_core::Fixed;
use core::ops::{Add, AddAssign, Neg, Sub, SubAssign};

/// Deterministic sine in Q16.16 from a binary angle (0..256 = 0..360°).
#[inline(always)]
pub fn sin_bams(bam: u8) -> Fixed {
    Fixed::from_raw(SINE_LUT[bam as usize])
}

/// Deterministic cosine in Q16.16 from a binary angle (0..256 = 0..360°).
#[inline(always)]
pub fn cos_bams(bam: u8) -> Fixed {
    Fixed::from_raw(SINE_LUT[bam.wrapping_add(64) as usize])
}

// 256-entry exact Sine table in Q16.16 (1.0 = 65536)
#[rustfmt::skip]
pub static SINE_LUT: [i64; 256] = [
         0,   1608,   3216,   4821,   6424,   8022,   9616,  11204,
     12785,  14359,  15924,  17479,  19024,  20557,  22078,  23586,
     25080,  26558,  28020,  29466,  30893,  32303,  33692,  35062,
     36410,  37736,  39040,  40320,  41576,  42806,  44011,  45190,
     46341,  47464,  48559,  49624,  50660,  51665,  52639,  53581,
     54491,  55368,  56212,  57022,  57798,  58538,  59244,  59914,
     60547,  61145,  61705,  62228,  62714,  63162,  63572,  63944,
     64277,  64571,  64827,  65043,  65220,  65358,  65457,  65516,
     65536,  65516,  65457,  65358,  65220,  65043,  64827,  64571,
     64277,  63944,  63572,  63162,  62714,  62228,  61705,  61145,
     60547,  59914,  59244,  58538,  57798,  57022,  56212,  55368,
     54491,  53581,  52639,  51665,  50660,  49624,  48559,  47464,
     46341,  45190,  44011,  42806,  41576,  40320,  39040,  37736,
     36410,  35062,  33692,  32303,  30893,  29466,  28020,  26558,
     25080,  23586,  22078,  20557,  19024,  17479,  15924,  14359,
     12785,  11204,   9616,   8022,   6424,   4821,   3216,   1608,
         0,  -1608,  -3216,  -4821,  -6424,  -8022,  -9616, -11204,
    -12785, -14359, -15924, -17479, -19024, -20557, -22078, -23586,
    -25080, -26558, -28020, -29466, -30893, -32303, -33692, -35062,
    -36410, -37736, -39040, -40320, -41576, -42806, -44011, -45190,
    -46341, -47464, -48559, -49624, -50660, -51665, -52639, -53581,
    -54491, -55368, -56212, -57022, -57798, -58538, -59244, -59914,
    -60547, -61145, -61705, -62228, -62714, -63162, -63572, -63944,
    -64277, -64571, -64827, -65043, -65220, -65358, -65457, -65516,
    -65536, -65516, -65457, -65358, -65220, -65043, -64827, -64571,
    -64277, -63944, -63572, -63162, -62714, -62228, -61705, -61145,
    -60547, -59914, -59244, -58538, -57798, -57022, -56212, -55368,
    -54491, -53581, -52639, -51665, -50660, -49624, -48559, -47464,
    -46341, -45190, -44011, -42806, -41576, -40320, -39040, -37736,
    -36410, -35062, -33692, -32303, -30893, -29466, -28020, -26558,
    -25080, -23586, -22078, -20557, -19024, -17479, -15924, -14359,
    -12785, -11204,  -9616,  -8022,  -6424,  -4821,  -3216,  -1608,
];

/// 3D Vector in Q16.16 fixed-point arithmetic (24 bytes).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
#[repr(C)]
pub struct Vec3Fixed {
    pub x: Fixed,
    pub y: Fixed,
    pub z: Fixed,
}

impl Vec3Fixed {
    pub const ZERO: Self = Self {
        x: Fixed::ZERO,
        y: Fixed::ZERO,
        z: Fixed::ZERO,
    };

    pub const UP: Self = Self {
        x: Fixed::ZERO,
        y: Fixed::ONE,
        z: Fixed::ZERO,
    };

    pub const FORWARD: Self = Self {
        x: Fixed::ZERO,
        y: Fixed::ZERO,
        z: Fixed::ONE,
    };

    #[inline(always)]
    pub const fn new(x: Fixed, y: Fixed, z: Fixed) -> Self {
        Self { x, y, z }
    }

    #[inline(always)]
    pub const fn from_ints(x: i64, y: i64, z: i64) -> Self {
        Self {
            x: Fixed::from_int(x),
            y: Fixed::from_int(y),
            z: Fixed::from_int(z),
        }
    }

    /// Converts raw Q16.16 values to vector.
    #[inline(always)]
    pub const fn from_raw(rx: i64, ry: i64, rz: i64) -> Self {
        Self {
            x: Fixed::from_raw(rx),
            y: Fixed::from_raw(ry),
            z: Fixed::from_raw(rz),
        }
    }

    /// Convenience converter for asset importer (f32 -> Fixed Q16.16).
    #[inline(always)]
    pub fn from_f32(x: f32, y: f32, z: f32) -> Self {
        Self {
            x: Fixed::from_raw((x * 65536.0) as i64),
            y: Fixed::from_raw((y * 65536.0) as i64),
            z: Fixed::from_raw((z * 65536.0) as i64),
        }
    }

    #[inline(always)]
    pub fn dot(self, rhs: Self) -> Fixed {
        (self.x * rhs.x) + (self.y * rhs.y) + (self.z * rhs.z)
    }

    #[inline(always)]
    pub fn cross(self, rhs: Self) -> Self {
        Self {
            x: (self.y * rhs.z) - (self.z * rhs.y),
            y: (self.z * rhs.x) - (self.x * rhs.z),
            z: (self.x * rhs.y) - (self.y * rhs.x),
        }
    }

    #[inline(always)]
    pub fn magnitude_squared(self) -> Fixed {
        self.dot(self)
    }
}

impl Add for Vec3Fixed {
    type Output = Self;
    #[inline(always)]
    fn add(self, rhs: Self) -> Self {
        Self {
            x: self.x + rhs.x,
            y: self.y + rhs.y,
            z: self.z + rhs.z,
        }
    }
}

impl AddAssign for Vec3Fixed {
    #[inline(always)]
    fn add_assign(&mut self, rhs: Self) {
        self.x += rhs.x;
        self.y += rhs.y;
        self.z += rhs.z;
    }
}

impl Sub for Vec3Fixed {
    type Output = Self;
    #[inline(always)]
    fn sub(self, rhs: Self) -> Self {
        Self {
            x: self.x - rhs.x,
            y: self.y - rhs.y,
            z: self.z - rhs.z,
        }
    }
}

impl SubAssign for Vec3Fixed {
    #[inline(always)]
    fn sub_assign(&mut self, rhs: Self) {
        self.x -= rhs.x;
        self.y -= rhs.y;
        self.z -= rhs.z;
    }
}

impl Neg for Vec3Fixed {
    type Output = Self;
    #[inline(always)]
    fn neg(self) -> Self {
        Self {
            x: -self.x,
            y: -self.y,
            z: -self.z,
        }
    }
}

/// 4x4 Row-Major Matrix in Q16.16 Fixed-Point (128 bytes = exactly 2 L1 Cache Lines).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C, align(64))]
pub struct Mat4Fixed {
    pub m: [[Fixed; 4]; 4],
}

impl Default for Mat4Fixed {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Mat4Fixed {
    pub const IDENTITY: Self = Self {
        m: [
            [Fixed::ONE, Fixed::ZERO, Fixed::ZERO, Fixed::ZERO],
            [Fixed::ZERO, Fixed::ONE, Fixed::ZERO, Fixed::ZERO],
            [Fixed::ZERO, Fixed::ZERO, Fixed::ONE, Fixed::ZERO],
            [Fixed::ZERO, Fixed::ZERO, Fixed::ZERO, Fixed::ONE],
        ],
    };

    #[inline(always)]
    pub fn from_translation(v: Vec3Fixed) -> Self {
        let mut mat = Self::IDENTITY;
        mat.m[0][3] = v.x;
        mat.m[1][3] = v.y;
        mat.m[2][3] = v.z;
        mat
    }

    #[inline(always)]
    pub fn from_scale(s: Vec3Fixed) -> Self {
        let mut mat = Self::IDENTITY;
        mat.m[0][0] = s.x;
        mat.m[1][1] = s.y;
        mat.m[2][2] = s.z;
        mat
    }

    /// Rotation around Y-axis by binary angle (0..256 = 0..360 degrees).
    #[inline(always)]
    pub fn from_rotation_y(bam: u8) -> Self {
        let s = sin_bams(bam);
        let c = cos_bams(bam);
        let mut mat = Self::IDENTITY;
        mat.m[0][0] = c;
        mat.m[0][2] = s;
        mat.m[2][0] = -s;
        mat.m[2][2] = c;
        mat
    }

    /// Rotation around X-axis by binary angle.
    #[inline(always)]
    pub fn from_rotation_x(bam: u8) -> Self {
        let s = sin_bams(bam);
        let c = cos_bams(bam);
        let mut mat = Self::IDENTITY;
        mat.m[1][1] = c;
        mat.m[1][2] = -s;
        mat.m[2][1] = s;
        mat.m[2][2] = c;
        mat
    }

    /// Rotation around Z-axis by binary angle.
    #[inline(always)]
    pub fn from_rotation_z(bam: u8) -> Self {
        let s = sin_bams(bam);
        let c = cos_bams(bam);
        let mut mat = Self::IDENTITY;
        mat.m[0][0] = c;
        mat.m[0][1] = -s;
        mat.m[1][0] = s;
        mat.m[1][1] = c;
        mat
    }

    /// Matrix multiplication: result = self * rhs.
    #[inline(always)]
    pub fn mul_mat(&self, rhs: &Self) -> Self {
        let mut out = Self::default();
        for r in 0..4 {
            for c in 0..4 {
                let mut sum = Fixed::ZERO;
                for k in 0..4 {
                    sum += self.m[r][k] * rhs.m[k][c];
                }
                out.m[r][c] = sum;
            }
        }
        out
    }

    /// Transforms a 3D position vector: (p.x, p.y, p.z, 1.0).
    #[inline(always)]
    pub fn transform_point(&self, p: Vec3Fixed) -> Vec3Fixed {
        let x = self.m[0][0] * p.x + self.m[0][1] * p.y + self.m[0][2] * p.z + self.m[0][3];
        let y = self.m[1][0] * p.x + self.m[1][1] * p.y + self.m[1][2] * p.z + self.m[1][3];
        let z = self.m[2][0] * p.x + self.m[2][1] * p.y + self.m[2][2] * p.z + self.m[2][3];
        Vec3Fixed { x, y, z }
    }

    /// Transforms a 3D direction/normal vector (ignores translation): (v.x, v.y, v.z, 0.0).
    #[inline(always)]
    pub fn transform_vector(&self, v: Vec3Fixed) -> Vec3Fixed {
        let x = self.m[0][0] * v.x + self.m[0][1] * v.y + self.m[0][2] * v.z;
        let y = self.m[1][0] * v.x + self.m[1][1] * v.y + self.m[1][2] * v.z;
        let z = self.m[2][0] * v.x + self.m[2][1] * v.y + self.m[2][2] * v.z;
        Vec3Fixed { x, y, z }
    }
}
