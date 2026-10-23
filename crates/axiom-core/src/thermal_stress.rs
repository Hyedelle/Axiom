//! Deterministic 3D Thermal Stress Field & Phase Transformation Core Engine.
//!
//! Provides zero-float 64B L1-aligned integer temperature and mechanical shear stress
//! lattices for 16x16x16 voxel volumes. Features 6-neighbor Laplacian thermal diffusion,
//! cooling dissipation, and dynamic material phase transitions (Stone -> Molten).

use crate::voxel_collapse::DynamicCollapseSystem;
use crate::voxel_destruct::{
    VoxelVolume16, VOXEL_AIR, VOXEL_DIM, VOXEL_METAL, VOXEL_MOLTEN, VOXEL_STONE,
    VOXEL_TOTAL,
};

pub const TEMP_AMBIENT: u16 = 20;
pub const TEMP_WARM_THRESHOLD: u16 = 300;
pub const TEMP_INCANDESCENT_THRESHOLD: u16 = 700;
pub const TEMP_PHASE_MELT_THRESHOLD: u16 = 750;
pub const STRESS_FRACTURE_THRESHOLD: u16 = 800;

/// Material thermal & mechanical physical properties.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VoxelMaterialProps {
    /// Thermal conductivity in permille (higher = faster diffusion, e.g. Metal: 650, Stone: 120, Air: 50).
    pub conductivity_permille: u16,
    /// Heat capacity in permille (higher = absorbs more heat before temp changes, e.g. Stone: 800, Metal: 350).
    pub heat_capacity_permille: u16,
    /// Melting / phase transformation temperature threshold (0..1000°C).
    pub melt_temp_threshold: u16,
    /// Mechanical shear stress fracture threshold (0..1000 units).
    pub fracture_stress_threshold: u16,
    /// Thermal expansion stress coefficient in permille.
    pub thermal_expansion_permille: u16,
}

pub const MATERIAL_PROPERTIES: [VoxelMaterialProps; 5] = [
    // 0: VOXEL_AIR
    VoxelMaterialProps {
        conductivity_permille: 50,
        heat_capacity_permille: 100,
        melt_temp_threshold: 1000,
        fracture_stress_threshold: 1000,
        thermal_expansion_permille: 0,
    },
    // 1: VOXEL_STONE
    VoxelMaterialProps {
        conductivity_permille: 120,
        heat_capacity_permille: 800,
        melt_temp_threshold: 750,
        fracture_stress_threshold: 800,
        thermal_expansion_permille: 60,
    },
    // 2: VOXEL_METAL
    VoxelMaterialProps {
        conductivity_permille: 650,
        heat_capacity_permille: 350,
        melt_temp_threshold: 950,
        fracture_stress_threshold: 920,
        thermal_expansion_permille: 120,
    },
    // 3: VOXEL_MOLTEN
    VoxelMaterialProps {
        conductivity_permille: 500,
        heat_capacity_permille: 300,
        melt_temp_threshold: 1000,
        fracture_stress_threshold: 1000,
        thermal_expansion_permille: 180,
    },
    // 4: VOXEL_ARMOR
    VoxelMaterialProps {
        conductivity_permille: 40,
        heat_capacity_permille: 950,
        melt_temp_threshold: 1000,
        fracture_stress_threshold: 950,
        thermal_expansion_permille: 30,
    },
];

#[inline(always)]
pub const fn get_material_props(material: u8) -> &'static VoxelMaterialProps {
    if (material as usize) < MATERIAL_PROPERTIES.len() {
        &MATERIAL_PROPERTIES[material as usize]
    } else {
        &MATERIAL_PROPERTIES[1] // fallback to stone
    }
}

/// Result of an integrated thermal diffusion, phase transition, and structural collapse step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ThermalCollapseResult {
    pub phase_transitions: u32,
    pub fractures_triggered: u32,
    pub collapsed_chunks: usize,
}

