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
pub const VOXEL_WATER: u8 = 5;

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

/// Result of a 3D Fast Voxel DDA Raymarch query (16 bytes = exactly 1/4 of a 64B L1 cache line).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct VoxelRaymarchHit {
    pub hit: bool,
    pub voxel_x: i8,
    pub voxel_y: i8,
    pub voxel_z: i8,
    pub normal_x: i8,
    pub normal_y: i8,
    pub normal_z: i8,
    pub material: u8,
    pub steps_taken: u8,
    pub _pad: [u8; 3],
    pub distance_units: u32,
}

const _: () = assert!(core::mem::size_of::<VoxelRaymarchHit>() == 16);
const _: () = assert!(core::mem::align_of::<VoxelRaymarchHit>() == 4);

impl VoxelVolume16 {
    pub const VOXEL_SCALE: i32 = 1000;
    pub const VOLUME_EXTENT: i32 = (VOXEL_DIM as i32) * Self::VOXEL_SCALE; // 16,000 units

    /// Performs an integer Fast Voxel DDA (Amanatides-Woo) raymarch across the 3D voxel volume.
    pub fn raymarch_dda(
        &self,
        origin: [i32; 3],
        direction: [i32; 3],
        max_steps: u8,
    ) -> VoxelRaymarchHit {
        let dx = direction[0];
        let dy = direction[1];
        let dz = direction[2];

        if dx == 0 && dy == 0 && dz == 0 {
            return VoxelRaymarchHit::default();
        }

        // Ray clipping to [0, VOLUME_EXTENT]^3
        let mut t_near = i64::MIN;
        let mut t_far = i64::MAX;
        let mut enter_normal = [0i8; 3];

        let bounds = [
            (origin[0], dx, 0i32, Self::VOLUME_EXTENT),
            (origin[1], dy, 0i32, Self::VOLUME_EXTENT),
            (origin[2], dz, 0i32, Self::VOLUME_EXTENT),
        ];

        for (axis, &(orig, dir, b_min, b_max)) in bounds.iter().enumerate() {
            if dir == 0 {
                if orig < b_min || orig >= b_max {
                    return VoxelRaymarchHit::default(); // Parallel and outside
                }
            } else {
                let mut t1 = ((b_min - orig) as i64 * 1000) / (dir as i64);
                let mut t2 = ((b_max - orig) as i64 * 1000) / (dir as i64);
                let mut n1 = if dir > 0 { -1i8 } else { 1i8 };

                if t1 > t2 {
                    core::mem::swap(&mut t1, &mut t2);
                    n1 = -n1;
                }

                if t1 > t_near {
                    t_near = t1;
                    enter_normal = [0i8; 3];
                    enter_normal[axis] = n1;
                }
                if t2 < t_far {
                    t_far = t2;
                }

                if t_near > t_far || t_far < 0 {
                    return VoxelRaymarchHit::default(); // Misses bounding volume
                }
            }
        }

        // Determine starting position
        let (px, py, pz, initial_normal) = if t_near > 0 {
            // Origin is outside, start at entry boundary
            let ex = origin[0] as i64 + (dx as i64 * t_near) / 1000;
            let ey = origin[1] as i64 + (dy as i64 * t_near) / 1000;
            let ez = origin[2] as i64 + (dz as i64 * t_near) / 1000;
            (ex as i32, ey as i32, ez as i32, enter_normal)
        } else {
            // Origin is inside volume
            (origin[0], origin[1], origin[2], [0i8; 3])
        };

        // Clamp to voxel grid index
        let mut vx = (px / Self::VOXEL_SCALE).clamp(0, (VOXEL_DIM - 1) as i32);
        let mut vy = (py / Self::VOXEL_SCALE).clamp(0, (VOXEL_DIM - 1) as i32);
        let mut vz = (pz / Self::VOXEL_SCALE).clamp(0, (VOXEL_DIM - 1) as i32);

        let step_x = if dx > 0 { 1i32 } else if dx < 0 { -1i32 } else { 0i32 };
        let step_y = if dy > 0 { 1i32 } else if dy < 0 { -1i32 } else { 0i32 };
        let step_z = if dz > 0 { 1i32 } else if dz < 0 { -1i32 } else { 0i32 };

        let t_delta_x = if dx != 0 { ((Self::VOXEL_SCALE as i64 * 1000) / dx.abs() as i64).max(1) } else { i64::MAX };
        let t_delta_y = if dy != 0 { ((Self::VOXEL_SCALE as i64 * 1000) / dy.abs() as i64).max(1) } else { i64::MAX };
        let t_delta_z = if dz != 0 { ((Self::VOXEL_SCALE as i64 * 1000) / dz.abs() as i64).max(1) } else { i64::MAX };

        let mut t_max_x = if step_x > 0 {
            (((vx + 1) * Self::VOXEL_SCALE - px) as i64 * 1000) / dx.abs() as i64
        } else if step_x < 0 {
            ((px - vx * Self::VOXEL_SCALE) as i64 * 1000) / dx.abs() as i64
        } else {
            i64::MAX
        };

        let mut t_max_y = if step_y > 0 {
            (((vy + 1) * Self::VOXEL_SCALE - py) as i64 * 1000) / dy.abs() as i64
        } else if step_y < 0 {
            ((py - vy * Self::VOXEL_SCALE) as i64 * 1000) / dy.abs() as i64
        } else {
            i64::MAX
        };

        let mut t_max_z = if step_z > 0 {
            (((vz + 1) * Self::VOXEL_SCALE - pz) as i64 * 1000) / dz.abs() as i64
        } else if step_z < 0 {
            ((pz - vz * Self::VOXEL_SCALE) as i64 * 1000) / dz.abs() as i64
        } else {
            i64::MAX
        };

        let mut current_normal = initial_normal;
        let mut steps = 0u8;

        while steps < max_steps {
            steps += 1;

            if vx >= 0 && vx < (VOXEL_DIM as i32) && vy >= 0 && vy < (VOXEL_DIM as i32) && vz >= 0 && vz < (VOXEL_DIM as i32) {
                let mat = self.get(vx as usize, vy as usize, vz as usize);
                if mat != VOXEL_AIR {
                    let dist = t_near.max(0) as u32;
                    return VoxelRaymarchHit {
                        hit: true,
                        voxel_x: vx as i8,
                        voxel_y: vy as i8,
                        voxel_z: vz as i8,
                        normal_x: current_normal[0],
                        normal_y: current_normal[1],
                        normal_z: current_normal[2],
                        material: mat,
                        steps_taken: steps,
                        _pad: [0; 3],
                        distance_units: dist,
                    };
                }
            } else {
                break; // Exited volume
            }

            // Step to next voxel
            if t_max_x < t_max_y && t_max_x < t_max_z {
                vx += step_x;
                t_max_x += t_delta_x;
                current_normal = [-step_x as i8, 0, 0];
            } else if t_max_y < t_max_z {
                vy += step_y;
                t_max_y += t_delta_y;
                current_normal = [0, -step_y as i8, 0];
            } else {
                vz += step_z;
                t_max_z += t_delta_z;
                current_normal = [0, 0, -step_z as i8];
            }
        }

        VoxelRaymarchHit::default()
    }

    /// Tests if a point inside or near the volume has an unoccluded line-of-sight to a directional light.
    pub fn is_shadowed(&self, origin: [i32; 3], light_dir: [i32; 3], max_steps: u8) -> bool {
        let biased_origin = [
            origin[0] + (light_dir[0].clamp(-1, 1) * 50),
            origin[1] + (light_dir[1].clamp(-1, 1) * 50),
            origin[2] + (light_dir[2].clamp(-1, 1) * 50),
        ];
        let hit = self.raymarch_dda(biased_origin, light_dir, max_steps);
        hit.hit
    }
}
