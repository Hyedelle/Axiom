//! Pure integer, zero-float Punch-Out boxing combat simulator (Track 2: Mecha Boxing).
//!
//! Enforces 100% deterministic bit-level simulation, 64-byte L1 cache-aligned state records,
//! hit-stop game feel, core heat management, and opponent telegraph AI.

use core::fmt;

use crate::card::{CardKind, DeckState, TacticalCard};

/// Discrete actions a boxer can perform.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionKind {
    Idle = 0,
    SlipLeft = 1,
    SlipRight = 2,
    Guard = 3,
    Jab = 4,
    Cross = 5,
    Hook = 6,
    Finisher = 7,
    HitStagger = 8,
    Down = 9,
}

impl ActionKind {
    pub const fn to_u8(self) -> u8 {
        self as u8
    }

    pub const fn from_u8(val: u8) -> Self {
        match val {
            1 => Self::SlipLeft,
            2 => Self::SlipRight,
            3 => Self::Guard,
            4 => Self::Jab,
            5 => Self::Cross,
            6 => Self::Hook,
            7 => Self::Finisher,
            8 => Self::HitStagger,
            9 => Self::Down,
            _ => Self::Idle,
        }
    }

    pub const fn is_attack(&self) -> bool {
        matches!(self, Self::Jab | Self::Cross | Self::Hook | Self::Finisher)
    }

    pub const fn is_evasion(&self) -> bool {
        matches!(self, Self::SlipLeft | Self::SlipRight)
    }

    pub const fn duration_frames(&self) -> u16 {
        match self {
            Self::Idle => 1,
            Self::SlipLeft | Self::SlipRight => 22, // ~366ms at 60Hz
            Self::Guard => 1,                      // Continuous while held
            Self::Jab => 18,                       // Quick 300ms
            Self::Cross => 24,                     // Heavy 400ms
            Self::Hook => 32,                      // Heavy 533ms
            Self::Finisher => 40,                  // Screen-shattering blow
            Self::HitStagger => 18,
            Self::Down => 120,                     // 2.0s knock down
        }
    }

    pub const fn base_damage(&self) -> u16 {
        match self {
            Self::Jab => 15,
            Self::Cross => 25,
            Self::Hook => 38,
            Self::Finisher => 65,
            _ => 0,
        }
    }

    pub const fn heat_cost(&self) -> u16 {
        match self {
            Self::Jab => 10,
            Self::Cross => 20,
            Self::Hook => 35,
            Self::Finisher => 50,
            _ => 0,
        }
    }
}

/// 16-byte POD Boxer Action Record.
#[repr(C, align(16))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BoxerAction {
    pub kind: ActionKind,
    pub frame: u16,
    pub max_frame: u16,
    pub damage: u16,
    pub heat: u16,
    pub guard_power: u16,
    pub _pad: [u8; 4],
}

impl BoxerAction {
    pub const fn idle() -> Self {
        Self {
            kind: ActionKind::Idle,
            frame: 0,
            max_frame: 1,
            damage: 0,
            heat: 0,
            guard_power: 0,
            _pad: [0; 4],
        }
    }

    pub const fn from_kind(kind: ActionKind) -> Self {
        Self {
            kind,
            frame: 0,
            max_frame: kind.duration_frames(),
            damage: kind.base_damage(),
            heat: kind.heat_cost(),
            guard_power: if matches!(kind, ActionKind::Guard) { 80 } else { 0 },
            _pad: [0; 4],
        }
    }
}

/// 64-byte aligned Boxer state.
#[repr(C, align(64))]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct BoxerState {
    pub hp: i32,
    pub max_hp: i32,
    pub heat: u16,
    pub max_heat: u16,
    pub action: BoxerAction,
    pub x_offset_px: i16,
    pub combo: u8,
    pub is_counter_opportunity: bool,
    pub _pad: [u8; 28],
}

impl BoxerState {
    pub fn new(max_hp: i32) -> Self {
        Self {
            hp: max_hp,
            max_hp,
            heat: 0,
            max_heat: 100,
            action: BoxerAction::idle(),
            x_offset_px: 0,
            combo: 0,
            is_counter_opportunity: false,
            _pad: [0; 28],
        }
    }

    pub fn is_alive(&self) -> bool {
        self.hp > 0
    }
}