/// 16,448-byte (257 x 64B L1 Cache Lines) 16x16x16 Thermal & Stress Field.
#[repr(C, align(64))]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ThermalStressField16 {
    pub temp: [u16; VOXEL_TOTAL],       // 8192B: Integer temperature lattice (0..1000)
    pub stress: [u16; VOXEL_TOTAL],     // 8192B: Mechanical shear stress lattice (0..1000)
    pub total_dissipated: u32,          // 4B: Total thermal energy dissipated
    pub phase_transitions: u32,         // 4B: Total voxel material phase transitions
    pub fractures_triggered: u32,       // 4B: Total stress fractures occurred
    pub active_thermal_sources: u16,    // 2B: Active heat source count
    pub _pad: [u8; 50],                 // 50B: Exact 64-byte alignment padding
}

const _: () = assert!(core::mem::size_of::<ThermalStressField16>() == 16448);
const _: () = assert!(core::mem::align_of::<ThermalStressField16>() == 64);

impl Default for ThermalStressField16 {
    fn default() -> Self {
        Self::new()
    }
}

impl ThermalStressField16 {
    /// Creates a fresh thermal and stress field initialized to ambient room temperature.
    pub const fn new() -> Self {
        Self {
            temp: [TEMP_AMBIENT; VOXEL_TOTAL],
            stress: [0u16; VOXEL_TOTAL],
            total_dissipated: 0,
            phase_transitions: 0,
            fractures_triggered: 0,
            active_thermal_sources: 0,
            _pad: [0u8; 50],
        }
    }

    #[inline(always)]
    pub const fn index(x: usize, y: usize, z: usize) -> usize {
        y * (VOXEL_DIM * VOXEL_DIM) + z * VOXEL_DIM + x
    }

    #[inline(always)]
    pub fn get_temp(&self, x: usize, y: usize, z: usize) -> u16 {
        if x >= VOXEL_DIM || y >= VOXEL_DIM || z >= VOXEL_DIM {
            TEMP_AMBIENT
        } else {
            self.temp[Self::index(x, y, z)]
        }
    }

    #[inline(always)]
    pub fn get_stress(&self, x: usize, y: usize, z: usize) -> u16 {
        if x >= VOXEL_DIM || y >= VOXEL_DIM || z >= VOXEL_DIM {
            0
        } else {
            self.stress[Self::index(x, y, z)]
        }
    }

    /// Injects a high-energy spherical thermal and mechanical stress pulse.
    pub fn inject_thermal_pulse(
        &mut self,
        cx: usize,
        cy: usize,
        cz: usize,
        radius: usize,
        peak_temp: u16,
        peak_stress: u16,
    ) {
        let r = radius as i32;
        let r_sq = r * r;

        let min_x = (cx as i32 - r).max(0) as usize;
        let max_x = (cx as i32 + r).min(VOXEL_DIM as i32 - 1) as usize;
        let min_y = (cy as i32 - r).max(0) as usize;
        let max_y = (cy as i32 + r).min(VOXEL_DIM as i32 - 1) as usize;
        let min_z = (cz as i32 - r).max(0) as usize;
        let max_z = (cz as i32 + r).min(VOXEL_DIM as i32 - 1) as usize;

        for y in min_y..=max_y {
            let dy = y as i32 - cy as i32;
            for z in min_z..=max_z {
                let dz = z as i32 - cz as i32;
                for x in min_x..=max_x {
                    let dx = x as i32 - cx as i32;
                    let dist_sq = dx * dx + dy * dy + dz * dz;
                    if dist_sq <= r_sq {
                        let falloff = 1000 - (dist_sq * 1000) / (r_sq + 1);
                        let idx = Self::index(x, y, z);

                        let added_temp = (peak_temp as u32 * falloff as u32 / 1000) as u16;
                        let added_stress = (peak_stress as u32 * falloff as u32 / 1000) as u16;

                        self.temp[idx] = self.temp[idx].saturating_add(added_temp).min(1000);
                        self.stress[idx] = self.stress[idx].saturating_add(added_stress).min(1000);
                    }
                }
            }
        }

        self.active_thermal_sources = self.active_thermal_sources.saturating_add(1);
    }

