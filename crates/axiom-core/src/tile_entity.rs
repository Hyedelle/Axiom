//! Axiom Core — Data-Driven Tile Entity & Spatial Grid Component Pipeline (T-047)
//!
//! Provides cache-line aligned (64-byte) generic spatial tile cells and fixed-slab spatial grid
//! data structures completely decoupled from hardcoded game types or domains.

use crate::schema::{category, RecipeRegistry, TypeRegistry};

/// 64-Byte Cache-Line Aligned Generic Tile Entity Cell.
///
/// Designed to represent any world entity located on a spatial grid (belts, structures,
/// drills, chests, walls, turrets, crop soil, etc.) purely via integer `type_id`
/// and data-driven attributes without dynamic allocation.
#[repr(C, align(64))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TileCell {
    /// Schema Type identifier (0 = Empty/Void, referencing a `TypeDefinition`).
    pub type_id: u32,
    /// Bitflags representing runtime state (Active, Powered, Blocked, Jammed, etc.).
    pub state_flags: u32,
    /// Current durability / hitpoints.
    pub hp: u16,
    /// Maximum durability / hitpoints.
    pub max_hp: u16,
    /// Internal work progress ticks (for recipes, crafting, mining, or growth cycles).
    pub work_progress: u16,
    /// Assigned Recipe identifier (0 = None / auto-detect from inputs).
    pub recipe_id: u16,
    /// Fixed 4-slot inventory item types (0 = Empty).
    pub inventory_types: [u16; 4],
    /// Fixed 4-slot inventory item counts.
    pub inventory_counts: [u16; 4],
    /// 32-Byte generic custom payload for domain-specific state (direction, ammo, cooldown, worker ID, etc.).
    pub custom_payload: [u8; 32],
}

// Compile-time assertion: TileCell must be exactly 64 bytes for L1 cache alignment.
const _: () = assert!(core::mem::size_of::<TileCell>() == 64);

/// Tile State Bitflags
pub mod tile_flags {
    pub const ACTIVE: u32 = 1 << 0;
    pub const POWERED: u32 = 1 << 1;
    pub const BLOCKED: u32 = 1 << 2;
    pub const JAMMED: u32 = 1 << 3;
    pub const DAMAGED: u32 = 1 << 4;
    pub const LOGISTICS_SOURCE: u32 = 1 << 5;
    pub const LOGISTICS_SINK: u32 = 1 << 6;
    pub const PLAYER_PLACED: u32 = 1 << 7;
}

/// Discrete Grid Action Kinds for Deterministic Multiplayer Synchronization (T-051)
pub mod grid_action_kind {
    pub const NONE: u16 = 0;
    pub const SET_TILE: u16 = 1;
    pub const CLEAR_TILE: u16 = 2;
    pub const PUSH_ITEM: u16 = 3;
    pub const POP_ITEM: u16 = 4;
    pub const SET_RECIPE: u16 = 5;
    pub const SET_FLAGS: u16 = 6;
    pub const DAMAGE_TILE: u16 = 7;
}

/// 16-Byte Cache-Aligned Grid Action Record for Deterministic Network Synchronization.
#[repr(C, align(16))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct GridAction {
    pub kind: u16,
    pub player_id: u16,
    pub x: u16,
    pub y: u16,
    pub type_id: u32,
    pub param1: u16, // recipe_id / item_type / flags
    pub param2: u16, // count / hp / damage
}

// Compile-time assertion: GridAction must be exactly 16 bytes for zero-waste network packets.
const _: () = assert!(core::mem::size_of::<GridAction>() == 16);
const _: () = assert!(core::mem::align_of::<GridAction>() == 16);

impl GridAction {
    pub const NONE: Self = Self {
        kind: grid_action_kind::NONE,
        player_id: 0,
        x: 0,
        y: 0,
        type_id: 0,
        param1: 0,
        param2: 0,
    };

    pub const fn new(kind: u16, player_id: u16, x: u16, y: u16, type_id: u32, param1: u16, param2: u16) -> Self {
        Self {
            kind,
            player_id,
            x,
            y,
            type_id,
            param1,
            param2,
        }
    }

