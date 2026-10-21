//! Seed-based Deterministic Procedural Content Generation (PCG).
//!
//! Provides integer-only PRNG (SplitMix64 / XorShift64) and deterministic
//! tilemap generation without floating-point math or OS randomness.

use crate::hash::deterministic_hash;

/// Discrete world terrain tile types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum TileType {
    /// Walkable open ground
    Plains = 0,
    /// Tree cover (resource node, slows movement)
    Forest = 1,
    /// Impassable mineral/mountain terrain
    Rock = 2,
    /// Impassable water bodies
    Water = 3,
}

impl TileType {
    #[inline(always)]
    pub fn is_walkable(self) -> bool {
        matches!(self, Self::Plains | Self::Forest)
    }

    #[inline(always)]
    pub fn is_obstacle(self) -> bool {
        matches!(self, Self::Rock | Self::Water)
    }
}

/// 100% Deterministic SplitMix64 pseudo-random number generator.
///
/// Guaranteed identical output sequence across all platforms for a given 64-bit seed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rng {
    state: u64,
}

impl Rng {
    /// Initializes RNG with a non-zero deterministic seed.
    pub const fn with_seed(seed: u64) -> Self {
        // Guarantee state is not completely zero
        Self {
            state: if seed == 0 { 0x9E3779B97F4A7C15 } else { seed },
        }
    }

    /// Generates next pseudo-random 64-bit unsigned integer using SplitMix64.
    #[inline]
    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E3779B97F4A7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
        z ^ (z >> 31)
    }

    /// Generates next 32-bit integer.
    #[inline(always)]
    pub fn next_u32(&mut self) -> u32 {
        self.next_u64() as u32
    }

    /// Returns a pseudo-random integer in `[min, max]` (inclusive).
    pub fn range_u32(&mut self, min: u32, max: u32) -> u32 {
        if min >= max {
            return min;
        }
        let range = max - min + 1;
        min + (self.next_u32() % range)
    }
}

pub const DEFAULT_MAP_W: usize = 64;
pub const DEFAULT_MAP_H: usize = 64;

/// Fixed-size deterministic tilemap grid generated from a seed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TileMap<const W: usize = DEFAULT_MAP_W, const H: usize = DEFAULT_MAP_H> {
    pub tiles: [[TileType; W]; H],
    pub seed: u64,
}

impl<const W: usize, const H: usize> TileMap<W, H> {
    /// Creates an empty tilemap filled with plains.
    pub const fn empty(seed: u64) -> Self {
        Self {
            tiles: [[TileType::Plains; W]; H],
            seed,
        }
    }

    /// Generates a coherent world terrain using deterministic cellular automata.
    ///
    /// Forms natural lakes, rock formations, and forest patches.
    pub fn generate(seed: u64) -> Self {
        let mut map = Self::empty(seed);
        let mut rng = Rng::with_seed(seed);

        // 1. Initial probabilistic noise assignment
        for y in 0..H {
            for x in 0..W {
                // Border tiles are boundary rocks
                if x == 0 || y == 0 || x == W - 1 || y == H - 1 {
                    map.tiles[y][x] = TileType::Rock;
                    continue;
                }

                let roll = rng.range_u32(0, 99);
                map.tiles[y][x] = if roll < 15 {
                    TileType::Water
                } else if roll < 30 {
                    TileType::Rock
                } else if roll < 55 {
                    TileType::Forest
                } else {
                    TileType::Plains
                };
            }
        }

        // 2. Cellular automata smoothing pass (2 iterations)
        for _ in 0..2 {
            let prev = map.tiles;
            for y in 1..(H - 1) {
                for x in 1..(W - 1) {
                    let mut water_neighbors = 0;
                    let mut rock_neighbors = 0;
                    let mut forest_neighbors = 0;

                    for dy in [-1isize, 0, 1] {
                        for dx in [-1isize, 0, 1] {
                            if dx == 0 && dy == 0 {
                                continue;
                            }
                            let nx = (x as isize + dx) as usize;
                            let ny = (y as isize + dy) as usize;
                            match prev[ny][nx] {
                                TileType::Water => water_neighbors += 1,
                                TileType::Rock => rock_neighbors += 1,
                                TileType::Forest => forest_neighbors += 1,
                                TileType::Plains => {}
                            }
                        }
                    }

                    // Clustering rules
                    if water_neighbors >= 4 {
                        map.tiles[y][x] = TileType::Water;
                    } else if rock_neighbors >= 4 {
                        map.tiles[y][x] = TileType::Rock;
                    } else if forest_neighbors >= 4 {
                        map.tiles[y][x] = TileType::Forest;
                    }
                }
            }
        }

        map
    }

    /// Retrieves tile at (x, y) coordinates with boundary check.
    #[inline(always)]
    pub fn tile_at(&self, x: usize, y: usize) -> Option<TileType> {
        if x < W && y < H {
            Some(self.tiles[y][x])
        } else {
            None
        }
    }

    /// Returns true if the tile at (x, y) is walkable.
    #[inline(always)]
    pub fn is_walkable(&self, x: usize, y: usize) -> bool {
        self.tile_at(x, y).is_some_and(|t| t.is_walkable())
    }

    /// Computes deterministic hash across the full generated tilemap.
    #[inline]
    pub fn compute_hash(&self) -> u64 {
        deterministic_hash(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rng_determinism() {
        let mut rng1 = Rng::with_seed(123456789);
        let mut rng2 = Rng::with_seed(123456789);

        for _ in 0..100 {
            assert_eq!(rng1.next_u64(), rng2.next_u64());
        }
    }

    #[test]
    fn test_tilemap_procedural_generation_determinism() {
        let seed = 987654321;
        let map_a = TileMap::<32, 32>::generate(seed);
        let map_b = TileMap::<32, 32>::generate(seed);

        // Bit-for-bit identical assertion
        assert_eq!(map_a, map_b, "Same seed must generate identical tilemap");
        assert_eq!(map_a.compute_hash(), map_b.compute_hash());

        // Boundary rocks verification
        for x in 0..32 {
            assert_eq!(map_a.tile_at(x, 0), Some(TileType::Rock));
            assert_eq!(map_a.tile_at(x, 31), Some(TileType::Rock));
        }

        // Different seed generates different map
        let map_diff = TileMap::<32, 32>::generate(11111);
        assert_ne!(map_a.compute_hash(), map_diff.compute_hash());
    }
}