    /// Advances 1 tick of integer Laplacian thermal diffusion, cooling, and phase transitions.
    ///
    /// Returns `(new_phase_transitions, new_fractures)`.
    pub fn step_thermal_diffusion(
        &mut self,
        volume: &mut VoxelVolume16,
        cooling_rate_permille: u16,
    ) -> (u32, u32) {
        let mut new_transitions = 0u32;
        let mut new_fractures = 0u32;

        // 1. Integer Laplacian Thermal Diffusion (6-Neighbor Diffusion Step)
        let mut next_temp = self.temp;

        for y in 0..VOXEL_DIM {
            for z in 0..VOXEL_DIM {
                for x in 0..VOXEL_DIM {
                    let idx = Self::index(x, y, z);
                    let cur = self.temp[idx];

                    // 6-Neighbor sum with edge clamping
                    let t_left = if x > 0 { self.temp[Self::index(x - 1, y, z)] } else { cur };
                    let t_right = if x + 1 < VOXEL_DIM { self.temp[Self::index(x + 1, y, z)] } else { cur };
                    let t_down = if y > 0 { self.temp[Self::index(x, y - 1, z)] } else { cur };
                    let t_up = if y + 1 < VOXEL_DIM { self.temp[Self::index(x, y + 1, z)] } else { cur };
                    let t_back = if z > 0 { self.temp[Self::index(x, y, z - 1)] } else { cur };
                    let t_front = if z + 1 < VOXEL_DIM { self.temp[Self::index(x, y, z + 1)] } else { cur };

                    let neighbor_sum = t_left as i32 + t_right as i32 + t_down as i32
                        + t_up as i32 + t_back as i32 + t_front as i32;

                    // Laplacian: delta = (sum - 6*cur) / 16 (pure integer shift approximation)
                    let laplacian_delta = (neighbor_sum - (cur as i32 * 6)) / 16;
                    let mut diffused = (cur as i32 + laplacian_delta).clamp(TEMP_AMBIENT as i32, 1000) as u16;

                    // Atmospheric Cooling dissipation
                    if diffused > TEMP_AMBIENT {
                        let dissipation = ((diffused - TEMP_AMBIENT) as u32 * cooling_rate_permille as u32 / 1000) as u16;
                        diffused = diffused.saturating_sub(dissipation.max(1));
                        self.total_dissipated = self.total_dissipated.saturating_add(dissipation as u32);
                    }

                    next_temp[idx] = diffused;

                    // 2. Material Phase Transformation on Thermal Threshold
                    let mat = volume.data[idx];
                    let props = get_material_props(mat);

                    if (mat == VOXEL_STONE || mat == VOXEL_METAL) && diffused >= props.melt_temp_threshold {
                        volume.data[idx] = VOXEL_MOLTEN;
                        new_transitions += 1;
                        self.phase_transitions += 1;
                    }

                    // 3. Mechanical Stress Dissipation, Thermal Expansion & Fracture
                    let cur_stress = self.stress[idx];

                    // Accumulate thermal expansion stress if hot
                    let expansion_stress = if diffused > TEMP_WARM_THRESHOLD {
                        (((diffused - TEMP_WARM_THRESHOLD) as u32 * props.thermal_expansion_permille as u32) / 1000) as u16 / 8
                    } else {
                        0
                    };
                    let total_cell_stress = cur_stress.saturating_add(expansion_stress);

                    if total_cell_stress > 0 {
                        // Brittle rock & metal stress fracture
                        if total_cell_stress >= props.fracture_stress_threshold && mat != VOXEL_AIR {
                            new_fractures += 1;
                            self.fractures_triggered += 1;
                            self.stress[idx] = 200; // Relieve stress on fracture
                        } else {
                            // Dissipate 10% of stress per step
                            let stress_decay = (total_cell_stress as u32 * 100 / 1000) as u16;
                            self.stress[idx] = total_cell_stress.saturating_sub(stress_decay.max(1)).min(1000);
                        }
                    }
                }
            }
        }

        self.temp = next_temp;
        (new_transitions, new_fractures)
    }

