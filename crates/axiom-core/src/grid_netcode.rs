//! Axiom Core — Deterministic Grid Rollback Netcode Engine (T-051)
//!
//! Provides zero-input-delay client prediction, ring-buffered grid action queues,
//! and speculative rollback resimulation across simulated network latency and packet loss
//! for data-driven 64-byte `TileCell` grids.

extern crate alloc;

use alloc::boxed::Box;
use crate::schema::RecipeRegistry;
use crate::tile_entity::{grid_action_kind, GridAction, TileGrid};

pub const DEFAULT_GRID_NETCODE_BUFFER_SIZE: usize = 32;
pub const MAX_SUPPORTED_GRID_PLAYERS: usize = 2;

/// Status and statistics returned from a grid frame advancement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GridFrameStats {
    pub current_frame: u64,
    pub rolled_back: bool,
    pub rollback_frames_count: u32,
    pub predicted_inputs_used: u32,
}

/// Fixed-capacity grid action ring buffer for a single player with epoch/frame tagging.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridInputQueue<const FRAMES: usize = DEFAULT_GRID_NETCODE_BUFFER_SIZE> {
    pub inputs: [GridAction; FRAMES],
    pub frame_tags: [u64; FRAMES],
    pub is_confirmed: [bool; FRAMES],
    pub last_confirmed_frame: u64,
}

impl<const FRAMES: usize> Default for GridInputQueue<FRAMES> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const FRAMES: usize> GridInputQueue<FRAMES> {
    pub const fn new() -> Self {
        Self {
            inputs: [GridAction::NONE; FRAMES],
            frame_tags: [u64::MAX; FRAMES],
            is_confirmed: [false; FRAMES],
            last_confirmed_frame: 0,
        }
    }

    /// Sets a confirmed grid action for a specific frame.
    pub fn set_confirmed(&mut self, frame: u64, action: GridAction) {
        let slot = (frame as usize) % FRAMES;
        self.inputs[slot] = action;
        self.frame_tags[slot] = frame;
        self.is_confirmed[slot] = true;
        if frame > self.last_confirmed_frame {
            self.last_confirmed_frame = frame;
        }
    }

    /// Predicts action for `frame` based on discrete action heuristics (defaults to NONE).
    pub fn predict_for_frame(&mut self, frame: u64) -> GridAction {
        let slot = (frame as usize) % FRAMES;
        if self.frame_tags[slot] == frame && self.is_confirmed[slot] {
            return self.inputs[slot];
        }

        // Discrete building action heuristic: unreceived action predicts as NONE
        let predicted = GridAction::NONE;
        self.inputs[slot] = predicted;
        self.frame_tags[slot] = frame;
        self.is_confirmed[slot] = false;
        predicted
    }

    /// Retrieves the action recorded for a frame.
    #[inline(always)]
    pub fn get_input(&self, frame: u64) -> GridAction {
        let slot = (frame as usize) % FRAMES;
        if self.frame_tags[slot] == frame {
            self.inputs[slot]
        } else {
            GridAction::NONE
        }
    }

    /// Returns true if the action for a frame was confirmed from the network.
    #[inline(always)]
    pub fn is_frame_confirmed(&self, frame: u64) -> bool {
        let slot = (frame as usize) % FRAMES;
        self.frame_tags[slot] == frame && self.is_confirmed[slot]
    }
}

/// Peer-to-peer rollback simulation session for spatial tile grids.
#[derive(Clone)]
pub struct GridRollbackSession<
    const W: usize,
    const H: usize,
    const FRAMES: usize = DEFAULT_GRID_NETCODE_BUFFER_SIZE,
    const PLAYERS: usize = MAX_SUPPORTED_GRID_PLAYERS,
    const RECIPE_CAP: usize = 128,
> {
    pub local_player_id: usize,
    pub current_frame: u64,
    pub confirmed_frame: u64,
    pub input_queues: [GridInputQueue<FRAMES>; PLAYERS],
    pub saved_grids: [TileGrid<W, H>; FRAMES],
    pub current_grid: TileGrid<W, H>,
    pub recipes: RecipeRegistry<RECIPE_CAP>,
    pub total_rollbacks: u64,
    pub total_frames_resimulated: u64,
}

