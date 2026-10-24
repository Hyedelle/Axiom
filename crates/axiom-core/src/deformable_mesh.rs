//! # Procedural Deformable Terrain Mesh & Impact Crater Tessellation (`axiom-core::deformable_mesh`)
//!
//! T-207: High-Performance 0-Float, 64-Byte L1 Cache Line Aligned Procedural
//! Deformable Terrain Mesh and Impact Crater Tessellation Physics Engine.
//!
//! ## Architectural Guarantees:
//! - **0-Float in Core**: All positions and displacements in integer millimeters (`mm: i32`).
//!   Normal vectors, stress, and volume ratios in $Q16.16$ fixed-point arithmetic (`65536 = 1.0`).
//! - **64B L1 Cache Line Alignment**: Both [`TerrainVertexSlab64`] and [`CraterSlab64`]
//!   are `#[repr(C, align(64))]` with exact 64-byte size and zero implicit compiler padding.
//! - **Volume Conservation**: Exact analytical balance between excavation bowl volume
//!   and upheaval rim lip height governed by Melosh impact mechanics.
//! - **0-Heap Flat Array Topology**: Fixed-capacity compile-time generic arrays ([`DeformableMeshGrid<GRID_DIM, MAX_CRATERS>`]).

use core::mem::{align_of, size_of};
use crate::ragdoll::isqrt_i64;

/// Event type discriminant for terrain impact crater deformation.
pub const EVENT_TERRAIN_CRATER: u16 = 0x0250;

/// Vertex operational flag bitmasks.
pub const VERTEX_FLAG_MODIFIED: u8  = 1 << 0;
pub const VERTEX_FLAG_FRACTURED: u8 = 1 << 1;
pub const VERTEX_FLAG_PINNED: u8    = 1 << 2;
pub const VERTEX_FLAG_MOLTEN: u8    = 1 << 3;

/// Crater state flag bitmasks.
pub const CRATER_FLAG_ACTIVE: u8         = 1 << 0;
pub const CRATER_FLAG_PERMANENT: u8      = 1 << 1;
pub const CRATER_FLAG_SHARDS_SPAWNED: u8 = 1 << 2;

/// Terrain material classification.
pub const TERRAIN_MAT_DIRT: u8   = 0;
pub const TERRAIN_MAT_STONE: u8  = 1;
pub const TERRAIN_MAT_METAL: u8  = 2;
pub const TERRAIN_MAT_MOLTEN: u8 = 3;
pub const TERRAIN_MAT_SAND: u8   = 4;

/// 1.0 in Q16.16 fixed-point arithmetic.
pub const Q16_ONE: i32 = 65536;

/// Exact 64-Byte L1 Cache Line Aligned Terrain Vertex Slab.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(C, align(64))]
pub struct TerrainVertexSlab64 {
    /// 4B: Base world X position in integer millimeters
    pub base_pos_x: i32,
    /// 4B: Base world Y position in integer millimeters (undeformed elevation)
    pub base_pos_y: i32,
    /// 4B: Base world Z position in integer millimeters
    pub base_pos_z: i32,
    /// 4B: Accumulated dynamic displacement X in integer mm
    pub disp_x: i32,
    /// 4B: Accumulated dynamic displacement Y in integer mm (-bowl, +rim)
    pub disp_y: i32,
    /// 4B: Accumulated dynamic displacement Z in integer mm
    pub disp_z: i32,
    /// 4B: Surface normal X component in Q16.16 (-65536..=65536)
    pub normal_x_q16: i32,
    /// 4B: Surface normal Y component in Q16.16 (upward default = 65536)
    pub normal_y_q16: i32,
    /// 4B: Surface normal Z component in Q16.16
    pub normal_z_q16: i32,
    /// 1B: Terrain material classification (`TERRAIN_MAT_*`)
    pub material: u8,
    /// 1B: Vertex operational state flags (`VERTEX_FLAG_*`)
    pub flags: u8,
    /// 2B: Accumulated mechanical plastic strain / yield stress in Q16.16
    pub stress_q16: u16,
    /// 2B: Total count of crater impacts that influenced this vertex
    pub crater_count: u16,
    /// 2B: Local surface thermal signature in degrees Celsius
    pub temp_celsius: i16,
    /// 4B: Simulation tick when this vertex was last deformed
    pub last_tick: u32,
    /// 16B: Explicit alignment padding ensuring exact 64-byte size
    pub _pad: [u8; 16],
}

