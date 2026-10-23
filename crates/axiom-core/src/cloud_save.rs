//! Deterministic Steam Cloud Save Container & Synchronization Protocol.
//!
//! Zero-float, 64-byte L1 cache aligned save container with FNV-1a checksums,
//! discrete epoch timestamps, and deterministic two-way conflict resolution.

extern crate alloc;

use alloc::vec::Vec;

pub const CLOUD_SAVE_MAGIC: [u8; 4] = *b"AXCS";
pub const CLOUD_SAVE_VERSION: u32 = 1;

/// Error types occurring during cloud save serialization or validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloudSaveError {
    InvalidMagic,
    UnsupportedVersion(u32),
    CorruptedHeader,
    ChecksumMismatch { expected: u64, actual: u64 },
    UnexpectedEof,
}

/// 64-byte L1 cache aligned cloud save package header.
#[repr(C, align(64))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CloudSaveHeader {
    pub timestamp_epoch_sec: u64, // 8 bytes: Discrete Unix timestamp in seconds
    pub data_checksum: u64,       // 8 bytes: 64-bit FNV-1a checksum of the payload
    pub state_hash: u64,          // 8 bytes: 64-bit simulation state hash
    pub magic: [u8; 4],           // 4 bytes: "AXCS"
    pub version: u32,             // 4 bytes: 1
    pub payload_bytes: u32,       // 4 bytes: Length of payload in bytes
    pub save_slot: u32,           // 4 bytes: Slot index (0: quick, 1..=10: manual)
    pub player_progress: u32,     // 4 bytes: Wins / level / completion metric
    pub _padding: [u8; 20],       // 20 bytes: Strict 64-byte boundary padding
}

const _: () = assert!(core::mem::size_of::<CloudSaveHeader>() == 64);
const _: () = assert!(core::mem::align_of::<CloudSaveHeader>() == 64);

impl Default for CloudSaveHeader {
    fn default() -> Self {
        Self {
            timestamp_epoch_sec: 0,
            data_checksum: 0,
            state_hash: 0,
            magic: CLOUD_SAVE_MAGIC,
            version: CLOUD_SAVE_VERSION,
            payload_bytes: 0,
            save_slot: 0,
            player_progress: 0,
            _padding: [0; 20],
        }
    }
}

/// Computes a standard 64-bit FNV-1a hash over byte slice.
#[inline(always)]
pub fn fnv1a_hash(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xCBF29CE484222325;
    for &b in bytes {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x100000001B3);
    }
    hash
}

/// Synchronized, self-contained cloud save archive package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloudSavePackage {
    pub header: CloudSaveHeader,
    pub payload: Vec<u8>,
}

impl CloudSavePackage {
    /// Creates a new cloud save package with automatic FNV-1a checksumming.
    pub fn new(
        save_slot: u32,
        timestamp_epoch_sec: u64,
        state_hash: u64,
        player_progress: u32,
        payload: Vec<u8>,
    ) -> Self {
        let checksum = fnv1a_hash(&payload);
        let payload_bytes = payload.len() as u32;

        let header = CloudSaveHeader {
            timestamp_epoch_sec,
            data_checksum: checksum,
            state_hash,
            magic: CLOUD_SAVE_MAGIC,
            version: CLOUD_SAVE_VERSION,
            payload_bytes,
            save_slot,
            player_progress,
            _padding: [0; 20],
        };

        Self { header, payload }
    }

    /// Verifies if internal payload matches the recorded FNV-1a checksum.
    #[inline(always)]
    pub fn verify_checksum(&self) -> bool {
        fnv1a_hash(&self.payload) == self.header.data_checksum
    }

    /// Serializes header and payload into a single contiguous byte buffer.
    pub fn serialize(&self) -> Vec<u8> {
        let header_bytes = unsafe {
            core::slice::from_raw_parts(
                &self.header as *const CloudSaveHeader as *const u8,
                core::mem::size_of::<CloudSaveHeader>(),
            )
        };

        let mut buffer = Vec::with_capacity(header_bytes.len() + self.payload.len());
        buffer.extend_from_slice(header_bytes);
        buffer.extend_from_slice(&self.payload);
        buffer
    }

    /// Deserializes a byte buffer into a verified CloudSavePackage.
    pub fn deserialize(bytes: &[u8]) -> Result<Self, CloudSaveError> {
        let header_size = core::mem::size_of::<CloudSaveHeader>();
        if bytes.len() < header_size {
            return Err(CloudSaveError::UnexpectedEof);
        }

        let mut header = CloudSaveHeader::default();
        unsafe {
            let dest = &mut header as *mut CloudSaveHeader as *mut u8;
            core::ptr::copy_nonoverlapping(bytes.as_ptr(), dest, header_size);
        }

        if header.magic != CLOUD_SAVE_MAGIC {
            return Err(CloudSaveError::InvalidMagic);
        }

        if header.version != CLOUD_SAVE_VERSION {
            return Err(CloudSaveError::UnsupportedVersion(header.version));
        }

        let expected_payload_end = header_size + header.payload_bytes as usize;
        if bytes.len() < expected_payload_end {
            return Err(CloudSaveError::UnexpectedEof);
        }

        let payload = bytes[header_size..expected_payload_end].to_vec();
        let actual_checksum = fnv1a_hash(&payload);

        if actual_checksum != header.data_checksum {
            return Err(CloudSaveError::ChecksumMismatch {
                expected: header.data_checksum,
                actual: actual_checksum,
            });
        }

        Ok(Self { header, payload })
    }
}

/// Cloud sync resolution decisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncResolution {
    /// Local file has a newer timestamp or identical content.
    LocalNewer,
    /// Remote/Cloud file has a newer timestamp.
    RemoteNewer,
    /// Both copies are bit-for-bit identical in checksum and state hash.
    Identical,
}

/// Deterministically resolves sync priority between local and remote/cloud headers.
pub fn resolve_sync_conflict(local: &CloudSaveHeader, remote: &CloudSaveHeader) -> SyncResolution {
    if local.data_checksum == remote.data_checksum && local.state_hash == remote.state_hash {
        return SyncResolution::Identical;
    }

    if local.timestamp_epoch_sec > remote.timestamp_epoch_sec {
        SyncResolution::LocalNewer
    } else if remote.timestamp_epoch_sec > local.timestamp_epoch_sec {
        SyncResolution::RemoteNewer
    } else if local.player_progress >= remote.player_progress {
        SyncResolution::LocalNewer
    } else {
        SyncResolution::RemoteNewer
    }
}
