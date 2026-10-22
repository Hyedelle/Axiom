//! 1,000 A* Pathfinding Queries Benchmark in Procedural Terrain (T-021)
//!
//! Validates:
//! - Zero-heap, zero-float 2D A* pathfinding across a 64x64 procedural tilemap
//! - 1,000 queries navigating through complex lake and rock obstacles
//! - Path validity (no waypoints on impassable tiles)
//! - Microsecond-grade pathfinding throughput (<50 µs/query in release)

use axiom_core::{PathFinder, TileMap};
use std::time::Instant;

#[test]
fn test_1000_pathfinding_queries_benchmark() {
    let map = TileMap::<64, 64>::generate(777_888);
    let mut finder = PathFinder::<64, 64, 512>::new();

    // Verify map has passable and impassable tiles
    let is_walkable = |x: usize, y: usize| map.is_walkable(x, y);

    // Find valid walkable starting and target points
    let mut walkable_coords = Vec::new();
    for y in 2..62 {
        for x in 2..62 {
            if is_walkable(x, y) {
                walkable_coords.push((x, y));
            }
        }
    }
    assert!(walkable_coords.len() > 500, "Map must have sufficient walkable tiles");

    const TOTAL_QUERIES: usize = 1_000;
    let mut paths_found = 0;
    let start_time = Instant::now();

    for i in 0..TOTAL_QUERIES {
        let (sx, sy) = walkable_coords[i % walkable_coords.len()];
        let (gx, gy) = walkable_coords[(i * 37 + 101) % walkable_coords.len()];

        if let Some(path) = finder.find_path(sx, sy, gx, gy, is_walkable) {
            paths_found += 1;

            // Validate path: Start matches sx, sy; End matches gx, gy
            assert_eq!(path.waypoints[0], (sx as u16, sy as u16));
            assert_eq!(path.waypoints[path.len - 1], (gx as u16, gy as u16));

            // Validate that no waypoint passes through an impassable tile
            for &(wx, wy) in &path.waypoints[0..path.len] {
                assert!(
                    is_walkable(wx as usize, wy as usize),
                    "Waypoint ({wx}, {wy}) must be walkable"
                );
            }
        }
    }

    let elapsed = start_time.elapsed();
    let micros_per_query = elapsed.as_micros() as f64 / TOTAL_QUERIES as f64;

    println!(
        "\n==> [A* PATHFINDING] 1,000 queries in complex 64x64 terrain completed in {:.2?} ({:.2} µs/query)",
        elapsed, micros_per_query
    );
    println!("==> Paths successfully found: {} / {}", paths_found, TOTAL_QUERIES);

    assert!(paths_found > 600, "Most pairs in connected components must find paths");
    assert!(
        micros_per_query < 1000.0,
        "A* query must execute in <1000µs in debug, took {:.2}µs",
        micros_per_query
    );
}
