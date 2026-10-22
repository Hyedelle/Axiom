//! Phase 3 integration tests: infinite procedural open world, 8-biome PCG,
//! resource vein clustering, and eviction-proof player deltas (T-070).

use axiom_core::{
    sample_world_biome, sample_world_biome_cached, BiomeKind, BiomeNoiseContext, Chunk,
    ChunkCoord, ChunkManager, NpcCrowdPool, QuestJournal, ResourceDeposit, TileType,
    WorldSerializer, BIOME_DEPOSIT_TABLE, VEIN_MAX_RADIUS, VEIN_NO_DISTANCE,
};

fn biome_histogram(seed: u64, origin: i32, extent: i32, stride: i32) -> ([u64; 8], u64) {
    let mut hist = [0u64; 8];
    let mut total = 0u64;
    let mut ctx = BiomeNoiseContext::new();
    let mut wy = origin;
    while wy < origin + extent {
        let mut wx = origin;
        while wx < origin + extent {
            let s = sample_world_biome_cached(wx, wy, seed, &mut ctx);
            hist[s.biome as usize] += 1;
            total += 1;
            wx += stride;
        }
        wy += stride;
    }
    (hist, total)
}

#[test]
fn test_biome_distribution_and_determinism() {
    // 1. Every one of the 8 biomes must occupy a meaningful share of a wide region,
    //    and none may dominate it.
    for seed in [0xB10E_5EEDu64, 7, u64::MAX / 3] {
        let (hist, total) = biome_histogram(seed, -2048, 4096, 8);
        assert_eq!(total, 512 * 512);
        for kind in BiomeKind::ALL {
            let share_permille = hist[kind as usize] * 1000 / total;
            assert!(
                share_permille >= 15,
                "seed {seed:#x}: biome {} covers only {share_permille}‰ ({} / {total})",
                kind.name(),
                hist[kind as usize]
            );
            assert!(
                share_permille <= 450,
                "seed {seed:#x}: biome {} dominates with {share_permille}‰",
                kind.name()
            );
        }
    }

    // 2. Bit-identical determinism: the same seed yields the same sample at every tile,
    //    with or without the row-major memo, and chunks hash identically.
    let seed = 0xDE7E_7E57u64;
    for wy in (-300..300).step_by(7) {
        for wx in (-300..300).step_by(5) {
            assert_eq!(sample_world_biome(wx, wy, seed), sample_world_biome(wx, wy, seed));
        }
    }
    let a = Chunk::<32>::generate(seed, ChunkCoord::new(-17, 9), 1);
    let b = Chunk::<32>::generate(seed, ChunkCoord::new(-17, 9), 999);
    assert_eq!(a.tiles, b.tiles);
    assert_eq!(a.biomes, b.biomes);
    assert_eq!(a.resources, b.resources);
    assert_eq!(a.compute_hash(), b.compute_hash());
    assert_eq!(a.header.primary_biome, b.header.primary_biome);

    // 3. A different seed must change the world.
    let c = Chunk::<32>::generate(seed ^ 0x1234_5678, ChunkCoord::new(-17, 9), 1);
    assert_ne!(a.compute_hash(), c.compute_hash());

    // 4. Chunk biome layer must agree with the single-tile sampler across chunk borders.
    for coord in [ChunkCoord::new(0, 0), ChunkCoord::new(-1, -1), ChunkCoord::new(40, -33)] {
        let chunk = Chunk::<32>::generate(seed, coord, 0);
        for (lx, ly) in [(0usize, 0usize), (31, 0), (0, 31), (31, 31), (15, 16)] {
            let wx = coord.x * 32 + lx as i32;
            let wy = coord.y * 32 + ly as i32;
            let s = sample_world_biome(wx, wy, seed);
            assert_eq!(chunk.tiles[ly][lx], s.tile);
            assert_eq!(chunk.biome_at(lx, ly), Some(s.biome));
            assert_eq!(chunk.deposit_at(lx, ly).map(|d| d.0), Some(s.deposit_kind));
        }
    }
}

