//! # 3D Voxel Shard Ballistic Physics Engine
//!
//! Provides 64-byte L1 cache-aligned rigid voxel shard particles expelled by
//! explosions, cratering, and mecha collisions.
//! Fully deterministic, zero floating-point math, L1 cache-resident DOD structure.

use crate::voxel_destruct::{VOXEL_ARMOR, VOXEL_METAL, VOXEL_MOLTEN, VOXEL_STONE};

/// 64-Byte L1 Cache Line Aligned Rigid 3D Voxel Shard.
#[repr(C, align(64))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VoxelShard {
    /// World position $[X, Y, Z]$ in fixed-point $Q16.16$.
    pub pos: [i32; 3],
    /// World linear velocity $[V_x, V_y, V_z]$ in fixed-point $Q16.16$.
    pub vel: [i32; 3],
    /// Orientation in 8-bit Binary Angular Measurement $[Yaw, Pitch, Roll]$ (0..255).
    pub rot_bams: [u8; 3],
    /// Material ID (`VOXEL_STONE`, `VOXEL_METAL`, `VOXEL_MOLTEN`, `VOXEL_ARMOR`).
    pub material: u8,
    /// Angular tumbling velocity $[\omega_{yaw}, \omega_{pitch}, \omega_{roll}]$ in BAMS/tick.
    pub ang_vel: [i8; 3],
    /// Total count of ground / obstacle bounces experienced.
    pub bounces: u8,
    /// Remaining lifetime in simulation ticks.
    pub life: u16,
    /// Initial allocated lifetime in simulation ticks.
    pub max_life: u16,
    /// Coefficient of restitution in permille (1000 = 1.0 = perfectly elastic).
    pub restitution_permille: u16,
    /// Rendered visual scale in permille (1000 = standard 1-voxel dimension).
    pub scale_permille: u16,
    /// Active state flag.
    pub is_active: bool,
    /// Explicit padding to guarantee exact 64-byte L1 cache alignment.
    pub _pad: [u8; 23],
}

const _: () = assert!(core::mem::size_of::<VoxelShard>() == 64);
const _: () = assert!(core::mem::align_of::<VoxelShard>() == 64);

impl Default for VoxelShard {
    fn default() -> Self {
        Self {
            pos: [0; 3],
            vel: [0; 3],
            rot_bams: [0; 3],
            material: VOXEL_STONE,
            ang_vel: [0; 3],
            bounces: 0,
            life: 0,
            max_life: 0,
            restitution_permille: 450,
            scale_permille: 1000,
            is_active: false,
            _pad: [0u8; 23],
        }
    }
}

/// 64-Byte aligned, fixed-capacity rigid Voxel Shard ballistic pool.
#[repr(C, align(64))]
pub struct VoxelShardPool<const CAP: usize = 64> {
    pub shards: [VoxelShard; CAP],
    pub active_count: usize,
    pub total_spawned: u64,
    pub floor_y_q16: i32,
    pub gravity_q16: i32,
    pub drag_permille: u16,
    pub rng_state: u64,
    pub _pad: [u8; 30],
}

const _: () = assert!(core::mem::align_of::<VoxelShardPool<64>>() == 64);

impl<const CAP: usize> VoxelShardPool<CAP> {
    /// Creates a new shard pool with given PRNG seed and default physical environment.
    pub fn new(seed: u64) -> Self {
        Self {
            shards: [VoxelShard::default(); CAP],
            active_count: 0,
            total_spawned: 0,
            floor_y_q16: -45000, // Ground level in Q16.16 (~ -0.686m)
            gravity_q16: 180,    // Downward gravity in Q16.16 per tick
            drag_permille: 985,  // 1.5% air drag per tick
            rng_state: seed,
            _pad: [0u8; 30],
        }
    }