impl<
        const W: usize,
        const H: usize,
        const FRAMES: usize,
        const PLAYERS: usize,
        const RECIPE_CAP: usize,
    > GridRollbackSession<W, H, FRAMES, PLAYERS, RECIPE_CAP>
{
    /// Creates a new rollback session on the stack (recommended for small grid dimensions).
    pub fn new(
        local_player_id: usize,
        initial_grid: TileGrid<W, H>,
        recipes: RecipeRegistry<RECIPE_CAP>,
    ) -> Self {
        let mut session = Self {
            local_player_id,
            current_frame: 0,
            confirmed_frame: 0,
            input_queues: [GridInputQueue::new(); PLAYERS],
            saved_grids: [TileGrid::new(); FRAMES],
            current_grid: initial_grid,
            recipes,
            total_rollbacks: 0,
            total_frames_resimulated: 0,
        };

        // Initialize snapshot slot 0
        session.saved_grids[0] = initial_grid;
        session
    }

    /// Creates a heap-allocated rollback session directly avoiding stack overflows for large grids.
    pub fn new_boxed(
        local_player_id: usize,
        initial_grid: TileGrid<W, H>,
        recipes: RecipeRegistry<RECIPE_CAP>,
    ) -> Box<Self> {
        let layout = core::alloc::Layout::new::<Self>();
        let ptr = unsafe { alloc::alloc::alloc_zeroed(layout) as *mut Self };
        if ptr.is_null() {
            alloc::alloc::handle_alloc_error(layout);
        }
        let mut session = unsafe { Box::from_raw(ptr) };
        session.local_player_id = local_player_id;
        session.current_frame = 0;
        session.confirmed_frame = 0;
        for q in session.input_queues.iter_mut() {
            *q = GridInputQueue::new();
        }
        for g in session.saved_grids.iter_mut() {
            *g = initial_grid.clone();
        }
        session.current_grid = initial_grid;
        session.recipes = recipes;
        session.total_rollbacks = 0;
        session.total_frames_resimulated = 0;
        session
    }

    /// Advances the simulation by 1 local frame with zero input latency.
    pub fn advance_frame(&mut self, local_action: GridAction) -> GridFrameStats {
        let mut stats = GridFrameStats {
            current_frame: self.current_frame,
            rolled_back: false,
            rollback_frames_count: 0,
            predicted_inputs_used: 0,
        };

        // 1. Record confirmed local action
        if self.local_player_id < PLAYERS {
            self.input_queues[self.local_player_id].set_confirmed(self.current_frame, local_action);
        }

        // 2. Predict missing remote actions
        for p in 0..PLAYERS {
            if p != self.local_player_id {
                let slot = (self.current_frame as usize) % FRAMES;
                if !self.input_queues[p].is_confirmed[slot] {
                    self.input_queues[p].predict_for_frame(self.current_frame);
                    stats.predicted_inputs_used += 1;
                }
            }
        }

        // 3. Save snapshot before step
        let slot = (self.current_frame as usize) % FRAMES;
        self.saved_grids[slot] = self.current_grid.clone();

        // 4. Apply all players' actions for this frame
        for p in 0..PLAYERS {
            let act = self.input_queues[p].get_input(self.current_frame);
            if act.kind != grid_action_kind::NONE {
                self.current_grid.apply_action(&act);
            }
        }

        // 5. Autonomous recipe simulation step
        self.current_grid.process_recipes(&self.recipes);

        // 6. Advance frame counter
        self.current_frame += 1;
        stats.current_frame = self.current_frame;

        // 7. Advance confirmed watermark
        self.update_confirmed_watermark();

        stats
    }

    /// Delivers a confirmed remote action arriving from the network link.
    ///
    /// If the newly received action contradicts a previously predicted action,
    /// a speculative rollback is triggered immediately.
    pub fn receive_remote_action(
        &mut self,
        remote_player_id: usize,
        frame: u64,
        action: GridAction,
    ) -> bool {
        if remote_player_id >= PLAYERS || remote_player_id == self.local_player_id {
            return false;
        }

        if frame > self.current_frame {
            // Future action stored directly as confirmed
            self.input_queues[remote_player_id].set_confirmed(frame, action);
            return false;
        }

        // Check if frame falls within rollback buffer window
        if self.current_frame.saturating_sub(frame) >= FRAMES as u64 {
            // History is too old to rollback
            return false;
        }

        let slot = (frame as usize) % FRAMES;
        let was_confirmed = self.input_queues[remote_player_id].frame_tags[slot] == frame
            && self.input_queues[remote_player_id].is_confirmed[slot];
        let previous_action = if self.input_queues[remote_player_id].frame_tags[slot] == frame {
            self.input_queues[remote_player_id].inputs[slot]
        } else {
            GridAction::NONE
        };

        // Set confirmed action
        self.input_queues[remote_player_id].set_confirmed(frame, action);

        // If this frame was already executed speculatively and mispredicted:
        if !was_confirmed && previous_action != action {
            // MISPREDICTION! Rollback and resimulate from `frame`
            self.rollback_and_resimulate(frame);
            return true;
        }

        false
    }

    /// Rolls back grid state to `from_frame` and resimulates forward to `current_frame`.
    pub fn rollback_and_resimulate(&mut self, from_frame: u64) {
        let rollback_slot = (from_frame as usize) % FRAMES;
        // 1. Instantaneous state recall
        self.current_grid = self.saved_grids[rollback_slot].clone();

        let resim_count = self.current_frame - from_frame;
        self.total_rollbacks += 1;
        self.total_frames_resimulated += resim_count;

        // 2. Deterministic forward re-simulation
        for frame in from_frame..self.current_frame {
            let slot = (frame as usize) % FRAMES;
            self.saved_grids[slot] = self.current_grid.clone();

            for p in 0..PLAYERS {
                let act = self.input_queues[p].get_input(frame);
                if act.kind != grid_action_kind::NONE {
                    self.current_grid.apply_action(&act);
                }
            }

            self.current_grid.process_recipes(&self.recipes);
        }
    }

    /// Updates the earliest unconfirmed frame boundary.
    pub fn update_confirmed_watermark(&mut self) {
        while self.confirmed_frame < self.current_frame {
            let slot = (self.confirmed_frame as usize) % FRAMES;
            let mut all_confirmed = true;
            for p in 0..PLAYERS {
                if !self.input_queues[p].is_confirmed[slot] {
                    all_confirmed = false;
                    break;
                }
            }
            if all_confirmed {
                self.confirmed_frame += 1;
            } else {
                break;
            }
        }
    }

    /// Computes deterministic state hash of the current grid.
    #[inline(always)]
    pub fn state_hash(&self) -> u64 {
        self.current_grid.state_hash()
    }
}