impl fmt::Debug for BoxerState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BoxerState")
            .field("hp", &self.hp)
            .field("heat", &self.heat)
            .field("action", &self.action.kind)
            .field("frame", &self.action.frame)
            .field("x_offset", &self.x_offset_px)
            .finish()
    }
}

/// Opponent AI state machine phases (Punch-Out pattern).
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AiPhase {
    Idle = 0,
    Telegraph = 1,
    Attacking = 2,
    Recovery = 3,
}

/// Distinct Opponent Mecha Boss archetypes.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpponentKind {
    DummyMk2 = 0,   // Stage 1: Blue training mecha (120 HP, 35 telegraph frames)
    IronBull = 1,   // Stage 2: Crimson heavy armored mecha (200 HP, 50 telegraph frames)
    VoltViper = 2,  // Stage 3: Neon lightning mecha (160 HP, 22 telegraph frames)
}

impl OpponentKind {
    pub const fn name(&self) -> &'static str {
        match self {
            Self::DummyMk2 => "DUMMY-MK2 [TRAINING MECHA]",
            Self::IronBull => "IRON-BULL [HEAVY MECHA]",
            Self::VoltViper => "VOLT-VIPER [LIGHTNING MECHA]",
        }
    }

    pub const fn max_hp(&self) -> i32 {
        match self {
            Self::DummyMk2 => 120,
            Self::IronBull => 200,
            Self::VoltViper => 160,
        }
    }

    pub const fn telegraph_frames(&self) -> u16 {
        match self {
            Self::DummyMk2 => 35,
            Self::IronBull => 50,
            Self::VoltViper => 22,
        }
    }

    pub const fn idle_frames(&self) -> u16 {
        match self {
            Self::DummyMk2 => 60,
            Self::IronBull => 80,
            Self::VoltViper => 35,
        }
    }

    pub const fn armor_tint(&self) -> u32 {
        match self {
            Self::DummyMk2 => 0xFF38BDF8, // Sky Blue
            Self::IronBull => 0xFFDC2626, // Crimson Red
            Self::VoltViper => 0xFFFACC15, // Volt Yellow
        }
    }
}

/// Roguelite run fight phase.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FightPhase {
    Combat = 0,
    Drafting = 1,
    Victory = 2,
    Defeat = 3,
}

/// Events produced during a simulation tick.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoxingEvent {
    None = 0,
    PlayerPunched { damage: u16, is_counter: bool },
    PlayerEvaded,
    PlayerBlocked { chip: u16 },
    PlayerStruck { damage: u16 },
    OpponentDowned,
    PlayerDowned,
    DraftOpened,
    CardDrafted { kind: CardKind },
    StageCleared { next_stage: u16 },
}

/// 64-byte aligned Boxing Ring Simulation Engine (256 bytes total).
#[repr(C, align(64))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BoxingRingEngine {
    pub tick: u64,
    pub _pad0: [u8; 8],
    pub draft_options: [TacticalCard; 3],
    pub player: BoxerState,
    pub opponent: BoxerState,
    pub score: u32,
    pub stage: u16,
    pub bonus_power: u16,
    pub seed: u64,
    pub ai_timer: u16,
    pub counter_mult: u16,
    pub opponent_kind: OpponentKind,
    pub fight_phase: FightPhase,
    pub cooling_rate: u8,
    pub guard_absorption: u8,
    pub ai_phase: AiPhase,
    pub pending_opp_attack: ActionKind,
    pub hit_stop_frames: u8,
    pub camera_shake: u8,
    pub last_event: BoxingEvent,
    pub _pad1: [u8; 30],
}

impl BoxingRingEngine {
    pub fn new(seed: u64) -> Self {
        let opponent_kind = OpponentKind::DummyMk2;
        let mut deck = DeckState::<16>::new();
        let draft_options = deck.roll_draft_choices(seed.wrapping_add(1001));

        Self {
            tick: 0,
            _pad0: [0; 8],
            draft_options,
            player: BoxerState::new(100),
            opponent: BoxerState::new(opponent_kind.max_hp()),
            score: 0,
            stage: 1,
            bonus_power: 0,
            seed,
            ai_timer: opponent_kind.idle_frames(),
            counter_mult: 150,
            opponent_kind,
            fight_phase: FightPhase::Combat,
            cooling_rate: 0,
            guard_absorption: 80,
            ai_phase: AiPhase::Idle,
            pending_opp_attack: ActionKind::Jab,
            hit_stop_frames: 0,
            camera_shake: 0,
            last_event: BoxingEvent::None,
            _pad1: [0; 30],
        }
    }

