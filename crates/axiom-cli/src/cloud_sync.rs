//! Steam Auto-Cloud Synchronization & Save Storage Manager.
//!
//! Provides atomic, crash-resilient local and cloud save management,
//! directory synchronization, and timestamp conflict resolution.

use axiom_core::cloud_save::{
    resolve_sync_conflict, CloudSavePackage, SyncResolution,
};
use std::fs;
use std::path::{Path, PathBuf};

/// Metadata information about a save file on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveFileInfo {
    pub path: PathBuf,
    pub file_name: String,
    pub is_cloud_pkg: bool,
    pub slot: u32,
    pub timestamp: u64,
    pub state_hash: u64,
    pub progress: u32,
    pub size_bytes: u64,
    pub checksum: u64,
    pub is_valid: bool,
}

/// Result report from a directory sync operation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SyncReport {
    pub local_updated: usize,
    pub cloud_updated: usize,
    pub identical: usize,
    pub errors: Vec<String>,
}

/// Steam Auto-Cloud and Local Save Storage Synchronization Manager.
pub struct CloudSyncManager;

impl CloudSyncManager {
    /// Atomically writes data to `path` using a temporary file and atomic rename.
    pub fn atomic_write(path: &Path, data: &[u8]) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let tmp_path = path.with_extension("tmp");
        fs::write(&tmp_path, data)?;
        fs::rename(&tmp_path, path)?;
        Ok(())
    }

    /// Scans a directory and parses all `.axcs` (Cloud Save) and `.mimic` / `.axiom` files.
    pub fn list_saves(dir: &Path) -> Vec<SaveFileInfo> {
        let mut results = Vec::new();
        if !dir.exists() {
            return results;
        }

        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_file() {
                    let file_name = p.file_name().unwrap_or_default().to_string_lossy().to_string();
                    let ext = p.extension().map(|s| s.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
                    let size_bytes = entry.metadata().map(|m| m.len()).unwrap_or(0);

                    if ext == "axcs" {
                        if let Ok(bytes) = fs::read(&p) {
                            match CloudSavePackage::deserialize(&bytes) {
                                Ok(pkg) => {
                                    results.push(SaveFileInfo {
                                        path: p,
                                        file_name,
                                        is_cloud_pkg: true,
                                        slot: pkg.header.save_slot,
                                        timestamp: pkg.header.timestamp_epoch_sec,
                                        state_hash: pkg.header.state_hash,
                                        progress: pkg.header.player_progress,
                                        size_bytes,
                                        checksum: pkg.header.data_checksum,
                                        is_valid: true,
                                    });
                                }
                                Err(_) => {
                                    results.push(SaveFileInfo {
                                        path: p,
                                        file_name,
                                        is_cloud_pkg: true,
                                        slot: 0,
                                        timestamp: 0,
                                        state_hash: 0,
                                        progress: 0,
                                        size_bytes,
                                        checksum: 0,
                                        is_valid: false,
                                    });
                                }
                            }
                        }
                    } else if ext == "mimic" || ext == "axiom" {
                        // Legacy JSON or binary save file
                        results.push(SaveFileInfo {
                            path: p,
                            file_name,
                            is_cloud_pkg: false,
                            slot: 0,
                            timestamp: 0,
                            state_hash: 0,
                            progress: 0,
                            size_bytes,
                            checksum: 0,
                            is_valid: true,
                        });
                    }
                }
            }
        }

        // Sort by timestamp descending
        results.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        results
    }

    /// Synchronizes save files between local directory and cloud remote storage directory.
    pub fn sync_directories(local_dir: &Path, cloud_dir: &Path) -> SyncReport {
        let mut report = SyncReport::default();

        let _ = fs::create_dir_all(local_dir);
        let _ = fs::create_dir_all(cloud_dir);

        let local_saves = Self::list_saves(local_dir);
        let cloud_saves = Self::list_saves(cloud_dir);

        // Check local files against cloud
        for local in &local_saves {
            if !local.is_valid || !local.is_cloud_pkg {
                continue;
            }

            let target_cloud_path = cloud_dir.join(&local.file_name);
            let matching_cloud = cloud_saves.iter().find(|c| c.file_name == local.file_name);

            match matching_cloud {
                Some(cloud) if cloud.is_valid => {
                    let local_bytes = match fs::read(&local.path) {
                        Ok(b) => b,
                        Err(e) => {
                            report.errors.push(format!("Failed to read local {:?}: {}", local.path, e));
                            continue;
                        }
                    };
                    let cloud_bytes = match fs::read(&cloud.path) {
                        Ok(b) => b,
                        Err(e) => {
                            report.errors.push(format!("Failed to read cloud {:?}: {}", cloud.path, e));
                            continue;
                        }
                    };

                    let local_pkg = match CloudSavePackage::deserialize(&local_bytes) {
                        Ok(p) => p,
                        Err(_) => continue,
                    };
                    let cloud_pkg = match CloudSavePackage::deserialize(&cloud_bytes) {
                        Ok(p) => p,
                        Err(_) => continue,
                    };

                    match resolve_sync_conflict(&local_pkg.header, &cloud_pkg.header) {
                        SyncResolution::Identical => {
                            report.identical += 1;
                        }
                        SyncResolution::LocalNewer => {
                            // Mirror local to cloud
                            if Self::atomic_write(&target_cloud_path, &local_bytes).is_ok() {
                                report.cloud_updated += 1;
                            }
                        }
                        SyncResolution::RemoteNewer => {
                            // Mirror cloud to local
                            if Self::atomic_write(&local.path, &cloud_bytes).is_ok() {
                                report.local_updated += 1;
                            }
                        }
                    }
                }
                None => {
                    // Cloud missing file: mirror local to cloud
                    if let Ok(bytes) = fs::read(&local.path) {
                        if Self::atomic_write(&target_cloud_path, &bytes).is_ok() {
                            report.cloud_updated += 1;
                        }
                    }
                }
                _ => {}
            }
        }

        // Check for cloud files missing locally
        for cloud in &cloud_saves {
            if !cloud.is_valid || !cloud.is_cloud_pkg {
                continue;
            }
            let target_local_path = local_dir.join(&cloud.file_name);
            if !target_local_path.exists() {
                if let Ok(bytes) = fs::read(&cloud.path) {
                    if Self::atomic_write(&target_local_path, &bytes).is_ok() {
                        report.local_updated += 1;
                    }
                }
            }
        }

        report
    }
}
