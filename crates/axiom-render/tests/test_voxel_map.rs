use axiom_render::voxel_map::{
    deserialize_axvm, generate_voxel_preset, serialize_axvm, AxvmError, AxvmHeader, VoxelPreset,
    AXVM_MAGIC, AXVM_VERSION,
};
use core::mem::{align_of, size_of};

#[test]
fn test_axvm_header_geometry_and_alignment() {
    assert_eq!(size_of::<AxvmHeader>(), 64);
    assert_eq!(align_of::<AxvmHeader>(), 64);
    assert_eq!(AXVM_MAGIC, *b"AXVM");
    assert_eq!(AXVM_VERSION, 1);
}

#[test]
fn test_procedural_voxel_presets_generation() {
    let presets = [
        VoxelPreset::FlatArena,
        VoxelPreset::CrateredArena,
        VoxelPreset::FortifiedBunker,
        VoxelPreset::MonolithRuins,
        VoxelPreset::VolcanicVent,
    ];

    for preset in presets {
        let vol = generate_voxel_preset(preset, 1337);
        assert!(vol.total_solid > 0);
        // Determinism test: same seed produces identical voxel data
        let vol2 = generate_voxel_preset(preset, 1337);
        assert_eq!(vol.data, vol2.data);
    }
}

#[test]
fn test_axvm_serialization_roundtrip_and_checksum() {
    let vol = generate_voxel_preset(VoxelPreset::FortifiedBunker, 999);
    let bytes = serialize_axvm("Bunker Alpha", VoxelPreset::FortifiedBunker as u16, &vol, 1729700000);

    assert_eq!(bytes.len(), 64 + 4096);

    let (header, decoded_vol) = deserialize_axvm(&bytes).expect("Failed to deserialize valid AXVM");
    assert_eq!(header.magic, AXVM_MAGIC);
    assert_eq!(header.version, 1);
    assert_eq!(header.dim_x, 16);
    assert_eq!(header.dim_y, 16);
    assert_eq!(header.dim_z, 16);
    assert_eq!(decoded_vol.data, vol.data);
    assert_eq!(decoded_vol.total_solid, vol.total_solid);

    // Tampering test: corrupt a single byte
    let mut corrupted = bytes.clone();
    corrupted[100] ^= 0xFF;
    let err = deserialize_axvm(&corrupted);
    assert!(matches!(err, Err(AxvmError::ChecksumMismatch { .. })));
}