    /// Advance the simulation by 1 discrete frame (60Hz fixed tick).
    pub fn step(&mut self, player_input: ActionKind) -> BoxingEvent {
        self.tick += 1;
        self.last_event = BoxingEvent::None;

        // Hit-stop freeze frame management
        if self.hit_stop_frames > 0 {
            self.hit_stop_frames -= 1;
            return BoxingEvent::None;
        }

        // Camera shake decay
        if self.camera_shake > 0 {
            self.camera_shake -= 1;
        }

        // 1. In Drafting Phase: Player input picks cards!
        if self.fight_phase == FightPhase::Drafting {
            match player_input {
                ActionKind::Jab => { self.draft_card(0); }
                ActionKind::Cross => { self.draft_card(1); }
                ActionKind::Hook => { self.draft_card(2); }
                _ => {}
            }
            return self.last_event;
        }

        // 2. In Defeat / Victory Phase: Finisher / Space resets the run
        if self.fight_phase == FightPhase::Defeat || self.fight_phase == FightPhase::Victory {
            if player_input == ActionKind::Finisher {
                *self = BoxingRingEngine::new(self.seed.wrapping_add(self.tick));
            }
            return self.last_event;
        }

        // 3. Check for Down state transitions
        if !self.opponent.is_alive() {
            if self.opponent.action.kind == ActionKind::Down && self.opponent.action.frame >= self.opponent.action.max_frame {
                self.fight_phase = FightPhase::Drafting;
                let mut deck = DeckState::<16>::new();
                self.draft_options = deck.roll_draft_choices(self.seed.wrapping_add(self.stage as u64 * 7919 + self.tick));
                self.last_event = BoxingEvent::DraftOpened;
                return self.last_event;
            }
            self.update_action_frames();
            return self.last_event;
        }

        if !self.player.is_alive() {
            if self.player.action.kind == ActionKind::Down && self.player.action.frame >= self.player.action.max_frame {
                self.fight_phase = FightPhase::Defeat;
                return self.last_event;
            }
            self.update_action_frames();
            return self.last_event;
        }

        // 4. Natural Heat Dissipation (1 + cooling_rate units every 10 frames)
        if self.tick % 10 == 0 && self.player.heat > 0 {
            self.player.heat = self.player.heat.saturating_sub(1 + self.cooling_rate as u16);
        }

        // 5. Process Player Action
        self.process_player_input(player_input);

        // 6. Process Opponent AI State Machine
        self.process_opponent_ai();

        // 7. Update Ongoing Actions
        self.update_action_frames();

        self.last_event
    }

    /// Picks a card from the 3 draft options, applies perks, repairs player, and advances stage.
    pub fn draft_card(&mut self, choice_idx: usize) -> bool {
        if self.fight_phase != FightPhase::Drafting || choice_idx >= 3 {
            return false;
        }
        let chosen_card = self.draft_options[choice_idx];
        match chosen_card.kind {
            CardKind::Straight => { self.bonus_power = self.bonus_power.saturating_add(5); }
            CardKind::Guard => { self.guard_absorption = (self.guard_absorption + 5).min(95); }
            CardKind::OverheatHook => { self.bonus_power = self.bonus_power.saturating_add(15); }
            CardKind::CounterSlip => { self.counter_mult = self.counter_mult.saturating_add(40); }
            CardKind::PlasmaUpper => { self.bonus_power = self.bonus_power.saturating_add(20); }
            CardKind::IronBastion => { self.guard_absorption = 92; }
            CardKind::CryoCooler => { self.cooling_rate = self.cooling_rate.saturating_add(1); }
            CardKind::TitanPlating => {
                self.player.max_hp += 30;
                self.guard_absorption = (self.guard_absorption + 5).min(95);
            }
            CardKind::OverdriveCore => { self.bonus_power = self.bonus_power.saturating_add(15); }
        }
        // Base repair upon victory (+35 HP)
        self.player.hp = (self.player.hp + 35).min(self.player.max_hp);
        self.score += 500;
        self.last_event = BoxingEvent::CardDrafted { kind: chosen_card.kind };

        // Advance stage
        self.advance_to_next_stage();
        true
    }

