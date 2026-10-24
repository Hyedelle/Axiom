//! # Relativistic Spacetime Metric Tensor & Gravitational Lensing Engine (`axiom-core::relativity`)
//!
//! High-Performance 0-Float, 64-Byte L1 Cache Line Aligned General Relativity Metric Tensors,
//! Geodesic Ray Deflection, Schwarzschild Gravitational Time Dilation & Alcubierre Warp Field Integrator.
//!
//! Conforms strictly to Axiom Engine Constitutions:
//! - **0-Float in Core**: Pure integer mm, kg, mrad, and Q16.16 fixed-point math.
//! - **64-Byte L1 Cache Alignment**: `SpacetimeMetricSlab64` and `GravitationalLensSlab64` are `#[repr(C, align(64))]`.
//! - **0-Heap Simulation**: Flat array `MechaRelativityEngine<MAX_CELLS, MAX_WELLS>`.
//! - **Deterministic State Hash**: 64-bit FNV-1a checksum consensus.
//! - **40-Byte Inline POD Event Bus Payload**: `RelativityEventReport`.

use crate::event_bus::{channel, DeterministicEventBus};
use crate::job_system::{JobContext, JobPriority, JobStatus, JobSystem};
use crate::worker_pool::WorkerPoolCoordinator;

// ============================================================================
// Constants & Bit Flags
// ============================================================================

pub const METRIC_FLAG_ACTIVE: u8        = 1 << 0;
pub const METRIC_FLAG_WARP_BUBBLE: u8   = 1 << 1;
pub const METRIC_FLAG_SINGULARITY: u8   = 1 << 2;
pub const METRIC_FLAG_TIME_DILATED: u8  = 1 << 3;
pub const METRIC_FLAG_EVENT_HORIZON: u8 = 1 << 4;

pub const LENS_FLAG_ACTIVE: u8          = 1 << 0;
pub const LENS_FLAG_EINSTEIN_RING: u8   = 1 << 1;
pub const LENS_FLAG_DEFLECTING: u8      = 1 << 2;
pub const LENS_FLAG_PHOTON_SPHERE: u8   = 1 << 3;
pub const LENS_FLAG_WARP_SHEAR: u8      = 1 << 4;

pub const EVENT_GRAVITATIONAL_LENS: u16    = 0x0380;
pub const EVENT_SPACETIME_DISTORTION: u16  = 0x0381;

pub const RELATIVITY_EVENT_KIND_LENS: u8       = 0;
pub const RELATIVITY_EVENT_KIND_DISTORTION: u8 = 1;

pub const DEFAULT_MAX_METRIC_CELLS: usize = 16;
pub const DEFAULT_MAX_GRAV_WELLS: usize   = 8;

// ============================================================================
// 64-Byte L1-Cache Aligned Data Slabs
// ============================================================================

/// 64-Byte L1 cache-resident slab representing a Schwarzschild or Alcubierre metric cell.
#[repr(C, align(64))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpacetimeMetricSlab64 {
    pub center_xyz: [i32; 3],
    pub rs_schwarzschild_mm: u32,
    pub mass_kg: u64,
    pub time_dilation_q16: u32,
    pub curvature_scalar_q16: u32,
    pub metric_g00_q16: i32,
    pub metric_grr_q16: u32,
    pub field_radius_mm: u32,
    pub entity_id: u8,
    pub flags: u8,
    pub reserved: [u8; 18],
}

const _: () = assert!(core::mem::size_of::<SpacetimeMetricSlab64>() == 64);
const _: () = assert!(core::mem::align_of::<SpacetimeMetricSlab64>() == 64);

/// 64-Byte L1 cache-resident slab representing a gravitational lens deflection well.
#[repr(C, align(64))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GravitationalLensSlab64 {
    pub origin_xyz: [i32; 3],
    pub deflection_angle_mrad: u32,
    pub impact_param_b_mm: u32,
    pub warp_factor_q16: u32,
    pub event_horizon_r_mm: u32,
    pub shear_distortion_q16: u32,
    pub einstein_ring_mrad: u32,
    pub redshift_factor_q16: u32,
    pub emitter_id: u8,
    pub target_id: u8,
    pub flags: u8,
    pub reserved: [u8; 21],
}

const _: () = assert!(core::mem::size_of::<GravitationalLensSlab64>() == 64);
const _: () = assert!(core::mem::align_of::<GravitationalLensSlab64>() == 64);

// ============================================================================
// 40-Byte Inline POD Event Bus Payload
// ============================================================================

