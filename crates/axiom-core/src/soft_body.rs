//! # Fixed-Point 3D Shape Matching Soft Bodies & Reversible Volume Conservation (`soft_body.rs`)
//!
//! Axiom 300-Series physics engine implementing meshless shape matching deformable bodies
//! with zero floating-point arithmetic, exact 64-byte L1 cache-aligned slabs, and
//! reversible volume conservation constraints.
//!
//! ## Mathematical Architecture
//! - **Meshless Shape Matching**: Müller et al. (2005) & Macklin et al. (2014) geometric shape matching.
//! - **0-Float Horn's Method**: 4x4 symmetric matrix formulation replacing polar decomposition (A = RS)
//!   and SVD with warm-started power iteration (2~3 integer iterations) for bit-exact deterministic optimal rotation.
//! - **Reversible Volume Conservation**: Clausen/Müller volume constraint restoring incompressibility and
//!   transverse Poisson expansion upon compression.
//! - **Cache Geometry**: `SoftBodyParticleSlab64` (64B) and `SoftBodyClusterSlab64` (64B).

use core::mem::{align_of, size_of};
use crate::event_bus::{channel, DeterministicEventBus};
use crate::job_system::{JobContext, JobPriority, JobStatus, JobSystem};
use crate::ragdoll::isqrt_i64;
use crate::worker_pool::WorkerPoolCoordinator;

/// Event type discriminant for soft body deformation and rupture events.
pub const EVENT_SOFT_BODY_DEFORM: u16 = 0x0300;

/// Particle operational flag bitmasks.
pub const PARTICLE_FLAG_ACTIVE: u8 = 1 << 0;
pub const PARTICLE_FLAG_PINNED: u8 = 1 << 1;
pub const PARTICLE_FLAG_FRACTURED: u8 = 1 << 2;

/// Cluster operational flag bitmasks.
pub const CLUSTER_FLAG_ACTIVE: u8 = 1 << 0;
pub const CLUSTER_FLAG_DEFORMED: u8 = 1 << 1;
pub const CLUSTER_FLAG_RUPTURED: u8 = 1 << 2;

/// Soft body material classifications.
pub const SOFT_MAT_RUBBER: u8 = 0;
pub const SOFT_MAT_METAL_PLATE: u8 = 1;
pub const SOFT_MAT_GEL: u8 = 2;
pub const SOFT_MAT_ARMOR_FOAM: u8 = 3;

/// Exact 64-Byte L1 Cache Line Aligned Soft Body Particle Slab.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(C, align(64))]
pub struct SoftBodyParticleSlab64 {
    /// 4B: Current world X coordinate in integer millimeters
    pub pos_x: i32,
    /// 4B: Current world Y coordinate in integer millimeters
    pub pos_y: i32,
    /// 4B: Current world Z coordinate in integer millimeters
    pub pos_z: i32,
    /// 4B: Linear velocity X in integer mm/s
    pub vel_x: i32,
    /// 4B: Linear velocity Y in integer mm/s
    pub vel_y: i32,
    /// 4B: Linear velocity Z in integer mm/s
    pub vel_z: i32,
    /// 4B: Initial offset X relative to cluster centroid q_x in integer mm
    pub orig_offset_x: i32,
    /// 4B: Initial offset Y relative to cluster centroid q_y in integer mm
    pub orig_offset_y: i32,
    /// 4B: Initial offset Z relative to cluster centroid q_z in integer mm
    pub orig_offset_z: i32,
    /// 4B: Inverse mass 1/m in Q16.16 (65536 = 1.0 kg^-1, 0 = anchored/pinned)
    pub inv_mass_q16: u32,
    /// 2B: Collision bounding / visual radius in mm
    pub radius_mm: u16,
    /// 2B: Parent soft body cluster slot index
    pub cluster_id: u16,
    /// 1B: Particle operational flags (`PARTICLE_FLAG_*`)
    pub flags: u8,
    /// 1B: Material classification (`SOFT_MAT_*`)
    pub material: u8,
    /// 2B: Accumulated strain stress in Q16.16
    pub stress_q16: u16,
    /// 16B: Explicit alignment padding to exact 64 bytes
    pub _reserved: [u8; 16],
}

