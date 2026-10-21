//! Deterministic discrete player / agent input representation.

/// Discrete input sampled at a specific tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
#[repr(C)]
pub struct InputState {
    /// Integer direction or delta along X axis (-1, 0, 1 or discrete unit)
    pub move_x: i32,
    /// Integer direction or delta along Y axis (-1, 0, 1 or discrete unit)
    pub move_y: i32,
    /// Bitflags for discrete actions/buttons
    pub buttons: u32,
    /// Alignment padding to ensure 16-byte total size
    pub _pad: u32,
}

impl InputState {
    pub const NEUTRAL: Self = Self {
        move_x: 0,
        move_y: 0,
        buttons: 0,
        _pad: 0,
    };

    #[inline(always)]
    pub const fn new(move_x: i32, move_y: i32, buttons: u32) -> Self {
        Self {
            move_x,
            move_y,
            buttons,
            _pad: 0,
        }
    }
}
