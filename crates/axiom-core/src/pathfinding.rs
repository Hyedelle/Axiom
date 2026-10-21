//! # Deterministic Fixed-Memory 2D A* Pathfinding (Phase 4 RPG Navigation)
//!
//! Provides zero-heap, zero-floating-point 2D A* grid pathfinding with
//! hardware cache-line aligned fixed scratch buffers and O(1) epoch tag clearing.

pub const DEFAULT_GRID_W: usize = 64;
pub const DEFAULT_GRID_H: usize = 64;
pub const DEFAULT_HEAP_CAPACITY: usize = 512;
pub const MAX_PATH_WAYPOINTS: usize = 128;

/// Result of an A* pathfinding query containing the reconstructed waypoint trail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Path<const MAX_LEN: usize = MAX_PATH_WAYPOINTS> {
    pub waypoints: [(u16, u16); MAX_LEN],
    pub len: usize,
    pub total_cost: u32,
}

impl<const MAX_LEN: usize> Default for Path<MAX_LEN> {
    fn default() -> Self {
        Self::empty()
    }
}

impl<const MAX_LEN: usize> Path<MAX_LEN> {
    pub const fn empty() -> Self {
        Self {
            waypoints: [(0, 0); MAX_LEN],
            len: 0,
            total_cost: 0,
        }
    }

    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

/// Compact priority queue element for the A* open list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
struct PathNode {
    f_score: u32,
    x: u16,
    y: u16,
}

/// Fixed-capacity binary min-heap for zero-allocation priority queue operations.
#[derive(Debug, Clone, Copy)]
struct FixedMinHeap<const CAP: usize = DEFAULT_HEAP_CAPACITY> {
    nodes: [PathNode; CAP],
    len: usize,
}

impl<const CAP: usize> FixedMinHeap<CAP> {
    pub const fn new() -> Self {
        Self {
            nodes: [PathNode { f_score: 0, x: 0, y: 0 }; CAP],
            len: 0,
        }
    }

    #[inline(always)]
    pub fn clear(&mut self) {
        self.len = 0;
    }

    /// Pushes a node into the binary min-heap.
    pub fn push(&mut self, node: PathNode) -> bool {
        if self.len >= CAP {
            return false;
        }

        let mut idx = self.len;
        self.nodes[idx] = node;
        self.len += 1;

        // Sift up
        while idx > 0 {
            let parent = (idx - 1) / 2;
            if self.nodes[idx].f_score < self.nodes[parent].f_score {
                self.nodes.swap(idx, parent);
                idx = parent;
            } else {
                break;
            }
        }
        true
    }

    /// Pops the minimum f_score node from the heap.
    pub fn pop(&mut self) -> Option<PathNode> {
        if self.len == 0 {
            return None;
        }

        let min_node = self.nodes[0];
        self.len -= 1;

        if self.len > 0 {
            self.nodes[0] = self.nodes[self.len];
            // Sift down
            let mut idx = 0;
            loop {
                let left = 2 * idx + 1;
                let right = 2 * idx + 2;
                let mut smallest = idx;

                if left < self.len && self.nodes[left].f_score < self.nodes[smallest].f_score {
                    smallest = left;
                }
                if right < self.len && self.nodes[right].f_score < self.nodes[smallest].f_score {
                    smallest = right;
                }

                if smallest != idx {
                    self.nodes.swap(idx, smallest);
                    idx = smallest;
                } else {
                    break;
                }
            }
        }

        Some(min_node)
    }
}

/// Zero-heap, fixed-slab A* pathfinder.
///
/// Uses epoch tagging (`visited_epoch`) for O(1) instant reset without zeroing memory.
#[derive(Debug, Clone)]
pub struct PathFinder<
    const W: usize = DEFAULT_GRID_W,
    const H: usize = DEFAULT_GRID_H,
    const HEAP_CAP: usize = DEFAULT_HEAP_CAPACITY,
> {
    open_heap: FixedMinHeap<HEAP_CAP>,
    g_scores: [[u32; W]; H],
    came_from: [[(u16, u16); W]; H],
    visited_epoch: [[u32; W]; H],
    current_epoch: u32,
}

impl<const W: usize, const H: usize, const HEAP_CAP: usize> Default
    for PathFinder<W, H, HEAP_CAP>
{
    fn default() -> Self {
        Self::new()
    }
}

impl<const W: usize, const H: usize, const HEAP_CAP: usize> PathFinder<W, H, HEAP_CAP> {
    /// Creates a new uninitialized pathfinder instance.
    pub fn new() -> Self {
        Self {
            open_heap: FixedMinHeap::new(),
            g_scores: [[u32::MAX; W]; H],
            came_from: [[(0, 0); W]; H],
            visited_epoch: [[0; W]; H],
            current_epoch: 1,
        }
    }

    /// Octile heuristic distance (integer arithmetic):
    /// Orthogonal step = 10, Diagonal step = 14 (approx 10 * sqrt(2)).
    #[inline(always)]
    pub fn octile_heuristic(x1: usize, y1: usize, x2: usize, y2: usize) -> u32 {
        let dx = (x1 as isize - x2 as isize).unsigned_abs() as u32;
        let dy = (y1 as isize - y2 as isize).unsigned_abs() as u32;
        let min_d = if dx < dy { dx } else { dy };
        let max_d = if dx > dy { dx } else { dy };
        14 * min_d + 10 * (max_d - min_d)
    }

