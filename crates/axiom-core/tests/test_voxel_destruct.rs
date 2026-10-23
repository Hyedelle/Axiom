use axiom_core::voxel_destruct::{
    SeverancePlane, VoxelVolume16, VOXEL_AIR, VOXEL_DIM, VOXEL_METAL, VOXEL_MOLTEN, VOXEL_STONE,
    VOXEL_TOTAL,
};
use core::mem::{align_of, size_of};

#[test]
fn test_voxel_volume_geometry_and_alignment() {
    assert_eq!(size_of::<VoxelVolume16>(), 4160);
    assert_eq!(align_of::<VoxelVolume16>(), 64);
    assert_eq!(VOXEL_DIM, 16);
    assert_eq!(VOXEL_TOTAL, 4096);

    let vol = VoxelVolume16::new_solid(VOXEL_STONE);
    assert_eq!(vol.total_solid, 4096);
    assert_eq!(vol.damaged_count, 0);
    assert_eq!(vol.get(0, 0, 0), VOXEL_STONE);
    assert_eq!(vol.get(15, 15, 15), VOXEL_STONE);
}

#[test]
fn test_severance_plane_geometry_and_alignment() {
    assert_eq!(size_of::<SeverancePlane>(), 64);
    assert_eq!(align_of::<SeverancePlane>(), 64);

    let plane = SeverancePlane::new([0, 65536, 0], 0, 1); // Horizontal XZ plane at Y = 0
    assert_eq!(plane.point_side([0, 65536, 0]), 1);       // Above (Y=1)
    assert_eq!(plane.point_side([0, -65536, 0]), -1);     // Below (Y=-1)
    assert_eq!(plane.point_side([0, 0, 0]), 0);           // On plane

    // Test segment intersection
    let p0 = [0, -65536, 0];
    let p1 = [0, 65536, 0];
    let inter = plane.intersect_segment(p0, p1);
    assert!(inter.is_some());
    let ip = inter.unwrap();
    assert_eq!(ip[1], 0); // Intersected exactly at Y = 0
}

#[test]
fn test_voxel_crater_carving_and_raycast() {
    let mut vol = VoxelVolume16::new_solid(VOXEL_METAL);
    assert_eq!(vol.total_solid, 4096);

    // Carve top-down impact crater at center (8, 8)
    let carved = vol.carve_crater(8, 8, 4, 5);
    assert!(carved > 0);
    assert!(vol.total_solid < 4096);
    assert!(vol.damaged_count > 0);

    // Raycast from top straight down into the crater
    let hit = vol.raycast([8, 15, 8], [0, -1, 0], 16);
    assert!(hit.is_some());
    let (hit_pos, hit_mat) = hit.unwrap();
    assert!(hit_pos[1] < 15); // Hit lower down inside the crater
    assert!(hit_mat == VOXEL_MOLTEN || hit_mat == VOXEL_METAL);
}
