//! Axiom Core — Data-Driven Tile Entity & Spatial Grid Component Pipeline (T-047)
//!
//! Provides cache-line aligned (64-byte) generic spatial tile cells and fixed-slab spatial grid
//! data structures completely decoupled from hardcoded game types or domains.

use crate::schema::RecipeRegistry;

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
#[derive(Clone)]
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
    use crate::schema::{RecipeDefinition, RecipeRegistry};

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
}
