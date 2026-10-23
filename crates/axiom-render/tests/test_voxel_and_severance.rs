use axiom_core::fixed::Fixed;
use axiom_core::voxel_destruct::{SeverancePlane, VoxelVolume16, VOXEL_METAL, VOXEL_STONE};
use axiom_render::math3d::Vec3Fixed;
use axiom_render::mesh3d::PartMesh;
use axiom_render::voxel_mesh::{extract_voxel_mesh, sever_part_mesh};

#[test]
fn test_voxel_mesh_extraction_solid_cube() {
    let mut vol = VoxelVolume16::new_empty();
    // Place a 2x2x2 cube of stone in the center
    for y in 4..6 {
        for z in 4..6 {
            for x in 4..6 {
                vol.set(x, y, z, VOXEL_STONE);
            }
        }
    }

    let mesh = extract_voxel_mesh("stone_cube", &vol, Fixed::ONE);
    // 2x2 cube has 6 faces of 2x2 quads = 24 quads = 48 triangles
    assert_eq!(mesh.triangle_count(), 48);
    assert_eq!(mesh.name, "stone_cube");
}

#[test]
fn test_voxel_crater_mesh_extraction() {
    let mut vol = VoxelVolume16::new_solid(VOXEL_METAL);
    let carved = vol.carve_crater(8, 8, 4, 4);
    assert!(carved > 0);

    let mesh = extract_voxel_mesh("crater_floor", &vol, Fixed::from_raw(32768)); // 0.5m scale
    assert!(mesh.triangle_count() > 0);

    // Verify molten UV presence in the extracted mesh
    let has_molten_uv = mesh.vertices.iter().any(|v| v.u >= 49152 && v.v >= 49152);
    assert!(has_molten_uv);
}

#[test]
fn test_part_mesh_severance_along_plane() {
    // Create a 2x2x2 box centered at (0, 0, 0)
    let min = Vec3Fixed::new(
        Fixed::from_raw(-65536),
        Fixed::from_raw(-65536),
        Fixed::from_raw(-65536),
    );
    let max = Vec3Fixed::new(
        Fixed::from_raw(65536),
        Fixed::from_raw(65536),
        Fixed::from_raw(65536),
    );
    let box_mesh = PartMesh::create_box("hero_arm", min, max);
    assert_eq!(box_mesh.triangle_count(), 12);

    // Sever along horizontal plane at Y = 0
    let plane = SeverancePlane::new([0, 65536, 0], 0, 101);
    let (upper, lower) = sever_part_mesh(&box_mesh, &plane);

    assert_eq!(upper.name, "hero_arm_upper");
    assert_eq!(lower.name, "hero_arm_lower");

    // Both halves must contain cut geometry
    assert!(upper.triangle_count() > 0);
    assert!(lower.triangle_count() > 0);

    // Severed vertices must be present at Y = 0
    let has_zero_y_upper = upper.vertices.iter().any(|v| v.pos.y.raw() == 0);
    let has_zero_y_lower = lower.vertices.iter().any(|v| v.pos.y.raw() == 0);
    assert!(has_zero_y_upper);
    assert!(has_zero_y_lower);
}
