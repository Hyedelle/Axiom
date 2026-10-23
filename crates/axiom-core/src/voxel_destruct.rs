//! Deterministic 3D Voxel Destruction & Geometric Severance Core Engine.
//!
//! Provides zero-float 64B L1-aligned 3D voxel volume slabs for terrain craters
//! and fixed-point geometric cutting planes for physical mecha limb severance.

pub const VOXEL_DIM: usize = 16;
pub const VOXEL_TOTAL: usize = VOXEL_DIM * VOXEL_DIM * VOXEL_DIM; // 4096

pub const VOXEL_AIR: u8 = 0;
pub const VOXEL_STONE: u8 = 1;
pub const VOXEL_METAL: u8 = 2;
pub const VOXEL_MOLTEN: u8 = 3;
pub const VOXEL_ARMOR: u8 = 4;

/// 4160-byte (65 x 64B L1 Cache Lines) 16x16x16 Destructible Voxel Slab.
#[repr(C, align(64))]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct VoxelVolume16 {
    pub data: [u8; VOXEL_TOTAL],   // 4096B: 3D grid data [Y * 256 + Z * 16 + X]
    pub damaged_count: u32,        // 4B: Cumulative voxels destroyed/carved
    pub total_solid: u32,          // 4B: Currently active solid voxels
    pub _pad: [u8; 56],            // 56B: Exact alignment padding
}

const _: () = assert!(core::mem::size_of::<VoxelVolume16>() == 4160);
const _: () = assert!(core::mem::align_of::<VoxelVolume16>() == 64);

impl VoxelVolume16 {
    /// Creates a voxel volume filled with a single solid material.
    pub const fn new_solid(material: u8) -> Self {
        Self {
            data: [material; VOXEL_TOTAL],
            damaged_count: 0,
            total_solid: if material == VOXEL_AIR { 0 } else { VOXEL_TOTAL as u32 },
            _pad: [0u8; 56],
        }
    }

    /// Creates an empty (air) voxel volume.
    pub const fn new_empty() -> Self {
        Self::new_solid(VOXEL_AIR)
    }

    #[inline(always)]
    pub const fn index(x: usize, y: usize, z: usize) -> usize {
        y * (VOXEL_DIM * VOXEL_DIM) + z * VOXEL_DIM + x
    }

    #[inline(always)]
    pub fn get(&self, x: usize, y: usize, z: usize) -> u8 {
        if x < VOXEL_DIM && y < VOXEL_DIM && z < VOXEL_DIM {
            self.data[Self::index(x, y, z)]
        } else {
            VOXEL_AIR
        }
    }

    #[inline(always)]
    pub fn set(&mut self, x: usize, y: usize, z: usize, material: u8) {
        if x < VOXEL_DIM && y < VOXEL_DIM && z < VOXEL_DIM {
            let idx = Self::index(x, y, z);
            let prev = self.data[idx];
            if prev != material {
                if prev == VOXEL_AIR && material != VOXEL_AIR {
                    self.total_solid = self.total_solid.saturating_add(1);
                } else if prev != VOXEL_AIR && material == VOXEL_AIR {
                    self.total_solid = self.total_solid.saturating_sub(1);
                    self.damaged_count = self.damaged_count.saturating_add(1);
                }
                self.data[idx] = material;
            }
        }
    }

    /// Carves a spherical blast volume.
    /// Replaces solid voxels within radius with `fill_material` (e.g. `VOXEL_AIR` or `VOXEL_MOLTEN`).
    /// Returns the number of voxels newly carved out.
    pub fn carve_sphere(
        &mut self,
        cx: i16,
        cy: i16,
        cz: i16,
        radius: i16,
        fill_material: u8,
    ) -> u32 {
        let r2 = (radius as i32) * (radius as i32);
        let mut carved = 0u32;

        let min_x = (cx - radius).clamp(0, (VOXEL_DIM - 1) as i16) as usize;
        let max_x = (cx + radius).clamp(0, (VOXEL_DIM - 1) as i16) as usize;
        let min_y = (cy - radius).clamp(0, (VOXEL_DIM - 1) as i16) as usize;
        let max_y = (cy + radius).clamp(0, (VOXEL_DIM - 1) as i16) as usize;
        let min_z = (cz - radius).clamp(0, (VOXEL_DIM - 1) as i16) as usize;
        let max_z = (cz + radius).clamp(0, (VOXEL_DIM - 1) as i16) as usize;

        for y in min_y..=max_y {
            let dy = y as i32 - cy as i32;
            let dy2 = dy * dy;
            for z in min_z..=max_z {
                let dz = z as i32 - cz as i32;
                let dz2 = dz * dz;
                for x in min_x..=max_x {
                    let dx = x as i32 - cx as i32;
                    if dx * dx + dy2 + dz2 <= r2 {
                        let idx = Self::index(x, y, z);
                        let prev = self.data[idx];
                        if prev != fill_material {
                            if prev != VOXEL_AIR && fill_material == VOXEL_AIR {
                                carved += 1;
                            }
                            self.set(x, y, z, fill_material);
                        }
                    }
                }
            }
        }
        carved
    }

