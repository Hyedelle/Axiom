//! Deterministic 3D Voxel Fluid Dynamics & Infiltration Core Engine.
//!
//! Provides zero-float 64B L1-aligned integer fluid level and porous rock infiltration
//! lattices for 16x16x16 voxel volumes. Features gravity downwards flow, horizontal hydrostatic
//! pressure spreading, rock permeability infiltration, thermal evaporation, and 100% mass conservation.

use crate::thermal_stress::{ThermalStressField16, TEMP_AMBIENT, TEMP_WARM_THRESHOLD};
use crate::voxel_destruct::{
    VoxelVolume16, VOXEL_AIR, VOXEL_DIM, VOXEL_STONE, VOXEL_TOTAL, VOXEL_WATER,
};

pub const FLUID_MAX_LEVEL: u8 = 255;
pub const ROCK_SATURATION_THRESHOLD: u8 = 180;

/// Result summary of one tick of 3D voxel fluid dynamics simulation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FluidStepResult {
    pub active_fluid_cells: usize,
    pub units_moved: u32,
    pub units_infiltrated: u32,
    pub units_evaporated: u32,
    pub steam_explosions: u16,
}

/// 8,256-byte (129 x 64B L1 Cache Lines) 16x16x16 Fluid & Infiltration Field.
#[repr(C, align(64))]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct VoxelFluidField16 {
    pub level: [u8; VOXEL_TOTAL],       // 4096B: Free fluid level per cell (0..=255)
    pub infiltrated: [u8; VOXEL_TOTAL], // 4096B: Absorbed infiltration in porous voxels (0..=255)
    pub total_fluid_units: u32,         // 4B: Total free fluid units in lattice
    pub total_infiltrated: u32,         // 4B: Cumulative units absorbed by terrain
    pub total_evaporated: u32,          // 4B: Cumulative units lost to high temperature
    pub active_fluid_cells: u16,        // 2B: Count of cells with level > 0
    pub steam_explosions: u16,          // 2B: Cumulative steam explosions
    pub _pad: [u8; 48],                 // 48B: Exact 64-byte alignment padding
}

const _: () = assert!(core::mem::size_of::<VoxelFluidField16>() == 8256);
const _: () = assert!(core::mem::align_of::<VoxelFluidField16>() == 64);

impl Default for VoxelFluidField16 {
    fn default() -> Self {
        Self::new()
    }
}

impl VoxelFluidField16 {
    /// Creates an empty 3D voxel fluid field.
    pub const fn new() -> Self {
        Self {
            level: [0u8; VOXEL_TOTAL],
            infiltrated: [0u8; VOXEL_TOTAL],
            total_fluid_units: 0,
            total_infiltrated: 0,
            total_evaporated: 0,
            active_fluid_cells: 0,
            steam_explosions: 0,
            _pad: [0u8; 48],
        }
    }

    #[inline(always)]
    pub const fn index(x: usize, y: usize, z: usize) -> usize {
        y * (VOXEL_DIM * VOXEL_DIM) + z * VOXEL_DIM + x
    }

    #[inline(always)]
    pub fn get_level(&self, x: usize, y: usize, z: usize) -> u8 {
        if x >= VOXEL_DIM || y >= VOXEL_DIM || z >= VOXEL_DIM {
            0
        } else {
            self.level[Self::index(x, y, z)]
        }
    }

    #[inline(always)]
    pub fn get_infiltrated(&self, x: usize, y: usize, z: usize) -> u8 {
        if x >= VOXEL_DIM || y >= VOXEL_DIM || z >= VOXEL_DIM {
            0
        } else {
            self.infiltrated[Self::index(x, y, z)]
        }
    }

    /// Checks if a rock voxel is softened by high pore-water infiltration (exceeding ROCK_SATURATION_THRESHOLD).
    #[inline(always)]
    pub fn is_rock_softened(&self, x: usize, y: usize, z: usize) -> bool {
        self.get_infiltrated(x, y, z) >= ROCK_SATURATION_THRESHOLD
    }

