//! # 3D Voxel Connectivity Analysis & Dynamic Mesh Collapse Simulation
//!
//! Provides zero-float deterministic connected component analysis on `VoxelVolume16`
//! to detect unsupported floating voxel clusters caused by impacts, trenches, or explosions.
//! Automatically detaches unsupported structures and instantiates 64-byte L1 cache-aligned
//! `DynamicVoxelChunk` rigid bodies that fall under gravity, tumble in 3D, and shatter
//! into `VoxelShardPool` particles upon high-velocity ground impact.

use crate::voxel_destruct::{VoxelVolume16, VOXEL_AIR, VOXEL_DIM, VOXEL_STONE, VOXEL_TOTAL};
use crate::voxel_shard::VoxelShardPool;

/// 64-Byte L1 Cache Line Aligned Dynamic Voxel Chunk.
///
/// Represents an unsupported or detached voxel fragment falling through 3D space.
#[repr(C, align(64))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DynamicVoxelChunk {
    /// World position in Q16.16 fixed-point (X, Y, Z).
    pub pos: [i32; 3],
    /// Linear velocity in Q16.16 fixed-point (X, Y, Z).
    pub vel: [i32; 3],
    /// Binary Angular Measurement (BAMS) rotation angles (0..255).
    pub rot_bams: [u8; 3],
    /// Angular tumbling speeds per tick (-128..127).
    pub ang_vel: [i8; 3],
    /// Primary material identifier (VOXEL_STONE, VOXEL_METAL, etc.).
    pub material: u8,
    /// Number of ground impacts/bounces before settling or shattering.
    pub bounces: u8,
    /// Number of individual voxels contained within this collapsed fragment.
    pub voxel_count: u16,
    /// Remaining lifetime in ticks.
    pub life: u16,
    /// Maximum initial lifetime in ticks.
    pub max_life: u16,
    /// Visual rendering scale in permille (0..1000).
    pub scale_permille: u16,
    /// Approximate bounding extent in voxel grid units (dx, dy, dz).
    pub shape_extent: [u8; 3],
    /// Active state flag.
    pub is_active: bool,
    /// Minimum downward velocity in Q16.16 required to shatter upon ground collision.
    pub shatter_threshold: i32,
    /// Cache line padding to guarantee exact 64 bytes.
    pub _pad: [u8; 16],
}

const _: () = assert!(core::mem::size_of::<DynamicVoxelChunk>() == 64);
const _: () = assert!(core::mem::align_of::<DynamicVoxelChunk>() == 64);

impl Default for DynamicVoxelChunk {
    fn default() -> Self {
        Self {
            pos: [0; 3],
            vel: [0; 3],
            rot_bams: [0; 3],
            ang_vel: [0; 3],
            material: VOXEL_STONE,
            bounces: 0,
            voxel_count: 0,
            life: 0,
            max_life: 0,
            scale_permille: 1000,
            shape_extent: [1, 1, 1],
            is_active: false,
            shatter_threshold: 18000, // ~0.27m/s impact speed
            _pad: [0u8; 16],
        }
    }
}

/// Fixed-capacity Dynamic Voxel Collapse & Structural Integrity System.
#[derive(Clone, Copy, Debug)]
pub struct DynamicCollapseSystem<const MAX_CHUNKS: usize = 16> {
    pub chunks: [DynamicVoxelChunk; MAX_CHUNKS],
    pub active_count: usize,
    pub total_collapsed: u64,
    pub total_shattered: u64,
    pub floor_y_q16: i32,
    pub gravity_q16: i32,
    pub drag_permille: u16,
    pub rng_state: u64,
}

impl<const MAX_CHUNKS: usize> DynamicCollapseSystem<MAX_CHUNKS> {
    /// Creates a new structural collapse system with given PRNG seed and default physical parameters.
    pub fn new(seed: u64) -> Self {
        Self {
            chunks: [DynamicVoxelChunk::default(); MAX_CHUNKS],
            active_count: 0,
            total_collapsed: 0,
            total_shattered: 0,
            floor_y_q16: -45000, // Ground level in Q16.16 (~ -0.686m)
            gravity_q16: 180,    // Standard Axiom downward gravity
            drag_permille: 985,  // 1.5% air drag per tick
            rng_state: seed ^ 0x9E37_79B9_7F4A_7C15,
        }
    }

