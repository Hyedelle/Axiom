//! Deterministic hashing implementation (FNV-1a 64-bit).
//!
//! Unlike standard library `DefaultHasher` which randomizes its keys per process,
//! `Fnv1aHasher` produces bit-for-bit identical hashes across any machine, process, or OS.

use core::hash::{Hash, Hasher};

pub const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
pub const FNV_PRIME: u64 = 0x00000100000001b3;

#[derive(Debug, Clone, Copy)]
pub struct Fnv1aHasher {
    state: u64,
}

impl Default for Fnv1aHasher {
    #[inline(always)]
    fn default() -> Self {
        Self::new()
    }
}

impl Fnv1aHasher {
    #[inline(always)]
    pub const fn new() -> Self {
        Self {
            state: FNV_OFFSET_BASIS,
        }
    }
}

impl Hasher for Fnv1aHasher {
    #[inline(always)]
    fn finish(&self) -> u64 {
        self.state
    }

    #[inline(always)]
    fn write(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.state ^= byte as u64;
            self.state = self.state.wrapping_mul(FNV_PRIME);
        }
    }
}

/// Compute a guaranteed-deterministic 64-bit hash of any `Hash` value.
#[inline]
pub fn deterministic_hash<T: Hash + ?Sized>(val: &T) -> u64 {
    let mut hasher = Fnv1aHasher::new();
    val.hash(&mut hasher);
    hasher.finish()
}