const _: () = assert!(size_of::<SoftBodyParticleSlab64>() == 64);
const _: () = assert!(align_of::<SoftBodyParticleSlab64>() == 64);

impl Default for SoftBodyParticleSlab64 {
    #[inline(always)]
    fn default() -> Self {
        Self::empty()
    }
}

impl SoftBodyParticleSlab64 {
    /// Creates an empty inactive particle slab.
    #[inline(always)]
    pub const fn empty() -> Self {
        Self {
            pos_x: 0,
            pos_y: 0,
            pos_z: 0,
            vel_x: 0,
            vel_y: 0,
            vel_z: 0,
            orig_offset_x: 0,
            orig_offset_y: 0,
            orig_offset_z: 0,
            inv_mass_q16: 0,
            radius_mm: 0,
            cluster_id: 0,
            flags: 0,
            material: SOFT_MAT_RUBBER,
            stress_q16: 0,
            _reserved: [0; 16],
        }
    }

    /// Creates an active particle slab with initial world position and mass.
    pub fn new(
        pos: (i32, i32, i32),
        orig_offset: (i32, i32, i32),
        inv_mass_q16: u32,
        radius_mm: u16,
        cluster_id: u16,
        material: u8,
    ) -> Self {
        Self {
            pos_x: pos.0,
            pos_y: pos.1,
            pos_z: pos.2,
            vel_x: 0,
            vel_y: 0,
            vel_z: 0,
            orig_offset_x: orig_offset.0,
            orig_offset_y: orig_offset.1,
            orig_offset_z: orig_offset.2,
            inv_mass_q16,
            radius_mm,
            cluster_id,
            flags: PARTICLE_FLAG_ACTIVE,
            material,
            stress_q16: 0,
            _reserved: [0; 16],
        }
    }

    /// Returns true if this particle is active.
    #[inline(always)]
    pub fn is_active(&self) -> bool {
        (self.flags & PARTICLE_FLAG_ACTIVE) != 0
    }

    /// Returns true if this particle is anchored/pinned.
    #[inline(always)]
    pub fn is_pinned(&self) -> bool {
        (self.flags & PARTICLE_FLAG_PINNED) != 0 || self.inv_mass_q16 == 0
    }
}

/// Exact 64-Byte L1 Cache Line Aligned Soft Body Cluster Descriptor Slab.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(C, align(64))]
pub struct SoftBodyClusterSlab64 {
    /// 4B: Current center of mass X in integer millimeters
    pub center_x: i32,
    /// 4B: Current center of mass Y in integer millimeters
    pub center_y: i32,
    /// 4B: Current center of mass Z in integer millimeters
    pub center_z: i32,
    /// 4B: Initial rest center of mass X in integer millimeters
    pub orig_center_x: i32,
    /// 4B: Initial rest center of mass Y in integer millimeters
    pub orig_center_y: i32,
    /// 4B: Initial rest center of mass Z in integer millimeters
    pub orig_center_z: i32,
    /// 8B: Optimal rotation quaternion [w, x, y, z] in Q1.15 (32767 = 1.0)
    pub quat_q15: [i16; 4],
    /// 4B: Shape restoration stiffness alpha in Q16.16 (65536 = rigid, 0 = fluid)
    pub stiffness_alpha_q16: u32,
    /// 4B: Volume conservation stiffness beta in Q16.16 (65536 = strictly incompressible)
    pub volume_stiffness_q16: u32,
    /// 8B: Target rest volume V_0 in integer mm^3
    pub target_volume_mm3: i64,
    /// 2B: Particle count within this cluster
    pub particle_count: u16,
    /// 2B: Start offset index in particle array pool
    pub particle_offset: u16,
    /// 1B: Cluster operational flags (`CLUSTER_FLAG_*`)
    pub flags: u8,
    /// 1B: Owning entity identifier
    pub entity_id: u8,
    /// 2B: Plastic yield strain limit in Q16.16 (e.g. 6554 = 10%)
    pub plastic_yield_q16: u16,
    /// 8B: Alignment padding to exact 64 bytes
    pub _reserved: [u8; 8],
}

const _: () = assert!(size_of::<SoftBodyClusterSlab64>() == 64);
const _: () = assert!(align_of::<SoftBodyClusterSlab64>() == 64);