/// Simulated lossy network channel for grid actions with programmable latency.
#[derive(Debug, Clone)]
pub struct SimulatedGridNetworkPipe<const MAX_PACKETS: usize = 128> {
    packets: [Option<(u64, usize, u64, GridAction)>; MAX_PACKETS],
    count: usize,
    pub latency_ticks: u64,
}

impl<const MAX_PACKETS: usize> SimulatedGridNetworkPipe<MAX_PACKETS> {
    pub const fn new(latency_ticks: u64) -> Self {
        Self {
            packets: [None; MAX_PACKETS],
            count: 0,
            latency_ticks,
        }
    }

    /// Enqueues an action packet to be delivered after `latency_ticks`.
    pub fn send(
        &mut self,
        current_tick: u64,
        sender_id: usize,
        frame: u64,
        action: GridAction,
    ) -> bool {
        let arrive_tick = current_tick + self.latency_ticks;
        for slot in &mut self.packets {
            if slot.is_none() {
                *slot = Some((arrive_tick, sender_id, frame, action));
                self.count += 1;
                return true;
            }
        }
        false
    }

    /// Delivers all packets scheduled to arrive at or before `current_tick`.
    pub fn deliver_arrived<F>(&mut self, current_tick: u64, mut on_receive: F)
    where
        F: FnMut(usize, u64, GridAction),
    {
        for slot in &mut self.packets {
            if let Some((arrive_tick, sender_id, frame, action)) = *slot {
                if arrive_tick <= current_tick {
                    *slot = None;
                    self.count = self.count.saturating_sub(1);
                    on_receive(sender_id, frame, action);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::RecipeDefinition;

    #[test]
    fn test_grid_rollback_netcode_consensus_over_latency() {
        // Setup recipe: 2x Iron Ore (Type 1) -> 1x Iron Ingot (Type 2) in 2 ticks
        let mut recipes = RecipeRegistry::<32>::new();
        let mut r = RecipeDefinition::empty();
        r.id = 1;
        r.input_id = 1;
        r.input_count = 2;
        r.output_id = 2;
        r.output_count = 1;
        r.required_ticks = 2;
        recipes.register(r);

        // Peer 0 and Peer 1 on an 8x8 grid with 32 rollback frames
        let initial_grid = TileGrid::<8, 8>::new();
        let mut peer0 = GridRollbackSession::<8, 8, 32, 2, 32>::new(0, initial_grid.clone(), recipes.clone());
        let mut peer1 = GridRollbackSession::<8, 8, 32, 2, 32>::new(1, initial_grid, recipes);

        // 6 ticks of simulated network latency (100ms at 60Hz)
        let mut net_pipe0_to_1 = SimulatedGridNetworkPipe::<64>::new(6);
        let mut net_pipe1_to_0 = SimulatedGridNetworkPipe::<64>::new(6);

        for tick in 0..50 {
            // Player 0 actions:
            // At tick 4: Place smelter (type_id 10) at (2, 2)
            // At tick 8: Insert 4x Iron Ore (item 1) into (2, 2)
            // At tick 12: Set recipe 1 on (2, 2)
            let act0 = match tick {
                4 => GridAction::set_tile(0, 2, 2, 10, 500),
                8 => GridAction::push_item(0, 2, 2, 1, 4),
                12 => GridAction::set_recipe(0, 2, 2, 1),
                _ => GridAction::NONE,
            };

            // Player 1 actions:
            // At tick 5: Place chest (type_id 20) at (5, 5)
            // At tick 10: Insert 10x Stone (item 3) into (5, 5)
            // At tick 20: Place wall (type_id 30) at (1, 1)
            let act1 = match tick {
                5 => GridAction::set_tile(1, 5, 5, 20, 250),
                10 => GridAction::push_item(1, 5, 5, 3, 10),
                20 => GridAction::set_tile(1, 1, 1, 30, 1000),
                _ => GridAction::NONE,
            };

            // 1. Advance local frame immediately with zero lag
            peer0.advance_frame(act0);
            peer1.advance_frame(act1);

            // 2. Transmit actions across lossy network pipe
            if act0.kind != grid_action_kind::NONE {
                net_pipe0_to_1.send(tick, 0, tick, act0);
            }
            if act1.kind != grid_action_kind::NONE {
                net_pipe1_to_0.send(tick, 1, tick, act1);
            }

            // 3. Deliver arrived packets and trigger speculative rollbacks
            net_pipe0_to_1.deliver_arrived(tick, |sender, frame, act| {
                peer1.receive_remote_action(sender, frame, act);
            });
            net_pipe1_to_0.deliver_arrived(tick, |sender, frame, act| {
                peer0.receive_remote_action(sender, frame, act);
            });
        }

        // Flush remaining in-flight network packets
        for flush_tick in 50..65 {
            net_pipe0_to_1.deliver_arrived(flush_tick, |sender, frame, act| {
                peer1.receive_remote_action(sender, frame, act);
            });
            net_pipe1_to_0.deliver_arrived(flush_tick, |sender, frame, act| {
                peer0.receive_remote_action(sender, frame, act);
            });
        }

        // Both peers must have performed rollbacks successfully
        assert!(peer0.total_rollbacks > 0, "Peer 0 should have rolled back on remote input");
        assert!(peer1.total_rollbacks > 0, "Peer 1 should have rolled back on remote input");
        assert_eq!(peer0.current_frame, 50);
        assert_eq!(peer1.current_frame, 50);

        // 100% BIT-FOR-BIT IDENTICAL MASTER GRID STATE HASH VERIFICATION!
        let hash0 = peer0.state_hash();
        let hash1 = peer1.state_hash();
        assert_eq!(
            hash0, hash1,
            "Peer 0 and Peer 1 must reach 100% bit-identical state sync despite 100ms latency!"
        );

        // Verify specific tile entities are identical on both peers
        let cell0_smelter = peer0.current_grid.get(2, 2).unwrap();
        let cell1_smelter = peer1.current_grid.get(2, 2).unwrap();
        assert_eq!(cell0_smelter, cell1_smelter);
        assert_eq!(cell0_smelter.type_id, 10);
        // Recipe completed 2 full cycles (4 ore -> 2 ingots)
        assert_eq!(cell0_smelter.count_item(2), 2);
        assert_eq!(cell0_smelter.count_item(1), 0);

        let cell0_chest = peer0.current_grid.get(5, 5).unwrap();
        let cell1_chest = peer1.current_grid.get(5, 5).unwrap();
        assert_eq!(cell0_chest, cell1_chest);
        assert_eq!(cell0_chest.type_id, 20);
        assert_eq!(cell0_chest.count_item(3), 10);

        let cell0_wall = peer0.current_grid.get(1, 1).unwrap();
        let cell1_wall = peer1.current_grid.get(1, 1).unwrap();
        assert_eq!(cell0_wall, cell1_wall);
        assert_eq!(cell0_wall.type_id, 30);
    }

    #[test]
    fn test_grid_rollback_boxed_allocation() {
        let recipes = RecipeRegistry::<16>::new();
        let initial_grid = TileGrid::<16, 16>::new();
        let mut session = GridRollbackSession::<16, 16, 16, 2, 16>::new_boxed(0, initial_grid, recipes);

        let stats = session.advance_frame(GridAction::set_tile(0, 3, 3, 99, 100));
        assert_eq!(stats.current_frame, 1);
        assert_eq!(session.current_grid.get(3, 3).unwrap().type_id, 99);
    }
}
