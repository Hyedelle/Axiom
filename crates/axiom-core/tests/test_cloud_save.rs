//! Unit and Integration Tests for 64-Byte L1 Aligned Steam Cloud Save Engine.

use axiom_core::cloud_save::{
    resolve_sync_conflict, CloudSaveError, CloudSaveHeader, CloudSavePackage, SyncResolution,
    CLOUD_SAVE_MAGIC, CLOUD_SAVE_VERSION,
};

#[test]
fn test_cloud_save_header_alignment() {
    assert_eq!(core::mem::size_of::<CloudSaveHeader>(), 64);
    assert_eq!(core::mem::align_of::<CloudSaveHeader>(), 64);
}

#[test]
fn test_cloud_save_roundtrip() {
    let dummy_state_payload = b"{\"season\":3,\"league_wins\":7,\"parts\":[\"vulcan_arm\",\"titan_core\"]}";
    let pkg = CloudSavePackage::new(
        0, // quicksave slot
        1729699200, // Unix epoch timestamp
        0xAABBCCDDEEFF0011, // State hash
        7, // Progress wins
        dummy_state_payload.to_vec(),
    );

    assert!(pkg.verify_checksum());
    assert_eq!(pkg.header.magic, CLOUD_SAVE_MAGIC);
    assert_eq!(pkg.header.version, CLOUD_SAVE_VERSION);
    assert_eq!(pkg.header.payload_bytes, dummy_state_payload.len() as u32);

    let bytes = pkg.serialize();
    assert_eq!(bytes.len(), 64 + dummy_state_payload.len());

    let restored = CloudSavePackage::deserialize(&bytes).expect("Deserialization failed");
    assert_eq!(restored.header, pkg.header);
    assert_eq!(restored.payload, dummy_state_payload);
    assert!(restored.verify_checksum());
}

#[test]
fn test_cloud_save_corruption_detection() {
    let payload = b"critical_deterministic_save_data";
    let pkg = CloudSavePackage::new(1, 1000, 42, 1, payload.to_vec());
    let mut bytes = pkg.serialize();

    // 1. Corrupt payload byte -> ChecksumMismatch
    bytes[65] ^= 0xFF;
    match CloudSavePackage::deserialize(&bytes) {
        Err(CloudSaveError::ChecksumMismatch { .. }) => {}
        other => panic!("Expected ChecksumMismatch, got {:?}", other),
    }

    // 2. Corrupt magic -> InvalidMagic
    bytes[24] = b'Z'; // magic offset is 24
    match CloudSavePackage::deserialize(&bytes) {
        Err(CloudSaveError::InvalidMagic) => {}
        other => panic!("Expected InvalidMagic, got {:?}", other),
    }
}

#[test]
fn test_cloud_save_conflict_resolution() {
    let payload = b"state_v1";
    let pkg_local_newer = CloudSavePackage::new(0, 2000, 100, 5, payload.to_vec());
    let pkg_remote_older = CloudSavePackage::new(0, 1000, 90, 3, payload.to_vec());

    assert_eq!(
        resolve_sync_conflict(&pkg_local_newer.header, &pkg_remote_older.header),
        SyncResolution::LocalNewer
    );

    let pkg_remote_newer = CloudSavePackage::new(0, 3000, 110, 6, payload.to_vec());
    assert_eq!(
        resolve_sync_conflict(&pkg_local_newer.header, &pkg_remote_newer.header),
        SyncResolution::RemoteNewer
    );

    // Identical
    assert_eq!(
        resolve_sync_conflict(&pkg_local_newer.header, &pkg_local_newer.header),
        SyncResolution::Identical
    );
}
