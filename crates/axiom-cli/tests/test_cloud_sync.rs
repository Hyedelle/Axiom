//! Integration Tests for Steam Auto-Cloud Synchronization Manager.

use axiom_cli::cloud_sync::CloudSyncManager;
use axiom_core::cloud_save::CloudSavePackage;
use std::fs;
use std::path::Path;

#[test]
fn test_cloud_sync_atomic_write() {
    let test_dir = Path::new("tmp/test_cloud_atomic");
    let _ = fs::remove_dir_all(test_dir);
    fs::create_dir_all(test_dir).unwrap();

    let target_file = test_dir.join("test_save.dat");
    let data = b"deterministic_save_bytes_42";

    CloudSyncManager::atomic_write(&target_file, data).unwrap();
    assert!(target_file.exists());
    assert_eq!(fs::read(&target_file).unwrap(), data);

    let _ = fs::remove_dir_all(test_dir);
}

#[test]
fn test_cloud_sync_two_way_mirroring() {
    let local_dir = Path::new("tmp/test_sync_local");
    let cloud_dir = Path::new("tmp/test_sync_cloud");

    let _ = fs::remove_dir_all(local_dir);
    let _ = fs::remove_dir_all(cloud_dir);

    fs::create_dir_all(local_dir).unwrap();
    fs::create_dir_all(cloud_dir).unwrap();

    // 1. Create a newer save locally
    let local_payload = b"{\"season\":1,\"wins\":3}";
    let pkg_local = CloudSavePackage::new(0, 1729699200, 0x1234, 3, local_payload.to_vec());
    let local_file = local_dir.join("quick.axcs");
    CloudSyncManager::atomic_write(&local_file, &pkg_local.serialize()).unwrap();

    // 2. Sync: local should mirror to cloud
    let report1 = CloudSyncManager::sync_directories(local_dir, cloud_dir);
    assert_eq!(report1.cloud_updated, 1);
    assert_eq!(report1.local_updated, 0);
    assert!(cloud_dir.join("quick.axcs").exists());

    // 3. Second sync: identical, 0 updates
    let report2 = CloudSyncManager::sync_directories(local_dir, cloud_dir);
    assert_eq!(report2.identical, 1);
    assert_eq!(report2.cloud_updated, 0);
    assert_eq!(report2.local_updated, 0);

    // 4. Remote updated with newer timestamp
    let cloud_payload = b"{\"season\":1,\"wins\":5}";
    let pkg_remote = CloudSavePackage::new(0, 1729709200, 0x5678, 5, cloud_payload.to_vec());
    let cloud_file = cloud_dir.join("quick.axcs");
    CloudSyncManager::atomic_write(&cloud_file, &pkg_remote.serialize()).unwrap();

    // 5. Sync: remote should mirror to local
    let report3 = CloudSyncManager::sync_directories(local_dir, cloud_dir);
    assert_eq!(report3.local_updated, 1);
    assert_eq!(report3.cloud_updated, 0);

    // Verify local now has the newer remote payload
    let restored_local = CloudSavePackage::deserialize(&fs::read(&local_file).unwrap()).unwrap();
    assert_eq!(restored_local.payload, cloud_payload);

    let _ = fs::remove_dir_all(local_dir);
    let _ = fs::remove_dir_all(cloud_dir);
}