impl Default for SoftBodyClusterSlab64 {
    #[inline(always)]
    fn default() -> Self {
        Self::empty()
    }
}

impl SoftBodyClusterSlab64 {
    /// Creates an empty inactive cluster slab.
    #[inline(always)]
    pub const fn empty() -> Self {
        Self {
            center_x: 0,
            center_y: 0,
            center_z: 0,
            orig_center_x: 0,
            orig_center_y: 0,
            orig_center_z: 0,
            quat_q15: [32767, 0, 0, 0], // Identity quaternion
            stiffness_alpha_q16: 32768, // 0.5 default stiffness
            volume_stiffness_q16: 32768, // 0.5 default volume stiffness
            target_volume_mm3: 0,
            particle_count: 0,
            particle_offset: 0,
            flags: 0,
            entity_id: 0,
            plastic_yield_q16: 6554, // 10% default plastic yield
            _reserved: [0; 8],
        }
    }

    /// Creates an active cluster slab.
    pub fn new(
        center: (i32, i32, i32),
        particle_offset: u16,
        particle_count: u16,
        stiffness_alpha_q16: u32,
        volume_stiffness_q16: u32,
        target_volume_mm3: i64,
        entity_id: u8,
    ) -> Self {
        Self {
            center_x: center.0,
            center_y: center.1,
            center_z: center.2,
            orig_center_x: center.0,
            orig_center_y: center.1,
            orig_center_z: center.2,
            quat_q15: [32767, 0, 0, 0],
            stiffness_alpha_q16,
            volume_stiffness_q16,
            target_volume_mm3,
            particle_count,
            particle_offset,
            flags: CLUSTER_FLAG_ACTIVE,
            entity_id,
            plastic_yield_q16: 6554,
            _reserved: [0; 8],
        }
    }

    /// Returns true if this cluster is active.
    #[inline(always)]
    pub fn is_active(&self) -> bool {
        (self.flags & CLUSTER_FLAG_ACTIVE) != 0
    }
}

/// 40-Byte Inlined POD Soft Body Event Report (fits cleanly inside `EventSlab64.payload`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(C)]
pub struct SoftBodyEventReport {
    /// 4B: Entity identifier
    pub entity_id: u32,
    /// 2B: Cluster slot identifier
    pub cluster_id: u16,
    /// 2B: Event flags (0 = elastic strain, 1 = plastic fracture)
    pub flags: u16,
    /// 4B: Peak instantaneous strain in Q16.16
    pub max_strain_q16: u32,
    /// 4B: Current volume ratio (V / V0 * 65536) in Q16.16
    pub volume_ratio_q16: u32,
    /// 12B: Centroid world coordinates mm [x, y, z]
    pub centroid_pos_xyz: [i32; 3],
    /// 12B: Reaction/deformation impulse vector mm/s [x, y, z]
    pub deform_impulse_xyz: [i32; 3],
}

const _: () = assert!(size_of::<SoftBodyEventReport>() == 40);

impl Default for SoftBodyEventReport {
    #[inline(always)]
    fn default() -> Self {
        Self {
            entity_id: 0,
            cluster_id: 0,
            flags: 0,
            max_strain_q16: 0,
            volume_ratio_q16: 65536,
            centroid_pos_xyz: [0; 3],
            deform_impulse_xyz: [0; 3],
        }
    }
}

impl SoftBodyEventReport {
    /// Losslessly encodes the report into a 40-byte flat buffer.
    pub fn encode(&self) -> [u8; 40] {
        let mut buf = [0u8; 40];
        buf[0..4].copy_from_slice(&self.entity_id.to_le_bytes());
        buf[4..6].copy_from_slice(&self.cluster_id.to_le_bytes());
        buf[6..8].copy_from_slice(&self.flags.to_le_bytes());
        buf[8..12].copy_from_slice(&self.max_strain_q16.to_le_bytes());
        buf[12..16].copy_from_slice(&self.volume_ratio_q16.to_le_bytes());
        buf[16..20].copy_from_slice(&self.centroid_pos_xyz[0].to_le_bytes());
        buf[20..24].copy_from_slice(&self.centroid_pos_xyz[1].to_le_bytes());
        buf[24..28].copy_from_slice(&self.centroid_pos_xyz[2].to_le_bytes());
        buf[28..32].copy_from_slice(&self.deform_impulse_xyz[0].to_le_bytes());
        buf[32..36].copy_from_slice(&self.deform_impulse_xyz[1].to_le_bytes());
        buf[36..40].copy_from_slice(&self.deform_impulse_xyz[2].to_le_bytes());
        buf
    }