    pub const fn set_tile(player_id: u16, x: u16, y: u16, type_id: u32, hp: u16) -> Self {
        Self {
            kind: grid_action_kind::SET_TILE,
            player_id,
            x,
            y,
            type_id,
            param1: 0,
            param2: hp,
        }
    }

    pub const fn clear_tile(player_id: u16, x: u16, y: u16) -> Self {
        Self {
            kind: grid_action_kind::CLEAR_TILE,
            player_id,
            x,
            y,
            type_id: 0,
            param1: 0,
            param2: 0,
        }
    }

    pub const fn push_item(player_id: u16, x: u16, y: u16, item_type: u16, count: u16) -> Self {
        Self {
            kind: grid_action_kind::PUSH_ITEM,
            player_id,
            x,
            y,
            type_id: 0,
            param1: item_type,
            param2: count,
        }
    }

    pub const fn pop_item(player_id: u16, x: u16, y: u16, item_type: u16, count: u16) -> Self {
        Self {
            kind: grid_action_kind::POP_ITEM,
            player_id,
            x,
            y,
            type_id: 0,
            param1: item_type,
            param2: count,
        }
    }

    pub const fn set_recipe(player_id: u16, x: u16, y: u16, recipe_id: u16) -> Self {
        Self {
            kind: grid_action_kind::SET_RECIPE,
            player_id,
            x,
            y,
            type_id: 0,
            param1: recipe_id,
            param2: 0,
        }
    }

    pub const fn set_flags(player_id: u16, x: u16, y: u16, flags: u32) -> Self {
        Self {
            kind: grid_action_kind::SET_FLAGS,
            player_id,
            x,
            y,
            type_id: 0,
            param1: flags as u16,
            param2: (flags >> 16) as u16,
        }
    }

    pub const fn damage_tile(player_id: u16, x: u16, y: u16, damage: u16) -> Self {
        Self {
            kind: grid_action_kind::DAMAGE_TILE,
            player_id,
            x,
            y,
            type_id: 0,
            param1: 0,
            param2: damage,
        }
    }
}

impl TileCell {
    /// Creates an empty tile cell.
    pub const fn empty() -> Self {
        Self {
            type_id: 0,
            state_flags: 0,
            hp: 0,
            max_hp: 0,
            work_progress: 0,
            recipe_id: 0,
            inventory_types: [0; 4],
            inventory_counts: [0; 4],
            custom_payload: [0; 32],
        }
    }

    /// Creates a tile cell with an assigned type and maximum HP.
    pub const fn new(type_id: u32, max_hp: u16) -> Self {
        Self {
            type_id,
            state_flags: tile_flags::ACTIVE,
            hp: max_hp,
            max_hp,
            work_progress: 0,
            recipe_id: 0,
            inventory_types: [0; 4],
            inventory_counts: [0; 4],
            custom_payload: [0; 32],
        }
    }

    /// Returns true if this cell is empty (type_id == 0).
    #[inline(always)]
    pub const fn is_empty(&self) -> bool {
        self.type_id == 0
    }

    /// Clears the cell back to empty state.
    #[inline(always)]
    pub fn clear(&mut self) {
        *self = Self::empty();
    }

    /// Checks if a specific state flag is set.
    #[inline(always)]
    pub const fn has_flag(&self, flag: u32) -> bool {
        (self.state_flags & flag) != 0
    }

    /// Sets or unsets a state flag.
    #[inline(always)]
    pub fn set_flag(&mut self, flag: u32, enabled: bool) {
        if enabled {
            self.state_flags |= flag;
        } else {
            self.state_flags &= !flag;
        }
    }

    /// Returns the total count of a specific item type in this cell's inventory.
    pub fn count_item(&self, item_type: u16) -> u16 {
        if item_type == 0 {
            return 0;
        }
        let mut total = 0u16;
        for i in 0..4 {
            if self.inventory_types[i] == item_type {
                total = total.saturating_add(self.inventory_counts[i]);
            }
        }
        total
    }

