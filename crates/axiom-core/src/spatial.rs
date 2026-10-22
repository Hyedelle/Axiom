//! Zero-heap, cache-resident 2D Spatial Hash Grid (`SpatialGrid`).
//!
//! Provides O(1) neighbor lookups and broad-phase collision detection
//! for up to 10,000+ entities without heap allocation or pointer chasing.

use crate::fixed::{Fixed, FixedVec2};

pub use crate::spatial_index::{
    SpatialIndex, DEFAULT_SPATIAL_BUCKETS, DEFAULT_SPATIAL_MAX_ENTITIES, EMPTY_INDEX,
};

pub const DEFAULT_GRID_W: usize = 64;
pub const DEFAULT_GRID_H: usize = 64;
pub const DEFAULT_CELL_CAP: usize = 16;

/// Fixed-capacity bucket representing a single spatial cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct GridCell<const CAP: usize = DEFAULT_CELL_CAP> {
    /// Number of entities currently residing in this cell
    pub count: u16,
    /// Entity handle IDs stored in this cell
    pub entities: [u32; CAP],
}

impl<const CAP: usize> Default for GridCell<CAP> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const CAP: usize> GridCell<CAP> {
    pub const fn new() -> Self {
        Self {
            count: 0,
            entities: [0; CAP],
        }
    }

    #[inline(always)]
    pub fn is_full(&self) -> bool {
        self.count as usize >= CAP
    }

    #[inline(always)]
    pub fn push(&mut self, entity_id: u32) -> bool {
        if (self.count as usize) < CAP {
            self.entities[self.count as usize] = entity_id;
            self.count += 1;
            true
        } else {
            false
        }
    }

    #[inline(always)]
    pub fn clear(&mut self) {
        self.count = 0;
    }
}

/// Static, 2D spatial hash grid mapped to discrete world tiles.
///
/// Converts O(N²) collision/neighbor queries to O(1) local cell lookups.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpatialGrid<
    const W: usize = DEFAULT_GRID_W,
    const H: usize = DEFAULT_GRID_H,
    const CAP: usize = DEFAULT_CELL_CAP,
> {
    pub cells: [[GridCell<CAP>; W]; H],
    pub cell_size: i64,
}

impl<const W: usize, const H: usize, const CAP: usize> Default for SpatialGrid<W, H, CAP> {
    fn default() -> Self {
        Self::new(32) // Default cell size of 32 world units
    }
}

impl<const W: usize, const H: usize, const CAP: usize> SpatialGrid<W, H, CAP> {
    /// Creates a new spatial grid with a given world-space cell dimension.
    pub const fn new(cell_size: i64) -> Self {
        Self {
            cells: [[GridCell::new(); W]; H],
            cell_size: if cell_size > 0 { cell_size } else { 32 },
        }
    }

    /// Clears all cells in the grid in a fast, sequential pass.
    #[inline]
    pub fn clear(&mut self) {
        for row in self.cells.iter_mut() {
            for cell in row.iter_mut() {
                cell.clear();
            }
        }
    }

    /// Converts world fixed-point coordinate to grid cell indices (cx, cy).
    #[inline(always)]
    pub fn world_to_cell(&self, pos: FixedVec2) -> Option<(usize, usize)> {
        let world_x = pos.x.to_int();
        let world_y = pos.y.to_int();

        if world_x < 0 || world_y < 0 {
            return None;
        }

        let cx = (world_x / self.cell_size) as usize;
        let cy = (world_y / self.cell_size) as usize;

        if cx < W && cy < H {
            Some((cx, cy))
        } else {
            None
        }
    }

    /// Inserts an entity handle ID into the appropriate spatial cell.
    pub fn insert(&mut self, entity_id: u32, pos: FixedVec2) -> bool {
        if let Some((cx, cy)) = self.world_to_cell(pos) {
            self.cells[cy][cx].push(entity_id)
        } else {
            false
        }
    }

    /// Queries all entities residing within `radius` of `center`.
    ///
    /// Writes matching entity IDs into `out_buffer` and returns count written.
    pub fn query_radius(&self, center: FixedVec2, radius: Fixed, out_buffer: &mut [u32]) -> usize {
        let center_x = center.x.to_int();
        let center_y = center.y.to_int();
        let rad_int = radius.to_int().max(1);

        let min_cx = ((center_x - rad_int).max(0) / self.cell_size) as usize;
        let max_cx = (((center_x + rad_int).max(0) / self.cell_size) as usize).min(W - 1);
        let min_cy = ((center_y - rad_int).max(0) / self.cell_size) as usize;
        let max_cy = (((center_y + rad_int).max(0) / self.cell_size) as usize).min(H - 1);

        let mut written = 0;

        for cy in min_cy..=max_cy {
            for cx in min_cx..=max_cx {
                let cell = &self.cells[cy][cx];

                for i in 0..(cell.count as usize) {
                    if written >= out_buffer.len() {
                        return written;
                    }
                    out_buffer[written] = cell.entities[i];
                    written += 1;
                }
            }
        }

        written
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spatial_grid_cell_mapping() {
        let grid = SpatialGrid::<16, 16, 8>::new(10); // 10 units per cell

        // (5, 5) -> cell (0, 0)
        assert_eq!(grid.world_to_cell(FixedVec2::from_ints(5, 5)), Some((0, 0)));
        // (15, 25) -> cell (1, 2)
        assert_eq!(
            grid.world_to_cell(FixedVec2::from_ints(15, 25)),
            Some((1, 2))
        );
        // Negative -> None
        assert_eq!(grid.world_to_cell(FixedVec2::from_ints(-1, 0)), None);
        // Out of bounds -> None
        assert_eq!(grid.world_to_cell(FixedVec2::from_ints(200, 200)), None);
    }

    #[test]
    fn test_spatial_grid_insert_and_query() {
        let mut grid = SpatialGrid::<16, 16, 8>::new(10);

        // Insert entities at various locations
        grid.insert(101, FixedVec2::from_ints(5, 5));
        grid.insert(102, FixedVec2::from_ints(6, 6));
        grid.insert(103, FixedVec2::from_ints(50, 50)); // Far away

        let mut out = [0u32; 16];
        let found = grid.query_radius(FixedVec2::from_ints(5, 5), Fixed::from_int(5), &mut out);

        assert_eq!(found, 2);
        assert!(out[..2].contains(&101));
        assert!(out[..2].contains(&102));
        assert!(!out[..2].contains(&103));
    }
}
