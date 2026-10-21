//! # Deterministic Binary World Serialization & Instant Save/Load Engine (Phase 4)
//!
//! Provides zero-pointer, POD byte-stream serialization for 100k NPCs,
//! procedural chunk modifications, and quest journals with sub-millisecond restore speeds.

extern crate alloc;

use alloc::vec::Vec;

use crate::chunk::{Chunk, ChunkCoord, ChunkManager, DEFAULT_CHUNK_SIZE, DEFAULT_MAX_CHUNKS};
use crate::npc_crowd::{Npc, NpcCrowdPool, DEFAULT_NPC_TABLE_CAPACITY};
use crate::quest::{QuestJournal, DEFAULT_MAX_ACTIVE_QUESTS, DEFAULT_MAX_COMPLETED_QUESTS};

pub const AXM_MAGIC: [u8; 4] = *b"AXM1";
pub const AXM_VERSION: u32 = 1;

/// Binary serialization errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SerializationError {
    InvalidMagic,
    UnsupportedVersion(u32),
    BufferTooSmall,
    CorruptedData,
}

/// 64-byte aligned header encoding metadata for an Axiom save archive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C, align(64))]
pub struct SaveHeader {
    pub magic: [u8; 4],
    pub version: u32,
    pub world_seed: u64,
    pub tick: u64,
    pub state_hash: u64,
    pub active_npc_count: u32,
    pub loaded_chunks_count: u32,
    pub quest_xp_earned: u32,
    pub _padding: [u8; 20],
}

impl Default for SaveHeader {
    fn default() -> Self {
        Self {
            magic: AXM_MAGIC,
            version: AXM_VERSION,
            world_seed: 0,
            tick: 0,
            state_hash: 0,
            active_npc_count: 0,
            loaded_chunks_count: 0,
            quest_xp_earned: 0,
            _padding: [0; 20],
        }
    }
}

/// Stateless binary serialization engine.
pub struct WorldSerializer;

impl WorldSerializer {
    /// Serializes entire open-world state (100k NPCs + Chunks + Quest Journal) into a byte vector.
    pub fn save_world<const SLABS: usize>(
        world_seed: u64,
        tick: u64,
        state_hash: u64,
        npcs: &NpcCrowdPool<SLABS>,
        chunks: &ChunkManager<DEFAULT_CHUNK_SIZE, DEFAULT_MAX_CHUNKS>,
        quests: &QuestJournal<DEFAULT_MAX_ACTIVE_QUESTS, DEFAULT_MAX_COMPLETED_QUESTS>,
    ) -> Vec<u8> {
        let active_npc_count = npcs.len() as u32;
        let loaded_chunks_count = chunks.loaded_count() as u32;

        let header = SaveHeader {
            magic: AXM_MAGIC,
            version: AXM_VERSION,
            world_seed,
            tick,
            state_hash,
            active_npc_count,
            loaded_chunks_count,
            quest_xp_earned: quests.total_xp_earned,
            _padding: [0; 20],
        };

        let header_size = core::mem::size_of::<SaveHeader>();
        let npc_size = core::mem::size_of::<Npc>();
        let chunk_size = core::mem::size_of::<Chunk<DEFAULT_CHUNK_SIZE>>();
        let quest_size = core::mem::size_of::<QuestJournal<DEFAULT_MAX_ACTIVE_QUESTS, DEFAULT_MAX_COMPLETED_QUESTS>>();

        let estimated_capacity = header_size
            + (active_npc_count as usize * npc_size)
            + (loaded_chunks_count as usize * (chunk_size + 4))
            + quest_size;

        let mut buffer = Vec::with_capacity(estimated_capacity);

        // 1. Write Header
        let header_bytes = unsafe {
            core::slice::from_raw_parts(
                &header as *const SaveHeader as *const u8,
                header_size,
            )
        };
        buffer.extend_from_slice(header_bytes);

        // 2. Write Active NPCs (POD serialization)
        for slab in &npcs.slabs {
            if slab.active_count == 0 {
                continue;
            }
            for npc in &slab.npcs {
                if npc.is_active() {
                    let npc_bytes = unsafe {
                        core::slice::from_raw_parts(
                            npc as *const Npc as *const u8,
                            npc_size,
                        )
                    };
                    buffer.extend_from_slice(npc_bytes);
                }
            }
        }

        // 3. Write Loaded Chunks (Coordinate + Tiles)
        for slot in &chunks.slots {
            if slot.active {
                // Write Chunk Coordinate (i32, i32 = 8 bytes)
                buffer.extend_from_slice(&slot.chunk.coord.x.to_le_bytes());
                buffer.extend_from_slice(&slot.chunk.coord.y.to_le_bytes());
                // Write 32x32 Tiles (1024 bytes)
                let tiles_bytes = unsafe {
                    core::slice::from_raw_parts(
                        slot.chunk.tiles.as_ptr() as *const u8,
                        DEFAULT_CHUNK_SIZE * DEFAULT_CHUNK_SIZE,
                    )
                };
                buffer.extend_from_slice(tiles_bytes);
            }
        }

        // 4. Write Quest Journal (POD slice)
        let quest_bytes = unsafe {
            core::slice::from_raw_parts(
                quests as *const QuestJournal<DEFAULT_MAX_ACTIVE_QUESTS, DEFAULT_MAX_COMPLETED_QUESTS> as *const u8,
                quest_size,
            )
        };
        buffer.extend_from_slice(quest_bytes);

        buffer
    }

