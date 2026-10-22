//! Pure integer, zero-float Punch-Out boxing combat simulator (Track 2: Mecha Boxing).
//!
//! Enforces 100% deterministic bit-level simulation, 64-byte L1 cache-aligned state records,
//! hit-stop game feel, core heat management, and opponent telegraph AI.

use core::fmt;

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
    pub _pad: [u8; 6],
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
            _pad: [0; 6],
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
            _pad: [0; 6],
        }
    }
}

/// 64-byte aligned Boxer state.
#[repr(C, align(64))]
#[derive(Clone, Copy)]
pub struct BoxerState {
    pub hp: i32,
    pub max_hp: i32,
    pub heat: u16,
    pub max_heat: u16,
    pub action: BoxerAction,
    pub x_offset_px: i16,
    pub combo: u8,
    pub is_counter_opportunity: bool,
    pub _pad: [u8; 32],
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
            _pad: [0; 32],
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
}

/// 64-byte aligned Boxing Ring Simulation Engine.
#[repr(C, align(64))]
#[derive(Clone)]
pub struct BoxingRingEngine {
    pub tick: u64,
    pub player: BoxerState,
    pub opponent: BoxerState,
    pub ai_phase: AiPhase,
    pub ai_timer: u16,
    pub pending_opp_attack: ActionKind,
    pub hit_stop_frames: u8,
    pub camera_shake: u8,
    pub last_event: BoxingEvent,
    pub score: u32,
    pub seed: u64,
    pub _pad: [u8; 16],
}

impl BoxingRingEngine {
    pub fn new(seed: u64) -> Self {
        Self {
            tick: 0,
            player: BoxerState::new(100),
            opponent: BoxerState::new(120),
            ai_phase: AiPhase::Idle,
            ai_timer: 90, // 1.5s initial idle
            pending_opp_attack: ActionKind::Jab,
            hit_stop_frames: 0,
            camera_shake: 0,
            last_event: BoxingEvent::None,
            score: 0,
            seed,
            _pad: [0; 16],
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

        if !self.player.is_alive() || !self.opponent.is_alive() {
            return BoxingEvent::None;
        }

        // 1. Natural Heat Dissipation (1 unit every 10 frames)
        if self.tick % 10 == 0 && self.player.heat > 0 {
            self.player.heat -= 1;
        }

        // 2. Process Player Action
        self.process_player_input(player_input);

        // 3. Process Opponent AI State Machine
        self.process_opponent_ai();

        // 4. Update Ongoing Actions
        self.update_action_frames();

        self.last_event
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
                // Determine next attack using PRNG
                let roll = self.prng_next() % 100;
                self.pending_opp_attack = if roll < 40 {
                    ActionKind::Jab
                } else if roll < 80 {
                    ActionKind::Cross
                } else {
                    ActionKind::Hook
                };

                self.ai_phase = AiPhase::Telegraph;
                self.ai_timer = 28; // ~460ms telegraph warning
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
                self.ai_timer = 24; // Vulnerable recovery window
            }
            AiPhase::Recovery => {
                self.ai_phase = AiPhase::Idle;
                self.ai_timer = 45 + (self.prng_next() % 40) as u16; // Random pause before next combo
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
            let chip = (full_dmg * 20 / 100).max(2);
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
        let base_dmg = self.player.action.damage;
        let is_counter = self.opponent.is_counter_opportunity || self.ai_phase == AiPhase::Recovery;
        let final_dmg = if is_counter {
            (base_dmg * 150) / 100
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