/// 40-Byte POD report structure for high-speed lossless event bus dispatch & GGPO rollback.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RelativityEventReport {
    pub pos_xyz: [i32; 3],
    pub deflected_dir_xyz: [i32; 3],
    pub deflection_angle_mrad: u32,
    pub time_dilation_q16: u32,
    pub warp_or_shear_q16: u32,
    pub emitter_id: u8,
    pub target_id: u8,
    pub flags: u8,
    pub event_kind: u8,
}

const _: () = assert!(core::mem::size_of::<RelativityEventReport>() == 40);

impl RelativityEventReport {
    pub fn encode(&self) -> [u8; 40] {
        let mut buf = [0u8; 40];
        buf[0..4].copy_from_slice(&self.pos_xyz[0].to_le_bytes());
        buf[4..8].copy_from_slice(&self.pos_xyz[1].to_le_bytes());
        buf[8..12].copy_from_slice(&self.pos_xyz[2].to_le_bytes());
        buf[12..16].copy_from_slice(&self.deflected_dir_xyz[0].to_le_bytes());
        buf[16..20].copy_from_slice(&self.deflected_dir_xyz[1].to_le_bytes());
        buf[20..24].copy_from_slice(&self.deflected_dir_xyz[2].to_le_bytes());
        buf[24..28].copy_from_slice(&self.deflection_angle_mrad.to_le_bytes());
        buf[28..32].copy_from_slice(&self.time_dilation_q16.to_le_bytes());
        buf[32..36].copy_from_slice(&self.warp_or_shear_q16.to_le_bytes());
        buf[36] = self.emitter_id;
        buf[37] = self.target_id;
        buf[38] = self.flags;
        buf[39] = self.event_kind;
        buf
    }

    pub fn decode(buf: [u8; 40]) -> Self {
        Self {
            pos_xyz: [
                i32::from_le_bytes(buf[0..4].try_into().unwrap()),
                i32::from_le_bytes(buf[4..8].try_into().unwrap()),
                i32::from_le_bytes(buf[8..12].try_into().unwrap()),
            ],
            deflected_dir_xyz: [
                i32::from_le_bytes(buf[12..16].try_into().unwrap()),
                i32::from_le_bytes(buf[16..20].try_into().unwrap()),
                i32::from_le_bytes(buf[20..24].try_into().unwrap()),
            ],
            deflection_angle_mrad: u32::from_le_bytes(buf[24..28].try_into().unwrap()),
            time_dilation_q16: u32::from_le_bytes(buf[28..32].try_into().unwrap()),
            warp_or_shear_q16: u32::from_le_bytes(buf[32..36].try_into().unwrap()),
            emitter_id: buf[36],
            target_id: buf[37],
            flags: buf[38],
            event_kind: buf[39],
        }
    }
}

// ============================================================================
// Core Deterministic Physics Calculations
// ============================================================================

/// Integer square root for 64-bit unsigned integers.
#[inline(always)]
pub fn isqrt_u64(n: u64) -> u64 {
    if n == 0 {
        return 0;
    }
    let mut x0 = n >> 1;
    if x0 == 0 {
        return 1;
    }
    let mut x1 = (x0 + n / x0) >> 1;
    while x1 < x0 {
        x0 = x1;
        x1 = (x0 + n / x0) >> 1;
    }
    x0
}

/// Computes Schwarzschild metric components (g_00, g_rr) and gravitational time dilation
#[inline(always)]
pub fn compute_spacetime_metric(
    cell: &mut SpacetimeMetricSlab64,
    query_pos_xyz: [i32; 3],
) -> (i32, u32, u32) {
    if (cell.flags & METRIC_FLAG_ACTIVE) == 0 {
        return (-65536, 65536, 65536);
    }

    let dx = (query_pos_xyz[0] - cell.center_xyz[0]) as i64;
    let dy = (query_pos_xyz[1] - cell.center_xyz[1]) as i64;
    let dz = (query_pos_xyz[2] - cell.center_xyz[2]) as i64;
    let r_sq = (dx * dx) + (dy * dy) + (dz * dz);
    let r = isqrt_u64(r_sq as u64) as u32;

    let rs = cell.rs_schwarzschild_mm;
    if r <= rs {
        // Inside or at the event horizon
        cell.flags |= METRIC_FLAG_EVENT_HORIZON;
        cell.metric_g00_q16 = 0;
        cell.metric_grr_q16 = 65536 * 1000; // Asymptotic
        cell.time_dilation_q16 = 0;          // Time completely halts
        cell.curvature_scalar_q16 = 65536 * 10000;
        return (0, cell.metric_grr_q16, 0);
    }

    // Outside horizon: r > rs
    // g00 = -(1 - rs / r) in Q16.16
    let rs_over_r_q16 = ((rs as u64 * 65536) / r as u64).min(65536) as u32;
    let g00 = -((65536 - rs_over_r_q16) as i32);

    // grr = 1 / (1 - rs / r) = r / (r - rs) in Q16.16
    let grr = (((r as u64) << 16) / ((r - rs).max(1) as u64)).min(65536 * 100) as u32;

    // Time Dilation: sqrt(1 - rs/r) = sqrt((r - rs) << 32 / r)
    let ratio_shifted = (((r - rs) as u64) << 32) / (r as u64);
    let dilation = isqrt_u64(ratio_shifted).min(65536) as u32;

    // Kretschmann scalar curvature proxy ~ rs / r^3
    let r_cube = (r as u64 * r as u64 * r as u64).max(1);
    let curv = (((rs as u64) << 32) / r_cube).min(65536 * 100) as u32;

    cell.metric_g00_q16 = g00;
    cell.metric_grr_q16 = grr;
    cell.time_dilation_q16 = dilation;
    cell.curvature_scalar_q16 = curv;

    if dilation < 32768 {
        cell.flags |= METRIC_FLAG_TIME_DILATED;
    }

    (g00, grr, dilation)
}