    /// Adds items to the cell inventory. Returns true if successfully inserted.
    pub fn push_item(&mut self, item_type: u16, count: u16) -> bool {
        if item_type == 0 || count == 0 {
            return false;
        }

        // Try existing slot with matching item type
        for i in 0..4 {
            if self.inventory_types[i] == item_type {
                self.inventory_counts[i] = self.inventory_counts[i].saturating_add(count);
                return true;
            }
        }

        // Try empty slot
        for i in 0..4 {
            if self.inventory_types[i] == 0 || self.inventory_counts[i] == 0 {
                self.inventory_types[i] = item_type;
                self.inventory_counts[i] = count;
                return true;
            }
        }

        false
    }

    /// Removes items from the cell inventory. Returns true if the full count was removed.
    pub fn pop_item(&mut self, item_type: u16, count: u16) -> bool {
        if item_type == 0 || count == 0 {
            return false;
        }

        if self.count_item(item_type) < count {
            return false;
        }

        let mut remaining = count;
        for i in 0..4 {
            if self.inventory_types[i] == item_type {
                if self.inventory_counts[i] <= remaining {
                    remaining -= self.inventory_counts[i];
                    self.inventory_types[i] = 0;
                    self.inventory_counts[i] = 0;
                } else {
                    self.inventory_counts[i] -= remaining;
                    break;
                }
            }
        }

        true
    }
}

/// Generic Fixed-Slab Tile Entity Grid.
///
/// L1/L2 cache friendly 2D grid of 64-byte `TileCell`s stored in a flat contiguous row-major array.
/// Completely heap-free, zero-allocation, deterministic.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct TileGrid<const W: usize, const H: usize> {
    pub cells: [[TileCell; W]; H],
}

impl<const W: usize, const H: usize> TileGrid<W, H> {
    /// Creates a new tile grid filled with empty cells.
    pub const fn new() -> Self {
        Self {
            cells: [[TileCell::empty(); W]; H],
        }
    }

    /// Returns an immutable reference to the cell at (x, y).
    #[inline(always)]
    pub fn get(&self, x: usize, y: usize) -> Option<&TileCell> {
        if x < W && y < H {
            Some(&self.cells[y][x])
        } else {
            None
        }
    }

    /// Returns a mutable reference to the cell at (x, y).
    #[inline(always)]
    pub fn get_mut(&mut self, x: usize, y: usize) -> Option<&mut TileCell> {
        if x < W && y < H {
            Some(&mut self.cells[y][x])
        } else {
            None
        }
    }

    /// Sets the cell at (x, y). Returns true if within bounds.
    #[inline(always)]
    pub fn set(&mut self, x: usize, y: usize, cell: TileCell) -> bool {
        if x < W && y < H {
            self.cells[y][x] = cell;
            true
        } else {
            false
        }
    }

    /// Clears the cell at (x, y). Returns true if within bounds.
    #[inline(always)]
    pub fn clear_at(&mut self, x: usize, y: usize) -> bool {
        if x < W && y < H {
            self.cells[y][x].clear();
            true
        } else {
            false
        }
    }

    /// Applies a deterministic grid action. Returns true if executed successfully.
    pub fn apply_action(&mut self, action: &GridAction) -> bool {
        let x = action.x as usize;
        let y = action.y as usize;
        if x >= W || y >= H {
            return false;
        }

        match action.kind {
            grid_action_kind::NONE => true,
            grid_action_kind::SET_TILE => {
                let mut cell = TileCell::new(action.type_id, action.param2);
                cell.state_flags |= tile_flags::PLAYER_PLACED;
                cell.custom_payload[0] = action.player_id as u8;
                cell.custom_payload[1] = action.param1 as u8;
                self.set(x, y, cell)
            }
            grid_action_kind::CLEAR_TILE => self.clear_at(x, y),
            grid_action_kind::PUSH_ITEM => {
                if let Some(cell) = self.get_mut(x, y) {
                    cell.push_item(action.param1, action.param2)
                } else {
                    false
                }
            }
            grid_action_kind::POP_ITEM => {
                if let Some(cell) = self.get_mut(x, y) {
                    cell.pop_item(action.param1, action.param2)
                } else {
                    false
                }
            }
            grid_action_kind::SET_RECIPE => {
                if let Some(cell) = self.get_mut(x, y) {
                    cell.recipe_id = action.param1;
                    true
                } else {
                    false
                }
            }
            grid_action_kind::SET_FLAGS => {
                if let Some(cell) = self.get_mut(x, y) {
                    let flags = (action.param1 as u32) | ((action.param2 as u32) << 16);
                    cell.state_flags = flags;
                    true
                } else {
                    false
                }
            }
            grid_action_kind::DAMAGE_TILE => {
                if let Some(cell) = self.get_mut(x, y) {
                    cell.hp = cell.hp.saturating_sub(action.param2);
                    if cell.hp == 0 {
                        cell.clear();
                    }
                    true
                } else {
                    false
                }
            }
            _ => false,
        }
    }

