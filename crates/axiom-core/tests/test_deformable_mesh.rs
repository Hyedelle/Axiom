//! Unit tests for Axiom Core Deformable Mesh & Impact Crater Tessellation (T-207)

use core::mem::{align_of, size_of};
use axiom_core::deformable_mesh::{
    icbrt_u64, CraterSlab64, DeformableMeshGrid, TerrainVertexSlab64, CRATER_FLAG_ACTIVE,
    TERRAIN_MAT_DIRT, TERRAIN_MAT_STONE, VERTEX_FLAG_MODIFIED,
};

#[test]
fn test_terrain_slabs_64b_l1_alignment() {
    assert_eq!(
        size_of::<TerrainVertexSlab64>(),
        64,
        "TerrainVertexSlab64 must be exactly 64 bytes"
    );
    assert_eq!(
        align_of::<TerrainVertexSlab64>(),
        64,
        "TerrainVertexSlab64 must align to 64-byte L1 cache line"
    );

    assert_eq!(
        size_of::<CraterSlab64>(),
        64,
        "CraterSlab64 must be exactly 64 bytes"
    );
    assert_eq!(
        align_of::<CraterSlab64>(),
        64,
        "CraterSlab64 must align to 64-byte L1 cache line"
    );

    assert_eq!(
        align_of::<DeformableMeshGrid<17, 289, 8>>(),
        64,
        "DeformableMeshGrid must align to 64-byte L1 cache line"
    );
}

#[test]
fn test_integer_cube_root() {
    assert_eq!(icbrt_u64(0), 0);
    assert_eq!(icbrt_u64(1), 1);
    assert_eq!(icbrt_u64(7), 1);
    assert_eq!(icbrt_u64(8), 2);
    assert_eq!(icbrt_u64(26), 2);
    assert_eq!(icbrt_u64(27), 3);
    assert_eq!(icbrt_u64(64), 4);
    assert_eq!(icbrt_u64(125), 5);
    assert_eq!(icbrt_u64(1_000), 10);
    assert_eq!(icbrt_u64(1_000_000), 100);
    assert_eq!(icbrt_u64(1_000_000_000), 1_000);
    assert_eq!(icbrt_u64(1_000_000_000_000), 10_000);
}

#[test]
fn test_kinetic_crater_construction_and_volume_conservation() {
    // 50,000 J punch / explosion (roughly equivalent to a heavyweight mecha body slam)
    let crater = CraterSlab64::from_kinetic_impact(
        0, 0, 0,
        50_000,     // energy: 50 kJ
        1_200,      // impulse: 1,200 kg*m/s
        13_107,     // ejecta ratio: 20% in Q16.16 (~13107)
        TERRAIN_MAT_DIRT,
        101,        // source_id
        100,        // tick
    );

    assert_eq!(crater.flags & CRATER_FLAG_ACTIVE, CRATER_FLAG_ACTIVE);
    assert!(crater.radius_inner_mm >= 300, "R_in should be at least 300mm");
    assert_eq!(
        crater.radius_outer_mm,
        (crater.radius_inner_mm * 3) / 2,
        "R_out must be 1.5 * R_in"
    );
    assert!(crater.depth_mm > 0, "Excavation depth must be positive");
    assert!(crater.rim_height_mm > 0, "Rim lip height must be positive");
    assert!(
        crater.rim_height_mm < crater.depth_mm,
        "Rim height should be physically smaller than bowl depth due to broader outer annular area"
    );
}