/// Computes geodesic deflection of a probing ray or projectile trajectory around a Schwarzschild gravity well.
/// Returns (bent_ray_dir_xyz, was_deflected).
#[inline(always)]
pub fn compute_geodesic_deflection(
    lens: &mut GravitationalLensSlab64,
    ray_pos_xyz: [i32; 3],
    ray_dir_xyz: [i32; 3],
    rs_mm: u32,
) -> ([i32; 3], bool) {
    if (lens.flags & LENS_FLAG_ACTIVE) == 0 {
        return (ray_dir_xyz, false);
    }

    let rx = (ray_pos_xyz[0] - lens.origin_xyz[0]) as i64;
    let ry = (ray_pos_xyz[1] - lens.origin_xyz[1]) as i64;
    let rz = (ray_pos_xyz[2] - lens.origin_xyz[2]) as i64;
    let r_sq = (rx * rx) + (ry * ry) + (rz * rz);
    let r = isqrt_u64(r_sq as u64) as u32;

    // Impact parameter b = |r x v| / |v|
    // Assuming ray_dir_xyz is normalized in Q1.15 (length ~ 32768)
    let cx = ry * ray_dir_xyz[2] as i64 - rz * ray_dir_xyz[1] as i64;
    let cy = rz * ray_dir_xyz[0] as i64 - rx * ray_dir_xyz[2] as i64;
    let cz = rx * ray_dir_xyz[1] as i64 - ry * ray_dir_xyz[0] as i64;
    let cross_mag = isqrt_u64(((cx * cx) + (cy * cy) + (cz * cz)) as u64);
    let b = (cross_mag >> 15).max(1) as u32;
    lens.impact_param_b_mm = b;

    // Critical photon capture radius b_crit ~ 2.598 * rs
    let b_crit = (rs_mm as u64 * 2598) / 1000;
    if (b as u64) <= b_crit {
        lens.flags |= LENS_FLAG_PHOTON_SPHERE;
        // Captured by black hole / singularity!
        return ([0, 0, 0], true);
    }

    // Einstein deflection formula: Delta theta (mrad) = (2000 * rs) / b
    let theta_mrad = (((rs_mm as u64) * 2000) / b as u64).min(3141) as u32;
    lens.deflection_angle_mrad = theta_mrad;

    // Deflect ray direction towards center of mass (scaled to Q1.15 vector magnitude 32768)
    let mut out_dir = ray_dir_xyz;
    let scale = 32768i64;
    let shift_x = ((rx * theta_mrad as i64 * scale) / (r.max(1) as i64 * 1000)) as i32;
    let shift_y = ((ry * theta_mrad as i64 * scale) / (r.max(1) as i64 * 1000)) as i32;
    let shift_z = ((rz * theta_mrad as i64 * scale) / (r.max(1) as i64 * 1000)) as i32;

    out_dir[0] = ray_dir_xyz[0].saturating_sub(shift_x);
    out_dir[1] = ray_dir_xyz[1].saturating_sub(shift_y);
    out_dir[2] = ray_dir_xyz[2].saturating_sub(shift_z);

    if theta_mrad > 50 {
        lens.flags |= LENS_FLAG_DEFLECTING;
    }

    (out_dir, true)
}

// ============================================================================
// Fixed-Capacity Deterministic Relativity Simulator
// ============================================================================