const _: () = assert!(size_of::<TerrainVertexSlab64>() == 64);
const _: () = assert!(align_of::<TerrainVertexSlab64>() == 64);

impl Default for TerrainVertexSlab64 {
    #[inline(always)]
    fn default() -> Self {
        Self::empty()
    }
}

impl TerrainVertexSlab64 {
    /// Creates a flat undeformed terrain vertex slab at the given base coordinate.
    #[inline(always)]
    pub const fn new(x_mm: i32, y_mm: i32, z_mm: i32, material: u8) -> Self {
        Self {
            base_pos_x: x_mm,
            base_pos_y: y_mm,
            base_pos_z: z_mm,
            disp_x: 0,
            disp_y: 0,
            disp_z: 0,
            normal_x_q16: 0,
            normal_y_q16: Q16_ONE,
            normal_z_q16: 0,
            material,
            flags: 0,
            stress_q16: 0,
            crater_count: 0,
            temp_celsius: 20, // Ambient 20°C
            last_tick: 0,
            _pad: [0; 16],
        }
    }

    /// Creates a blank inactive vertex slab.
    #[inline(always)]
    pub const fn empty() -> Self {
        Self::new(0, 0, 0, TERRAIN_MAT_DIRT)
    }

    /// Returns the deformed world position in millimeters.
    #[inline(always)]
    pub const fn world_pos(&self) -> (i32, i32, i32) {
        (
            self.base_pos_x + self.disp_x,
            self.base_pos_y + self.disp_y,
            self.base_pos_z + self.disp_z,
        )
    }
}

/// Exact 64-Byte L1 Cache Line Aligned Impact Crater Descriptor Slab.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(C, align(64))]
pub struct CraterSlab64 {
    /// 4B: World center X coordinate in mm
    pub center_x: i32,
    /// 4B: World center Y coordinate in mm (impact plane elevation)
    pub center_y: i32,
    /// 4B: World center Z coordinate in mm
    pub center_z: i32,
    /// 4B: Excavation bowl radius in mm ($R_{in}$)
    pub radius_inner_mm: u32,
    /// 4B: Upheaval rim lip outer boundary radius in mm ($R_{out}$)
    pub radius_outer_mm: u32,
    /// 4B: Maximum excavation bowl depth at center in mm ($D_{depth} > 0$)
    pub depth_mm: i32,
    /// 4B: Maximum rim lip upheaval height at peak in mm ($H_{rim} > 0$)
    pub rim_height_mm: i32,
    /// 4B: Impact momentum impulse in kg*m/s
    pub impulse_kg_mps: u32,
    /// 8B: Impact kinetic energy in Joules (natural 8-byte alignment)
    pub energy_joules: u64,
    /// 4B: Simulation tick when impact occurred
    pub tick: u32,
    /// 4B: Originating entity or projectile ID
    pub source_id: u32,
    /// 2B: Fraction of volume ejected as ballistic shards in Q16.16 (0..=65536)
    pub ejecta_ratio_q16: u16,
    /// 1B: Terrain material target at impact point
    pub material_target: u8,
    /// 1B: Operational flags (`CRATER_FLAG_*`)
    pub flags: u8,
    /// 12B: Explicit alignment padding
    pub _pad: [u8; 12],
}

const _: () = assert!(size_of::<CraterSlab64>() == 64);
const _: () = assert!(align_of::<CraterSlab64>() == 64);

impl Default for CraterSlab64 {
    #[inline(always)]
    fn default() -> Self {
        Self::empty()
    }
}

impl CraterSlab64 {
    /// Creates a blank inactive crater slab.
    #[inline(always)]
    pub const fn empty() -> Self {
        Self {
            center_x: 0,
            center_y: 0,
            center_z: 0,
            radius_inner_mm: 0,
            radius_outer_mm: 0,
            depth_mm: 0,
            rim_height_mm: 0,
            impulse_kg_mps: 0,
            energy_joules: 0,
            tick: 0,
            source_id: 0,
            ejecta_ratio_q16: 0,
            material_target: 0,
            flags: 0,
            _pad: [0; 12],
        }
    }