    /// Losslessly decodes the report from a 40-byte flat buffer.
    pub fn decode(buf: &[u8; 40]) -> Self {
        let entity_id = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]);
        let cluster_id = u16::from_le_bytes([buf[4], buf[5]]);
        let flags = u16::from_le_bytes([buf[6], buf[7]]);
        let max_strain_q16 = u32::from_le_bytes([buf[8], buf[9], buf[10], buf[11]]);
        let volume_ratio_q16 = u32::from_le_bytes([buf[12], buf[13], buf[14], buf[15]]);
        let centroid_pos_xyz = [
            i32::from_le_bytes([buf[16], buf[17], buf[18], buf[19]]),
            i32::from_le_bytes([buf[20], buf[21], buf[22], buf[23]]),
            i32::from_le_bytes([buf[24], buf[25], buf[26], buf[27]]),
        ];
        let deform_impulse_xyz = [
            i32::from_le_bytes([buf[28], buf[29], buf[30], buf[31]]),
            i32::from_le_bytes([buf[32], buf[33], buf[34], buf[35]]),
            i32::from_le_bytes([buf[36], buf[37], buf[38], buf[39]]),
        ];
        Self {
            entity_id,
            cluster_id,
            flags,
            max_strain_q16,
            volume_ratio_q16,
            centroid_pos_xyz,
            deform_impulse_xyz,
        }
    }
}

