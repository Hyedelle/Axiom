//! Cache-line resident Entity structure.

use crate::fixed::FixedVec2;

/// A 64-byte cache-line aligned entity record.
///
/// Designed to fit entirely inside a single L1 data cache line without crossing boundaries,
/// eliminating cache line splits during batch iteration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(C, align(64))]
pub struct Entity {
    /// Unique entity identifier (0 = inactive / dead)
    pub id: u64,
    /// 2D position in fixed-point coordinates (16 bytes)
    pub pos: FixedVec2,
    /// 2D velocity in fixed-point coordinates per tick (16 bytes)
    pub vel: FixedVec2,
    /// Status flags (active, collision, layer) (8 bytes)
    pub flags: u64,
    /// User/Agent custom payload or tag (8 bytes)
    pub user_data: u64,
    /// Explicit padding to ensure exact 64-byte size (8 bytes)
    pub _reserved: [u8; 8],
}

impl Default for Entity {
    fn default() -> Self {
        Self {
            id: 0,
            pos: FixedVec2::ZERO,
            vel: FixedVec2::ZERO,
            flags: 0,
            user_data: 0,
            _reserved: [0; 8],
        }
    }
}

impl Entity {
    pub const fn empty() -> Self {
        Self {
            id: 0,
            pos: FixedVec2::ZERO,
            vel: FixedVec2::ZERO,
            flags: 0,
            user_data: 0,
            _reserved: [0; 8],
        }
    }

    #[inline(always)]
    pub fn is_active(&self) -> bool {
        self.id != 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::{align_of, size_of};

    #[test]
    fn test_entity_cache_line_geometry() {
        assert_eq!(size_of::<Entity>(), 64, "Entity must be exactly 64 bytes");
        assert_eq!(
            align_of::<Entity>(),
            64,
            "Entity must be aligned to 64 bytes"
        );
    }
}