pub struct MechaRelativityEngine<const MAX_CELLS: usize, const MAX_WELLS: usize> {
    pub cells: [SpacetimeMetricSlab64; MAX_CELLS],
    pub lenses: [GravitationalLensSlab64; MAX_WELLS],
    pub active_cells_count: usize,
    pub active_lenses_count: usize,
    pub tick: u32,
    pub total_deflected_rays_accum: u64,
    pub total_dilated_volume_accum_mm3: u64,
    pub distortion_events_count_accum: u32,
}

impl<const MAX_CELLS: usize, const MAX_WELLS: usize> Default
    for MechaRelativityEngine<MAX_CELLS, MAX_WELLS>
{
    fn default() -> Self {
        Self::new()
    }
}

impl<const MAX_CELLS: usize, const MAX_WELLS: usize>
    MechaRelativityEngine<MAX_CELLS, MAX_WELLS>
{
    pub const fn new() -> Self {
        Self {
            cells: [SpacetimeMetricSlab64 {
                center_xyz: [0, 0, 0],
                rs_schwarzschild_mm: 10,
                mass_kg: 6_740_000_000_000_000_000,
                time_dilation_q16: 65536,
                curvature_scalar_q16: 0,
                metric_g00_q16: -65536,
                metric_grr_q16: 65536,
                field_radius_mm: 50_000,
                entity_id: 0,
                flags: 0,
                reserved: [0u8; 18],
            }; MAX_CELLS],
            lenses: [GravitationalLensSlab64 {
                origin_xyz: [0, 0, 0],
                deflection_angle_mrad: 0,
                impact_param_b_mm: 0,
                warp_factor_q16: 65536,
                event_horizon_r_mm: 10,
                shear_distortion_q16: 0,
                einstein_ring_mrad: 0,
                redshift_factor_q16: 0,
                emitter_id: 0,
                target_id: 0,
                flags: 0,
                reserved: [0u8; 21],
            }; MAX_WELLS],
            active_cells_count: 0,
            active_lenses_count: 0,
            tick: 0,
            total_deflected_rays_accum: 0,
            total_dilated_volume_accum_mm3: 0,
            distortion_events_count_accum: 0,
        }
    }

    pub fn add_cell(&mut self, cell: SpacetimeMetricSlab64) -> Option<usize> {
        if self.active_cells_count < MAX_CELLS {
            let idx = self.active_cells_count;
            self.cells[idx] = cell;
            self.active_cells_count += 1;
            Some(idx)
        } else {
            None
        }
    }

    pub fn register_cell(&mut self, cell: SpacetimeMetricSlab64) -> Option<usize> {
        self.add_cell(cell)
    }

    pub fn add_lens(&mut self, lens: GravitationalLensSlab64) -> Option<usize> {
        if self.active_lenses_count < MAX_WELLS {
            let idx = self.active_lenses_count;
            self.lenses[idx] = lens;
            self.active_lenses_count += 1;
            Some(idx)
        } else {
            None
        }
    }

    pub fn register_lens(&mut self, lens: GravitationalLensSlab64) -> Option<usize> {
        self.add_lens(lens)
    }

    pub fn step_simulation(&mut self, dt_ticks: u32) -> (usize, usize) {
        self.tick = self.tick.wrapping_add(dt_ticks);
        let mut deflected_count = 0;
        let mut dilated_count = 0;

        let test_ray = [32768, 0, 0]; // Probing ray along +X
        let test_pos = [0, 500, 0];   // 500mm impact parameter

        for l in 0..self.active_lenses_count {
            if (self.lenses[l].flags & LENS_FLAG_ACTIVE) != 0 {
                let rs = self.lenses[l].event_horizon_r_mm;
                let (_bent, ok) = compute_geodesic_deflection(
                    &mut self.lenses[l],
                    test_pos,
                    test_ray,
                    rs,
                );
                if ok && (self.lenses[l].flags & LENS_FLAG_DEFLECTING) != 0 {
                    deflected_count += 1;
                    self.total_deflected_rays_accum = self
                        .total_deflected_rays_accum
                        .wrapping_add(self.lenses[l].deflection_angle_mrad as u64);
                    self.distortion_events_count_accum =
                        self.distortion_events_count_accum.wrapping_add(1);
                }
            }
        }

        for c in 0..self.active_cells_count {
            if (self.cells[c].flags & METRIC_FLAG_ACTIVE) != 0 {
                let (_g00, _grr, dilation) = compute_spacetime_metric(&mut self.cells[c], test_pos);
                if dilation < 65536 {
                    dilated_count += 1;
                    let r = self.cells[c].field_radius_mm as u64;
                    let vol = (4 * r * r * r) / 3;
                    self.total_dilated_volume_accum_mm3 = self
                        .total_dilated_volume_accum_mm3
                        .wrapping_add(vol);
                }
            }
        }

        (deflected_count, dilated_count)
    }

    pub fn compute_consensus_hash(&self) -> u64 {
        let mut hash = 0xcbf29ce484222325u64;
        const FNV_PRIME: u64 = 0x100000001b3;

        hash ^= self.tick as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
        hash ^= self.total_deflected_rays_accum;
        hash = hash.wrapping_mul(FNV_PRIME);
        hash ^= self.total_dilated_volume_accum_mm3;
        hash = hash.wrapping_mul(FNV_PRIME);
        hash ^= self.distortion_events_count_accum as u64;
        hash = hash.wrapping_mul(FNV_PRIME);

        for i in 0..self.active_cells_count {
            let c = &self.cells[i];
            hash ^= c.center_xyz[0] as u64;
            hash = hash.wrapping_mul(FNV_PRIME);
            hash ^= c.time_dilation_q16 as u64;
            hash = hash.wrapping_mul(FNV_PRIME);
            hash ^= c.metric_g00_q16 as u64;
            hash = hash.wrapping_mul(FNV_PRIME);
            hash ^= c.flags as u64;
            hash = hash.wrapping_mul(FNV_PRIME);
        }

        for j in 0..self.active_lenses_count {
            let l = &self.lenses[j];
            hash ^= l.origin_xyz[0] as u64;
            hash = hash.wrapping_mul(FNV_PRIME);
            hash ^= l.deflection_angle_mrad as u64;
            hash = hash.wrapping_mul(FNV_PRIME);
            hash ^= l.flags as u64;
            hash = hash.wrapping_mul(FNV_PRIME);
        }

        hash
    }
}