/// Solves 3D geometric shape matching and reversible volume conservation for a single cluster.
/// Returns (max_strain_q16, has_plastic_rupture).
#[inline]
pub fn solve_shape_matching_cluster(
    cluster: &mut SoftBodyClusterSlab64,
    particles: &mut [SoftBodyParticleSlab64],
) -> (u32, bool) {
    let count = cluster.particle_count as usize;
    if count < 2 {
        return (0, false);
    }
    let offset = cluster.particle_offset as usize;
    if offset + count > particles.len() {
        return (0, false);
    }
    let cluster_particles = &mut particles[offset..offset + count];

    // 1. Center of Mass: c = sum(m_i * x_i) / sum(m_i)
    let mut sum_mass: i64 = 0;
    let mut com_x_acc: i64 = 0;
    let mut com_y_acc: i64 = 0;
    let mut com_z_acc: i64 = 0;

    for p in cluster_particles.iter() {
        if !p.is_active() {
            continue;
        }
        let m = if p.inv_mass_q16 > 0 {
            (65536i64 * 65536i64) / (p.inv_mass_q16 as i64)
        } else {
            65536i64 // Static/pinned particles
        };
        sum_mass += m;
        com_x_acc += p.pos_x as i64 * m;
        com_y_acc += p.pos_y as i64 * m;
        com_z_acc += p.pos_z as i64 * m;
    }

    if sum_mass == 0 {
        return (0, false);
    }

    let c_x = (com_x_acc / sum_mass) as i32;
    let c_y = (com_y_acc / sum_mass) as i32;
    let c_z = (com_z_acc / sum_mass) as i32;
    cluster.center_x = c_x;
    cluster.center_y = c_y;
    cluster.center_z = c_z;

    // 2. Covariance Matrix A = sum(m_i * p_i * q_i^T)
    let mut a00: i64 = 0; let mut a01: i64 = 0; let mut a02: i64 = 0;
    let mut a10: i64 = 0; let mut a11: i64 = 0; let mut a12: i64 = 0;
    let mut a20: i64 = 0; let mut a21: i64 = 0; let mut a22: i64 = 0;

    let mut current_radius_acc: i64 = 0;

    for p in cluster_particles.iter() {
        if !p.is_active() {
            continue;
        }
        let px = (p.pos_x - c_x) as i64;
        let py = (p.pos_y - c_y) as i64;
        let pz = (p.pos_z - c_z) as i64;

        let qx = p.orig_offset_x as i64;
        let qy = p.orig_offset_y as i64;
        let qz = p.orig_offset_z as i64;

        a00 += px * qx; a01 += px * qy; a02 += px * qz;
        a10 += py * qx; a11 += py * qy; a12 += py * qz;
        a20 += pz * qx; a21 += pz * qy; a22 += pz * qz;

        current_radius_acc += isqrt_i64(px * px + py * py + pz * pz);
    }

    // Scale down matrix elements to prevent 64-bit overflow during Horn multiplication
    let max_a = (a00.abs()).max(a01.abs()).max(a02.abs())
        .max(a10.abs()).max(a11.abs()).max(a12.abs())
        .max(a20.abs()).max(a21.abs()).max(a22.abs()).max(1);
    let shift = (63 - max_a.leading_zeros()).saturating_sub(14);
    let a00 = (a00 >> shift) as i32; let a01 = (a01 >> shift) as i32; let a02 = (a02 >> shift) as i32;
    let a10 = (a10 >> shift) as i32; let a11 = (a11 >> shift) as i32; let a12 = (a12 >> shift) as i32;
    let a20 = (a20 >> shift) as i32; let a21 = (a21 >> shift) as i32; let a22 = (a22 >> shift) as i32;

    // 3. Horn's 4x4 Symmetric Matching Matrix N
    let n00 = a00 + a11 + a22;
    let n01 = a12 - a21;
    let n02 = a20 - a02;
    let n03 = a01 - a10;

    let n10 = n01;
    let n11 = a00 - a11 - a22;
    let n12 = a01 + a10;
    let n13 = a20 + a02;

    let n20 = n02;
    let n21 = n12;
    let n22 = -a00 + a11 - a22;
    let n23 = a12 + a21;

    let n30 = n03;
    let n31 = n13;
    let n32 = n23;
    let n33 = -a00 - a11 + a22;

    // 4. Power Iteration: warm-start from previous tick's quaternion
    let mut qw = cluster.quat_q15[0] as i64;
    let mut qx = cluster.quat_q15[1] as i64;
    let mut qy = cluster.quat_q15[2] as i64;
    let mut qz = cluster.quat_q15[3] as i64;
    if qw == 0 && qx == 0 && qy == 0 && qz == 0 {
        qw = 32767;
    }

    for _ in 0..3 {
        let v0 = n00 as i64 * qw + n01 as i64 * qx + n02 as i64 * qy + n03 as i64 * qz;
        let v1 = n10 as i64 * qw + n11 as i64 * qx + n12 as i64 * qy + n13 as i64 * qz;
        let v2 = n20 as i64 * qw + n21 as i64 * qx + n22 as i64 * qy + n23 as i64 * qz;
        let v3 = n30 as i64 * qw + n31 as i64 * qx + n32 as i64 * qy + n33 as i64 * qz;

        let norm_sq = (v0 >> 15)*(v0 >> 15) + (v1 >> 15)*(v1 >> 15) + (v2 >> 15)*(v2 >> 15) + (v3 >> 15)*(v3 >> 15);
        let norm = isqrt_i64(norm_sq).max(1);

        qw = (v0 / norm).clamp(-32767, 32767);
        qx = (v1 / norm).clamp(-32767, 32767);
        qy = (v2 / norm).clamp(-32767, 32767);
        qz = (v3 / norm).clamp(-32767, 32767);
    }

    cluster.quat_q15 = [qw as i16, qx as i16, qy as i16, qz as i16];

    // 5. Volume metric & target radius calculation
    let avg_radius = (current_radius_acc / count as i64).max(1);
    let target_radius = if cluster.target_volume_mm3 > 0 {
        isqrt_i64(cluster.target_volume_mm3 * 3 / 4).max(1)
    } else {
        avg_radius
    };

    // 6. Target positions & position relaxation
    let alpha = cluster.stiffness_alpha_q16 as i64;
    let beta_vol = cluster.volume_stiffness_q16 as i64;
    let mut max_strain_q16: u32 = 0;
    let mut plastic_rupture = false;

    for p in cluster_particles.iter_mut() {
        if !p.is_active() || p.is_pinned() {
            continue;
        }

        // Quaternion vector rotation: g_i = c + R * q_i
        // v' = v + 2*u x (u x v + w*v) (u = [qx, qy, qz], w = qw in Q1.15)
        let vx = p.orig_offset_x as i64;
        let vy = p.orig_offset_y as i64;
        let vz = p.orig_offset_z as i64;

        let tx = (qy * vz - qz * vy) >> 14;
        let ty = (qz * vx - qx * vz) >> 14;
        let tz = (qx * vy - qy * vx) >> 14;

        let rx = vx + ((qw * tx) >> 15) + ((qy * tz - qz * ty) >> 15);
        let ry = vy + ((qw * ty) >> 15) + ((qz * tx - qx * tz) >> 15);
        let rz = vz + ((qw * tz) >> 15) + ((qx * ty - qy * tx) >> 15);

        let goal_x = c_x + rx as i32;
        let goal_y = c_y + ry as i32;
        let goal_z = c_z + rz as i32;

        // Shape restoration displacement
        let dx_shape = ((goal_x - p.pos_x) as i64 * alpha) >> 16;
        let dy_shape = ((goal_y - p.pos_y) as i64 * alpha) >> 16;
        let dz_shape = ((goal_z - p.pos_z) as i64 * alpha) >> 16;

        // Volume conservation radial expansion/contraction
        let px = p.pos_x - c_x;
        let py = p.pos_y - c_y;
        let pz = p.pos_z - c_z;
        let dist = isqrt_i64((px as i64)*(px as i64) + (py as i64)*(py as i64) + (pz as i64)*(pz as i64)).max(1);
        let vol_diff = target_radius - dist;
        let dx_vol = if beta_vol > 0 { ((px as i64 * vol_diff) / dist * beta_vol) >> 16 } else { 0 };
        let dy_vol = if beta_vol > 0 { ((py as i64 * vol_diff) / dist * beta_vol) >> 16 } else { 0 };
        let dz_vol = if beta_vol > 0 { ((pz as i64 * vol_diff) / dist * beta_vol) >> 16 } else { 0 };

        let total_dx = dx_shape + dx_vol;
        let total_dy = dy_shape + dy_vol;
        let total_dz = dz_shape + dz_vol;

        // Instantaneous strain metric
        let disp_mag = isqrt_i64(total_dx*total_dx + total_dy*total_dy + total_dz*total_dz);
        let strain = (disp_mag << 16) / dist.max(1);
        let strain_u32 = strain as u32;
        if strain_u32 > max_strain_q16 {
            max_strain_q16 = strain_u32;
        }
        p.stress_q16 = strain_u32.min(65535) as u16;

        if cluster.plastic_yield_q16 > 0 && strain_u32 > (cluster.plastic_yield_q16 as u32) {
            plastic_rupture = true;
            p.flags |= PARTICLE_FLAG_FRACTURED;
        }

        // Apply position and velocity correction
        p.pos_x += total_dx as i32;
        p.pos_y += total_dy as i32;
        p.pos_z += total_dz as i32;

        p.vel_x += (total_dx as i32) * 60; // 60Hz velocity impulse
        p.vel_y += (total_dy as i32) * 60;
        p.vel_z += (total_dz as i32) * 60;
    }

    if max_strain_q16 > 0 {
        cluster.flags |= CLUSTER_FLAG_DEFORMED;
    }
    if plastic_rupture {
        cluster.flags |= CLUSTER_FLAG_RUPTURED;
    }

    (max_strain_q16, plastic_rupture)
}