#[test]
fn test_resource_vein_clustering() {
    let seed = 0x0DE_C0DE_5EEDu64;
    let origin = -512i32;
    let extent = 1024i32;

    let mut deposit_tiles = 0u64;
    let mut clustered = 0u64;
    let mut kinds_seen = [false; ResourceDeposit::COUNT];
    let mut ctx = BiomeNoiseContext::new();

    for wy in origin..origin + extent {
        for wx in origin..origin + extent {
            let s = sample_world_biome_cached(wx, wy, seed, &mut ctx);
            if s.deposit_kind.is_none() {
                assert_eq!(s.deposit_richness, 0);
                assert_eq!(s.vein_distance, VEIN_NO_DISTANCE);
                continue;
            }

            // Invariants of a deposit tile.
            assert!(s.deposit_richness > 0, "deposit at ({wx},{wy}) has zero richness");
            assert!(s.vein_distance as i32 <= VEIN_MAX_RADIUS);
            assert_ne!(s.tile, TileType::Water, "deposits never sit under water");
            assert!(s.biome.supports_deposits());
            assert!(
                BIOME_DEPOSIT_TABLE[s.biome as usize].contains(&s.deposit_kind),
                "{} is not allowed in {}",
                s.deposit_kind.name(),
                s.biome.name()
            );
            kinds_seen[s.deposit_kind as usize] = true;
            deposit_tiles += 1;

            // Richness decays monotonically away from the vein centre: a neighbour that
            // is closer to the centre is at least as rich.
            let mut has_same_kind_neighbour = false;
            for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                let n = sample_world_biome(wx + dx, wy + dy, seed);
                if n.deposit_kind == s.deposit_kind {
                    has_same_kind_neighbour = true;
                    if n.vein_distance < s.vein_distance {
                        assert!(
                            n.deposit_richness >= s.deposit_richness,
                            "richness must not increase away from the centre at ({wx},{wy})"
                        );
                    }
                }
            }
            if has_same_kind_neighbour {
                clustered += 1;
            }
        }
    }

    assert!(deposit_tiles > 2_000, "only {deposit_tiles} deposit tiles in a 1024x1024 area");
    let clustered_permille = clustered * 1000 / deposit_tiles;
    assert!(
        clustered_permille >= 900,
        "deposits must form veins, only {clustered_permille}‰ have a same-kind neighbour"
    );
    for kind in [
        ResourceDeposit::IronOre,
        ResourceDeposit::CopperOre,
        ResourceDeposit::Coal,
        ResourceDeposit::Stone,
        ResourceDeposit::Uranium,
        ResourceDeposit::CrudeOil,
    ] {
        assert!(kinds_seen[kind as usize], "{} never spawned", kind.name());
    }
    assert!(!kinds_seen[ResourceDeposit::None as usize]);

    // Determinism of the whole deposit layer across two independent sweeps.
    let x = Chunk::<32>::generate(seed, ChunkCoord::new(3, -4), 0);
    let y = Chunk::<32>::generate(seed, ChunkCoord::new(3, -4), 0);
    assert_eq!(x.resources, y.resources);
    assert_eq!(
        x.header.deposit_tile_count as usize,
        x.resources.iter().flatten().filter(|&&r| r >> 5 != 0).count()
    );
}