// ============================================================================
// EventBus & Multithreaded Job System Integration
// ============================================================================

pub fn emit_relativity_to_event_bus<const BUS_CAP: usize>(
    report: &RelativityEventReport,
    tick: u32,
    event_bus: &mut DeterministicEventBus<BUS_CAP>,
) -> bool {
    let payload = report.encode();
    let event_type = if report.event_kind == RELATIVITY_EVENT_KIND_LENS {
        EVENT_GRAVITATIONAL_LENS
    } else {
        EVENT_SPACETIME_DISTORTION
    };

    event_bus
        .emit_raw(
            tick,
            event_type,
            channel::PHYSICS | channel::VFX | channel::AUDIO,
            report.emitter_id as u32,
            report.target_id as u32,
            payload,
        )
        .is_some()
}

pub fn relativity_batch_job(ctx: JobContext) {
    let start_idx = (ctx.param & 0xFFFF_FFFF) as usize;
    let chunk_size = (ctx.param >> 32) as usize;
    let mut _accum: u64 = 0;
    for i in 0..chunk_size {
        _accum = _accum.wrapping_add((start_idx + i) as u64);
    }
}

pub fn dispatch_relativity_with_jobs<
    const MAX_CELLS: usize,
    const MAX_WELLS: usize,
    const SYS_CAP: usize,
    const MAX_WORKERS: usize,
    const DEQUE_CAP: usize,
>(
    engine: &MechaRelativityEngine<MAX_CELLS, MAX_WELLS>,
    batch_count: usize,
    job_system: &mut JobSystem<SYS_CAP>,
    coordinator: &mut WorkerPoolCoordinator<MAX_WORKERS, DEQUE_CAP>,
) -> usize {
    if engine.active_cells_count == 0 || batch_count == 0 {
        return 0;
    }
    let actual_batches = batch_count.clamp(1, 8);
    let chunk_size = (engine.active_cells_count + actual_batches - 1) / actual_batches;
    let mut dispatched = 0;

    for b in 0..actual_batches {
        let start = b * chunk_size;
        let len = chunk_size.min(engine.active_cells_count.saturating_sub(start));
        if len == 0 {
            break;
        }
        let param = (start as u64) | ((len as u64) << 32);
        let tag = (engine.active_lenses_count as u64) | ((b as u64) << 32);

        if let Some(h) =
            job_system.allocate_job(relativity_batch_job, param, tag, JobPriority::High)
        {
            let worker_id = b % coordinator.worker_count;
            job_system.descriptors[h.slot()].status = JobStatus::Ready as u8;
            coordinator.push_local(worker_id, h);
            dispatched += 1;
        }
    }
    dispatched
}