    /// Retrieves 4 orthogonal neighbors (North, South, East, West) within bounds.
    pub fn neighbors_4(&self, x: usize, y: usize) -> [(Option<(usize, usize)>, Option<&TileCell>); 4] {
        let coords: [(isize, isize); 4] = [
            (x as isize, y as isize - 1), // North
            (x as isize, y as isize + 1), // South
            (x as isize + 1, y as isize), // East
            (x as isize - 1, y as isize), // West
        ];

        let mut res = [(None, None); 4];
        for (i, &(nx, ny)) in coords.iter().enumerate() {
            if nx >= 0 && nx < W as isize && ny >= 0 && ny < H as isize {
                let ux = nx as usize;
                let uy = ny as usize;
                res[i] = (Some((ux, uy)), self.get(ux, uy));
            }
        }
        res
    }

    /// Iterates over all cells with coordinate context for deterministic world updates.
    pub fn tick_all<F>(&mut self, mut tick_fn: F)
    where
        F: FnMut(usize, usize, &mut TileCell),
    {
        for y in 0..H {
            for x in 0..W {
                tick_fn(x, y, &mut self.cells[y][x]);
            }
        }
    }

    /// Processes data-driven crafting recipes across all active cells using the given RecipeRegistry.
    ///
    /// If a cell has inputs satisfying its assigned `recipe_id` (or matching recipe),
    /// progress accumulates until `duration_ticks`, consuming inputs and producing outputs.
    pub fn process_recipes<const RECIPE_CAP: usize>(&mut self, recipes: &RecipeRegistry<RECIPE_CAP>) {
        for row in self.cells.iter_mut() {
            for cell in row.iter_mut() {
                if cell.is_empty() || !cell.has_flag(tile_flags::ACTIVE) {
                    continue;
                }

                let recipe = if cell.recipe_id != 0 {
                    recipes.find_by_id(cell.recipe_id)
                } else {
                    // Auto-detect recipe based on first inventory item
                    let first_item = cell.inventory_types.iter().find(|&&t| t != 0).copied().unwrap_or(0);
                    if first_item != 0 {
                        recipes.find_by_input(first_item)
                    } else {
                        None
                    }
                };

                if let Some(r) = recipe {
                    // Check if inputs are available
                    let can_input1 = r.input_id == 0 || cell.count_item(r.input_id) >= r.input_count;
                    let can_input2 = r.secondary_input_id == 0 || cell.count_item(r.secondary_input_id) >= r.secondary_input_count;

                    if can_input1 && can_input2 {
                        cell.work_progress = cell.work_progress.saturating_add(1);
                        if cell.work_progress >= r.required_ticks {
                            // Produce recipe
                            cell.work_progress = 0;
                            if r.input_id != 0 {
                                cell.pop_item(r.input_id, r.input_count);
                            }
                            if r.secondary_input_id != 0 {
                                cell.pop_item(r.secondary_input_id, r.secondary_input_count);
                            }
                            if r.output_id != 0 {
                                cell.push_item(r.output_id, r.output_count);
                            }
                        }
                    }
                }
            }
        }
    }