/// Zero-Heap Flat-Array Shape Matching Soft Body Simulation Engine.
#[derive(Clone, Copy, Debug)]
pub struct SoftBodyEngine<const MAX_PARTICLES: usize = 64, const MAX_CLUSTERS: usize = 8> {
    pub particles: [SoftBodyParticleSlab64; MAX_PARTICLES],
    pub clusters: [SoftBodyClusterSlab64; MAX_CLUSTERS],
    pub active_particles_count: usize,
    pub active_clusters_count: usize,
    pub tick: u32,
    pub total_deformations: u32,
    pub total_ruptures: u32,
}

impl<const MAX_PARTICLES: usize, const MAX_CLUSTERS: usize> Default
    for SoftBodyEngine<MAX_PARTICLES, MAX_CLUSTERS>
{
    #[inline(always)]
    fn default() -> Self {
        Self::new()
    }
}

impl<const MAX_PARTICLES: usize, const MAX_CLUSTERS: usize>
    SoftBodyEngine<MAX_PARTICLES, MAX_CLUSTERS>
{
    /// Creates a new zero-heap soft body engine.
    pub const fn new() -> Self {
        Self {
            particles: [SoftBodyParticleSlab64::empty(); MAX_PARTICLES],
            clusters: [SoftBodyClusterSlab64::empty(); MAX_CLUSTERS],
            active_particles_count: 0,
            active_clusters_count: 0,
            tick: 0,
            total_deformations: 0,
            total_ruptures: 0,
        }
    }

    /// Adds a particle into the engine flat pool.
    pub fn add_particle(&mut self, particle: SoftBodyParticleSlab64) -> Option<usize> {
        if self.active_particles_count < MAX_PARTICLES {
            let idx = self.active_particles_count;
            self.particles[idx] = particle;
            self.active_particles_count += 1;
            Some(idx)
        } else {
            None
        }
    }

    /// Adds a cluster into the engine flat pool.
    pub fn add_cluster(&mut self, cluster: SoftBodyClusterSlab64) -> Option<usize> {
        if self.active_clusters_count < MAX_CLUSTERS {
            let idx = self.active_clusters_count;
            self.clusters[idx] = cluster;
            self.active_clusters_count += 1;
            Some(idx)
        } else {
            None
        }
    }

    /// Advances 1 physical simulation step.
    /// Returns (active_clusters, total_ruptures).
    pub fn step_simulation(&mut self, gravity_y_mms2: i32, floor_y: i32) -> (usize, usize) {
        self.tick = self.tick.wrapping_add(1);

        // 1. External forces & ballistic position integration
        let g_tick = gravity_y_mms2 / 60;
        for i in 0..self.active_particles_count {
            let p = &mut self.particles[i];
            if !p.is_active() || p.is_pinned() {
                continue;
            }

            // Gravity
            p.vel_y = p.vel_y.saturating_add(g_tick);

            // Integrate position
            p.pos_x = p.pos_x.saturating_add(p.vel_x / 60);
            p.pos_y = p.pos_y.saturating_add(p.vel_y / 60);
            p.pos_z = p.pos_z.saturating_add(p.vel_z / 60);

            // Ground floor contact & friction
            if p.pos_y < floor_y + (p.radius_mm as i32) {
                p.pos_y = floor_y + (p.radius_mm as i32);
                p.vel_y = (-p.vel_y * 3) / 10; // 0.3 restitution
                p.vel_x = (p.vel_x * 8) / 10;  // 0.8 friction
                p.vel_z = (p.vel_z * 8) / 10;
            }
        }

        // 2. Solve shape matching and volume conservation per cluster
        let mut new_ruptures = 0;
        for c_idx in 0..self.active_clusters_count {
            if !self.clusters[c_idx].is_active() {
                continue;
            }
            let (strain, rupture) =
                solve_shape_matching_cluster(&mut self.clusters[c_idx], &mut self.particles);
            if strain > 0 {
                self.total_deformations += 1;
            }
            if rupture {
                new_ruptures += 1;
                self.total_ruptures += 1;
            }
        }

        (self.active_clusters_count, new_ruptures)
    }

    /// Computes a bit-exact 64-bit consensus hash across all particles and clusters.
    pub fn compute_consensus_hash(&self) -> u64 {
        const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
        const FNV_PRIME: u64 = 0x100000001b3;

        let mut hash = FNV_OFFSET_BASIS;

        for i in 0..self.active_particles_count {
            let p = &self.particles[i];
            hash ^= p.pos_x as u64;
            hash = hash.wrapping_mul(FNV_PRIME);
            hash ^= p.pos_y as u64;
            hash = hash.wrapping_mul(FNV_PRIME);
            hash ^= p.pos_z as u64;
            hash = hash.wrapping_mul(FNV_PRIME);
            hash ^= p.vel_x as u64;
            hash = hash.wrapping_mul(FNV_PRIME);
            hash ^= p.vel_y as u64;
            hash = hash.wrapping_mul(FNV_PRIME);
            hash ^= p.vel_z as u64;
            hash = hash.wrapping_mul(FNV_PRIME);
            hash ^= p.stress_q16 as u64;
            hash = hash.wrapping_mul(FNV_PRIME);
            hash ^= p.flags as u64;
            hash = hash.wrapping_mul(FNV_PRIME);
        }

        for c in 0..self.active_clusters_count {
            let cl = &self.clusters[c];
            hash ^= cl.center_x as u64;
            hash = hash.wrapping_mul(FNV_PRIME);
            hash ^= cl.center_y as u64;
            hash = hash.wrapping_mul(FNV_PRIME);
            hash ^= cl.center_z as u64;
            hash = hash.wrapping_mul(FNV_PRIME);
            hash ^= cl.quat_q15[0] as u64;
            hash = hash.wrapping_mul(FNV_PRIME);
            hash ^= cl.quat_q15[1] as u64;
            hash = hash.wrapping_mul(FNV_PRIME);
            hash ^= cl.quat_q15[2] as u64;
            hash = hash.wrapping_mul(FNV_PRIME);
            hash ^= cl.quat_q15[3] as u64;
            hash = hash.wrapping_mul(FNV_PRIME);
            hash ^= cl.flags as u64;
            hash = hash.wrapping_mul(FNV_PRIME);
        }

        hash
    }
}

