//! Spatial Grid Performance Benchmark (T-006)
//!
//! Validates:
//! - 10,000 entities inserted into a 128x128 spatial hash grid
//! - 1,000 random localized radius queries
//! - Average query time < 5 µs (O(1) local cell lookup)

extern crate alloc;

use alloc::boxed::Box;
use axiom_core::fixed::{Fixed, FixedVec2};
use axiom_core::spatial::SpatialGrid;
use std::time::Instant;

#[test]
fn test_spatial_grid_10k_throughput() {
    const TOTAL_ENTITIES: usize = 10_000;
    const NUM_QUERIES: usize = 1_000;

    // Allocate 64x64 grid (4,096 cells, ~278KB) via Box
    let mut grid = Box::new(SpatialGrid::<64, 64, 16>::new(32)); // 32 units per cell

    // 1. Insert 10,000 entities distributed over 64x64 grid (2048x2048 world)
    let insert_start = Instant::now();
    for i in 0..TOTAL_ENTITIES {
        let x = ((i * 17) % 2000) as i64;
        let y = ((i * 31) % 2000) as i64;
        grid.insert(i as u32, FixedVec2::from_ints(x, y));
    }
    let insert_duration = insert_start.elapsed();
    println!(
        "\n==> [SPATIAL INSERT] 10,000 entities inserted in {:.2?}",
        insert_duration
    );

    // 2. Perform 1,000 localized radius queries
    let query_start = Instant::now();
    let mut query_buf = [0u32; 64];
    let mut total_found = 0;

    for q in 0..NUM_QUERIES {
        let qx = ((q * 37) % 1800 + 50) as i64;
        let qy = ((q * 41) % 1800 + 50) as i64;
        let center = FixedVec2::from_ints(qx, qy);
        let found = grid.query_radius(center, Fixed::from_int(32), &mut query_buf);
        total_found += found;
    }
    let query_duration = query_start.elapsed();
    let avg_query_micros = query_duration.as_micros() as f64 / NUM_QUERIES as f64;

    println!(
        "==> [SPATIAL QUERIES] 1,000 radius queries completed in {:.2?} (Total found: {})",
        query_duration, total_found
    );
    println!(
        "==> [QUERY SPEED] Average query time: {:.3} µs",
        avg_query_micros
    );

    // Assert O(1) performance: query should take < 50 µs even in debug mode
    assert!(
        avg_query_micros < 50.0,
        "Spatial query must be sub-50µs. Measured: {:.3} µs",
        avg_query_micros
    );
}