    /// Carves an arena floor impact crater from top-down ($Y$ surface).
    pub fn carve_crater(&mut self, cx: i16, cz: i16, depth: i16, radius: i16) -> u32 {
        let top_y = (VOXEL_DIM - 1) as i16;
        let c1 = self.carve_sphere(cx, top_y, cz, radius, VOXEL_AIR);
        // Melt inner boundary rim
        self.carve_sphere(cx, top_y - depth, cz, (radius / 2).max(1), VOXEL_MOLTEN);
        c1
    }

    /// Carves an arena floor impact crater and expels kinetic rigid voxel shards into the shard pool.
    pub fn carve_crater_with_shards<const CAP: usize>(
        &mut self,
        cx: i16,
        cz: i16,
        depth: i16,
        radius: i16,
        pool: &mut crate::voxel_shard::VoxelShardPool<CAP>,
        world_origin_q16: [i32; 3],
        voxel_scale_q16: i32,
    ) -> u32 {
        let carved = self.carve_crater(cx, cz, depth, radius);
        if carved > 0 {
            let origin = [
                world_origin_q16[0] + (cx as i32 * voxel_scale_q16),
                world_origin_q16[1] + ((VOXEL_DIM as i32 - 1) * voxel_scale_q16),
                world_origin_q16[2] + (cz as i32 * voxel_scale_q16),
            ];
            let shard_count = (carved as usize).min(16);
            pool.spawn_burst(origin, VOXEL_STONE, shard_count, 1200);
        }
        carved
    }

    /// Deterministic 3D raycast traversing voxel grid using integer steps.
    pub fn raycast(
        &self,
        origin: [i16; 3],
        dir: [i16; 3],
        max_steps: u16,
    ) -> Option<([i16; 3], u8)> {
        let mut curr = origin;
        for _ in 0..max_steps {
            if curr[0] < 0
                || curr[0] >= VOXEL_DIM as i16
                || curr[1] < 0
                || curr[1] >= VOXEL_DIM as i16
                || curr[2] < 0
                || curr[2] >= VOXEL_DIM as i16
            {
                break;
            }

            let mat = self.get(curr[0] as usize, curr[1] as usize, curr[2] as usize);
            if mat != VOXEL_AIR {
                return Some((curr, mat));
            }

            curr[0] += dir[0].signum();
            curr[1] += dir[1].signum();
            curr[2] += dir[2].signum();
        }
        None
    }
}

/// 64-byte L1 Cache Aligned Deterministic Geometric Cutting Plane.
#[repr(C, align(64))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SeverancePlane {
    pub normal: [i32; 3], // Q16.16 fixed normal vector
    pub offset: i64,      // Q16.16 plane distance offset D
    pub cut_id: u32,      // Unique cut sequence identifier
    pub _pad: [u8; 36],   // Alignment padding to 64 bytes
}

const _: () = assert!(core::mem::size_of::<SeverancePlane>() == 64);
const _: () = assert!(core::mem::align_of::<SeverancePlane>() == 64);

impl SeverancePlane {
    /// Creates a cutting plane $N \cdot X + D = 0$.
    pub const fn new(normal: [i32; 3], offset: i64, cut_id: u32) -> Self {
        Self {
            normal,
            offset,
            cut_id,
            _pad: [0u8; 36],
        }
    }

    /// Evaluates signed distance of a Q16.16 point to the plane.
    /// Result > 0 is in the positive half-space, < 0 is negative half-space.
    #[inline(always)]
    pub fn signed_distance(&self, point: [i32; 3]) -> i64 {
        let dot = (self.normal[0] as i64 * point[0] as i64
            + self.normal[1] as i64 * point[1] as i64
            + self.normal[2] as i64 * point[2] as i64)
            >> 16;
        dot + self.offset
    }

    /// Determines which side of the plane a point lies on (+1, -1, or 0).
    #[inline(always)]
    pub fn point_side(&self, point: [i32; 3]) -> i8 {
        let dist = self.signed_distance(point);
        if dist > 0 {
            1
        } else if dist < 0 {
            -1
        } else {
            0
        }
    }

    /// Computes intersection point between segment P0-P1 and this cutting plane in Q16.16.
    pub fn intersect_segment(&self, p0: [i32; 3], p1: [i32; 3]) -> Option<[i32; 3]> {
        let d0 = self.signed_distance(p0);
        let d1 = self.signed_distance(p1);

        if (d0 > 0 && d1 > 0) || (d0 < 0 && d1 < 0) {
            return None; // No intersection, both on same side
        }

        let denom = d0 - d1;
        if denom == 0 {
            return Some(p0);
        }

        // Interpolation factor t in Q16 (0..65536)
        let t_num = d0 << 16;
        let t = (t_num / denom).clamp(0, 65536) as i64;

        let ix = p0[0] as i64 + (((p1[0] as i64 - p0[0] as i64) * t) >> 16);
        let iy = p0[1] as i64 + (((p1[1] as i64 - p0[1] as i64) * t) >> 16);
        let iz = p0[2] as i64 + (((p1[2] as i64 - p0[2] as i64) * t) >> 16);

        Some([ix as i32, iy as i32, iz as i32])
    }
}
