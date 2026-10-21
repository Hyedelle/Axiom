//! Tick definition and simulation timing constants.

use core::ops::{Add, AddAssign, Sub, SubAssign};

/// Monotonically increasing discrete simulation step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Hash)]
#[repr(transparent)]
pub struct Tick(pub u64);

/// Target simulation tick rate (60 Hz).
pub const TICK_RATE_HZ: u32 = 60;

/// Microseconds per tick at 60 Hz (16,666 µs).
pub const DELTA_TIME_MICROS: u64 = 1_000_000 / (TICK_RATE_HZ as u64);

impl Tick {
    pub const ZERO: Self = Self(0);

    #[inline(always)]
    pub const fn next(self) -> Self {
        Self(self.0 + 1)
    }

    #[inline(always)]
    pub const fn prev(self) -> Option<Self> {
        if self.0 > 0 {
            Some(Self(self.0 - 1))
        } else {
            None
        }
    }
}

impl Add<u64> for Tick {
    type Output = Self;
    #[inline(always)]
    fn add(self, rhs: u64) -> Self {
        Self(self.0 + rhs)
    }
}

impl AddAssign<u64> for Tick {
    #[inline(always)]
    fn add_assign(&mut self, rhs: u64) {
        self.0 += rhs;
    }
}

impl Sub<u64> for Tick {
    type Output = Self;
    #[inline(always)]
    fn sub(self, rhs: u64) -> Self {
        Self(self.0.saturating_sub(rhs))
    }
}

impl SubAssign<u64> for Tick {
    #[inline(always)]
    fn sub_assign(&mut self, rhs: u64) {
        self.0 = self.0.saturating_sub(rhs);
    }
}
