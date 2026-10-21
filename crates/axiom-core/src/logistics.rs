//! Deterministic Logistics & Automation Simulation (Phase 2 Gameplay).
//!
//! Provides zero-heap conveyor belts, mining drills, resource deposits,
//! and container storage simulation matching Factorio-grade performance.

pub const CELL_KIND_EMPTY: u8 = 0;
pub const CELL_KIND_ORE: u8 = 1;
pub const CELL_KIND_DRILL: u8 = 2;
pub const CELL_KIND_BELT: u8 = 3;
pub const CELL_KIND_CHEST: u8 = 4;

pub const DIR_NORTH: u8 = 0;
pub const DIR_EAST: u8 = 1;
pub const DIR_SOUTH: u8 = 2;
pub const DIR_WEST: u8 = 3;

pub const ITEM_NONE: u8 = 0;
pub const ITEM_IRON_ORE: u8 = 1;
pub const ITEM_COAL: u8 = 2;
pub const ITEM_COPPER_ORE: u8 = 3;

/// 32-byte cache-aligned logistics tile record.
/// Two cells pack exactly into a single 64-byte L1 CPU cache line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(C, align(32))]
pub struct LogisticsCell {
    pub kind: u8,
    pub direction: u8,
    pub item_type: u8,
    pub _pad1: u8,
    pub progress: u16,
    pub speed: u16,
    pub quantity: u32,
    pub target_x: u16,
    pub target_y: u16,
    pub _reserved: [u8; 16], // Exact 32 bytes
}

impl Default for LogisticsCell {
    fn default() -> Self {
        Self::empty()
    }
}

impl LogisticsCell {
    pub const fn empty() -> Self {
        Self {
            kind: CELL_KIND_EMPTY,
            direction: DIR_NORTH,
            item_type: ITEM_NONE,
            _pad1: 0,
            progress: 0,
            speed: 0,
            quantity: 0,
            target_x: 0,
            target_y: 0,
            _reserved: [0; 16],
        }
    }
}

pub const DEFAULT_FACTORY_W: usize = 64;
pub const DEFAULT_FACTORY_H: usize = 64;

/// 2D Logistics Grid simulating drills, belts, and chests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogisticsGrid<const W: usize = DEFAULT_FACTORY_W, const H: usize = DEFAULT_FACTORY_H> {
    pub cells: [[LogisticsCell; W]; H],
    pub current_tick: u64,
    pub total_mined: u64,
}

impl<const W: usize, const H: usize> Default for LogisticsGrid<W, H> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const W: usize, const H: usize> LogisticsGrid<W, H> {
    pub const fn new() -> Self {
        Self {
            cells: [[LogisticsCell::empty(); W]; H],
            current_tick: 0,
            total_mined: 0,
        }
    }

    /// Places a resource ore deposit on the grid.
    pub fn place_ore(&mut self, x: usize, y: usize, item_type: u8, amount: u32) {
        if x < W && y < H {
            self.cells[y][x] = LogisticsCell {
                kind: CELL_KIND_ORE,
                direction: DIR_NORTH,
                item_type,
                _pad1: 0,
                progress: 0,
                speed: 0,
                quantity: amount,
                target_x: 0,
                target_y: 0,
                _reserved: [0; 16],
            };
        }
    }

    /// Places a mining drill oriented towards a specified output direction.
    pub fn place_drill(&mut self, x: usize, y: usize, direction: u8, speed: u16) {
        if x < W && y < H {
            self.cells[y][x] = LogisticsCell {
                kind: CELL_KIND_DRILL,
                direction,
                item_type: ITEM_NONE,
                _pad1: 0,
                progress: 0,
                speed,
                quantity: 0,
                target_x: 0,
                target_y: 0,
                _reserved: [0; 16],
            };
        }
    }

    /// Places a conveyor belt moving in a direction.
    pub fn place_belt(&mut self, x: usize, y: usize, direction: u8, speed: u16) {
        if x < W && y < H {
            self.cells[y][x] = LogisticsCell {
                kind: CELL_KIND_BELT,
                direction,
                item_type: ITEM_NONE,
                _pad1: 0,
                progress: 0,
                speed,
                quantity: 0,
                target_x: 0,
                target_y: 0,
                _reserved: [0; 16],
            };
        }
    }

    /// Places a storage chest to accumulate items.
    pub fn place_chest(&mut self, x: usize, y: usize) {
        if x < W && y < H {
            self.cells[y][x] = LogisticsCell {
                kind: CELL_KIND_CHEST,
                direction: DIR_NORTH,
                item_type: ITEM_NONE,
                _pad1: 0,
                progress: 0,
                speed: 0,
                quantity: 0,
                target_x: 0,
                target_y: 0,
                _reserved: [0; 16],
            };
        }
    }