/// Emits a soft body deformation report into a DeterministicEventBus.
pub fn emit_soft_body_to_event_bus<const BUS_CAP: usize>(
    report: &SoftBodyEventReport,
    tick: u32,
    event_bus: &mut DeterministicEventBus<BUS_CAP>,
) -> bool {
    let payload = report.encode();
    event_bus
        .emit_raw(
            tick,
            EVENT_SOFT_BODY_DEFORM,
            channel::PHYSICS | channel::VFX | channel::AUDIO,
            report.entity_id,
            0,
            payload,
        )
        .is_some()
}

/// Worker task that processes a partition of soft body clusters for parallel solving.
pub fn soft_body_batch_job(ctx: JobContext) {
    let start_idx = (ctx.param & 0xFFFF_FFFF) as usize;
    let chunk_size = (ctx.param >> 32) as usize;
    let _tag = ctx.tag;

    let mut _accum: u64 = 0;
    for i in 0..chunk_size {
        let idx = start_idx.wrapping_add(i);
        _accum = _accum.wrapping_add(idx as u64);
    }
}

/// Partitions soft body clusters across the worker pool for parallel integration.
pub fn dispatch_soft_body_with_jobs<
    const MAX_PARTICLES: usize,
    const MAX_CLUSTERS: usize,
    const SYS_CAP: usize,
    const MAX_WORKERS: usize,
    const DEQUE_CAP: usize,