    /// Injects a discrete quantity of fluid into a specific voxel cell.
    pub fn inject_fluid(
        &mut self,
        volume: &mut VoxelVolume16,
        x: usize,
        y: usize,
        z: usize,
        amount: u8,
    ) {
        if x >= VOXEL_DIM || y >= VOXEL_DIM || z >= VOXEL_DIM {
            return;
        }
        let idx = Self::index(x, y, z);
        let cur = self.level[idx];
        let new_level = cur.saturating_add(amount);
        let added = new_level - cur;

        if cur == 0 && new_level > 0 {
            self.active_fluid_cells = self.active_fluid_cells.saturating_add(1);
        }
        self.level[idx] = new_level;
        self.total_fluid_units = self.total_fluid_units.saturating_add(added as u32);

        if volume.data[idx] == VOXEL_AIR && new_level > 0 {
            volume.data[idx] = VOXEL_WATER;
        }
    }

    /// Advances 1 tick of discrete 3D fluid flow, spreading, infiltration, evaporation, and steam explosions.
    pub fn step_fluid(
        &mut self,
        volume: &mut VoxelVolume16,
        mut thermal: Option<&mut ThermalStressField16>,
        infiltration_permille: u16,
    ) -> FluidStepResult {
        let mut next_level = self.level;
        let mut units_moved = 0u32;
        let mut units_infiltrated = 0u32;
        let mut units_evaporated = 0u32;
        let mut steam_explosions = 0u16;

        // Process from bottom to top (y = 0..15) to ensure stable gravity drainage
        for y in 0..VOXEL_DIM {
            for z in 0..VOXEL_DIM {
                for x in 0..VOXEL_DIM {
                    let idx = Self::index(x, y, z);
                    let mut cur_fluid = next_level[idx];
                    if cur_fluid == 0 {
                        continue;
                    }

                    // 1. High-Temperature Evaporation & Steam Explosion Flashing
                    if let Some(ref mut t_field) = thermal {
                        let temp = t_field.temp[idx];
                        if temp >= 750 && cur_fluid >= 40 {
                            // Violent steam explosion: instantaneous flashing of water to steam
                            let evap = cur_fluid;
                            units_evaporated += evap as u32;
                            self.total_evaporated += evap as u32;
                            self.total_fluid_units = self.total_fluid_units.saturating_sub(evap as u32);
                            next_level[idx] = 0;
                            steam_explosions = steam_explosions.saturating_add(1);
                            self.steam_explosions = self.steam_explosions.saturating_add(1);

                            // Latent heat quench (-250°C) and steam expansion shockwave (+300 stress)
                            let new_t = temp.saturating_sub(250).max(TEMP_AMBIENT);
                            t_field.temp[idx] = new_t;
                            t_field.stress[idx] = t_field.stress[idx].saturating_add(300);

                            if volume.data[idx] == VOXEL_WATER {
                                volume.data[idx] = VOXEL_AIR;
                            }
                            continue;
                        } else if temp >= TEMP_WARM_THRESHOLD {
                            let evap = ((cur_fluid as u32 * (temp - TEMP_WARM_THRESHOLD) as u32) / 4000)
                                .max(1)
                                .min(cur_fluid as u32) as u8;
                            cur_fluid = cur_fluid.saturating_sub(evap);
                            units_evaporated += evap as u32;
                            self.total_evaporated += evap as u32;
                            self.total_fluid_units = self.total_fluid_units.saturating_sub(evap as u32);
                            next_level[idx] = cur_fluid;
                            if cur_fluid == 0 {
                                if volume.data[idx] == VOXEL_WATER {
                                    volume.data[idx] = VOXEL_AIR;
                                }
                                continue;
                            }
                        }
                    }

                    // 2. Downwards Gravity Flow (y - 1)
                    if y > 0 {
                        let down_idx = Self::index(x, y - 1, z);
                        let down_mat = volume.data[down_idx];

                        if down_mat == VOXEL_AIR || down_mat == VOXEL_WATER {
                            let down_cur = next_level[down_idx];
                            let down_space = FLUID_MAX_LEVEL.saturating_sub(down_cur);

                            if down_space > 0 {
                                let flow = cur_fluid.min(down_space);
                                cur_fluid -= flow;
                                next_level[down_idx] += flow;
                                next_level[idx] = cur_fluid;
                                units_moved += flow as u32;

                                if volume.data[down_idx] == VOXEL_AIR && next_level[down_idx] > 0 {
                                    volume.data[down_idx] = VOXEL_WATER;
                                }

                                if cur_fluid == 0 {
                                    if volume.data[idx] == VOXEL_WATER {
                                        volume.data[idx] = VOXEL_AIR;
                                    }
                                    continue;
                                }
                            }
                        } else if down_mat == VOXEL_STONE && infiltration_permille > 0 {
                            // Infiltration into bottom rock
                            let cur_inf = self.infiltrated[down_idx];
                            if cur_inf < FLUID_MAX_LEVEL {
                                let inf_capacity = FLUID_MAX_LEVEL - cur_inf;
                                let inf_rate = (((cur_fluid as u32 * infiltration_permille as u32) / 1000)
                                    .max(1)
                                    .min(cur_fluid as u32) as u8)
                                    .min(inf_capacity);

                                if inf_rate > 0 {
                                    cur_fluid -= inf_rate;
                                    self.infiltrated[down_idx] += inf_rate;
                                    units_infiltrated += inf_rate as u32;
                                    self.total_infiltrated += inf_rate as u32;
                                    self.total_fluid_units =
                                        self.total_fluid_units.saturating_sub(inf_rate as u32);
                                    next_level[idx] = cur_fluid;

                                    if cur_fluid == 0 {
                                        if volume.data[idx] == VOXEL_WATER {
                                            volume.data[idx] = VOXEL_AIR;
                                        }
                                        continue;
                                    }
                                }
                            }
                        }
                    }

                    // 3. Horizontal Hydrostatic Pressure Spreading (4-Neighbors)
                    if cur_fluid > 1 {
                        let neighbors = [
                            if x > 0 { Some((Self::index(x - 1, y, z), x - 1, y, z)) } else { None },
                            if x + 1 < VOXEL_DIM { Some((Self::index(x + 1, y, z), x + 1, y, z)) } else { None },
                            if z > 0 { Some((Self::index(x, y, z - 1), x, y, z - 1)) } else { None },
                            if z + 1 < VOXEL_DIM { Some((Self::index(x, y, z + 1), x, y, z + 1)) } else { None },
                        ];

                        for n_opt in neighbors.into_iter().flatten() {
                            let (n_idx, _nx, _ny, _nz) = n_opt;
                            let n_mat = volume.data[n_idx];

                            if n_mat == VOXEL_AIR || n_mat == VOXEL_WATER {
                                let n_level = next_level[n_idx];
                                if cur_fluid > n_level.saturating_add(1) {
                                    let diff = cur_fluid - n_level;
                                    let flow = (diff / 4).max(1).min(cur_fluid);

                                    cur_fluid -= flow;
                                    next_level[n_idx] += flow;
                                    next_level[idx] = cur_fluid;
                                    units_moved += flow as u32;

                                    if volume.data[n_idx] == VOXEL_AIR && next_level[n_idx] > 0 {
                                        volume.data[n_idx] = VOXEL_WATER;
                                    }

                                    if cur_fluid <= 1 {
                                        break;
                                    }
                                }
                            } else if n_mat == VOXEL_STONE && infiltration_permille > 0 {
                                let cur_inf = self.infiltrated[n_idx];
                                if cur_inf < FLUID_MAX_LEVEL {
                                    let inf_rate = (((cur_fluid as u32 * (infiltration_permille / 2) as u32) / 1000)
                                        .max(1)
                                        .min(cur_fluid as u32) as u8)
                                        .min(FLUID_MAX_LEVEL - cur_inf);

                                    if inf_rate > 0 {
                                        cur_fluid -= inf_rate;
                                        self.infiltrated[n_idx] += inf_rate;
                                        units_infiltrated += inf_rate as u32;
                                        self.total_infiltrated += inf_rate as u32;
                                        self.total_fluid_units =
                                            self.total_fluid_units.saturating_sub(inf_rate as u32);
                                        next_level[idx] = cur_fluid;

                                        if cur_fluid <= 1 {
                                            break;
                                        }
                                    }
                                }
                            }
                        }
                    }

                    if cur_fluid == 0 && volume.data[idx] == VOXEL_WATER {
                        volume.data[idx] = VOXEL_AIR;
                    }
                }
            }
        }

        self.level = next_level;

        // Recount active cells and update telemetry
        let mut active_count = 0usize;
        let mut total_check = 0u32;
        for &lvl in self.level.iter() {
            if lvl > 0 {
                active_count += 1;
                total_check += lvl as u32;
            }
        }
        self.active_fluid_cells = active_count.min(65535) as u16;
        self.total_fluid_units = total_check;

        FluidStepResult {
            active_fluid_cells: active_count,
            units_moved,
            units_infiltrated,
            units_evaporated,
            steam_explosions,
        }
    }

