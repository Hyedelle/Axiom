//! # Unit Tests for 3D Spatial Hashing Grid & AABB Broadphase (`axiom-core::spatial_grid`)

use core::mem::{align_of, size_of};
use axiom_core::spatial_grid::{
    spatial_hash_3d, AabbSlab64, CollisionPair64, SpatialCell64, SpatialHashGrid,
    COLLISION_FLAG_DYNAMIC, COLLISION_FLAG_STATIC, COLLISION_FLAG_TRIGGER,
    DEFAULT_GRID_TABLE_CAP,
};

#[test]
fn test_spatial_grid_l1_cache_alignment_and_sizes() {
    assert_eq!(size_of::<AabbSlab64>(), 64);
    assert_eq!(align_of::<AabbSlab64>(), 64);

    assert_eq!(size_of::<SpatialCell64<14>>(), 64);
    assert_eq!(align_of::<SpatialCell64<14>>(), 64);

    assert_eq!(align_of::<SpatialHashGrid<1024, 14>>(), 64);
    assert_eq!(size_of::<CollisionPair64>(), 16);
}

#[test]
fn test_spatial_hash_3d_determinism_and_negative_coords() {
    let mask = DEFAULT_GRID_TABLE_CAP - 1;

    let h1 = spatial_hash_3d(500, 1500, -2500, mask);
    let h2 = spatial_hash_3d(500, 1500, -2500, mask);
    assert_eq!(h1, h2, "Hash must be strictly deterministic");

    // Negative coordinates should distribute uniformly across table mask
    let h_neg1 = spatial_hash_3d(-1000, -2000, -3000, mask);
    let h_neg2 = spatial_hash_3d(-1001, -2000, -3000, mask);
    assert!(h_neg1 < DEFAULT_GRID_TABLE_CAP);
    assert!(h_neg2 < DEFAULT_GRID_TABLE_CAP);
}

#[test]
fn test_aabb_slab_intersection_and_collision_rules() {
    let slab_static = AabbSlab64::new(
        1,
        1,
        0b01,
        COLLISION_FLAG_STATIC,
        (0, 0, 0),
        (1000, 1000, 1000),
    );
    let slab_static2 = AabbSlab64::new(
        2,
        1,
        0b01,
        COLLISION_FLAG_STATIC,
        (500, 500, 500),
        (1500, 1500, 1500),
    );
    let slab_dynamic = AabbSlab64::new(
        3,
        1,
        0b01,
        COLLISION_FLAG_DYNAMIC,
        (500, 500, 500),
        (1500, 1500, 1500),
    );
    let slab_far = AabbSlab64::new(
        4,
        1,
        0b01,
        COLLISION_FLAG_DYNAMIC,
        (5000, 5000, 5000),
        (6000, 6000, 6000),
    );

    // Overlap checks
    assert!(slab_static.intersects(&slab_static2));
    assert!(slab_static.intersects(&slab_dynamic));
    assert!(!slab_static.intersects(&slab_far));

    // Static vs Static should not collide
    assert!(!slab_static.can_collide_with(&slab_static2));
    // Static vs Dynamic should collide
    assert!(slab_static.can_collide_with(&slab_dynamic));
    // Self collision should not occur
    assert!(!slab_dynamic.can_collide_with(&slab_dynamic));
}

#[test]
fn test_spatial_grid_insert_and_query_candidates() {
    let mut grid = SpatialHashGrid::<1024, 14>::new(1000); // 1.0m cells

    // Insert entity 1 covering 8 cells (from -500 to 500 in XYZ)
    let slab1 = AabbSlab64::new(
        1,
        1,
        0b01,
        COLLISION_FLAG_DYNAMIC,
        (-500, -500, -500),
        (500, 500, 500),
    );
    let cells_populated = grid.insert(&slab1);
    assert_eq!(cells_populated, 8, "AABB spanning 0 in all axes must span 2x2x2=8 cells");

    // Insert entity 2 far away
    let slab2 = AabbSlab64::new(
        2,
        1,
        0b01,
        COLLISION_FLAG_DYNAMIC,
        (10000, 10000, 10000),
        (11000, 11000, 11000),
    );
    grid.insert(&slab2);

    // Query entity 1 vicinity
    let mut candidates = [0u32; 16];
    let count = grid.query_aabb_candidates(&slab1, &mut candidates);
    // Entity 1 is excluded from its own query, entity 2 is far away
    assert_eq!(count, 0);

    // Insert entity 3 overlapping with entity 1
    let slab3 = AabbSlab64::new(
        3,
        1,
        0b01,
        COLLISION_FLAG_DYNAMIC,
        (200, 200, 200),
        (400, 400, 400),
    );
    grid.insert(&slab3);

    let count2 = grid.query_aabb_candidates(&slab1, &mut candidates);
    assert_eq!(count2, 1);
    assert_eq!(candidates[0], 3);
}

#[test]
fn test_lexicographical_minimum_overlap_rule_exact_single_pair() {
    let mut grid = SpatialHashGrid::<1024, 14>::new(1000);

    // Create two entities that both span across 8 cells (-500..500)
    let slab_a = AabbSlab64::new(
        10,
        1,
        0b01,
        COLLISION_FLAG_DYNAMIC,
        (-500, -500, -500),
        (500, 500, 500),
    );
    let slab_b = AabbSlab64::new(
        20,
        1,
        0b01,
        COLLISION_FLAG_DYNAMIC,
        (-200, -200, -200),
        (600, 600, 600),
    );

    let slabs = [slab_a, slab_b];
    grid.insert(&slab_a);
    grid.insert(&slab_b);

    let mut pairs = [CollisionPair64::default(); 16];
    let pair_count = grid.query_all_collision_pairs(&slabs, &mut pairs);

    // Despite both entities overlapping across multiple cells,
    // the Lexicographical Minimum Overlap Rule must emit the pair EXACTLY ONCE!
    assert_eq!(pair_count, 1, "Duplicate pairs across multi-cell overlap must be eliminated");
    assert_eq!(pairs[0].entity_a, 10);
    assert_eq!(pairs[0].entity_b, 20);
    assert_eq!(pairs[0].pair_key, ((10u64) << 32) | 20u64);
}

#[test]
fn test_spatial_grid_consensus_hash() {
    let mut grid1 = SpatialHashGrid::<1024, 14>::new(1000);
    let mut grid2 = SpatialHashGrid::<1024, 14>::new(1000);

    let slab = AabbSlab64::new(
        42,
        1,
        0b01,
        COLLISION_FLAG_DYNAMIC,
        (100, 200, 300),
        (500, 600, 700),
    );

    grid1.insert(&slab);
    grid2.insert(&slab);

    let hash1 = grid1.compute_consensus_hash();
    let hash2 = grid2.compute_consensus_hash();
    assert_eq!(hash1, hash2, "Identical grid states must yield identical consensus hashes");
    assert_ne!(hash1, 0);

    // Modify grid2
    let slab2 = AabbSlab64::new(
        99,
        1,
        0b01,
        COLLISION_FLAG_TRIGGER,
        (2000, 2000, 2000),
        (2500, 2500, 2500),
    );
    grid2.insert(&slab2);
    let hash2_mod = grid2.compute_consensus_hash();
    assert_ne!(hash1, hash2_mod, "Different grid states must yield different consensus hashes");
}