    /// Advances the stage and spawns the next opponent mecha.
    pub fn advance_to_next_stage(&mut self) {
        self.stage += 1;
        self.opponent_kind = match self.stage % 3 {
            1 => OpponentKind::DummyMk2,
            2 => OpponentKind::IronBull,
            0 => OpponentKind::VoltViper,
            _ => OpponentKind::DummyMk2,
        };
        let extra_hp = ((self.stage as i32 - 1) / 3) * 30;
        self.opponent = BoxerState::new(self.opponent_kind.max_hp() + extra_hp);
        self.ai_phase = AiPhase::Idle;
        self.ai_timer = self.opponent_kind.idle_frames();
        self.fight_phase = FightPhase::Combat;
        self.last_event = BoxingEvent::StageCleared { next_stage: self.stage };
    }

    fn process_player_input(&mut self, input: ActionKind) {
        if self.player.action.kind != ActionKind::Idle && self.player.action.kind != ActionKind::Guard {
            return;
        }

        match input {
            ActionKind::Guard => {
                self.player.action = BoxerAction::from_kind(ActionKind::Guard);
                self.player.x_offset_px = 0;
            }
            ActionKind::SlipLeft => {
                self.player.action = BoxerAction::from_kind(ActionKind::SlipLeft);
                self.player.x_offset_px = -24;
            }
            ActionKind::SlipRight => {
                self.player.action = BoxerAction::from_kind(ActionKind::SlipRight);
                self.player.x_offset_px = 24;
            }
            ActionKind::Jab | ActionKind::Cross | ActionKind::Hook | ActionKind::Finisher => {
                if self.player.heat + input.heat_cost() <= self.player.max_heat + 20 {
                    self.player.action = BoxerAction::from_kind(input);
                    self.player.heat = (self.player.heat + input.heat_cost()).min(self.player.max_heat);
                    self.player.x_offset_px = 0;
                }
            }
            _ => {
                if self.player.action.kind == ActionKind::Guard && input == ActionKind::Idle {
                    self.player.action = BoxerAction::idle();
                }
            }
        }
    }

    fn process_opponent_ai(&mut self) {
        if self.ai_timer > 0 {
            self.ai_timer -= 1;
            return;
        }

        match self.ai_phase {
            AiPhase::Idle => {
                // Determine next attack using PRNG and boss archetype
                let roll = self.prng_next() % 100;
                self.pending_opp_attack = match self.opponent_kind {
                    OpponentKind::DummyMk2 => {
                        if roll < 60 { ActionKind::Jab } else if roll < 90 { ActionKind::Cross } else { ActionKind::Hook }
                    }
                    OpponentKind::IronBull => {
                        if roll < 20 { ActionKind::Jab } else if roll < 60 { ActionKind::Cross } else { ActionKind::Hook }
                    }
                    OpponentKind::VoltViper => {
                        if roll < 65 { ActionKind::Jab } else if roll < 85 { ActionKind::Cross } else { ActionKind::Hook }
                    }
                };

                self.ai_phase = AiPhase::Telegraph;
                self.ai_timer = self.opponent_kind.telegraph_frames();
                self.opponent.action = BoxerAction::idle();
                self.opponent.is_counter_opportunity = true;
            }
            AiPhase::Telegraph => {
                // Opponent attack swings!
                self.ai_phase = AiPhase::Attacking;
                self.ai_timer = self.pending_opp_attack.duration_frames() / 2;
                self.opponent.action = BoxerAction::from_kind(self.pending_opp_attack);
                self.opponent.is_counter_opportunity = false;
            }
            AiPhase::Attacking => {
                // Resolve opponent attack impact on player
                self.resolve_opponent_hit();
                self.ai_phase = AiPhase::Recovery;
                self.ai_timer = match self.opponent_kind {
                    OpponentKind::DummyMk2 => 24,
                    OpponentKind::IronBull => 32, // Heavy recovery window!
                    OpponentKind::VoltViper => 14, // Brief recovery window!
                };
            }
            AiPhase::Recovery => {
                self.ai_phase = AiPhase::Idle;
                self.ai_timer = self.opponent_kind.idle_frames() + (self.prng_next() % 25) as u16;
                self.opponent.action = BoxerAction::idle();
            }
        }
    }