    /// Couples fluid field with thermal stress field:
    /// 1. Rapidly quenches elevated temperatures in saturated cells (accelerated latent heat cooling).
    /// 2. Relaxes mechanical thermal shear stress (hydraulic damping / pore pressure dissipation).
    /// Returns (quenched_temp_units, relaxed_stress_units).
    pub fn relax_thermal_and_stress(
        &mut self,
        thermal: &mut ThermalStressField16,
    ) -> (u32, u32) {
        let mut quenched_temp = 0u32;
        let mut relaxed_stress = 0u32;

        for idx in 0..VOXEL_TOTAL {
            let lvl = self.level[idx] as u32;
            let inf = self.infiltrated[idx] as u32;
            let saturation = lvl + inf;

            if saturation > 0 {
                let cur_t = thermal.temp[idx];
                if cur_t > TEMP_AMBIENT {
                    let quench = (((cur_t - TEMP_AMBIENT) as u32 * saturation) / 512).max(1) as u16;
                    let new_t = cur_t.saturating_sub(quench).max(TEMP_AMBIENT);
                    quenched_temp += (cur_t - new_t) as u32;
                    thermal.temp[idx] = new_t;
                }

                let cur_s = thermal.stress[idx];
                if cur_s > 0 {
                    let relief = ((cur_s as u32 * saturation) / 400).max(1) as u16;
                    let new_s = cur_s.saturating_sub(relief);
                    relaxed_stress += (cur_s - new_s) as u32;
                    thermal.stress[idx] = new_s;
                }
            }
        }

        (quenched_temp, relaxed_stress)
    }

    /// Computes full 64-bit FNV-1a deterministic state hash.
    pub fn compute_consensus_hash(&self) -> u64 {
        let mut hash: u64 = 0xCBF2_9CE4_8422_2325;

        for &lvl in self.level.iter() {
            hash ^= lvl as u64;
            hash = hash.wrapping_mul(0x100_0000_01B3);
        }

        for &inf in self.infiltrated.iter() {
            hash ^= inf as u64;
            hash = hash.wrapping_mul(0x100_0000_01B3);
        }

        hash ^= self.total_fluid_units as u64;
        hash = hash.wrapping_mul(0x100_0000_01B3);

        hash ^= self.total_infiltrated as u64;
        hash = hash.wrapping_mul(0x100_0000_01B3);

        hash ^= self.total_evaporated as u64;
        hash = hash.wrapping_mul(0x100_0000_01B3);

        hash ^= self.steam_explosions as u64;
        hash = hash.wrapping_mul(0x100_0000_01B3);

        hash
    }
}