    /// Calculates adjacent target coordinates given an orientation direction.
    #[inline(always)]
    pub fn target_coord(x: usize, y: usize, dir: u8) -> Option<(usize, usize)> {
        match dir {
            DIR_NORTH => {
                if y > 0 {
                    Some((x, y - 1))
                } else {
                    None
                }
            }
            DIR_EAST => {
                if x + 1 < W {
                    Some((x + 1, y))
                } else {
                    None
                }
            }
            DIR_SOUTH => {
                if y + 1 < H {
                    Some((x, y + 1))
                } else {
                    None
                }
            }
            DIR_WEST => {
                if x > 0 {
                    Some((x - 1, y))
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// Advances the logistics simulation by 1 discrete tick.
    ///
    /// 1. Belts transport existing items towards destination.
    /// 2. Drills extract ore and deposit onto forward belt.
    pub fn step(&mut self) {
        self.current_tick += 1;

        // 1. Process Conveyor Belts (Back-to-front or two-phase movement to prevent race conditions)
        let mut transfers = [(0usize, 0usize, 0usize, 0usize, 0u8); 256];
        let mut transfer_count = 0;

        for y in 0..H {
            for x in 0..W {
                if self.cells[y][x].kind == CELL_KIND_BELT
                    && self.cells[y][x].item_type != ITEM_NONE
                {
                    self.cells[y][x].progress = self.cells[y][x]
                        .progress
                        .saturating_add(self.cells[y][x].speed);

                    // When item reaches cell boundary (progress >= 100), schedule transfer
                    if self.cells[y][x].progress >= 100 {
                        if let Some((tx, ty)) = Self::target_coord(x, y, self.cells[y][x].direction)
                        {
                            if transfer_count < transfers.len() {
                                transfers[transfer_count] =
                                    (x, y, tx, ty, self.cells[y][x].item_type);
                                transfer_count += 1;
                            }
                        }
                    }
                }
            }
        }

        // Apply belt transfers
        for &(sx, sy, tx, ty, item) in transfers[..transfer_count].iter() {
            match self.cells[ty][tx].kind {
                CELL_KIND_BELT => {
                    // Transfer to next belt if empty
                    if self.cells[ty][tx].item_type == ITEM_NONE {
                        self.cells[ty][tx].item_type = item;
                        self.cells[ty][tx].progress = 0;
                        self.cells[sy][sx].item_type = ITEM_NONE;
                        self.cells[sy][sx].progress = 0;
                    }
                }
                CELL_KIND_CHEST => {
                    // Deposit directly into chest
                    self.cells[ty][tx].item_type = item;
                    self.cells[ty][tx].quantity = self.cells[ty][tx].quantity.saturating_add(1);
                    self.cells[sy][sx].item_type = ITEM_NONE;
                    self.cells[sy][sx].progress = 0;
                }
                _ => {}
            }
        }

        // 2. Process Mining Drills
        for y in 0..H {
            for x in 0..W {
                if self.cells[y][x].kind == CELL_KIND_DRILL {
                    self.cells[y][x].progress = self.cells[y][x]
                        .progress
                        .saturating_add(self.cells[y][x].speed);

                    // When mining cycle completes (progress >= 100)
                    if self.cells[y][x].progress >= 100 {
                        if let Some((tx, ty)) = Self::target_coord(x, y, self.cells[y][x].direction)
                        {
                            // Output iron ore onto target belt if empty
                            if self.cells[ty][tx].kind == CELL_KIND_BELT
                                && self.cells[ty][tx].item_type == ITEM_NONE
                            {
                                self.cells[ty][tx].item_type = ITEM_IRON_ORE;
                                self.cells[ty][tx].progress = 0;
                                self.cells[y][x].progress = 0;
                                self.total_mined += 1;
                            }
                        }
                    }
                }
            }
        }
    }

    /// Counts total items currently in transit on belts.
    pub fn total_belt_items(&self) -> u32 {
        let mut count = 0;
        for y in 0..H {
            for x in 0..W {
                if self.cells[y][x].kind == CELL_KIND_BELT
                    && self.cells[y][x].item_type != ITEM_NONE
                {
                    count += 1;
                }
            }
        }
        count
    }

    /// Retrieves stored item quantity in a chest.
    pub fn get_chest_quantity(&self, x: usize, y: usize) -> u32 {
        if x < W && y < H && self.cells[y][x].kind == CELL_KIND_CHEST {
            self.cells[y][x].quantity
        } else {
            0
        }
    }

    /// Computes deterministic hash across logistics grid cells and inventory.
    pub fn compute_hash(&self) -> u64 {
        let mut h = self.current_tick.wrapping_mul(0x9E3779B97F4A7C15) ^ self.total_mined;
        for row in &self.cells {
            for cell in row {
                if cell.kind != CELL_KIND_EMPTY {
                    h ^= crate::hash::deterministic_hash(cell);
                    h = h.rotate_left(5);
                }
            }
        }
        h
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::{align_of, size_of};

    #[test]
    fn test_logistics_cell_geometry() {
        assert_eq!(
            size_of::<LogisticsCell>(),
            32,
            "LogisticsCell must be exactly 32 bytes"
        );
        assert_eq!(
            align_of::<LogisticsCell>(),
            32,
            "LogisticsCell must be 32-byte aligned"
        );
    }

    #[test]
    fn test_drill_to_belt_to_chest_pipeline() {
        let mut factory = LogisticsGrid::<16, 16>::new();

        // Layout:
        // (0, 1): Drill facing East (DIR_EAST)
        // (1, 1): Belt facing East
        // (2, 1): Belt facing East
        // (3, 1): Chest
        factory.place_drill(0, 1, DIR_EAST, 50); // Speed 50: mines 1 ore every 2 ticks
        factory.place_belt(1, 1, DIR_EAST, 100); // Speed 100: moves 1 tile per tick
        factory.place_belt(2, 1, DIR_EAST, 100);
        factory.place_chest(3, 1);

        // Run 10 ticks
        for _ in 0..10 {
            factory.step();
        }

        // Chest must have collected mined items
        let collected = factory.get_chest_quantity(3, 1);
        assert!(
            collected > 0,
            "Chest must have accumulated mined ore. Collected: {}",
            collected
        );
        assert!(factory.total_mined > 0);
    }
}