#[test]
fn test_infinite_chunk_streaming_with_deltas() {
    let seed = 0x5A7E_D317u64;
    let mut mgr = ChunkManager::<32, 32>::new(seed, 2);
    mgr.update_center(ChunkCoord::new(0, 0), 1);

    // 1. Player edits 20 tiles inside the origin chunk, cycling every tile type.
    let edits: Vec<(i64, i64, TileType)> = (0..20)
        .map(|i| {
            let tile = TileType::from_u8((i % 4) as u8);
            (3 + i as i64, 7 + (i as i64 % 5), tile)
        })
        .collect();
    for &(x, y, t) in &edits {
        assert!(mgr.set_tile(x, y, t));
        assert_eq!(mgr.tile_at(x, y), t);
    }
    let origin = mgr.chunk_at(ChunkCoord::new(0, 0)).unwrap();
    assert!(origin.header.dirty);
    assert_eq!(origin.header.modification_count, 20);
    assert_eq!(mgr.deltas.len(), 20);
    assert_eq!(mgr.deltas.count_for_chunk(ChunkCoord::new(0, 0)), 20);
    let edited_hash = mgr.compute_hash();

    // 2. Editing a tile of an *unloaded* far-away chunk is recorded for later.
    let far = (50 * 32 + 9, 50 * 32 + 21);
    let far_tile = if mgr.tile_at(far.0, far.1) == TileType::Rock {
        TileType::Forest
    } else {
        TileType::Rock
    };
    assert!(mgr.chunk_at(ChunkCoord::new(50, 50)).is_none());
    assert!(mgr.set_tile(far.0, far.1, far_tile));
    assert_eq!(mgr.tile_at(far.0, far.1), far_tile);

    // 3. Travel far away: the origin chunk is evicted...
    for tick in 2..=40u64 {
        mgr.update_center(ChunkCoord::new(tick as i32 * 3, tick as i32 * 3), tick);
    }
    assert!(mgr.chunk_at(ChunkCoord::new(0, 0)).is_none());
    assert!(mgr.total_evicted > 0);

    // ...but every edit is still visible through the delta slab.
    for &(x, y, t) in &edits {
        assert_eq!(mgr.tile_at(x, y), t, "unloaded edit at ({x},{y}) lost");
    }

    // 4. Walk through chunk (50, 50): the pre-recorded edit must be applied on load.
    let stats = mgr.update_center(ChunkCoord::new(50, 50), 41);
    assert!(stats.reapplied_deltas >= 1);
    let far_chunk = mgr.chunk_at(ChunkCoord::new(50, 50)).unwrap();
    assert!(far_chunk.header.dirty);
    assert_eq!(far_chunk.tiles[21][9], far_tile);

    // 5. Return to the origin: regenerated chunk == pristine PCG + all 20 edits.
    let stats = mgr.update_center(ChunkCoord::new(0, 0), 42);
    assert_eq!(stats.reapplied_deltas, 20);
    let origin = mgr.chunk_at(ChunkCoord::new(0, 0)).unwrap();
    assert!(origin.header.dirty);
    assert_eq!(origin.header.modification_count, 20);
    let pristine = Chunk::<32>::generate(seed, ChunkCoord::new(0, 0), 0);
    for ly in 0..32 {
        for lx in 0..32 {
            let expected = edits
                .iter()
                .find(|&&(x, y, _)| x as usize == lx && y as usize == ly)
                .map(|&(_, _, t)| t)
                .unwrap_or(pristine.tiles[ly][lx]);
            assert_eq!(origin.tiles[ly][lx], expected, "tile ({lx},{ly}) after reload");
        }
    }
    assert_eq!(origin.biomes, pristine.biomes, "biome layer is never edited");
    assert_eq!(origin.resources, pristine.resources, "resource layer is never edited");
    assert_eq!(
        mgr.chunk_at(ChunkCoord::new(0, 0)).unwrap().compute_hash(),
        {
            let mut m2 = ChunkManager::<32, 32>::new(seed, 2);
            m2.update_center(ChunkCoord::new(0, 0), 1);
            for &(x, y, t) in &edits {
                m2.set_tile(x, y, t);
            }
            m2.chunk_at(ChunkCoord::new(0, 0)).unwrap().compute_hash()
        },
        "evict ➔ regenerate ➔ re-apply must be bit-identical to never evicting"
    );
    let _ = edited_hash;

    // 6. Deltas survive binary save / load, including the one for the evicted chunk.
    let npcs = NpcCrowdPool::<1>::new();
    let quests = QuestJournal::<32, 64>::new();
    let bytes = WorldSerializer::save_world(seed, 42, mgr.compute_hash(), &npcs, &mgr, &quests);
    let mut npcs_loaded = NpcCrowdPool::<1>::new();
    let mut quests_loaded = QuestJournal::<32, 64>::new();
    let mut loaded = ChunkManager::<32, 32>::new(0, 2);
    let header = WorldSerializer::load_world(&bytes, &mut npcs_loaded, &mut loaded, &mut quests_loaded)
        .expect("archive must load");
    assert_eq!(header.world_seed, seed);
    assert_eq!(loaded.world_seed, seed);
    assert_eq!(loaded.loaded_count(), mgr.loaded_count());
    assert_eq!(loaded.deltas.len(), mgr.deltas.len());
    assert_eq!(loaded.compute_hash(), mgr.compute_hash());
    assert_eq!(loaded.tile_at(far.0, far.1), far_tile);
    for &(x, y, t) in &edits {
        assert_eq!(loaded.tile_at(x, y), t);
    }
    for (i, slot) in loaded.slots.iter().enumerate() {
        if slot.active {
            assert_eq!(loaded.find_chunk_index(slot.chunk.header.coord), Some(i));
            let original = mgr.chunk_at(slot.chunk.header.coord).expect("same chunk set");
            assert_eq!(&slot.chunk, original, "chunk {:?} must round-trip bit-exactly", slot.chunk.header.coord);
        }
    }
}