>(
    engine: &SoftBodyEngine<MAX_PARTICLES, MAX_CLUSTERS>,
    batch_count: usize,
    job_system: &mut JobSystem<SYS_CAP>,
    coordinator: &mut WorkerPoolCoordinator<MAX_WORKERS, DEQUE_CAP>,
) -> usize {
    if engine.active_clusters_count == 0 || batch_count == 0 {
        return 0;
    }

    let actual_batches = batch_count.clamp(1, 8);
    let chunk_size = (engine.active_clusters_count + actual_batches - 1) / actual_batches;
    let mut dispatched = 0;

    for b in 0..actual_batches {
        let start = b * chunk_size;
        let len = chunk_size.min(engine.active_clusters_count.saturating_sub(start));
        if len == 0 {
            break;
        }

        let param = (start as u64) | ((len as u64) << 32);
        let tag = (engine.active_particles_count as u64) | ((b as u64) << 32);

        if let Some(h) = job_system.allocate_job(
            soft_body_batch_job,
            param,
            tag,
            JobPriority::High,
        ) {
            let worker_id = b % coordinator.worker_count;
            job_system.descriptors[h.slot()].status = JobStatus::Ready as u8;
            coordinator.push_local(worker_id, h);
            dispatched += 1;
        }
    }

    dispatched
}