    fn resolve_opponent_hit(&mut self) {
        // A. Check if player successfully slipped (evaded)
        if self.player.action.kind.is_evasion() {
            self.last_event = BoxingEvent::PlayerEvaded;
            self.score += 50;
            return;
        }

        // B. Check if player guarded
        if self.player.action.kind == ActionKind::Guard {
            let full_dmg = self.pending_opp_attack.base_damage();
            // IronBull punches penetrate guard with heavier chip damage!
            let absorb = if self.opponent_kind == OpponentKind::IronBull {
                self.guard_absorption.saturating_sub(25).max(40)
            } else {
                self.guard_absorption
            };
            let chip = (full_dmg * (100 - absorb as u16) / 100).max(2);
            self.player.hp = (self.player.hp - chip as i32).max(0);
            self.last_event = BoxingEvent::PlayerBlocked { chip };
            self.hit_stop_frames = 3;
            self.camera_shake = 2;
            if self.player.hp == 0 {
                self.player.action = BoxerAction::from_kind(ActionKind::Down);
                self.last_event = BoxingEvent::PlayerDowned;
            }
            return;
        }

        // C. Clean hit taken by player
        let dmg = self.pending_opp_attack.base_damage();
        self.player.hp = (self.player.hp - dmg as i32).max(0);
        self.hit_stop_frames = 5;
        self.camera_shake = 6;
        self.player.action = BoxerAction::from_kind(ActionKind::HitStagger);

        if self.player.hp == 0 {
            self.player.action = BoxerAction::from_kind(ActionKind::Down);
            self.last_event = BoxingEvent::PlayerDowned;
        } else {
            self.last_event = BoxingEvent::PlayerStruck { damage: dmg };
        }
    }

    fn update_action_frames(&mut self) {
        // Player action tick
        if self.player.action.kind != ActionKind::Idle && self.player.action.kind != ActionKind::Guard {
            self.player.action.frame += 1;

            // Attack contact point occurs at mid-animation
            let contact_frame = self.player.action.max_frame / 2;
            if self.player.action.frame == contact_frame && self.player.action.kind.is_attack() {
                self.resolve_player_hit();
            }

            if self.player.action.frame >= self.player.action.max_frame {
                self.player.action = BoxerAction::idle();
                self.player.x_offset_px = 0;
            }
        }

        // Opponent action tick
        if self.opponent.action.kind != ActionKind::Idle {
            self.opponent.action.frame += 1;
            if self.opponent.action.frame >= self.opponent.action.max_frame {
                self.opponent.action = BoxerAction::idle();
            }
        }
    }

    fn resolve_player_hit(&mut self) {
        let base_dmg = self.player.action.damage.saturating_add(self.bonus_power);
        let is_counter = self.opponent.is_counter_opportunity || self.ai_phase == AiPhase::Recovery;
        let final_dmg = if is_counter {
            (base_dmg * self.counter_mult) / 100
        } else {
            base_dmg
        };

        self.opponent.hp = (self.opponent.hp - final_dmg as i32).max(0);
        self.hit_stop_frames = if is_counter { 6 } else { 4 };
        self.camera_shake = if is_counter { 5 } else { 3 };
        self.score += (final_dmg as u32) * if is_counter { 2 } else { 1 };

        if self.opponent.hp == 0 {
            self.opponent.action = BoxerAction::from_kind(ActionKind::Down);
            self.last_event = BoxingEvent::OpponentDowned;
        } else {
            self.opponent.action = BoxerAction::from_kind(ActionKind::HitStagger);
            self.last_event = BoxingEvent::PlayerPunched {
                damage: final_dmg,
                is_counter,
            };
        }
    }

