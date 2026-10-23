//! Axiom Voxel Map (.axvm) Container & Procedural Destruction Map Generator.
//!
//! Provides 64B L1-aligned binary serialization for 3D destructible voxel maps,
//! cryptographic FNV-1a integrity validation, and deterministic procedural terrain presets.

use core::hash::Hasher;
use axiom_core::hash::Fnv1aHasher;
use axiom_core::voxel_destruct::{
    VoxelVolume16, VOXEL_AIR, VOXEL_ARMOR, VOXEL_DIM, VOXEL_METAL, VOXEL_MOLTEN, VOXEL_STONE,
    VOXEL_TOTAL,
};

pub const AXVM_MAGIC: [u8; 4] = *b"AXVM";
pub const AXVM_VERSION: u32 = 1;

/// Error types during voxel map parsing or serialization.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AxvmError {
    InvalidMagic,
    UnsupportedVersion(u32),
    CorruptedHeader,
    PayloadSizeMismatch { expected: usize, actual: usize },
    ChecksumMismatch { expected: u64, actual: u64 },
}

/// 64-byte L1 Cache Aligned Voxel Map Header.
#[repr(C, align(64))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AxvmHeader {
    pub magic: [u8; 4],             // 4B: b"AXVM"
    pub version: u32,               // 4B: format revision
    pub dim_x: u16,                 // 2B: 16
    pub dim_y: u16,                 // 2B: 16
    pub dim_z: u16,                 // 2B: 16
    pub preset_id: u16,             // 2B: preset tag
    pub solid_voxels: u32,          // 4B: count of solid voxels
    pub damaged_count: u32,         // 4B: carved/damaged count
    pub checksum: u64,              // 8B: 64-bit FNV-1a payload checksum
    pub created_timestamp: u64,     // 8B: deterministic unix epoch or seed
    pub name: [u8; 24],             // 24B: UTF-8 name tag (zero-padded, brings total to 64B)
}

const _: () = assert!(core::mem::size_of::<AxvmHeader>() == 64);
const _: () = assert!(core::mem::align_of::<AxvmHeader>() == 64);

/// Built-in procedural terrain preset archetypes.
#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoxelPreset {
    FlatArena = 0,
    CrateredArena = 1,
    FortifiedBunker = 2,
    MonolithRuins = 3,
    VolcanicVent = 4,
}

impl VoxelPreset {
    pub const fn name(&self) -> &'static str {
        match self {
            Self::FlatArena => "Flat Training Arena",
            Self::CrateredArena => "Battle-Scarred Crater Arena",
            Self::FortifiedBunker => "Fortified Military Bunker",
            Self::MonolithRuins => "Ancient Monolith Ruins",
            Self::VolcanicVent => "Volcanic Magma Vents",
        }
    }
}

/// Generates a procedural 3D destructible voxel map adhering to 0-float determinism.
pub fn generate_voxel_preset(preset: VoxelPreset, seed: u64) -> VoxelVolume16 {
    let mut vol = VoxelVolume16::new_empty();

    match preset {
        VoxelPreset::FlatArena => {
            // Solid stone floor at lower half (y: 0..8)
            for y in 0..8 {
                for z in 0..VOXEL_DIM {
                    for x in 0..VOXEL_DIM {
                        vol.set(x, y, z, VOXEL_STONE);
                    }
                }
            }
        }
        VoxelPreset::CrateredArena => {
            // Solid stone floor at y: 0..10 with reinforced metal base
            for y in 0..10 {
                for z in 0..VOXEL_DIM {
                    for x in 0..VOXEL_DIM {
                        let mat = if y <= 2 { VOXEL_METAL } else { VOXEL_STONE };
                        vol.set(x, y, z, mat);
                    }
                }
            }
            // Carve primary impact crater at center (8, 8)
            vol.carve_crater(8, 8, 4, 6);
            // Secondary peripheral impact blast
            let off_x = ((seed % 5) as i16) + 3;
            let off_z = (((seed >> 4) % 5) as i16) + 3;
            vol.carve_sphere(off_x, 9, off_z, 3, VOXEL_AIR);
            vol.carve_sphere(off_x, 7, off_z, 2, VOXEL_MOLTEN);
        }
        VoxelPreset::FortifiedBunker => {
            // Heavy armor bunker foundation (y: 0..3)
            for y in 0..4 {
                for z in 0..VOXEL_DIM {
                    for x in 0..VOXEL_DIM {
                        vol.set(x, y, z, VOXEL_ARMOR);
                    }
                }
            }
            // Perimeter walls with pillbox slits
            for y in 4..12 {
                for z in 2..14 {
                    for x in 2..14 {
                        let is_wall = x == 2 || x == 13 || z == 2 || z == 13;
                        let is_slit = y == 7 && (x == 7 || x == 8 || z == 7 || z == 8);
                        if is_wall && !is_slit {
                            vol.set(x, y, z, VOXEL_METAL);
                        }
                    }
                }
            }
            // Bunker roof slab
            for z in 2..14 {
                for x in 2..14 {
                    vol.set(x, 12, z, VOXEL_ARMOR);
                }
            }
        }
        VoxelPreset::MonolithRuins => {
            // Stone ground (y: 0..6)
            for y in 0..6 {
                for z in 0..VOXEL_DIM {
                    for x in 0..VOXEL_DIM {
                        vol.set(x, y, z, VOXEL_STONE);
                    }
                }
            }
            // 4 Corner Monolith Pillars
            let pillars = [(3, 3), (12, 3), (3, 12), (12, 12)];
            for &(px, pz) in &pillars {
                for y in 6..14 {
                    vol.set(px, y, pz, VOXEL_STONE);
                    vol.set(px + 1, y, pz, VOXEL_STONE);
                    vol.set(px, y, pz + 1, VOXEL_STONE);
                    vol.set(px + 1, y, pz + 1, VOXEL_STONE);
                }
            }
            // Fallen broken monolith pillar across center
            for i in 4..12 {
                vol.set(i, 6, 7, VOXEL_STONE);
                vol.set(i, 7, 7, VOXEL_STONE);
            }
        }
        VoxelPreset::VolcanicVent => {
            // Dark stone ground
            for y in 0..8 {
                for z in 0..VOXEL_DIM {
                    for x in 0..VOXEL_DIM {
                        vol.set(x, y, z, VOXEL_STONE);
                    }
                }
            }
            // Central magma vent chasm with incandescent molten slag
            for x in 0..VOXEL_DIM {
                let fissure_z = 7 + ((x as i16 * 3 + (seed as i16 % 5)) % 3) as usize;
                vol.set(x, 7, fissure_z, VOXEL_MOLTEN);
                vol.set(x, 6, fissure_z, VOXEL_MOLTEN);
                vol.set(x, 5, fissure_z, VOXEL_MOLTEN);
            }
        }
    }

    vol
}

