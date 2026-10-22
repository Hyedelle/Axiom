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
    CryoCooler = 6,
    TitanPlating = 7,
    OverdriveCore = 8,
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
            Self::CryoCooler => "CRYO VALVE",
            Self::TitanPlating => "NANO ARMOR",
            Self::OverdriveCore => "OVERDRIVE",
        }
    }

    pub const fn description(&self) -> &'static str {
        match self {
            Self::Straight => "QUICK 1-2 PUNCH [PWR:15 HEAT:10]",
            Self::Guard => "TITANIUM SHIELD [ABSORB:80%]",
            Self::OverheatHook => "MASSIVE HOOK [PWR:38 HEAT:35]",
            Self::CounterSlip => "COUNTER WEAVE [150% CRIT DMG]",
            Self::PlasmaUpper => "PLASMA CANNON [PWR:45 HEAT:50]",
            Self::IronBastion => "HEAVY FORTRESS [ABSORB:90%]",
            Self::CryoCooler => "COOLING SPEED x2 [HEAT RECOVERY]",
            Self::TitanPlating => "MAX HP +30 & IMMEDIATE REPAIR",
            Self::OverdriveCore => "OVERCLOCK [ALL DMG +15]",
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

    pub const fn cryo_cooler() -> Self {
        Self::new(7, CardKind::CryoCooler, 0, 0, 0, 0)
    }

    pub const fn titan_plating() -> Self {
        Self::new(8, CardKind::TitanPlating, 0, 0, 15, 0)
    }

    pub const fn overdrive_core() -> Self {
        Self::new(9, CardKind::OverdriveCore, 15, 0, 0, 0)
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

    /// Deterministically rolls 3 distinct card choices from a 9-card pool based on RNG seed.
    pub fn roll_draft_choices(&mut self, seed: u64) -> [TacticalCard; 3] {
        let pool = [
            TacticalCard::straight(),
            TacticalCard::guard_card(),
            TacticalCard::overheat_hook(),
            TacticalCard::counter_slip(),
            TacticalCard::plasma_upper(),
            TacticalCard::iron_bastion(),
            TacticalCard::cryo_cooler(),
            TacticalCard::titan_plating(),
            TacticalCard::overdrive_core(),
        ];
        let n = pool.len() as u64;
        let c0 = (seed.wrapping_mul(6364136223846793005) >> 32) % n;
        let c1 = ((seed.wrapping_add(1)).wrapping_mul(1442695040888963407) >> 32) % (n - 1);
        let c1_idx = if c1 >= c0 { c1 + 1 } else { c1 };
        let c2 = ((seed.wrapping_add(2)).wrapping_mul(2862933555777941757) >> 32) % (n - 2);
        let mut idx2 = 0;
        let mut skipped = 0;
        for i in 0..n {
            if i != c0 && i != c1_idx {
                if skipped == c2 {
                    idx2 = i;
                    break;
                }
                skipped += 1;
            }
        }
        self.draft_choices = [c0 as u8, c1_idx as u8, idx2 as u8];
        [pool[c0 as usize], pool[c1_idx as usize], pool[idx2 as usize]]
    }

    /// Adds the chosen card (index 0, 1, or 2) to the active deck and returns it.
    pub fn pick_draft_choice(&mut self, choice_idx: usize) -> Option<TacticalCard> {
        if choice_idx >= 3 {
            return None;
        }
        let pool = [
            TacticalCard::straight(),
            TacticalCard::guard_card(),
            TacticalCard::overheat_hook(),
            TacticalCard::counter_slip(),
            TacticalCard::plasma_upper(),
            TacticalCard::iron_bastion(),
            TacticalCard::cryo_cooler(),
            TacticalCard::titan_plating(),
            TacticalCard::overdrive_core(),
        ];
        let card_idx = self.draft_choices[choice_idx] as usize;
        if card_idx < pool.len() {
            let card = pool[card_idx];
            self.add_card(card);
            Some(card)
        } else {
            None
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
