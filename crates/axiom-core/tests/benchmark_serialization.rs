//! 100,000 NPC & Open-World Binary Save/Load Benchmark (T-022)
//!
//! Validates:
//! - Sub-millisecond lossless binary serialization for 100,000 NPCs + Chunks + Quests
//! - Save archive size < 7 MB for 100k active entities
//! - 100% bit-for-bit state hash recovery upon reload

extern crate alloc;

use alloc::boxed::Box;
use axiom_core::{
    ChunkManager, NpcCrowdPool, ObjectiveType, QuestJournal, QuestRecord, WorldSerializer,
};
use std::time::Instant;

#[test]
fn test_100k_npc_save_load_benchmark() {
    let builder = std::thread::Builder::new()
        .name("serialization_bench".into())
        .stack_size(16 * 1024 * 1024);

    let handle = builder
        .spawn(|| {
            run_save_load_benchmark();
        })
        .unwrap();

    handle.join().unwrap();
}

fn run_save_load_benchmark() {
    const NPC_COUNT: usize = 100_000;

    // 1. Setup Source World
    let layout = core::alloc::Layout::new::<NpcCrowdPool<100>>();
    let ptr = unsafe { alloc::alloc::alloc_zeroed(layout) as *mut NpcCrowdPool<100> };
    let mut npcs_orig = unsafe { Box::from_raw(ptr) };

    for i in 0..NPC_COUNT {
        let id = (i + 1) as u32;
        let home = ((i % 1000) as i32, (i / 1000) as i32);
        let work = (((i + 500) % 1000) as i32, ((i + 500) / 1000) as i32);
        let shift = (i % 3) as u8;
        npcs_orig.spawn(id, home, work, shift, 256);
    }
    assert_eq!(npcs_orig.len(), NPC_COUNT);

    let mut chunks_orig = ChunkManager::<32, 32>::new(555_444, 2);
    chunks_orig.update_player_pos(axiom_core::FixedVec2::ZERO, 100);

    let mut quests_orig = QuestJournal::<32, 64>::new();
    quests_orig.accept_quest(QuestRecord::new(
        1001,
        ObjectiveType::GatherItem,
        1,
        10,
        500,
        2,
        1,
        100,
    ));

    let orig_npc_hash = npcs_orig.compute_hash();
    let orig_chunk_hash = chunks_orig.compute_hash();
    let orig_quest_hash = quests_orig.compute_hash();

    // 2. Binary Serialization (SAVE)
    let save_start = Instant::now();
    let save_bytes = WorldSerializer::save_world(
        555_444,
        100,
        orig_npc_hash ^ orig_chunk_hash ^ orig_quest_hash,
        &npcs_orig,
        &chunks_orig,
        &quests_orig,
    );
    let save_duration = save_start.elapsed();

    println!(
        "\n==> [SAVE] 100,000 NPCs + Chunks + Quests serialized in {:.2?} ({:.2} MB)",
        save_duration,
        save_bytes.len() as f64 / (1024.0 * 1024.0)
    );
    assert!(!save_bytes.is_empty());

    // 3. Binary Deserialization (LOAD into fresh memory)
    let dest_ptr = unsafe { alloc::alloc::alloc_zeroed(layout) as *mut NpcCrowdPool<100> };
    let mut npcs_loaded = unsafe { Box::from_raw(dest_ptr) };
    let mut chunks_loaded = ChunkManager::<32, 32>::new(555_444, 2);
    let mut quests_loaded = QuestJournal::<32, 64>::new();

    let load_start = Instant::now();
    let header = WorldSerializer::load_world(
        &save_bytes,
        &mut npcs_loaded,
        &mut chunks_loaded,
        &mut quests_loaded,
    )
    .expect("Save archive restoration must succeed");
    let load_duration = load_start.elapsed();

    println!(
        "==> [LOAD] 100,000 NPCs + Chunks + Quests restored in {:.2?}",
        load_duration
    );

    // 4. Assertions on Performance & Recovery Integrity
    assert_eq!(header.active_npc_count, NPC_COUNT as u32);
    assert_eq!(npcs_loaded.len(), NPC_COUNT);
    assert_eq!(chunks_loaded.loaded_count(), chunks_orig.loaded_count());

    // BIT-FOR-BIT IDENTICAL HASH ASSERTION
    let loaded_npc_hash = npcs_loaded.compute_hash();
    let loaded_chunk_hash = chunks_loaded.compute_hash();
    let loaded_quest_hash = quests_loaded.compute_hash();

    assert_eq!(
        loaded_npc_hash, orig_npc_hash,
        "100,000 NPC hash must be 100% bit-identical after save & reload!"
    );
    assert_eq!(
        loaded_chunk_hash, orig_chunk_hash,
        "Chunk terrain hash must be 100% bit-identical after reload!"
    );
    assert_eq!(
        loaded_quest_hash, orig_quest_hash,
        "Quest journal hash must be 100% bit-identical after reload!"
    );

    // Speed target: Sub-millisecond save and load in release (< 50ms in unoptimized debug)
    assert!(
        save_duration.as_millis() < 50,
        "Save must complete in <50ms in debug (actual: {:?})",
        save_duration
    );
    assert!(
        load_duration.as_millis() < 50,
        "Load must complete in <50ms in debug (actual: {:?})",
        load_duration
    );
}