    #[inline(always)]
    fn prng_next(&mut self) -> u32 {
        self.rng_state = self
            .rng_state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.rng_state >> 32) as u32
    }

    /// Spawns a new dynamic voxel chunk into the pool.
    pub fn spawn_chunk(
        &mut self,
        pos_q16: [i32; 3],
        vel_q16: [i32; 3],
        material: u8,
        voxel_count: u16,
        shape_extent: [u8; 3],
        life: u16,
    ) -> bool {
        let r = self.prng_next();
        let rot_bams = [(r & 0xFF) as u8, ((r >> 8) & 0xFF) as u8, ((r >> 16) & 0xFF) as u8];
        let ang_vel = [
            (((r >> 24) & 0x0F) as i8) - 7,
            (((r >> 20) & 0x0F) as i8) - 7,
            (((r >> 16) & 0x0F) as i8) - 7,
        ];

        let slot = self.chunks.iter_mut().find(|c| !c.is_active);
        let chunk = match slot {
            Some(c) => c,
            None => return false,
        };

        *chunk = DynamicVoxelChunk {
            pos: pos_q16,
            vel: vel_q16,
            rot_bams,
            ang_vel,
            material,
            bounces: 0,
            voxel_count,
            life,
            max_life: life,
            scale_permille: 1000,
            shape_extent,
            is_active: true,
            shatter_threshold: 16000,
            _pad: [0u8; 16],
        };

        self.total_collapsed += 1;
        self.active_count += 1;
        true
    }

    /// Advances the ballistic physics simulation of all active collapsed fragments by 1 tick.
    ///
    /// Performs Euler integration, gravity, tumbling rotation, and floor collision.
    /// High-velocity floor impacts shatter the chunk into fine rigid shards in `shard_pool`.
    /// Returns `(bounce_count, shatter_count)`.
    pub fn step<const SHARD_CAP: usize>(
        &mut self,
        mut shard_pool: Option<&mut VoxelShardPool<SHARD_CAP>>,
    ) -> (u32, u32) {
        let mut bounces = 0u32;
        let mut shatters = 0u32;
        let mut active = 0usize;
        let floor = self.floor_y_q16;
        let grav = self.gravity_q16;
        let drag = self.drag_permille as i64;

        for chunk in &mut self.chunks {
            if !chunk.is_active {
                continue;
            }

            // 1. Position integration
            chunk.pos[0] = chunk.pos[0].saturating_add(chunk.vel[0]);
            chunk.pos[1] = chunk.pos[1].saturating_add(chunk.vel[1]);
            chunk.pos[2] = chunk.pos[2].saturating_add(chunk.vel[2]);

            // 2. Gravity application
            chunk.vel[1] = chunk.vel[1].saturating_sub(grav);

            // 3. Air drag damping
            chunk.vel[0] = ((chunk.vel[0] as i64 * drag) / 1000) as i32;
            chunk.vel[2] = ((chunk.vel[2] as i64 * drag) / 1000) as i32;

            // 4. Angular rotation integration
            chunk.rot_bams[0] = chunk.rot_bams[0].wrapping_add(chunk.ang_vel[0] as u8);
            chunk.rot_bams[1] = chunk.rot_bams[1].wrapping_add(chunk.ang_vel[1] as u8);
            chunk.rot_bams[2] = chunk.rot_bams[2].wrapping_add(chunk.ang_vel[2] as u8);

            // 5. Floor collision resolution & shattering
            if chunk.pos[1] <= floor {
                chunk.pos[1] = floor;

                if chunk.vel[1] < 0 {
                    let impact_speed = -chunk.vel[1];

                    // Check if impact is heavy enough to shatter into shards
                    if impact_speed >= chunk.shatter_threshold || chunk.bounces >= 3 {
                        chunk.is_active = false;
                        shatters += 1;
                        self.total_shattered += 1;

                        if let Some(ref mut pool) = shard_pool {
                            let shard_count = (chunk.voxel_count as usize).clamp(4, 16);
                            pool.spawn_burst(chunk.pos, chunk.material, shard_count, 950);
                        }
                        continue;
                    }

                    // Otherwise, elastic bounce with heavy fragment damping (350 permille)
                    chunk.vel[1] = ((-chunk.vel[1] as i64 * 350) / 1000) as i32;
                    chunk.vel[0] = (chunk.vel[0] * 50) / 100;
                    chunk.vel[2] = (chunk.vel[2] * 50) / 100;

                    chunk.ang_vel[0] = (chunk.ang_vel[0] * 5) / 10;
                    chunk.ang_vel[1] = (chunk.ang_vel[1] * 5) / 10;
                    chunk.ang_vel[2] = (chunk.ang_vel[2] * 5) / 10;

                    chunk.bounces = chunk.bounces.saturating_add(1);
                    bounces += 1;
                }
            }

            // 6. Aging
            if chunk.life > 0 {
                chunk.life -= 1;
                active += 1;
            } else {
                chunk.is_active = false;
            }
        }

        self.active_count = active;
        (bounces, shatters)
    }

    /// Performs structural connectivity analysis on `volume`.
    ///
    /// Identifies all solid voxels not connected to the base anchor (`y <= anchor_y`),
    /// extracts them into falling `DynamicVoxelChunk`s, and removes them from `volume`.
    /// Returns the number of newly spawned collapsing chunks.
    pub fn detect_and_collapse(
        &mut self,
        volume: &mut VoxelVolume16,
        anchor_y: i16,
        world_origin_q16: [i32; 3],
        voxel_scale_q16: i32,
    ) -> usize {
        let mut visited = [0u64; 64]; // 64 * 64 = 4096 bits
        let mut queue = [0u16; VOXEL_TOTAL];
        let mut q_head = 0usize;
        let mut q_tail = 0usize;

        #[inline(always)]
        fn is_visited(visited: &[u64; 64], idx: usize) -> bool {
            (visited[idx >> 6] & (1u64 << (idx & 63))) != 0
        }

        #[inline(always)]
        fn mark_visited(visited: &mut [u64; 64], idx: usize) {
            visited[idx >> 6] |= 1u64 << (idx & 63);
        }

        // 1. Seed BFS with all solid voxels anchored at the base (y <= anchor_y)
        let anchor_clamp = (anchor_y as usize).min(VOXEL_DIM - 1);
        for y in 0..=anchor_clamp {
            for z in 0..VOXEL_DIM {
                for x in 0..VOXEL_DIM {
                    let idx = VoxelVolume16::index(x, y, z);
                    if volume.data[idx] != VOXEL_AIR && !is_visited(&visited, idx) {
                        mark_visited(&mut visited, idx);
                        queue[q_tail] = idx as u16;
                        q_tail += 1;
                    }
                }
            }
        }

        // 2. BFS flood-fill to mark all voxels structurally supported by the anchor
        while q_head < q_tail {
            let idx = queue[q_head] as usize;
            q_head += 1;

            let x = idx % VOXEL_DIM;
            let z = (idx / VOXEL_DIM) % VOXEL_DIM;
            let y = idx / (VOXEL_DIM * VOXEL_DIM);

            // 6-connectivity neighbor check
            let neighbors: [(i32, i32, i32); 6] = [
                (x as i32 + 1, y as i32, z as i32),
                (x as i32 - 1, y as i32, z as i32),
                (x as i32, y as i32 + 1, z as i32),
                (x as i32, y as i32 - 1, z as i32),
                (x as i32, y as i32, z as i32 + 1),
                (x as i32, y as i32, z as i32 - 1),
            ];

            for (nx, ny, nz) in neighbors {
                if nx >= 0
                    && nx < VOXEL_DIM as i32
                    && ny >= 0
                    && ny < VOXEL_DIM as i32
                    && nz >= 0
                    && nz < VOXEL_DIM as i32
                {
                    let n_idx = VoxelVolume16::index(nx as usize, ny as usize, nz as usize);
                    if volume.data[n_idx] != VOXEL_AIR && !is_visited(&visited, n_idx) {
                        mark_visited(&mut visited, n_idx);
                        queue[q_tail] = n_idx as u16;
                        q_tail += 1;
                    }
                }
            }
        }

        // 3. Any solid voxel that remains unvisited is an unsupported floating fragment.
        // Group unsupported voxels into connected clusters and spawn DynamicVoxelChunks.
        let mut spawned_chunks = 0usize;

        for start_idx in 0..VOXEL_TOTAL {
            if volume.data[start_idx] == VOXEL_AIR || is_visited(&visited, start_idx) {
                continue;
            }

            // Available chunk slot check
            if self.active_count >= MAX_CHUNKS {
                break;
            }

            // Cluster BFS starting at start_idx
            let mut c_head = q_tail;
            queue[q_tail] = start_idx as u16;
            q_tail += 1;
            mark_visited(&mut visited, start_idx);

            let mut sum_x = 0i64;
            let mut sum_y = 0i64;
            let mut sum_z = 0i64;
            let mut count = 0u16;
            let mut min_x = VOXEL_DIM as u8;
            let mut max_x = 0u8;
            let mut min_y = VOXEL_DIM as u8;
            let mut max_y = 0u8;
            let mut min_z = VOXEL_DIM as u8;
            let mut max_z = 0u8;
            let dominant_material = volume.data[start_idx];

            while c_head < q_tail {
                let curr_idx = queue[c_head] as usize;
                c_head += 1;

                let cx = (curr_idx % VOXEL_DIM) as u8;
                let cz = ((curr_idx / VOXEL_DIM) % VOXEL_DIM) as u8;
                let cy = (curr_idx / (VOXEL_DIM * VOXEL_DIM)) as u8;

                sum_x += cx as i64;
                sum_y += cy as i64;
                sum_z += cz as i64;
                count += 1;

                min_x = min_x.min(cx);
                max_x = max_x.max(cx);
                min_y = min_y.min(cy);
                max_y = max_y.max(cy);
                min_z = min_z.min(cz);
                max_z = max_z.max(cz);

                // Clear the voxel from the static volume
                volume.data[curr_idx] = VOXEL_AIR;
                volume.damaged_count = volume.damaged_count.saturating_add(1);
                volume.total_solid = volume.total_solid.saturating_sub(1);

                // Explore neighbors in this unsupported cluster
                let c_neighbors: [(i32, i32, i32); 6] = [
                    (cx as i32 + 1, cy as i32, cz as i32),
                    (cx as i32 - 1, cy as i32, cz as i32),
                    (cx as i32, cy as i32 + 1, cz as i32),
                    (cx as i32, cy as i32 - 1, cz as i32),
                    (cx as i32, cy as i32, cz as i32 + 1),
                    (cx as i32, cy as i32, cz as i32 - 1),
                ];

                for (nx, ny, nz) in c_neighbors {
                    if nx >= 0
                        && nx < VOXEL_DIM as i32
                        && ny >= 0
                        && ny < VOXEL_DIM as i32
                        && nz >= 0
                        && nz < VOXEL_DIM as i32
                    {
                        let n_idx = VoxelVolume16::index(nx as usize, ny as usize, nz as usize);
                        if volume.data[n_idx] != VOXEL_AIR && !is_visited(&visited, n_idx) {
                            mark_visited(&mut visited, n_idx);
                            queue[q_tail] = n_idx as u16;
                            q_tail += 1;
                        }
                    }
                }
            }

            if count > 0 {
                // Compute Center of Mass (COM) in world Q16.16 coordinates
                let com_x = (sum_x * voxel_scale_q16 as i64) / count as i64;
                let com_y = (sum_y * voxel_scale_q16 as i64) / count as i64;
                let com_z = (sum_z * voxel_scale_q16 as i64) / count as i64;

                let world_pos = [
                    world_origin_q16[0] + com_x as i32,
                    world_origin_q16[1] + com_y as i32,
                    world_origin_q16[2] + com_z as i32,
                ];

                // Slight initial downward and outward scattering impulse
                let r = self.prng_next();
                let vel_x = ((((r & 0xFF) as i32) - 128) * 1200) / 128;
                let vel_y = -3000 - (((r >> 8) & 0x7F) as i32 * 50); // Downward velocity
                let vel_z = (((((r >> 16) & 0xFF) as i32) - 128) * 1200) / 128;

                let extent = [
                    (max_x.saturating_sub(min_x) + 1).max(1),
                    (max_y.saturating_sub(min_y) + 1).max(1),
                    (max_z.saturating_sub(min_z) + 1).max(1),
                ];

                if self.spawn_chunk(world_pos, [vel_x, vel_y, vel_z], dominant_material, count, extent, 240) {
                    spawned_chunks += 1;
                }
            }
        }

        spawned_chunks
    }

    /// Computes deterministic 64-bit FNV-1a consensus hash of all active collapsed fragments.
    pub fn compute_consensus_hash(&self) -> u64 {
        let mut hash = 0xCBF2_9CE4_8422_2325u64;

        for chunk in &self.chunks {
            if !chunk.is_active {
                continue;
            }
            hash ^= (chunk.pos[0] as u64) & 0xFFFF_FFFF;
            hash = hash.wrapping_mul(0x100_0000_01B3);
            hash ^= (chunk.pos[1] as u64) & 0xFFFF_FFFF;
            hash = hash.wrapping_mul(0x100_0000_01B3);
            hash ^= (chunk.pos[2] as u64) & 0xFFFF_FFFF;
            hash = hash.wrapping_mul(0x100_0000_01B3);

            hash ^= (chunk.vel[0] as u64) & 0xFFFF_FFFF;
            hash = hash.wrapping_mul(0x100_0000_01B3);
            hash ^= (chunk.vel[1] as u64) & 0xFFFF_FFFF;
            hash = hash.wrapping_mul(0x100_0000_01B3);

            hash ^= chunk.rot_bams[0] as u64;
            hash = hash.wrapping_mul(0x100_0000_01B3);
            hash ^= chunk.rot_bams[1] as u64;
            hash = hash.wrapping_mul(0x100_0000_01B3);
            hash ^= chunk.rot_bams[2] as u64;
            hash = hash.wrapping_mul(0x100_0000_01B3);

            hash ^= (chunk.voxel_count as u64) << 16 | (chunk.life as u64);
            hash = hash.wrapping_mul(0x100_0000_01B3);
        }

        hash
    }
}