    /// Advances the simulation by 1 discrete frame in 2-player P2P multiplayer mode.
    /// Evaluates simultaneous actions for Player 0 (player) and Player 1 (opponent).
    pub fn step_pvp(
        &mut self,
        p0_input: ActionKind,
        p1_input: ActionKind,
    ) -> (BoxingEvent, BoxingEvent) {
        self.tick += 1;
        let mut ev0 = BoxingEvent::None;
        let mut ev1 = BoxingEvent::None;

        if self.hit_stop_frames > 0 {
            self.hit_stop_frames -= 1;
            return (BoxingEvent::None, BoxingEvent::None);
        }

        if self.camera_shake > 0 {
            self.camera_shake -= 1;
        }

        // KO check: if either is Down, tick down animation and return
        if !self.player.is_alive() || !self.opponent.is_alive() {
            if self.player.action.kind == ActionKind::Down {
                self.player.action.frame = self.player.action.frame.saturating_add(1);
            }
            if self.opponent.action.kind == ActionKind::Down {
                self.opponent.action.frame = self.opponent.action.frame.saturating_add(1);
            }
            return (BoxingEvent::None, BoxingEvent::None);
        }

        // Natural heat dissipation every 10 frames
        if self.tick % 10 == 0 {
            if self.player.heat > 0 {
                self.player.heat = self.player.heat.saturating_sub(1);
            }
            if self.opponent.heat > 0 {
                self.opponent.heat = self.opponent.heat.saturating_sub(1);
            }
        }

        // Process inputs
        Self::apply_boxer_input(&mut self.player, p0_input);
        Self::apply_boxer_input(&mut self.opponent, p1_input);

        // Check contact frames
        let p0_contact = self.player.action.kind.is_attack()
            && self.player.action.frame == (self.player.action.max_frame / 2);
        let p1_contact = self.opponent.action.kind.is_attack()
            && self.opponent.action.frame == (self.opponent.action.max_frame / 2);

        // Resolve P0 attacking P1
        if p0_contact {
            ev0 = self.resolve_pvp_strike(0);
        }

        // Resolve P1 attacking P0
        if p1_contact {
            ev1 = self.resolve_pvp_strike(1);
        }

        // Advance animation frames for ongoing actions
        Self::advance_boxer_action(&mut self.player);
        Self::advance_boxer_action(&mut self.opponent);

        (ev0, ev1)
    }

    fn resolve_pvp_strike(&mut self, attacker_idx: usize) -> BoxingEvent {
        let (attacker_action, defender_action_kind, is_defender_alive) = if attacker_idx == 0 {
            (self.player.action, self.opponent.action.kind, self.opponent.is_alive())
        } else {
            (self.opponent.action, self.player.action.kind, self.player.is_alive())
        };

        if !is_defender_alive {
            return BoxingEvent::None;
        }

        let full_dmg = attacker_action.damage;

        // 1. Evasion / Slip
        if defender_action_kind.is_evasion() {
            return BoxingEvent::PlayerEvaded;
        }

        // 2. Guard
        if defender_action_kind == ActionKind::Guard {
            let chip = (full_dmg * (100 - self.guard_absorption as u16) / 100).max(2);
            self.hit_stop_frames = 3;
            self.camera_shake = 2;

            if attacker_idx == 0 {
                self.opponent.hp = (self.opponent.hp - chip as i32).max(0);
                if self.opponent.hp == 0 {
                    self.opponent.action = BoxerAction::from_kind(ActionKind::Down);
                    return BoxingEvent::OpponentDowned;
                }
            } else {
                self.player.hp = (self.player.hp - chip as i32).max(0);
                if self.player.hp == 0 {
                    self.player.action = BoxerAction::from_kind(ActionKind::Down);
                    return BoxingEvent::PlayerDowned;
                }
            }
            return BoxingEvent::PlayerBlocked { chip };
        }

        // 3. Counter Hit or Clean Hit
        let is_counter = defender_action_kind.is_attack();
        let dmg = if is_counter {
            (full_dmg * self.counter_mult) / 100
        } else {
            full_dmg
        };

        self.hit_stop_frames = if is_counter { 6 } else { 4 };
        self.camera_shake = if is_counter { 5 } else { 3 };

        if attacker_idx == 0 {
            self.opponent.hp = (self.opponent.hp - dmg as i32).max(0);
            if self.opponent.hp == 0 {
                self.opponent.action = BoxerAction::from_kind(ActionKind::Down);
                BoxingEvent::OpponentDowned
            } else {
                self.opponent.action = BoxerAction::from_kind(ActionKind::HitStagger);
                BoxingEvent::PlayerPunched { damage: dmg, is_counter }
            }
        } else {
            self.player.hp = (self.player.hp - dmg as i32).max(0);
            if self.player.hp == 0 {
                self.player.action = BoxerAction::from_kind(ActionKind::Down);
                BoxingEvent::PlayerDowned
            } else {
                self.player.action = BoxerAction::from_kind(ActionKind::HitStagger);
                BoxingEvent::PlayerStruck { damage: dmg }
            }
        }
    }