    /// Restores entire open-world state from a binary archive with sub-millisecond throughput.
    pub fn load_world<const SLABS: usize>(
        bytes: &[u8],
        npcs: &mut NpcCrowdPool<SLABS>,
        chunks: &mut ChunkManager<DEFAULT_CHUNK_SIZE, DEFAULT_MAX_CHUNKS>,
        quests: &mut QuestJournal<DEFAULT_MAX_ACTIVE_QUESTS, DEFAULT_MAX_COMPLETED_QUESTS>,
    ) -> Result<SaveHeader, SerializationError> {
        let header_size = core::mem::size_of::<SaveHeader>();
        if bytes.len() < header_size {
            return Err(SerializationError::BufferTooSmall);
        }

        // 1. Read & Validate Header
        let header = unsafe { core::ptr::read_unaligned(bytes.as_ptr() as *const SaveHeader) };
        if header.magic != AXM_MAGIC {
            return Err(SerializationError::InvalidMagic);
        }
        if header.version != AXM_VERSION {
            return Err(SerializationError::UnsupportedVersion(header.version));
        }

        let mut offset = header_size;
        let npc_size = core::mem::size_of::<Npc>();

        // 2. Clear & Restore NPCs
        for slab in &mut npcs.slabs {
            *slab = crate::npc_crowd::NpcTable::new();
        }
        npcs.active_count = 0;

        for _ in 0..header.active_npc_count {
            if offset + npc_size > bytes.len() {
                return Err(SerializationError::CorruptedData);
            }
            let npc = unsafe { core::ptr::read_unaligned(bytes[offset..].as_ptr() as *const Npc) };
            offset += npc_size;

            let slab_idx = (npcs.active_count / DEFAULT_NPC_TABLE_CAPACITY).min(SLABS - 1);
            let local_slot = npcs.active_count % DEFAULT_NPC_TABLE_CAPACITY;
            npcs.slabs[slab_idx].npcs[local_slot] = npc;
            npcs.slabs[slab_idx].active_count += 1;
            npcs.active_count += 1;
        }

        // 3. Clear & Restore Chunks
        for slot in &mut chunks.slots {
            slot.active = false;
        }

        let chunk_tiles_len = DEFAULT_CHUNK_SIZE * DEFAULT_CHUNK_SIZE;
        for i in 0..header.loaded_chunks_count as usize {
            if offset + 8 + chunk_tiles_len > bytes.len() {
                return Err(SerializationError::CorruptedData);
            }

            let cx = i32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
            let cy = i32::from_le_bytes(bytes[offset + 4..offset + 8].try_into().unwrap());
            offset += 8;

            if i < DEFAULT_MAX_CHUNKS {
                chunks.slots[i].chunk.coord = ChunkCoord::new(cx, cy);
                unsafe {
                    core::ptr::copy_nonoverlapping(
                        bytes[offset..].as_ptr(),
                        chunks.slots[i].chunk.tiles.as_mut_ptr() as *mut u8,
                        chunk_tiles_len,
                    );
                }
                chunks.slots[i].active = true;
                chunks.slots[i].last_accessed_tick = header.tick;
            }
            offset += chunk_tiles_len;
        }

        // 4. Restore Quest Journal
        let quest_size = core::mem::size_of::<QuestJournal<DEFAULT_MAX_ACTIVE_QUESTS, DEFAULT_MAX_COMPLETED_QUESTS>>();
        if offset + quest_size > bytes.len() {
            return Err(SerializationError::CorruptedData);
        }

        *quests = unsafe {
            core::ptr::read_unaligned(bytes[offset..].as_ptr() as *const QuestJournal<DEFAULT_MAX_ACTIVE_QUESTS, DEFAULT_MAX_COMPLETED_QUESTS>)
        };

        Ok(header)
    }
}