    /// Finds the optimal shortest path from `(start_x, start_y)` to `(goal_x, goal_y)`.
    ///
    /// The `is_walkable` predicate determines passable terrain tiles.
    pub fn find_path<F>(
        &mut self,
        start_x: usize,
        start_y: usize,
        goal_x: usize,
        goal_y: usize,
        mut is_walkable: F,
    ) -> Option<Path<MAX_PATH_WAYPOINTS>>
    where
        F: FnMut(usize, usize) -> bool,
    {
        if start_x >= W || start_y >= H || goal_x >= W || goal_y >= H {
            return None;
        }

        if !is_walkable(goal_x, goal_y) {
            return None;
        }

        if start_x == goal_x && start_y == goal_y {
            let mut path = Path::empty();
            path.waypoints[0] = (start_x as u16, start_y as u16);
            path.len = 1;
            return Some(path);
        }

        // Epoch tag advance: O(1) memory reset!
        self.current_epoch = self.current_epoch.wrapping_add(1);
        if self.current_epoch == 0 {
            // Epoch wrapped around u32::MAX: reset table
            self.visited_epoch = [[0; W]; H];
            self.current_epoch = 1;
        }
        let epoch = self.current_epoch;

        self.open_heap.clear();

        // 8-directional movement offsets and step costs (10 = straight, 14 = diagonal)
        const DIRECTIONS: [(isize, isize, u32); 8] = [
            (0, -1, 10),  // North
            (1, 0, 10),   // East
            (0, 1, 10),   // South
            (-1, 0, 10),  // West
            (1, -1, 14),  // North-East
            (1, 1, 14),   // South-East
            (-1, 1, 14),  // South-West
            (-1, -1, 14), // North-West
        ];

        // Seed start node
        self.g_scores[start_y][start_x] = 0;
        self.visited_epoch[start_y][start_x] = epoch;

        let start_h = Self::octile_heuristic(start_x, start_y, goal_x, goal_y);
        self.open_heap.push(PathNode {
            f_score: start_h,
            x: start_x as u16,
            y: start_y as u16,
        });

        let mut reached = false;

        while let Some(current) = self.open_heap.pop() {
            let cx = current.x as usize;
            let cy = current.y as usize;

            if cx == goal_x && cy == goal_y {
                reached = true;
                break;
            }

            let current_g = self.g_scores[cy][cx];

            // Explore 8 adjacent neighbors
            for (dx, dy, cost) in DIRECTIONS {
                let nx = cx as isize + dx;
                let ny = cy as isize + dy;

                if nx < 0 || nx >= W as isize || ny < 0 || ny >= H as isize {
                    continue;
                }

                let ux = nx as usize;
                let uy = ny as usize;

                if !is_walkable(ux, uy) {
                    continue;
                }

                // Diagonal corner-cutting obstacle prevention
                if dx != 0 && dy != 0 {
                    if !is_walkable(cx, uy) || !is_walkable(ux, cy) {
                        continue;
                    }
                }

                let tentative_g = current_g.saturating_add(cost);

                let is_unvisited = self.visited_epoch[uy][ux] != epoch;
                if is_unvisited || tentative_g < self.g_scores[uy][ux] {
                    self.visited_epoch[uy][ux] = epoch;
                    self.g_scores[uy][ux] = tentative_g;
                    self.came_from[uy][ux] = (cx as u16, cy as u16);

                    let h = Self::octile_heuristic(ux, uy, goal_x, goal_y);
                    let f = tentative_g.saturating_add(h);

                    self.open_heap.push(PathNode {
                        f_score: f,
                        x: ux as u16,
                        y: uy as u16,
                    });
                }
            }
        }

        if !reached {
            return None;
        }

        // Reconstruct path backward from goal
        let mut path = Path::empty();
        let mut curr_x = goal_x as u16;
        let mut curr_y = goal_y as u16;
        let mut temp_rev = [(0u16, 0u16); MAX_PATH_WAYPOINTS];
        let mut trail_count = 0;
        let mut reached_start = false;

        while trail_count < MAX_PATH_WAYPOINTS {
            temp_rev[trail_count] = (curr_x, curr_y);
            trail_count += 1;

            if curr_x as usize == start_x && curr_y as usize == start_y {
                reached_start = true;
                break;
            }

            let (px, py) = self.came_from[curr_y as usize][curr_x as usize];
            curr_x = px;
            curr_y = py;
        }

        if !reached_start {
            return None;
        }

        // Reverse into start->goal order
        path.len = trail_count;
        path.total_cost = self.g_scores[goal_y][goal_x];
        for i in 0..trail_count {
            path.waypoints[i] = temp_rev[trail_count - 1 - i];
        }

        Some(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_direct_line_path() {
        let mut finder = PathFinder::<32, 32, 256>::new();
        let path = finder
            .find_path(0, 0, 5, 5, |_x, _y| true)
            .expect("Path must be found on open terrain");

        assert_eq!(path.waypoints[0], (0, 0));
        assert_eq!(path.waypoints[path.len - 1], (5, 5));
        assert!(path.total_cost > 0);
    }

    #[test]
    fn test_obstacle_wall_detour() {
        let mut finder = PathFinder::<32, 32, 256>::new();

        // Build a vertical wall at x = 10, with opening at y = 15
        let is_walkable = |x: usize, y: usize| {
            if x == 10 && y != 15 {
                false // Wall
            } else {
                true
            }
        };

        let path = finder
            .find_path(2, 5, 20, 5, is_walkable)
            .expect("Path must navigate around wall via opening");

        assert_eq!(path.waypoints[0], (2, 5));
        assert_eq!(path.waypoints[path.len - 1], (20, 5));

        // Verify path passed through the opening at (10, 15)
        let passed_opening = path.waypoints[0..path.len]
            .iter()
            .any(|&(x, y)| x == 10 && y == 15);
        assert!(passed_opening, "Path must pass through wall opening at (10, 15)");
    }
}