    /// Advances 1 tick of multi-material weighted thermal diffusion, cooling, and phase transitions.
    ///
    /// Considers individual heat capacity, thermal conductivity, thermal expansion,
    /// and overburden vertical compression stress.
    pub fn step_thermal_diffusion_advanced(
        &mut self,
        volume: &mut VoxelVolume16,
        cooling_rate_permille: u16,
    ) -> (u32, u32) {
        let mut new_transitions = 0u32;
        let mut new_fractures = 0u32;

        let mut next_temp = self.temp;

        for y in 0..VOXEL_DIM {
            // Overburden depth compression load from resting weight above (0 at top, higher at bottom)
            let depth_compression = ((15 - y) as u16) * 3;

            for z in 0..VOXEL_DIM {
                for x in 0..VOXEL_DIM {
                    let idx = Self::index(x, y, z);
                    let cur_t = self.temp[idx];
                    let mat = volume.data[idx];
                    let props = get_material_props(mat);

                    // 6-neighbor weighted flux calculation based on material conductivity
                    let neighbors = [
                        if x > 0 { (self.temp[Self::index(x - 1, y, z)], volume.data[Self::index(x - 1, y, z)]) } else { (cur_t, mat) },
                        if x + 1 < VOXEL_DIM { (self.temp[Self::index(x + 1, y, z)], volume.data[Self::index(x + 1, y, z)]) } else { (cur_t, mat) },
                        if y > 0 { (self.temp[Self::index(x, y - 1, z)], volume.data[Self::index(x, y - 1, z)]) } else { (cur_t, mat) },
                        if y + 1 < VOXEL_DIM { (self.temp[Self::index(x, y + 1, z)], volume.data[Self::index(x, y + 1, z)]) } else { (cur_t, mat) },
                        if z > 0 { (self.temp[Self::index(x, y, z - 1)], volume.data[Self::index(x, y, z - 1)]) } else { (cur_t, mat) },
                        if z + 1 < VOXEL_DIM { (self.temp[Self::index(x, y, z + 1)], volume.data[Self::index(x, y, z + 1)]) } else { (cur_t, mat) },
                    ];

                    let mut total_flux: i32 = 0;
                    for (nb_t, nb_mat) in neighbors {
                        let nb_props = get_material_props(nb_mat);
                        // Harmonic average approximation for interface conductivity
                        let eff_cond = (props.conductivity_permille as u32 + nb_props.conductivity_permille as u32) / 2;
                        let delta_t = nb_t as i32 - cur_t as i32;
                        total_flux += (delta_t * eff_cond as i32) / 1000;
                    }

                    // Delta T inverse to heat capacity
                    let cap = props.heat_capacity_permille.max(100) as i32;
                    let temp_delta = (total_flux * 400) / (cap * 6);
                    let mut diffused = (cur_t as i32 + temp_delta).clamp(TEMP_AMBIENT as i32, 1000) as u16;

                    // Atmospheric Cooling dissipation
                    if diffused > TEMP_AMBIENT {
                        let dissipation = ((diffused - TEMP_AMBIENT) as u32 * cooling_rate_permille as u32 / 1000) as u16;
                        diffused = diffused.saturating_sub(dissipation.max(1));
                        self.total_dissipated = self.total_dissipated.saturating_add(dissipation as u32);
                    }

                    next_temp[idx] = diffused;

                    // Phase transformation
                    if (mat == VOXEL_STONE || mat == VOXEL_METAL) && diffused >= props.melt_temp_threshold {
                        volume.data[idx] = VOXEL_MOLTEN;
                        new_transitions += 1;
                        self.phase_transitions += 1;
                    }

                    // Mechanical Shear Stress + Thermal Expansion + Overburden
                    let cur_stress = self.stress[idx];
                    let expansion_stress = if diffused > TEMP_WARM_THRESHOLD {
                        (((diffused - TEMP_WARM_THRESHOLD) as u32 * props.thermal_expansion_permille as u32) / 1000) as u16 / 4
                    } else {
                        0
                    };
                    let total_shear = cur_stress
                        .saturating_add(expansion_stress)
                        .saturating_add(depth_compression);

                    if total_shear > 0 {
                        if total_shear >= props.fracture_stress_threshold && mat != VOXEL_AIR {
                            new_fractures += 1;
                            self.fractures_triggered += 1;
                            self.stress[idx] = 200; // Relieve stress on fracture
                        } else {
                            let stress_decay = (total_shear as u32 * 100 / 1000) as u16;
                            self.stress[idx] = total_shear.saturating_sub(stress_decay.max(1)).min(1000);
                        }
                    }
                }
            }
        }

        self.temp = next_temp;
        (new_transitions, new_fractures)
    }