    /// Constructs a volume-conserving impact crater from physical kinetic parameters.
    pub fn from_kinetic_impact(
        center_x: i32,
        center_y: i32,
        center_z: i32,
        energy_joules: u64,
        impulse_kg_mps: u32,
        ejecta_ratio_q16: u16,
        material_target: u8,
        source_id: u32,
        tick: u32,
    ) -> Self {
        // Approximate Melosh scaling: Radius ~ C * E^(1/3)
        let r_in = (icbrt_u64(energy_joules) as u32 * 15).clamp(300, 15_000); // 0.3m ~ 15m radius
        let r_out = (r_in * 3) / 2; // R_out = 1.5 * R_in
        let depth = (r_in as i32) / 3; // Aspect ratio ~ 1:3

        // Analytical Volume Conservation formula:
        // H_rim = (3/4) * (1 - ejecta_ratio) * depth * R_in^2 / (R_out^2 - R_in^2)
        let r_in_sq = (r_in as i64) * (r_in as i64);
        let r_out_sq = (r_out as i64) * (r_out as i64);
        let delta_r_sq = (r_out_sq - r_in_sq).max(1);

        let retain_ratio_q16 = (65536u32.saturating_sub(ejecta_ratio_q16 as u32)) as i64;
        let factor = (3 * retain_ratio_q16) / 4;
        let rim_height = ((factor * (depth as i64) * r_in_sq) / (65536 * delta_r_sq)) as i32;

        Self {
            center_x,
            center_y,
            center_z,
            radius_inner_mm: r_in,
            radius_outer_mm: r_out,
            depth_mm: depth,
            rim_height_mm: rim_height,
            impulse_kg_mps,
            energy_joules,
            tick,
            source_id,
            ejecta_ratio_q16,
            material_target,
            flags: CRATER_FLAG_ACTIVE,
            _pad: [0; 12],
        }
    }
}

/// Computes the exact integer cube root of an integer via Newton-Raphson.
#[inline(always)]
pub fn icbrt_u64(n: u64) -> u64 {
    if n == 0 {
        return 0;
    }
    let mut x = 1u64 << ((64 - n.leading_zeros() + 2) / 3);
    loop {
        let y = (2 * x + n / (x * x)) / 3;
        if y >= x {
            return x;
        }
        x = y;
    }
}

pub const DEFAULT_GRID_DIM: usize = 33;
pub const DEFAULT_MESH_VERTICES: usize = 1089; // 33 * 33
pub const DEFAULT_MAX_CRATERS: usize = 32;

/// 0-Heap Fixed-Capacity Procedural Deformable Terrain Mesh Grid.
#[repr(C, align(64))]
pub struct DeformableMeshGrid<
    const GRID_DIM: usize = DEFAULT_GRID_DIM,
    const TOTAL_VERTICES: usize = DEFAULT_MESH_VERTICES,
    const MAX_CRATERS: usize = DEFAULT_MAX_CRATERS,
> {
    /// Contiguous flat array of terrain vertex slabs: [z * GRID_DIM + x]
    pub vertices: [TerrainVertexSlab64; TOTAL_VERTICES],
    /// Ring buffer of applied impact craters
    pub craters: [CraterSlab64; MAX_CRATERS],
    /// Total count of craters currently stamped into this grid
    pub crater_count: usize,
    /// Origin world X in millimeters (position of vertex [0, 0])
    pub origin_x_mm: i32,
    /// Origin world Z in millimeters (position of vertex [0, 0])
    pub origin_z_mm: i32,
    /// Horizontal distance between adjacent vertices in mm (e.g. 500 = 0.5m)
    pub cell_step_mm: i32,
    /// Bounding box minimum elevation in mm
    pub min_elevation_mm: i32,
    /// Bounding box maximum elevation in mm
    pub max_elevation_mm: i32,
    /// Dirty flag indicating CPU-to-GPU mesh synchronization is required
    pub is_dirty: bool,
    /// Dirty bounding rectangle in grid coordinates [min_x, min_z, max_x, max_z]
    pub dirty_rect: [u16; 4],
    /// Alignment padding
    pub _pad: [u8; 39],
}