    fn prng_next(&mut self) -> u32 {
        self.rng_state = self
            .rng_state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.rng_state >> 32) as u32
    }

    /// Spawns a single rigid shard with exact coordinates, velocity, and material properties.
    pub fn spawn_shard(
        &mut self,
        pos_q16: [i32; 3],
        vel_q16: [i32; 3],
        material: u8,
        life_ticks: u16,
    ) -> bool {
        // Find inactive slot or overwrite oldest near-expiration shard
        let mut target_idx = None;
        let mut min_life = u16::MAX;
        let mut oldest_idx = 0;

        for (i, shard) in self.shards.iter().enumerate() {
            if !shard.is_active {
                target_idx = Some(i);
                break;
            }
            if shard.life < min_life {
                min_life = shard.life;
                oldest_idx = i;
            }
        }

        let idx = target_idx.unwrap_or(oldest_idx);
        let r1 = self.prng_next();
        let r2 = self.prng_next();

        let restitution = match material {
            VOXEL_METAL => 650,  // High metallic springiness
            VOXEL_ARMOR => 550,  // Armored deflection bounce
            VOXEL_STONE => 350,  // Brittle rock dampening
            VOXEL_MOLTEN => 180, // Viscous molten slag splatter
            _ => 400,
        };

        let scale = match material {
            VOXEL_ARMOR => 1200,
            VOXEL_METAL => 1000,
            VOXEL_STONE => 850,
            VOXEL_MOLTEN => 650,
            _ => 1000,
        };

        self.shards[idx] = VoxelShard {
            pos: pos_q16,
            vel: vel_q16,
            rot_bams: [(r1 & 0xFF) as u8, ((r1 >> 8) & 0xFF) as u8, ((r1 >> 16) & 0xFF) as u8],
            material,
            ang_vel: [
                (((r2 & 0x1F) as i8) - 16),
                ((((r2 >> 8) & 0x1F) as i8) - 16),
                ((((r2 >> 16) & 0x1F) as i8) - 16),
            ],
            bounces: 0,
            life: life_ticks,
            max_life: life_ticks,
            restitution_permille: restitution,
            scale_permille: scale,
            is_active: true,
            _pad: [0u8; 23],
        };

        if target_idx.is_some() {
            self.active_count += 1;
        }
        self.total_spawned += 1;
        true
    }

    /// Spawns a radial kinetic burst of voxel shards radiating outward from an explosion.
    pub fn spawn_burst(
        &mut self,
        origin_q16: [i32; 3],
        material: u8,
        count: usize,
        base_speed_q16: i32,
    ) {
        let spawn_n = count.min(CAP);
        for _ in 0..spawn_n {
            let r = self.prng_next();
            let dx = (((r & 0xFF) as i32) - 128) * 8;
            let dy = 250 + (((r >> 8) & 0xFF) as i32) * 12; // upward initial boost
            let dz = ((((r >> 16) & 0xFF) as i32) - 128) * 8;

            let vel = [
                (dx * base_speed_q16) / 1000,
                (dy * base_speed_q16) / 1000,
                (dz * base_speed_q16) / 1000,
            ];

            let life = 60 + ((r >> 24) % 60) as u16; // 1~2 seconds
            self.spawn_shard(origin_q16, vel, material, life);
        }
    }

    /// Advances the ballistic physics simulation by 1 tick.
    /// Performs Euler integration, gravity, air drag, ground collision with restitution,
    /// and surface friction. Returns the count of ground bounce impacts this tick.
    pub fn step(&mut self) -> u32 {
        let mut bounces = 0u32;
        let mut active = 0usize;
        let floor = self.floor_y_q16;
        let grav = self.gravity_q16;
        let drag = self.drag_permille as i64;

        for s in &mut self.shards {
            if !s.is_active {
                continue;
            }

            // 1. Position integration
            s.pos[0] = s.pos[0].saturating_add(s.vel[0]);
            s.pos[1] = s.pos[1].saturating_add(s.vel[1]);
            s.pos[2] = s.pos[2].saturating_add(s.vel[2]);

            // 2. Gravity application
            s.vel[1] = s.vel[1].saturating_sub(grav);

            // 3. Air drag damping
            s.vel[0] = ((s.vel[0] as i64 * drag) / 1000) as i32;
            s.vel[2] = ((s.vel[2] as i64 * drag) / 1000) as i32;

            // 4. Angular rotation integration
            s.rot_bams[0] = s.rot_bams[0].wrapping_add(s.ang_vel[0] as u8);
            s.rot_bams[1] = s.rot_bams[1].wrapping_add(s.ang_vel[1] as u8);
            s.rot_bams[2] = s.rot_bams[2].wrapping_add(s.ang_vel[2] as u8);

            // 5. Floor collision resolution
            if s.pos[1] <= floor {
                s.pos[1] = floor;
                if s.vel[1] < 0 {
                    let rest = s.restitution_permille as i64;
                    s.vel[1] = ((s.vel[1].saturating_neg() as i64 * rest) / 1000) as i32;

                    // Ground friction damping on lateral velocities (cast to i64 to prevent overflow)
                    s.vel[0] = ((s.vel[0] as i64 * 65) / 100) as i32;
                    s.vel[2] = ((s.vel[2] as i64 * 65) / 100) as i32;

                    // Angular velocity decay on ground impact
                    s.ang_vel[0] = (s.ang_vel[0] * 6) / 10;
                    s.ang_vel[1] = (s.ang_vel[1] * 6) / 10;
                    s.ang_vel[2] = (s.ang_vel[2] * 6) / 10;

                    s.bounces = s.bounces.saturating_add(1);
                    bounces += 1;
                }
            }

            // 6. Age & expiration
            if s.life > 0 {
                s.life -= 1;
                active += 1;
            } else {
                s.is_active = false;
            }
        }

        self.active_count = active;
        bounces
    }

    /// Computes deterministic 64-bit FNV-1a consensus hash of all active shards.
    pub fn compute_consensus_hash(&self) -> u64 {
        let mut hash = 0xCBF2_9CE4_8422_2325u64;

        for s in &self.shards {
            if !s.is_active {
                continue;
            }
            hash ^= (s.pos[0] as u64) & 0xFFFF_FFFF;
            hash = hash.wrapping_mul(0x100_0000_01B3);
            hash ^= (s.pos[1] as u64) & 0xFFFF_FFFF;
            hash = hash.wrapping_mul(0x100_0000_01B3);
            hash ^= (s.pos[2] as u64) & 0xFFFF_FFFF;
            hash = hash.wrapping_mul(0x100_0000_01B3);

            hash ^= (s.vel[0] as u64) & 0xFFFF_FFFF;
            hash = hash.wrapping_mul(0x100_0000_01B3);
            hash ^= (s.vel[1] as u64) & 0xFFFF_FFFF;
            hash = hash.wrapping_mul(0x100_0000_01B3);
            hash ^= (s.vel[2] as u64) & 0xFFFF_FFFF;
            hash = hash.wrapping_mul(0x100_0000_01B3);

            hash ^= s.material as u64;
            hash = hash.wrapping_mul(0x100_0000_01B3);
            hash ^= s.bounces as u64;
            hash = hash.wrapping_mul(0x100_0000_01B3);
        }

        hash
    }
}