    fn apply_boxer_input(boxer: &mut BoxerState, input: ActionKind) {
        if boxer.action.kind != ActionKind::Idle && boxer.action.kind != ActionKind::Guard {
            return;
        }

        match input {
            ActionKind::Guard => {
                boxer.action = BoxerAction::from_kind(ActionKind::Guard);
                boxer.x_offset_px = 0;
            }
            ActionKind::SlipLeft => {
                boxer.action = BoxerAction::from_kind(ActionKind::SlipLeft);
                boxer.x_offset_px = -24;
            }
            ActionKind::SlipRight => {
                boxer.action = BoxerAction::from_kind(ActionKind::SlipRight);
                boxer.x_offset_px = 24;
            }
            ActionKind::Jab | ActionKind::Cross | ActionKind::Hook | ActionKind::Finisher => {
                if boxer.heat + input.heat_cost() <= boxer.max_heat + 20 {
                    boxer.action = BoxerAction::from_kind(input);
                    boxer.heat = (boxer.heat + input.heat_cost()).min(boxer.max_heat);
                    boxer.x_offset_px = 0;
                }
            }
            _ => {
                if boxer.action.kind == ActionKind::Guard && input == ActionKind::Idle {
                    boxer.action = BoxerAction::idle();
                }
            }
        }
    }

    fn advance_boxer_action(boxer: &mut BoxerState) {
        if boxer.action.kind != ActionKind::Idle && boxer.action.kind != ActionKind::Guard {
            boxer.action.frame += 1;
            if boxer.action.frame >= boxer.action.max_frame {
                boxer.action = BoxerAction::idle();
                boxer.x_offset_px = 0;
            }
        }
    }

    fn prng_next(&mut self) -> u64 {
        self.seed = self.seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.seed
    }

    /// FNV-1a 64-bit state hash for absolute bit-for-bit determinism.
    pub fn compute_hash(&self) -> u64 {
        const FNV_OFFSET: u64 = 0xcbf29ce484222325;
        const FNV_PRIME: u64 = 0x100000001b3;

        let mut hash = FNV_OFFSET;
        let bytes = [
            self.tick.to_le_bytes(),
            (self.player.hp as u64).to_le_bytes(),
            (self.player.heat as u64).to_le_bytes(),
            (self.opponent.hp as u64).to_le_bytes(),
            (self.ai_phase as u64).to_le_bytes(),
            (self.score as u64).to_le_bytes(),
        ];

        for b_arr in &bytes {
            for &b in b_arr {
                hash ^= b as u64;
                hash = hash.wrapping_mul(FNV_PRIME);
            }
        }
        hash
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_boxing_engine_size_and_alignment() {
        assert_eq!(core::mem::align_of::<BoxerState>(), 64);
        assert_eq!(core::mem::size_of::<BoxerState>(), 64);
        assert_eq!(core::mem::align_of::<BoxingRingEngine>(), 64);
        assert_eq!(core::mem::size_of::<BoxingRingEngine>(), 256);
    }

    #[test]
    fn test_card_drafting_and_boss_progression() {
        let mut engine = BoxingRingEngine::new(42);
        assert_eq!(engine.stage, 1);
        assert_eq!(engine.opponent_kind, OpponentKind::DummyMk2);
        assert_eq!(engine.fight_phase, FightPhase::Combat);

        // Simulate opponent KO
        engine.opponent.hp = 0;
        engine.opponent.action = BoxerAction::from_kind(ActionKind::Down);
        engine.opponent.action.frame = engine.opponent.action.max_frame;

        let ev = engine.step(ActionKind::Idle);
        assert_eq!(ev, BoxingEvent::DraftOpened);
        assert_eq!(engine.fight_phase, FightPhase::Drafting);

        // Draft card #0
        let _prev_power = engine.bonus_power;
        assert!(engine.draft_card(0));
        assert_eq!(engine.stage, 2);
        assert_eq!(engine.opponent_kind, OpponentKind::IronBull);
        assert_eq!(engine.fight_phase, FightPhase::Combat);
        assert!(engine.opponent.hp >= 200);
    }
}