/// Serializes a voxel volume into the binary `.axvm` container.
pub fn serialize_axvm(name: &str, preset_id: u16, volume: &VoxelVolume16, timestamp: u64) -> Vec<u8> {
    let mut out = Vec::with_capacity(64 + VOXEL_TOTAL);

    let mut name_buf = [0u8; 24];
    let name_bytes = name.as_bytes();
    let copy_len = name_bytes.len().min(23);
    name_buf[..copy_len].copy_from_slice(&name_bytes[..copy_len]);

    let mut hasher = Fnv1aHasher::new();
    hasher.write(&volume.data);
    let checksum = hasher.finish();

    let header = AxvmHeader {
        magic: AXVM_MAGIC,
        version: AXVM_VERSION,
        dim_x: VOXEL_DIM as u16,
        dim_y: VOXEL_DIM as u16,
        dim_z: VOXEL_DIM as u16,
        preset_id,
        solid_voxels: volume.total_solid,
        damaged_count: volume.damaged_count,
        checksum,
        created_timestamp: timestamp,
        name: name_buf,
    };

    // Safety: AxvmHeader is exact 64-byte POD repr(C)
    let header_bytes = unsafe {
        core::slice::from_raw_parts(
            &header as *const AxvmHeader as *const u8,
            core::mem::size_of::<AxvmHeader>(),
        )
    };
    out.extend_from_slice(header_bytes);
    out.extend_from_slice(&volume.data);

    out
}

/// Deserializes a binary `.axvm` container into `AxvmHeader` and `VoxelVolume16`.
pub fn deserialize_axvm(bytes: &[u8]) -> Result<(AxvmHeader, VoxelVolume16), AxvmError> {
    if bytes.len() < 64 {
        return Err(AxvmError::CorruptedHeader);
    }

    let header = unsafe {
        let ptr = bytes.as_ptr() as *const AxvmHeader;
        core::ptr::read_unaligned(ptr)
    };

    if header.magic != AXVM_MAGIC {
        return Err(AxvmError::InvalidMagic);
    }
    if header.version != AXVM_VERSION {
        return Err(AxvmError::UnsupportedVersion(header.version));
    }
    if header.dim_x != 16 || header.dim_y != 16 || header.dim_z != 16 {
        return Err(AxvmError::CorruptedHeader);
    }

    let payload = &bytes[64..];
    if payload.len() != VOXEL_TOTAL {
        return Err(AxvmError::PayloadSizeMismatch {
            expected: VOXEL_TOTAL,
            actual: payload.len(),
        });
    }

    let mut hasher = Fnv1aHasher::new();
    hasher.write(payload);
    let computed_checksum = hasher.finish();
    if computed_checksum != header.checksum {
        return Err(AxvmError::ChecksumMismatch {
            expected: header.checksum,
            actual: computed_checksum,
        });
    }

    let mut volume = VoxelVolume16::new_empty();
    volume.data.copy_from_slice(payload);
    volume.total_solid = header.solid_voxels;
    volume.damaged_count = header.damaged_count;

    Ok((header, volume))
}
