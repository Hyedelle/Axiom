//! Cooperative Multiplayer Raid Battle Session (T-031).
//!
//! Simulates two networked players (Fire Knight & Lightning Mage) teaming up
//! against a Raid Boss Dragon across a latency-simulated network link with
//! deterministic rollback resimulation upon delayed peer commands.

extern crate alloc;

use alloc::vec::Vec;
use axiom_core::effector::{AtbArena, AtbUnit, Element, StatusKind, MAX_ACTIVE_UNITS};

pub const HERO_1_ID: u32 = 101;
pub const HERO_2_ID: u32 = 102;
pub const BOSS_ID: u32 = 9999;
pub const HISTORY_CAPACITY: usize = 64;

/// Player action command sent across network peers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RaidCommand {
    pub player_id: u32,
    pub frame: u64,
    pub action_type: u8, // 1 = Elemental Strike, 2 = Buff/Haste, 3 = Heavy Slam
}

/// Multiplayer Raid Peer maintaining local speculative timeline and rollback capacity.
pub struct MultiplayerRaidPeer {
    pub local_player_id: usize,
    pub current_tick: u64,
    pub arena: AtbArena<MAX_ACTIVE_UNITS>,
    pub history: [(u64, AtbArena<MAX_ACTIVE_UNITS>); HISTORY_CAPACITY],
    pub command_history: [Vec<RaidCommand>; HISTORY_CAPACITY],
    pub rollbacks_executed: u64,
}

impl MultiplayerRaidPeer {
    /// Initializes the multiplayer raid encounter.
    pub fn new(local_player_id: usize) -> Self {
        let mut arena = AtbArena::new();

        // Team 1: Player 1 (Fire Knight) & Player 2 (Lightning Mage)
        let mut hero1 = AtbUnit::new(HERO_1_ID, 1, Element::Fire, 600, 80, 30, 45);
        hero1.atb_gauge = 400;
        arena.spawn_unit(hero1);

        let mut hero2 = AtbUnit::new(HERO_2_ID, 1, Element::Lightning, 450, 95, 20, 55);
        hero2.atb_gauge = 300;
        arena.spawn_unit(hero2);

        // Team 2: Raid Boss Dragon (Ice Element, 650 HP)
        let mut boss = AtbUnit::new(BOSS_ID, 2, Element::Ice, 650, 110, 40, 35);
        boss.atb_gauge = 200;
        arena.spawn_unit(boss);

        let history = core::array::from_fn(|_| (0, arena.clone()));
        let command_history = core::array::from_fn(|_| Vec::new());

        Self {
            local_player_id,
            current_tick: 0,
            arena,
            history,
            command_history,
            rollbacks_executed: 0,
        }
    }

    /// Receives a remote player command arrived after network transit.
    /// If the command is for a past frame, triggers speculative rollback.
    pub fn receive_remote_command(&mut self, cmd: RaidCommand) {
        if cmd.frame <= self.current_tick && self.current_tick - cmd.frame < HISTORY_CAPACITY as u64 {
            // Register command in history
            let slot = (cmd.frame as usize) % HISTORY_CAPACITY;
            self.command_history[slot].push(cmd);

            // Rollback to cmd.frame
            self.rollback_and_resimulate(cmd.frame);
        }
    }

    /// Rolls back arena state to `from_frame - 1`, reapplies commands, and resimulates forward.
    fn rollback_and_resimulate(&mut self, from_frame: u64) {
        let prev_frame = from_frame.saturating_sub(1);
        let prev_slot = (prev_frame as usize) % HISTORY_CAPACITY;
        let (saved_frame, saved_arena) = self.history[prev_slot].clone();

        if saved_frame == prev_frame {
            self.arena = saved_arena;
            self.rollbacks_executed += 1;

            // Resimulate forward from from_frame to current_tick
            for f in from_frame..=self.current_tick {
                let slot = (f as usize) % HISTORY_CAPACITY;
                // Re-apply all commands for frame f
                let cmds = self.command_history[slot].clone();
                for cmd in cmds {
                    self.execute_command(cmd);
                }

                self.arena.step();
                self.history[slot] = (f, self.arena.clone());
            }
        }
    }

    /// Applies an action command to target units in arena.
    fn execute_command(&mut self, cmd: RaidCommand) {
        if cmd.action_type == 3 {
            // Heavy Slam: Direct hit to boss
            for enemy in &mut self.arena.units {
                if enemy.id == BOSS_ID && enemy.is_alive() {
                    enemy.hp = enemy.hp.saturating_sub(150);
                }
            }
        } else {
            for unit in &mut self.arena.units {
                if unit.id == cmd.player_id && unit.is_alive() {
                    match cmd.action_type {
                        1 => {
                            // Elemental Strike: Boost next attack & fill ATB
                            unit.atb_gauge = unit.atb_gauge.saturating_add(300).min(1000);
                        }
                        2 => {
                            // Buff / Haste: Apply Haste status
                            unit.apply_status(StatusKind::Haste, 30, 0);
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    /// Advances 1 simulation tick locally.
    pub fn step(&mut self, local_cmd: Option<RaidCommand>) {
        self.current_tick += 1;
        let slot = (self.current_tick as usize) % HISTORY_CAPACITY;

        // Clear previous commands for this slot (from previous ring loop)
        self.command_history[slot].clear();

        // Apply any local command
        if let Some(cmd) = local_cmd {
            self.command_history[slot].push(cmd);
            self.execute_command(cmd);
        }

        // Step arena ATB ticks
        self.arena.step();

        // Save history state
        self.history[slot] = (self.current_tick, self.arena.clone());
    }

    /// Checks if raid boss is defeated.
    pub fn is_boss_defeated(&self) -> bool {
        !self.arena.units.iter().any(|u| u.id == BOSS_ID && u.is_alive())
    }

    /// Computes deterministic state hash of the raid session.
    pub fn compute_hash(&self) -> u64 {
        let mut h = self.current_tick.wrapping_mul(0x9E3779B97F4A7C15);
        h ^= self.arena.compute_hash();
        h ^= (self.is_boss_defeated() as u64).wrapping_mul(0xBF58476D1CE4E5B9);
        h
    }
}
