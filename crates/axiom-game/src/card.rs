//! Pure integer tactical card draft and roguelite deck state (Track 2: Mecha Boxing).
//!
//! Fully deterministic, zero floating-point, 64-byte L1 cache-aligned POD structures.

use core::fmt;

/// Tactical Card classification.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardKind {
    Straight = 0,
    Guard = 1,
    OverheatHook = 2,
    CounterSlip = 3,
    PlasmaUpper = 4,
    IronBastion = 5,
}

impl CardKind {
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Straight => "STRAIGHT",
            Self::Guard => "TITAN GUARD",
            Self::OverheatHook => "OVERHEAT HOOK",
            Self::CounterSlip => "COUNTER SLIP",
            Self::PlasmaUpper => "PLASMA UPPER",
            Self::IronBastion => "IRON BASTION",
        }
    }
}

/// 16-byte POD Tactical Card representation.
#[repr(C, align(16))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TacticalCard {
    pub id: u16,
    pub power: u16,
    pub heat: u16,
    pub guard: u16,
    pub kind: CardKind,
    pub fin_kind: u8,
    pub _pad: [u8; 6],
}

impl TacticalCard {
    pub const fn new(id: u16, kind: CardKind, power: u16, heat: u16, guard: u16, fin_kind: u8) -> Self {
        Self {
            id,
            power,
            heat,
            guard,
            kind,
            fin_kind,
            _pad: [0; 6],
        }
    }

    pub const fn straight() -> Self {
        Self::new(1, CardKind::Straight, 15, 10, 0, 0)
    }

    pub const fn guard_card() -> Self {
        Self::new(2, CardKind::Guard, 0, 0, 20, 1)
    }

    pub const fn overheat_hook() -> Self {
        Self::new(3, CardKind::OverheatHook, 38, 35, 0, 2)
    }

    pub const fn counter_slip() -> Self {
        Self::new(4, CardKind::CounterSlip, 20, 15, 10, 3)
    }

    pub const fn plasma_upper() -> Self {
        Self::new(5, CardKind::PlasmaUpper, 45, 50, 0, 4)
    }

    pub const fn iron_bastion() -> Self {
        Self::new(6, CardKind::IronBastion, 0, 0, 35, 1)
    }
}

/// 64-byte aligned Deck State holding active player cards.
#[repr(C, align(64))]
#[derive(Clone, Copy)]
pub struct DeckState<const CAP: usize = 16> {
    pub cards: [TacticalCard; CAP],
    pub count: usize,
    pub active_card_idx: usize,
    pub draft_choices: [u8; 3],
    pub _pad: [u8; 32],
}

impl<const CAP: usize> DeckState<CAP> {
    pub fn new() -> Self {
        let mut s = Self {
            cards: [TacticalCard::straight(); CAP],
            count: 0,
            active_card_idx: 0,
            draft_choices: [0, 1, 2],
            _pad: [0; 32],
        };
        s.add_card(TacticalCard::straight());
        s.add_card(TacticalCard::guard_card());
        s.add_card(TacticalCard::overheat_hook());
        s
    }

    pub fn add_card(&mut self, card: TacticalCard) -> bool {
        if self.count < CAP {
            self.cards[self.count] = card;
            self.count += 1;
            true
        } else {
            false
        }
    }

    pub fn active_card(&self) -> TacticalCard {
        if self.count == 0 {
            TacticalCard::straight()
        } else {
            self.cards[self.active_card_idx % self.count]
        }
    }

    pub fn select_active(&mut self, idx: usize) {
        if idx < self.count {
            self.active_card_idx = idx;
        }
    }
}

impl<const CAP: usize> Default for DeckState<CAP> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const CAP: usize> fmt::Debug for DeckState<CAP> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DeckState")
            .field("count", &self.count)
            .field("active_card_idx", &self.active_card_idx)
            .field("active_card", &self.active_card())
            .finish()
    }
}