#[test]
fn test_crater_displacement_geometry_and_normals() {
    // 17x17 grid with 200mm (0.2m) spacing -> total span 3.2m x 3.2m centered at (-1600, -1600)
    let mut grid = DeformableMeshGrid::<17, 289, 8>::new_flat(-1600, -1600, 0, 200);

    // Apply crater with R_in = 600mm, R_out = 900mm, depth = 300mm, rim_height = 100mm
    let mut crater = CraterSlab64::empty();
    crater.center_x = 0;
    crater.center_y = 0;
    crater.center_z = 0;
    crater.radius_inner_mm = 600;
    crater.radius_outer_mm = 900;
    crater.depth_mm = 300;
    crater.rim_height_mm = 100;
    crater.tick = 42;
    crater.flags = CRATER_FLAG_ACTIVE;

    let modified = grid.apply_crater_displacement(&crater);
    assert!(modified > 0, "At least some vertices must be deformed");
    assert!(grid.is_dirty, "Grid must be marked dirty");
    assert_eq!(grid.crater_count, 1);

    // Center vertex is at grid [8, 8] -> (0, 0, 0)
    let center_v = &grid.vertices[8 * 17 + 8];
    assert_eq!(center_v.base_pos_x, 0);
    assert_eq!(center_v.base_pos_z, 0);
    assert_eq!(
        center_v.disp_y, -300,
        "Center vertex must be excavated to full crater depth"
    );
    assert_ne!(
        center_v.flags & VERTEX_FLAG_MODIFIED,
        0,
        "Center vertex must be flagged modified"
    );

    // Vertex at (800mm, 0) is between R_in (600) and R_out (900) -> upheaval rim
    // In grid coords: base_pos_x = 800 corresponds to index gx = (800 - (-1600)) / 200 = 12
    let rim_v = &grid.vertices[8 * 17 + 12];
    assert_eq!(rim_v.base_pos_x, 800);
    assert_eq!(rim_v.base_pos_z, 0);
    assert!(
        rim_v.disp_y > 0,
        "Rim vertex at 800mm must have positive upheaval displacement, got {}",
        rim_v.disp_y
    );

    // Vertex far outside at corner [0, 0] -> (-1600, -1600) -> distance ~ 2262mm > 900mm
    let far_v = &grid.vertices[0];
    assert_eq!(far_v.disp_y, 0, "Distant vertex must remain undeformed");
    assert_eq!(far_v.flags, 0, "Distant vertex flags must remain clear");

    // Check bounds
    assert_eq!(grid.min_elevation_mm, -300);
    assert!(grid.max_elevation_mm > 0);

    // Normal vector check: length must equal ~65536 in Q16.16
    let (nx, ny, nz) = (
        rim_v.normal_x_q16 as i64,
        rim_v.normal_y_q16 as i64,
        rim_v.normal_z_q16 as i64,
    );
    let len = axiom_core::ragdoll::isqrt_i64(nx * nx + ny * ny + nz * nz);
    assert!(
        (len - 65536).abs() <= 2,
        "Surface normal must be normalized to Q16.16 (expected ~65536, got {})",
        len
    );
}

#[test]
fn test_mesh_consensus_hash_determinism() {
    let mut grid1 = DeformableMeshGrid::<17, 289, 8>::new_flat(-1600, -1600, 0, 200);
    let mut grid2 = DeformableMeshGrid::<17, 289, 8>::new_flat(-1600, -1600, 0, 200);

    let crater = CraterSlab64::from_kinetic_impact(
        100, 0, -50,
        75_000,
        2_500,
        10_000,
        TERRAIN_MAT_STONE,
        99,
        500,
    );

    grid1.apply_crater_displacement(&crater);
    grid2.apply_crater_displacement(&crater);

    let hash1 = grid1.compute_consensus_hash();
    let hash2 = grid2.compute_consensus_hash();

    assert_eq!(
        hash1, hash2,
        "Consensus hashes must match bit-for-bit across identical runs"
    );

    // Different crater should alter hash
    let mut grid3 = DeformableMeshGrid::<17, 289, 8>::new_flat(-1600, -1600, 0, 200);
    let mut crater_diff = crater;
    crater_diff.depth_mm += 1; // 1mm difference
    grid3.apply_crater_displacement(&crater_diff);
    let hash3 = grid3.compute_consensus_hash();

    assert_ne!(hash1, hash3, "Different deformation must produce different consensus hash");
}
