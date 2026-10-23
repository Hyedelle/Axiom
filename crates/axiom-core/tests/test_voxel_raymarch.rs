use axiom_core::voxel_destruct::{
    VoxelRaymarchHit, VoxelVolume16, VOXEL_ARMOR, VOXEL_METAL, VOXEL_STONE,
};
use core::mem::{align_of, size_of};

#[test]
fn test_voxel_raymarch_hit_geometry_and_alignment() {
    assert_eq!(size_of::<VoxelRaymarchHit>(), 16);
    assert_eq!(align_of::<VoxelRaymarchHit>(), 4);

    let default_hit = VoxelRaymarchHit::default();
    assert!(!default_hit.hit);
    assert_eq!(default_hit.steps_taken, 0);
    assert_eq!(default_hit.distance_units, 0);
}

#[test]
fn test_raymarch_dda_solid_hit_and_miss() {
    let mut vol = VoxelVolume16::new_empty();
    // Place a single solid block at (8, 8, 8)
    vol.set(8, 8, 8, VOXEL_ARMOR);

    // 1. Direct hit from -Z outside the volume towards +Z
    // Center of voxel 8 is at 8,500 units
    let hit = vol.raymarch_dda([8500, 8500, -2000], [0, 0, 1000], 64);
    assert!(hit.hit);
    assert_eq!(hit.voxel_x, 8);
    assert_eq!(hit.voxel_y, 8);
    assert_eq!(hit.voxel_z, 8);
    assert_eq!(hit.material, VOXEL_ARMOR);
    assert_eq!(hit.normal_x, 0);
    assert_eq!(hit.normal_y, 0);
    assert_eq!(hit.normal_z, -1);

    // 2. Miss ray shooting past the block at X=12
    let miss = vol.raymarch_dda([12500, 8500, -2000], [0, 0, 1000], 64);
    assert!(!miss.hit);

    // 3. Complete miss that doesn't intersect volume bounds
    let bounds_miss = vol.raymarch_dda([-5000, 8500, -2000], [0, 0, 1000], 64);
    assert!(!bounds_miss.hit);
}

#[test]
fn test_raymarch_dda_six_face_normals() {
    let mut vol = VoxelVolume16::new_empty();

    // Fill a 6x6x6 cube at center [5..=10, 5..=10, 5..=10]
    for x in 5..=10 {
        for y in 5..=10 {
            for z in 5..=10 {
                vol.set(x, y, z, VOXEL_METAL);
            }
        }
    }

    let center = 7500; // coordinate along orthogonal axes

    // Ray from -X direction
    let hit_neg_x = vol.raymarch_dda([1000, center, center], [1000, 0, 0], 32);
    assert!(hit_neg_x.hit);
    assert_eq!(hit_neg_x.voxel_x, 5);
    assert_eq!(hit_neg_x.normal_x, -1);
    assert_eq!(hit_neg_x.normal_y, 0);
    assert_eq!(hit_neg_x.normal_z, 0);

    // Ray from +X direction
    let hit_pos_x = vol.raymarch_dda([15000, center, center], [-1000, 0, 0], 32);
    assert!(hit_pos_x.hit);
    assert_eq!(hit_pos_x.voxel_x, 10);
    assert_eq!(hit_pos_x.normal_x, 1);
    assert_eq!(hit_pos_x.normal_y, 0);
    assert_eq!(hit_pos_x.normal_z, 0);

    // Ray from -Y direction
    let hit_neg_y = vol.raymarch_dda([center, 1000, center], [0, 1000, 0], 32);
    assert!(hit_neg_y.hit);
    assert_eq!(hit_neg_y.voxel_y, 5);
    assert_eq!(hit_neg_y.normal_x, 0);
    assert_eq!(hit_neg_y.normal_y, -1);
    assert_eq!(hit_neg_y.normal_z, 0);

    // Ray from +Y direction
    let hit_pos_y = vol.raymarch_dda([center, 15000, center], [0, -1000, 0], 32);
    assert!(hit_pos_y.hit);
    assert_eq!(hit_pos_y.voxel_y, 10);
    assert_eq!(hit_pos_y.normal_x, 0);
    assert_eq!(hit_pos_y.normal_y, 1);
    assert_eq!(hit_pos_y.normal_z, 0);

    // Ray from -Z direction
    let hit_neg_z = vol.raymarch_dda([center, center, 1000], [0, 0, 1000], 32);
    assert!(hit_neg_z.hit);
    assert_eq!(hit_neg_z.voxel_z, 5);
    assert_eq!(hit_neg_z.normal_x, 0);
    assert_eq!(hit_neg_z.normal_y, 0);
    assert_neg_z_ok(hit_neg_z);

    // Ray from +Z direction
    let hit_pos_z = vol.raymarch_dda([center, center, 15000], [0, 0, -1000], 32);
    assert!(hit_pos_z.hit);
    assert_eq!(hit_pos_z.voxel_z, 10);
    assert_eq!(hit_pos_z.normal_x, 0);
    assert_eq!(hit_pos_z.normal_y, 0);
    assert_eq!(hit_pos_z.normal_z, 1);
}

fn assert_neg_z_ok(hit: VoxelRaymarchHit) {
    assert_eq!(hit.normal_z, -1);
}

#[test]
fn test_voxel_raymarch_shadow_occlusion() {
    let mut vol = VoxelVolume16::new_empty();

    // Create a barrier wall at Z = 8
    for x in 0..16 {
        for y in 0..16 {
            vol.set(x, y, 8, VOXEL_STONE);
        }
    }

    // Light source is in +Z direction: light direction vector is [0, 0, 1000]
    let light_dir = [0, 0, 1000];

    // Point behind the wall (Z = 4) looking towards +Z should be in shadow
    let point_behind = [8000, 8000, 4000];
    assert!(vol.is_shadowed(point_behind, light_dir, 32));

    // Point in front of the wall (Z = 12) looking towards +Z has clear path to light
    let point_in_front = [8000, 8000, 12000];
    assert!(!vol.is_shadowed(point_in_front, light_dir, 32));
}

#[test]
fn test_voxel_raymarch_throughput_benchmark() {
    let mut vol = VoxelVolume16::new_solid(VOXEL_STONE);
    vol.carve_crater(8, 8, 4, 6);

    let mut hit_count = 0usize;
    let mut normal_hash = 0u64;

    // Execute 10,000 raymarch queries
    for i in 0..10_000 {
        let x = (i % 16) as i32 * 1000 + 500;
        let y = 15000;
        let z = ((i / 16) % 16) as i32 * 1000 + 500;

        let hit = vol.raymarch_dda([x, y, z], [0, -1000, 0], 32);
        if hit.hit {
            hit_count += 1;
            normal_hash = normal_hash.wrapping_add(hit.normal_y as u64);
        }
    }

    assert!(hit_count > 0, "Must hit voxels in crater test");
}