#[test]
fn test_chunk_manager_chebyshev_radius() {
    let seed = 0xC4E8_7u64;
    let radius = 3;
    let mut mgr = ChunkManager::<32, 64>::new(seed, radius);

    let center = ChunkCoord::new(-100, 250);
    let stats = mgr.update_center(center, 1);
    let window = ((2 * radius + 1) * (2 * radius + 1)) as usize;
    assert_eq!(stats.generated_chunks as usize, window);
    assert_eq!(stats.evicted_chunks, 0);
    assert_eq!(stats.loaded_count, window);
    assert_eq!(mgr.loaded_count(), window);

    // Every chunk inside the Chebyshev radius is loaded; the ring just outside is not.
    for dy in -(radius + 1)..=(radius + 1) {
        for dx in -(radius + 1)..=(radius + 1) {
            let c = ChunkCoord::new(center.x + dx, center.y + dy);
            let inside = c.chebyshev_dist(center) <= radius;
            assert_eq!(mgr.chunk_at(c).is_some(), inside, "chunk {c:?} inside={inside}");
            if inside {
                assert_eq!(mgr.chunk_at(c).unwrap().header.coord, c);
            }
        }
    }

    // Diagonal step: 7x7 windows shifted by (1, 1) overlap in 6x6 = 36 chunks.
    let moved = ChunkCoord::new(center.x + 1, center.y + 1);
    let stats = mgr.update_center(moved, 2);
    let overlap = ((2 * radius) * (2 * radius)) as usize;
    assert_eq!(stats.evicted_chunks as usize, window - overlap);
    assert_eq!(stats.generated_chunks as usize, window - overlap);
    assert_eq!(mgr.loaded_count(), window);
    assert_eq!(mgr.total_generated, (window + window - overlap) as u64);
    assert_eq!(mgr.total_evicted, (window - overlap) as u64);

    // Big jump: everything is replaced exactly once.
    let jump = ChunkCoord::new(i32::MAX - 10, i32::MIN + 10);
    let stats = mgr.update_center(jump, 3);
    assert_eq!(stats.evicted_chunks as usize, window);
    assert_eq!(stats.generated_chunks as usize, window);
    assert_eq!(mgr.loaded_count(), window);

    // O(1) index stays coherent with the slab after every transition.
    for (i, slot) in mgr.slots.iter().enumerate() {
        if slot.active {
            assert_eq!(mgr.find_chunk_index(slot.chunk.header.coord), Some(i));
            assert!(slot.chunk.header.coord.chebyshev_dist(jump) <= radius);
        }
    }
    assert!(mgr.chunk_at(center).is_none());
    assert!(mgr.chunk_at(ChunkCoord::new(jump.x + radius + 1, jump.y)).is_none());
}