    /// Advances the generic data-driven simulation across the entire grid by 1 tick:
    ///
    /// 1. **Producers**: Extract / generate items according to their `base_speed`.
    /// 2. **Transporters**: Move items forward into neighboring tiles (2-phase conflict-free buffer).
    /// 3. **Recipes**: Process item transformations via `RecipeRegistry`.
    /// 4. **Defenders**: Engage enemy targets within weapon `range`.
    pub fn step_simulation<const TYPE_CAP: usize, const RECIPE_CAP: usize>(
        &mut self,
        types: &TypeRegistry<TYPE_CAP>,
        recipes: &RecipeRegistry<RECIPE_CAP>,
    ) {
        // --- Phase 1: Producers ---
        for y in 0..H {
            for x in 0..W {
                let cell = self.cells[y][x];
                if cell.is_empty() || !cell.has_flag(tile_flags::ACTIVE) {
                    continue;
                }

                if let Some(tdef) = types.get(cell.type_id as u16) {
                    if tdef.has_category(category::PRODUCER)
                        && !tdef.has_category(category::CONSUMER)
                    {
                        let target_item = u16::from_le_bytes([cell.custom_payload[2], cell.custom_payload[3]]);
                        if target_item != 0 {
                            let speed = tdef.base_speed.max(1);
                            let new_progress = cell.work_progress.saturating_add(speed);
                            if new_progress >= 100 {
                                self.cells[y][x].work_progress = 0;
                                let dir = cell.custom_payload[0];
                                let fwd = match dir {
                                    0 if y > 0 => Some((x, y - 1)),
                                    1 if x + 1 < W => Some((x + 1, y)),
                                    2 if y + 1 < H => Some((x, y + 1)),
                                    3 if x > 0 => Some((x - 1, y)),
                                    _ => None,
                                };
                                let mut pushed = false;
                                if let Some((fx, fy)) = fwd {
                                    if !self.cells[fy][fx].is_empty() && self.cells[fy][fx].has_flag(tile_flags::ACTIVE) {
                                        pushed = self.cells[fy][fx].push_item(target_item, 1);
                                    }
                                }
                                if !pushed {
                                    self.cells[y][x].push_item(target_item, 1);
                                }
                            } else {
                                self.cells[y][x].work_progress = new_progress;
                            }
                        }
                    }
                }
            }
        }

        // --- Phase 2: Transporters (Conveyors & Inserters) ---
        const MAX_TRANSFERS: usize = 256;
        let mut transfers = [(0u16, 0u16, 0u16, 0u16, 0u16); MAX_TRANSFERS];
        let mut transfer_count = 0;

        for y in 0..H {
            for x in 0..W {
                let cell = &self.cells[y][x];
                if cell.is_empty() || !cell.has_flag(tile_flags::ACTIVE) {
                    continue;
                }

                if let Some(tdef) = types.get(cell.type_id as u16) {
                    if tdef.has_category(category::TRANSPORTER) {
                        let dir = cell.custom_payload[0];
                        let is_pull_mode = cell.custom_payload[1] == 1;

                        // Check items held
                        let mut item_held = 0u16;
                        for i in 0..4 {
                            if cell.inventory_types[i] != 0 && cell.inventory_counts[i] > 0 {
                                item_held = cell.inventory_types[i];
                                break;
                            }
                        }

                        if is_pull_mode && item_held == 0 {
                            // Inserter: pull item from behind (opposite direction)
                            let back_dir = (dir + 2) % 4;
                            let back_pos = match back_dir {
                                0 if y > 0 => Some((x, y - 1)),
                                1 if x + 1 < W => Some((x + 1, y)),
                                2 if y + 1 < H => Some((x, y + 1)),
                                3 if x > 0 => Some((x - 1, y)),
                                _ => None,
                            };
                            if let Some((bx, by)) = back_pos {
                                let back_cell = &self.cells[by][bx];
                                for i in 0..4 {
                                    if back_cell.inventory_types[i] != 0 && back_cell.inventory_counts[i] > 0 {
                                        let cand = back_cell.inventory_types[i];
                                        if transfer_count < MAX_TRANSFERS {
                                            transfers[transfer_count] = (bx as u16, by as u16, x as u16, y as u16, cand);
                                            transfer_count += 1;
                                            break;
                                        }
                                    }
                                }
                            }
                        } else if item_held != 0 {
                            // Forward push to neighbor in front
                            let fwd_pos = match dir {
                                0 if y > 0 => Some((x, y - 1)),
                                1 if x + 1 < W => Some((x + 1, y)),
                                2 if y + 1 < H => Some((x, y + 1)),
                                3 if x > 0 => Some((x - 1, y)),
                                _ => None,
                            };
                            if let Some((fx, fy)) = fwd_pos {
                                let dest = &self.cells[fy][fx];
                                if !dest.is_empty() && dest.has_flag(tile_flags::ACTIVE) {
                                    if transfer_count < MAX_TRANSFERS {
                                        transfers[transfer_count] = (x as u16, y as u16, fx as u16, fy as u16, item_held);
                                        transfer_count += 1;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // Apply transfers
        for i in 0..transfer_count {
            let (fx, fy, tx, ty, item) = transfers[i];
            let fx = fx as usize;
            let fy = fy as usize;
            let tx = tx as usize;
            let ty = ty as usize;

            if self.cells[ty][tx].push_item(item, 1) {
                self.cells[fy][fx].pop_item(item, 1);
            }
        }

        // --- Phase 3: Recipes Transformation ---
        self.process_recipes(recipes);

        // --- Phase 4: Defensive & Combatant ---
        for y in 0..H {
            for x in 0..W {
                let cell = &mut self.cells[y][x];
                if cell.is_empty() || !cell.has_flag(tile_flags::ACTIVE) {
                    continue;
                }

                if let Some(tdef) = types.get(cell.type_id as u16) {
                    if tdef.has_category(category::COMBATANT)
                        && tdef.range > 0
                        && tdef.base_power > 0
                    {
                        let cooldown = tdef.cooldown.max(1);
                        let timer = &mut cell.custom_payload[8];
                        *timer = timer.saturating_add(1);
                        if *timer as u16 >= cooldown {
                            *timer = 0;
                            let r = tdef.range as isize;
                            let mut target_found = false;
                            for dy in -r..=r {
                                if target_found { break; }
                                for dx in -r..=r {
                                    if dx == 0 && dy == 0 { continue; }
                                    let tx = x as isize + dx;
                                    let ty = y as isize + dy;
                                    if tx >= 0 && tx < W as isize && ty >= 0 && ty < H as isize {
                                        let target = &mut self.cells[ty as usize][tx as usize];
                                        if !target.is_empty() && target.has_flag(tile_flags::DAMAGED) {
                                            target.hp = target.hp.saturating_sub(tdef.base_power);
                                            if target.hp == 0 {
                                                target.clear();
                                            }
                                            target_found = true;
                                            break;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    /// Computes a 64-bit FNV-1a hash of the entire grid state for deterministic synchronization verification.
    pub fn state_hash(&self) -> u64 {
        const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
        const FNV_PRIME: u64 = 0x100000001b3;

        let mut hash = FNV_OFFSET_BASIS;
        let bytes = unsafe {
            core::slice::from_raw_parts(
                self.cells.as_ptr() as *const u8,
                core::mem::size_of_val(&self.cells),
            )
        };

        for &b in bytes {
            hash ^= b as u64;
            hash = hash.wrapping_mul(FNV_PRIME);
        }
        hash
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{RecipeDefinition, RecipeRegistry, TypeDefinition};

    #[test]
    fn test_tile_cell_size_and_alignment() {
        assert_eq!(core::mem::size_of::<TileCell>(), 64);
        assert_eq!(core::mem::align_of::<TileCell>(), 64);
    }

    #[test]
    fn test_tile_cell_inventory_operations() {
        let mut cell = TileCell::new(100, 500);
        assert_eq!(cell.count_item(10), 0);

        // Push item 10 x 5
        assert!(cell.push_item(10, 5));
        assert_eq!(cell.count_item(10), 5);

        // Push item 10 x 3 (stacks)
        assert!(cell.push_item(10, 3));
        assert_eq!(cell.count_item(10), 8);

        // Pop item 10 x 4
        assert!(cell.pop_item(10, 4));
        assert_eq!(cell.count_item(10), 4);

        // Cannot pop more than available
        assert!(!cell.pop_item(10, 10));
        assert_eq!(cell.count_item(10), 4);
    }

    #[test]
    fn test_tile_grid_operations() {
        let mut grid = TileGrid::<16, 16>::new();
        assert_eq!(grid.cells.len() * grid.cells[0].len(), 256);

        let cell = TileCell::new(42, 100);
        assert!(grid.set(5, 5, cell));

        let retrieved = grid.get(5, 5).unwrap();
        assert_eq!(retrieved.type_id, 42);
        assert_eq!(retrieved.hp, 100);

        // Out of bounds
        assert!(grid.get(16, 5).is_none());
        assert!(!grid.set(16, 5, cell));

        // Neighbors check
        let neighbors = grid.neighbors_4(5, 5);
        assert!(neighbors[0].0.is_some()); // North (5, 4)
        assert!(neighbors[1].0.is_some()); // South (5, 6)
        assert!(neighbors[2].0.is_some()); // East (6, 5)
        assert!(neighbors[3].0.is_some()); // West (4, 5)
    }

    #[test]
    fn test_tile_grid_recipe_pipeline() {
        let mut grid = TileGrid::<4, 4>::new();
        let mut recipes = RecipeRegistry::<16>::new();

        // Register Recipe: 2x Iron Ore (Type 1) -> 1x Iron Ingot (Type 2) in 3 ticks
        let mut r = RecipeDefinition::empty();
        r.id = 1;
        r.input_id = 1;
        r.input_count = 2;
        r.output_id = 2;
        r.output_count = 1;
        r.required_ticks = 3;
        assert!(recipes.register(r));

        // Place Smelter-like cell at (1, 1) with 4x Iron Ore
        let mut smelter = TileCell::new(10, 1000);
        smelter.push_item(1, 4);
        grid.set(1, 1, smelter);

        // Tick 1
        grid.process_recipes(&recipes);
        assert_eq!(grid.get(1, 1).unwrap().count_item(2), 0);
        assert_eq!(grid.get(1, 1).unwrap().work_progress, 1);

        // Tick 2
        grid.process_recipes(&recipes);
        assert_eq!(grid.get(1, 1).unwrap().count_item(2), 0);
        assert_eq!(grid.get(1, 1).unwrap().work_progress, 2);

        // Tick 3: Recipe completes! 2 Iron Ore consumed, 1 Iron Ingot produced
        grid.process_recipes(&recipes);
        assert_eq!(grid.get(1, 1).unwrap().count_item(2), 1);
        assert_eq!(grid.get(1, 1).unwrap().count_item(1), 2);
        assert_eq!(grid.get(1, 1).unwrap().work_progress, 0);
    }

    #[test]
    fn test_grid_state_hash_determinism() {
        let mut g1 = TileGrid::<8, 8>::new();
        let mut g2 = TileGrid::<8, 8>::new();

        assert_eq!(g1.state_hash(), g2.state_hash());

        g1.set(2, 3, TileCell::new(12, 100));
        g2.set(2, 3, TileCell::new(12, 100));

        assert_eq!(g1.state_hash(), g2.state_hash());
    }

    #[test]
    fn test_grid_action_application() {
        let mut grid = TileGrid::<8, 8>::new();

        // 1. Place building via GridAction
        let place_act = GridAction::set_tile(1, 2, 2, 10, 500);
        assert!(grid.apply_action(&place_act));
        let cell = grid.get(2, 2).unwrap();
        assert_eq!(cell.type_id, 10);
        assert_eq!(cell.hp, 500);
        assert!(cell.has_flag(tile_flags::PLAYER_PLACED));

        // 2. Push items
        let push_act = GridAction::push_item(1, 2, 2, 5, 20);
        assert!(grid.apply_action(&push_act));
        assert_eq!(grid.get(2, 2).unwrap().count_item(5), 20);

        // 3. Pop items
        let pop_act = GridAction::pop_item(1, 2, 2, 5, 5);
        assert!(grid.apply_action(&pop_act));
        assert_eq!(grid.get(2, 2).unwrap().count_item(5), 15);

        // 4. Set recipe
        let recipe_act = GridAction::set_recipe(1, 2, 2, 42);
        assert!(grid.apply_action(&recipe_act));
        assert_eq!(grid.get(2, 2).unwrap().recipe_id, 42);

        // 5. Damage tile
        let dmg_act = GridAction::damage_tile(1, 2, 2, 200);
        assert!(grid.apply_action(&dmg_act));
        assert_eq!(grid.get(2, 2).unwrap().hp, 300);

        // 6. Clear tile
        let clear_act = GridAction::clear_tile(1, 2, 2);
        assert!(grid.apply_action(&clear_act));
        assert!(grid.get(2, 2).unwrap().is_empty());
    }

    #[test]
    fn test_data_driven_production_and_logistics_pipeline() {
        let mut types = TypeRegistry::<32>::new();
        let mut recipes = RecipeRegistry::<16>::new();

        // Type 10: Mining Drill (PRODUCER, speed 100)
        let mut drill = TypeDefinition::empty();
        drill.id = 10;
        drill.category_flags = category::SOLID | category::PRODUCER;
        drill.base_speed = 100;
        types.register(drill);

        // Type 11: Conveyor Belt (TRANSPORTER, mode 0 = push)
        let mut belt = TypeDefinition::empty();
        belt.id = 11;
        belt.category_flags = category::PASSABLE | category::TRANSPORTER;
        types.register(belt);

        // Type 12: Smelter Furnace (CONSUMER | PRODUCER, recipe transforms IronOre 101 -> IronPlate 201)
        let mut smelter = TypeDefinition::empty();
        smelter.id = 12;
        smelter.category_flags = category::SOLID | category::CONSUMER | category::PRODUCER;
        types.register(smelter);

        // Type 13: Inserter Arm (TRANSPORTER, mode 1 = pull from behind, push forward)
        let mut inserter = TypeDefinition::empty();
        inserter.id = 13;
        inserter.category_flags = category::SOLID | category::TRANSPORTER;
        types.register(inserter);

        // Type 14: Storage Chest (CONTAINER)
        let mut chest = TypeDefinition::empty();
        chest.id = 14;
        chest.category_flags = category::SOLID | category::CONTAINER;
        types.register(chest);

        // Recipe: 1 IronOre (101) -> 1 IronPlate (201) in 2 ticks
        let mut recipe = RecipeDefinition::empty();
        recipe.id = 1;
        recipe.input_id = 101;
        recipe.input_count = 1;
        recipe.output_id = 201;
        recipe.output_count = 1;
        recipe.required_ticks = 2;
        recipes.register(recipe);

        // Build layout on a 6x1 grid:
        // (0,0): Drill (facing East, target item 101)
        // (1,0): Belt (facing East, mode 0)
        // (2,0): Smelter (Recipe 1)
        // (3,0): Inserter (facing East, mode 1)
        // (4,0): Chest
        let mut grid = TileGrid::<6, 1>::new();

        let mut drill_cell = TileCell::new(10, 300);
        drill_cell.custom_payload[0] = 1; // Facing East
        drill_cell.custom_payload[2..4].copy_from_slice(&101u16.to_le_bytes()); // Target item 101
        grid.set(0, 0, drill_cell);

        let mut belt_cell = TileCell::new(11, 100);
        belt_cell.custom_payload[0] = 1; // Facing East, mode 0
        grid.set(1, 0, belt_cell);

        let mut smelter_cell = TileCell::new(12, 500);
        smelter_cell.recipe_id = 1;
        grid.set(2, 0, smelter_cell);

        let mut inserter_cell = TileCell::new(13, 150);
        inserter_cell.custom_payload[0] = 1; // Facing East
        inserter_cell.custom_payload[1] = 1; // Mode 1: Inserter (pull from behind)
        grid.set(3, 0, inserter_cell);

        let chest_cell = TileCell::new(14, 200);
        grid.set(4, 0, chest_cell);

        // Run simulation for 10 ticks
        for _ in 0..10 {
            grid.step_simulation(&types, &recipes);
        }

        // Verify that IronPlates were produced and stored in Chest (4, 0)
        let chest_content = grid.get(4, 0).unwrap().count_item(201);
        assert!(chest_content > 0, "Chest at (4, 0) should contain at least 1 IronPlate (got {})", chest_content);
    }
}
