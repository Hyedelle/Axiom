//! SpatialIndex 100,000-entity Performance Benchmark (T-068)
//!
//! Validates:
//! - 100,000 entities inserted into a 16,384-bucket zero-allocation spatial index
//! - 1,000 localized radius queries + 1,000 closest-entity searches
//! - Per-query cost stays well under 1ms on a single core (even in debug builds)

use axiom_core::fixed::{Fixed, FixedVec2};
use axiom_core::spatial::SpatialIndex;
use std::time::Instant;

fn lcg(state: &mut u64) -> u64 {
    *state = state
        .wrapping_mul(6_364_136_223_846_793_005)
        .wrapping_add(1_442_695_040_888_963_407);
    *state >> 33
}

#[test]
fn test_spatial_index_100k_throughput() {
    const TOTAL_ENTITIES: usize = 100_000;
    const NUM_QUERIES: usize = 1_000;

    let mut index = SpatialIndex::<16_384, TOTAL_ENTITIES>::new(32);

    // 1. Insert 100,000 entities over an 8192x8192 world (deterministic LCG placement).
    let mut seed = 0xA51_0Fu64;
    let insert_start = Instant::now();
    for i in 0..TOTAL_ENTITIES {
        let x = (lcg(&mut seed) % 8192) as i64;
        let y = (lcg(&mut seed) % 8192) as i64;
        assert!(index.insert(i as u32, FixedVec2::from_ints(x, y)));
    }
    let insert_duration = insert_start.elapsed();
    println!(
        "\n==> [SPATIAL INDEX INSERT] 100,000 entities inserted in {:.2?}",
        insert_duration
    );

    // 2. Clear cost (single 64KB head-table fill) and deterministic re-population.
    let clear_start = Instant::now();
    index.clear();
    let clear_duration = clear_start.elapsed();
    println!("==> [SPATIAL INDEX CLEAR] reset in {:.2?}", clear_duration);
    let mut seed = 0xA51_0Fu64;
    for i in 0..TOTAL_ENTITIES {
        let x = (lcg(&mut seed) % 8192) as i64;
        let y = (lcg(&mut seed) % 8192) as i64;
        index.insert(i as u32, FixedVec2::from_ints(x, y));
    }

    // 3. Localized radius queries.
    let mut out = [0u32; 256];
    let mut total_found = 0usize;
    let query_start = Instant::now();
    for q in 0..NUM_QUERIES {
        let qx = ((q * 37) % 8000 + 50) as i64;
        let qy = ((q * 41) % 8000 + 50) as i64;
        total_found += index.query_radius(
            FixedVec2::from_ints(qx, qy),
            Fixed::from_int(32),
            &mut out,
        );
    }
    let query_duration = query_start.elapsed();
    let avg_query_micros = query_duration.as_micros() as f64 / NUM_QUERIES as f64;
    println!(
        "==> [SPATIAL INDEX RADIUS] 1,000 queries in {:.2?} (found {}), avg {:.3} µs",
        query_duration, total_found, avg_query_micros
    );

    // 4. Closest-entity searches.
    let mut hits = 0usize;
    let closest_start = Instant::now();
    for q in 0..NUM_QUERIES {
        let qx = ((q * 53) % 8000 + 50) as i64;
        let qy = ((q * 59) % 8000 + 50) as i64;
        if index
            .find_closest(FixedVec2::from_ints(qx, qy), Fixed::from_int(256))
            .is_some()
        {
            hits += 1;
        }
    }
    let closest_duration = closest_start.elapsed();
    let avg_closest_micros = closest_duration.as_micros() as f64 / NUM_QUERIES as f64;
    println!(
        "==> [SPATIAL INDEX CLOSEST] 1,000 searches in {:.2?} (hits {}), avg {:.3} µs",
        closest_duration, hits, avg_closest_micros
    );

    assert!(total_found > 0);
    assert_eq!(hits, NUM_QUERIES, "dense field: every search must hit");
    assert!(
        avg_query_micros < 1000.0,
        "radius query must be sub-1ms even in debug. Measured: {:.3} µs",
        avg_query_micros
    );
    assert!(
        avg_closest_micros < 1000.0,
        "closest search must be sub-1ms even in debug. Measured: {:.3} µs",
        avg_closest_micros
    );
}