    /// Advanced thermal diffusion step coupled with structural collapse analysis.
    pub fn step_thermal_and_collapse_advanced<const MAX_CHUNKS: usize>(
        &mut self,
        volume: &mut VoxelVolume16,
        collapse_system: &mut DynamicCollapseSystem<MAX_CHUNKS>,
        cooling_rate_permille: u16,
        anchor_y: i16,
        world_origin_q16: [i32; 3],
        voxel_scale_q16: i32,
    ) -> ThermalCollapseResult {
        let (phase_transitions, fractures_triggered) =
            self.step_thermal_diffusion_advanced(volume, cooling_rate_permille);

        let collapsed_chunks = if phase_transitions > 0 || fractures_triggered > 0 {
            collapse_system.detect_and_collapse(
                volume,
                anchor_y,
                world_origin_q16,
                voxel_scale_q16,
            )
        } else {
            0
        };

        ThermalCollapseResult {
            phase_transitions,
            fractures_triggered,
            collapsed_chunks,
        }
    }

    /// Executes 1 tick of thermal diffusion, cooling, and material phase transition.
    ///
    /// If any phase transitions (melting) or stress fractures occurred, automatically
    /// runs connected component analysis on `collapse_system` to detach floating structures.
    pub fn step_thermal_and_collapse<const MAX_CHUNKS: usize>(
        &mut self,
        volume: &mut VoxelVolume16,
        collapse_system: &mut DynamicCollapseSystem<MAX_CHUNKS>,
        cooling_rate_permille: u16,
        anchor_y: i16,
        world_origin_q16: [i32; 3],
        voxel_scale_q16: i32,
    ) -> ThermalCollapseResult {
        let (phase_transitions, fractures_triggered) =
            self.step_thermal_diffusion(volume, cooling_rate_permille);

        let collapsed_chunks = if phase_transitions > 0 || fractures_triggered > 0 {
            collapse_system.detect_and_collapse(
                volume,
                anchor_y,
                world_origin_q16,
                voxel_scale_q16,
            )
        } else {
            0
        };

        ThermalCollapseResult {
            phase_transitions,
            fractures_triggered,
            collapsed_chunks,
        }
    }

    /// Returns the maximum temperature across all cells in the lattice.
    pub fn max_temp(&self) -> u16 {
        let mut m = 0u16;
        for &t in self.temp.iter() {
            if t > m {
                m = t;
            }
        }
        m
    }

    /// Computes full 64-bit FNV-1a consensus state hash across thermal and stress fields.
    pub fn compute_consensus_hash(&self) -> u64 {
        let mut hash: u64 = 0xCBF2_9CE4_8422_2325;

        for &t in self.temp.iter() {
            hash ^= t as u64;
            hash = hash.wrapping_mul(0x100_0000_01B3);
        }

        for &s in self.stress.iter() {
            hash ^= s as u64;
            hash = hash.wrapping_mul(0x100_0000_01B3);
        }

        hash ^= self.total_dissipated as u64;
        hash = hash.wrapping_mul(0x100_0000_01B3);

        hash ^= self.phase_transitions as u64;
        hash = hash.wrapping_mul(0x100_0000_01B3);

        hash ^= self.fractures_triggered as u64;
        hash = hash.wrapping_mul(0x100_0000_01B3);

        hash
    }
}