const _: () = assert!(align_of::<DeformableMeshGrid<33, 1089, 32>>() == 64);

impl<const GRID_DIM: usize, const TOTAL_VERTICES: usize, const MAX_CRATERS: usize>
    DeformableMeshGrid<GRID_DIM, TOTAL_VERTICES, MAX_CRATERS>
{
    /// Initializes a flat terrain mesh grid with given origin and spacing.
    pub fn new_flat(origin_x: i32, origin_z: i32, base_elevation_y: i32, cell_step: i32) -> Self {
        assert!(
            GRID_DIM * GRID_DIM <= TOTAL_VERTICES,
            "TOTAL_VERTICES must accommodate GRID_DIM * GRID_DIM"
        );
        let mut grid = Self {
            vertices: [TerrainVertexSlab64::empty(); TOTAL_VERTICES],
            craters: [CraterSlab64::empty(); MAX_CRATERS],
            crater_count: 0,
            origin_x_mm: origin_x,
            origin_z_mm: origin_z,
            cell_step_mm: cell_step,
            min_elevation_mm: base_elevation_y,
            max_elevation_mm: base_elevation_y,
            is_dirty: false,
            dirty_rect: [0, 0, (GRID_DIM - 1) as u16, (GRID_DIM - 1) as u16],
            _pad: [0; 39],
        };

        for z in 0..GRID_DIM {
            for x in 0..GRID_DIM {
                let vx = origin_x + (x as i32) * cell_step;
                let vz = origin_z + (z as i32) * cell_step;
                grid.vertices[z * GRID_DIM + x] =
                    TerrainVertexSlab64::new(vx, base_elevation_y, vz, TERRAIN_MAT_DIRT);
            }
        }

        grid
    }

    /// Applies a volume-conserving crater displacement stamp across the mesh.
    pub fn apply_crater_displacement(&mut self, crater: &CraterSlab64) -> usize {
        let r_out = crater.radius_outer_mm as i32;
        let r_in = crater.radius_inner_mm as i32;
        let r_in_sq = (r_in as i64) * (r_in as i64);
        let r_out_sq = (r_out as i64) * (r_out as i64);
        let rim_width = (r_out - r_in).max(1) as i64;

        // Compute grid bounding rect affected by the crater
        let min_gx = ((crater.center_x - r_out - self.origin_x_mm) / self.cell_step_mm)
            .clamp(0, (GRID_DIM - 1) as i32) as usize;
        let max_gx = ((crater.center_x + r_out - self.origin_x_mm + self.cell_step_mm - 1)
            / self.cell_step_mm)
            .clamp(0, (GRID_DIM - 1) as i32) as usize;
        let min_gz = ((crater.center_z - r_out - self.origin_z_mm) / self.cell_step_mm)
            .clamp(0, (GRID_DIM - 1) as i32) as usize;
        let max_gz = ((crater.center_z + r_out - self.origin_z_mm + self.cell_step_mm - 1)
            / self.cell_step_mm)
            .clamp(0, (GRID_DIM - 1) as i32) as usize;

        if min_gx > max_gx || min_gz > max_gz {
            return 0;
        }

        let mut deformed_count = 0;

        for gz in min_gz..=max_gz {
            for gx in min_gx..=max_gx {
                let v_idx = gz * GRID_DIM + gx;
                let v = &mut self.vertices[v_idx];

                let dx = (v.base_pos_x - crater.center_x) as i64;
                let dz = (v.base_pos_z - crater.center_z) as i64;
                let dist_sq = dx * dx + dz * dz;

                if dist_sq >= r_out_sq {
                    continue;
                }

                let dist = isqrt_i64(dist_sq);

                if dist < (r_in as i64) {
                    // 1. Excavation Bowl: Delta_Y = -D * (R_in^2 - dist^2) / R_in^2
                    let delta_y = ((crater.depth_mm as i64) * (r_in_sq - dist_sq)) / r_in_sq;
                    v.disp_y = v.disp_y.saturating_sub(delta_y as i32);

                    // Analytical normal gradient: dY/dx = 2 * D * dx / R_in^2
                    let grad_x = (2 * (crater.depth_mm as i64) * dx * 65536) / r_in_sq;
                    let grad_z = (2 * (crater.depth_mm as i64) * dz * 65536) / r_in_sq;
                    Self::update_normal_q16(v, grad_x, grad_z);
                } else {
                    // 2. Upheaval Rim: Delta_Y = +H_rim * 4 * xi * (W - xi) / W^2
                    let xi = dist - (r_in as i64);
                    let delta_y = ((crater.rim_height_mm as i64) * 4 * xi * (rim_width - xi))
                        / (rim_width * rim_width);
                    v.disp_y = v.disp_y.saturating_add(delta_y as i32);

                    // Analytical normal gradient
                    let d_delta = ((crater.rim_height_mm as i64) * 4 * (rim_width - 2 * xi) * 65536)
                        / (rim_width * rim_width);
                    let grad_x = -(d_delta * dx) / dist.max(1);
                    let grad_z = -(d_delta * dz) / dist.max(1);
                    Self::update_normal_q16(v, grad_x, grad_z);
                }

                v.flags |= VERTEX_FLAG_MODIFIED;
                v.crater_count = v.crater_count.saturating_add(1);
                v.stress_q16 = v.stress_q16.saturating_add(4096); // Plastic strain
                v.last_tick = crater.tick;
                deformed_count += 1;

                // Update bounding elevation
                let world_y = v.base_pos_y + v.disp_y;
                if world_y < self.min_elevation_mm {
                    self.min_elevation_mm = world_y;
                }
                if world_y > self.max_elevation_mm {
                    self.max_elevation_mm = world_y;
                }
            }
        }

        if deformed_count > 0 {
            self.is_dirty = true;
            // Record crater in ring buffer
            let slot = self.crater_count % MAX_CRATERS;
            self.craters[slot] = *crater;
            self.crater_count += 1;
        }

        deformed_count
    }

    /// Normalizes and updates the 3D surface normal vector in Q16.16.
    #[inline(always)]
    fn update_normal_q16(v: &mut TerrainVertexSlab64, grad_x: i64, grad_z: i64) {
        let nx = -grad_x;
        let ny = Q16_ONE as i64;
        let nz = -grad_z;
        let len_sq = nx * nx + ny * ny + nz * nz;
        let len = isqrt_i64(len_sq).max(1);

        v.normal_x_q16 = ((nx * (Q16_ONE as i64)) / len) as i32;
        v.normal_y_q16 = ((ny * (Q16_ONE as i64)) / len) as i32;
        v.normal_z_q16 = ((nz * (Q16_ONE as i64)) / len) as i32;
    }

    /// Computes guaranteed bit-exact 64-bit FNV-1a checksum of the entire mesh grid.
    pub fn compute_consensus_hash(&self) -> u64 {
        const FNV_OFFSET: u64 = 0xcbf29ce484222325;
        const FNV_PRIME: u64 = 0x100000001b3;

        let mut h = FNV_OFFSET ^ (self.crater_count as u64);

        for v in &self.vertices {
            if (v.flags & VERTEX_FLAG_MODIFIED) != 0 {
                let bytes = [
                    v.disp_x.to_le_bytes(),
                    v.disp_y.to_le_bytes(),
                    v.disp_z.to_le_bytes(),
                    v.normal_x_q16.to_le_bytes(),
                    v.normal_y_q16.to_le_bytes(),
                    v.normal_z_q16.to_le_bytes(),
                ];
                for chunk in &bytes {
                    for &b in chunk {
                        h ^= b as u64;
                        h = h.wrapping_mul(FNV_PRIME);
                    }
                }
                h ^= v.stress_q16 as u64;
                h = h.wrapping_mul(FNV_PRIME);
            }
        }

        h ^= (self.min_elevation_mm as u64) | ((self.max_elevation_mm as u64) << 32);
        h = h.wrapping_mul(FNV_PRIME);
        h
    }
}
