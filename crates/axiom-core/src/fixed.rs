//! Fixed-point arithmetic module for zero-floating-point determinism.

use core::ops::{Add, AddAssign, Sub, SubAssign, Mul, MulAssign, Div, DivAssign, Neg};

/// 32.16 fixed-point number (16 bits of fractional precision).
/// 1.0 = 65,536 (1 << 16)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Hash)]
#[repr(transparent)]
pub struct Fixed(pub i64);

impl Fixed {
    pub const FRACTIONAL_BITS: u32 = 16;
    pub const ONE: Self = Self(1 << Self::FRACTIONAL_BITS);
    pub const ZERO: Self = Self(0);

    #[inline(always)]
    pub const fn from_int(v: i64) -> Self {
        Self(v << Self::FRACTIONAL_BITS)
    }

    #[inline(always)]
    pub const fn to_int(self) -> i64 {
        self.0 >> Self::FRACTIONAL_BITS
    }

    #[inline(always)]
    pub const fn from_raw(raw: i64) -> Self {
        Self(raw)
    }

    #[inline(always)]
    pub const fn raw(self) -> i64 {
        self.0
    }
}

impl Add for Fixed {
    type Output = Self;
    #[inline(always)]
    fn add(self, rhs: Self) -> Self {
        Self(self.0 + rhs.0)
    }
}

impl AddAssign for Fixed {
    #[inline(always)]
    fn add_assign(&mut self, rhs: Self) {
        self.0 += rhs.0;
    }
}

impl Sub for Fixed {
    type Output = Self;
    #[inline(always)]
    fn sub(self, rhs: Self) -> Self {
        Self(self.0 - rhs.0)
    }
}

impl SubAssign for Fixed {
    #[inline(always)]
    fn sub_assign(&mut self, rhs: Self) {
        self.0 -= rhs.0;
    }
}

impl Mul for Fixed {
    type Output = Self;
    #[inline(always)]
    fn mul(self, rhs: Self) -> Self {
        Self((self.0 * rhs.0) >> Self::FRACTIONAL_BITS)
    }
}

impl MulAssign for Fixed {
    #[inline(always)]
    fn mul_assign(&mut self, rhs: Self) {
        self.0 = (self.0 * rhs.0) >> Self::FRACTIONAL_BITS;
    }
}

impl Div for Fixed {
    type Output = Self;
    #[inline(always)]
    fn div(self, rhs: Self) -> Self {
        Self((self.0 << Self::FRACTIONAL_BITS) / rhs.0)
    }
}

impl DivAssign for Fixed {
    #[inline(always)]
    fn div_assign(&mut self, rhs: Self) {
        self.0 = (self.0 << Self::FRACTIONAL_BITS) / rhs.0;
    }
}

impl Neg for Fixed {
    type Output = Self;
    #[inline(always)]
    fn neg(self) -> Self {
        Self(-self.0)
    }
}

/// 2D Fixed-point Vector (16 bytes, 8-byte aligned)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
#[repr(C)]
pub struct FixedVec2 {
    pub x: Fixed,
    pub y: Fixed,
}

impl FixedVec2 {
    pub const ZERO: Self = Self { x: Fixed::ZERO, y: Fixed::ZERO };

    #[inline(always)]
    pub const fn new(x: Fixed, y: Fixed) -> Self {
        Self { x, y }
    }

    #[inline(always)]
    pub const fn from_ints(x: i64, y: i64) -> Self {
        Self {
            x: Fixed::from_int(x),
            y: Fixed::from_int(y),
        }
    }
}

impl Add for FixedVec2 {
    type Output = Self;
    #[inline(always)]
    fn add(self, rhs: Self) -> Self {
        Self {
            x: self.x + rhs.x,
            y: self.y + rhs.y,
        }
    }
}

impl AddAssign for FixedVec2 {
    #[inline(always)]
    fn add_assign(&mut self, rhs: Self) {
        self.x += rhs.x;
        self.y += rhs.y;
    }
}

impl Sub for FixedVec2 {
    type Output = Self;
    #[inline(always)]
    fn sub(self, rhs: Self) -> Self {
        Self {
            x: self.x - rhs.x,
            y: self.y - rhs.y,
        }
    }
}

impl SubAssign for FixedVec2 {
    #[inline(always)]
    fn sub_assign(&mut self, rhs: Self) {
        self.x -= rhs.x;
        self.y -= rhs.y;
    }
}
