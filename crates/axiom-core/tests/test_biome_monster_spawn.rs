//! Phase 3 integration tests: biome-driven dynamic monster spawning and stronghold
//! territory defence (T-071) on top of the T-070 infinite chunk pipeline.

use axiom_core::{
    biome_allows, BiomeKind, ChunkCoord, ChunkManager, MonsterArchetype, MonsterSpawner,
    MonsterState, TerritoryDefense, TileType, DEFAULT_MAX_MONSTERS_PER_CHUNK,
    DEFAULT_SPAWN_MIN_OPEN_AREA, DEFAULT_TERRITORY_RADIUS,
};

type Mgr = ChunkManager<32, 64>;
type Spawner = MonsterSpawner<256>;

const SEED: u64 = 0xB10E_5EED_0C7A_0071;

/// Each chunk manager is ~240 KB of stack; run the bodies on a roomy thread so several
/// managers (original + replay + divergent seed) can coexist in one test.
fn with_big_stack<F: FnOnce() + Send + 'static>(f: F) {
    std::thread::Builder::new()
        .stack_size(64 << 20)
        .spawn(f)
        .expect("spawn test thread")
        .join()
        .expect("test body panicked");
}

fn run_ambient(seed: u64, center: ChunkCoord, ticks: u64) -> (Mgr, Spawner) {
    let mut mgr = Mgr::new(seed, 3);
    mgr.update_center(center, 0);
    let mut spawner = Spawner::new(seed);
    for tick in 1..=ticks {
        spawner.update(&mgr, tick);
    }
    (mgr, spawner)
}

#[test]
fn test_biome_spawn_affinity_and_determinism() {
    with_big_stack(test_biome_spawn_affinity_and_determinism_body);
}

fn test_biome_spawn_affinity_and_determinism_body() {
    let (mgr, spawner) = run_ambient(SEED, ChunkCoord::new(0, 0), 400);

    assert!(spawner.alive() >= 32, "only {} monsters spawned", spawner.alive());
    assert!(spawner.total_spawned >= spawner.alive() as u64);

    let mut archetypes_seen = [false; MonsterArchetype::COUNT];
    let mut biomes_seen = [false; BiomeKind::COUNT];
    for m in spawner.monsters.iter().filter(|m| m.is_alive()) {
        let tile = mgr.tile_at(m.tile_x as i64, m.tile_y as i64);
        let biome = mgr.biome_at(m.tile_x as i64, m.tile_y as i64);

        // 1. Habitat: aquatic archetypes live on water, everything else on walkable ground.
        if m.archetype.is_aquatic() {
            assert_eq!(tile, TileType::Water, "{} on {:?}", m.archetype.name(), tile);
        } else {
            assert!(tile.is_walkable(), "{} on {:?}", m.archetype.name(), tile);
        }

        // 2. Biome affinity: the recorded spawn biome allows the archetype, and the pack
        //    anchor's biome is at most a short walk from the member tile.
        assert!(
            biome_allows(m.biome, m.archetype),
            "{} spawned from {} table",
            m.archetype.name(),
            m.biome.name()
        );
        let _ = biome;

        // 3. Home chunk is the pack anchor chunk: members stay within one chunk of it.
        let (here, _, _) = ChunkCoord::from_world_tile(m.tile_x as i64, m.tile_y as i64, 32);
        assert!(here.chebyshev_dist(m.home()) <= 1);
        assert!(mgr.chunk_at(m.home()).is_some(), "home chunk must be loaded");

        // 4. Stats follow the tier curve; origin ring is tier 0.
        assert_eq!(m.hp, m.archetype.hp_at_tier(m.tier));
        assert_eq!(m.attack, m.archetype.attack_at_tier(m.tier));
        assert_eq!(m.state, MonsterState::Roaming);

        archetypes_seen[m.archetype as usize] = true;
        biomes_seen[m.biome as usize] = true;
    }

    // 5. Per-chunk population cap.
    for slot in mgr.slots.iter().filter(|s| s.active) {
        let n = spawner.count_in_chunk(slot.chunk.header.coord);
        assert!(
            n <= DEFAULT_MAX_MONSTERS_PER_CHUNK as usize,
            "chunk {:?} holds {n} monsters",
            slot.chunk.header.coord
        );
    }

    // 6. Several biomes and archetypes are represented in a 7x7 chunk window.
    let biome_count = biomes_seen.iter().filter(|b| **b).count();
    let archetype_count = archetypes_seen.iter().filter(|a| **a).count();
    assert!(biome_count >= 3, "only {biome_count} spawn biomes represented");
    assert!(archetype_count >= 3, "only {archetype_count} archetypes represented");

    // 7. Bit-identical replay with the same seed; a different seed diverges.
    let (_, replay) = run_ambient(SEED, ChunkCoord::new(0, 0), 400);
    assert_eq!(spawner.compute_hash(), replay.compute_hash());
    assert_eq!(spawner.monsters, replay.monsters);
    let (_, other) = run_ambient(SEED ^ 0xFFFF, ChunkCoord::new(0, 0), 400);
    assert_ne!(spawner.compute_hash(), other.compute_hash());
}

#[test]
fn test_monster_tier_scales_with_distance_and_ocean_is_aquatic() {
    with_big_stack(test_monster_tier_scales_with_distance_and_ocean_is_aquatic_body);
}

fn test_monster_tier_scales_with_distance_and_ocean_is_aquatic_body() {
    // Far from the origin the tier rises with Chebyshev chunk distance.
    let far = ChunkCoord::new(40, -37);
    let (mgr, spawner) = run_ambient(SEED, far, 300);
    assert!(spawner.alive() > 0);
    for m in spawner.monsters.iter().filter(|m| m.is_alive()) {
        let d = m.home().chebyshev_dist(ChunkCoord::new(0, 0));
        assert_eq!(m.tier as i32, (d / 4).min(15));
        assert!(m.tier >= 8, "tier {} at chunk distance {d}", m.tier);
        assert!(m.hp > m.archetype.base_hp());
    }

    // Ocean chunks only ever produce the aquatic leviathan, placed on water.
    let mut ocean_checked = 0usize;
    for slot in mgr.slots.iter().filter(|s| s.active) {
        if slot.chunk.header.primary_biome != BiomeKind::OceanDepths {
            continue;
        }
        for m in spawner.monsters.iter().filter(|m| m.is_alive() && m.home() == slot.chunk.header.coord) {
            if m.biome == BiomeKind::OceanDepths {
                assert_eq!(m.archetype, MonsterArchetype::DeepLeviathan);
                assert_eq!(mgr.tile_at(m.tile_x as i64, m.tile_y as i64), TileType::Water);
                ocean_checked += 1;
            }
        }
    }
    let _ = ocean_checked;
}

#[test]
fn test_monster_despawn_on_chunk_eviction() {
    with_big_stack(test_monster_despawn_on_chunk_eviction_body);
}

fn test_monster_despawn_on_chunk_eviction_body() {
    let mut mgr = Mgr::new(SEED, 3);
    mgr.update_center(ChunkCoord::new(0, 0), 0);
    let mut spawner = Spawner::new(SEED);
    for tick in 1..=300 {
        spawner.update(&mgr, tick);
    }
    let before = spawner.alive();
    assert!(before > 0);

    // Walk 6 chunks east: the western half of the ring is evicted.
    let new_center = ChunkCoord::new(6, 0);
    mgr.update_center(new_center, 301);
    let stats = spawner.update(&mgr, 301);
    assert!(stats.despawned > 0, "eviction must despawn monsters");
    for m in spawner.monsters.iter().filter(|m| m.is_alive()) {
        assert!(m.home().chebyshev_dist(new_center) <= spawner.despawn_radius);
        assert!(mgr.chunk_at(m.home()).is_some());
    }
    assert_eq!(spawner.total_despawned, stats.despawned as u64);

    // The population recovers in the newly streamed chunks.
    for tick in 302..=700 {
        spawner.update(&mgr, tick);
    }
    assert!(spawner.alive() >= 16, "population did not recover: {}", spawner.alive());
    let east = spawner
        .monsters
        .iter()
        .filter(|m| m.is_alive() && m.home_cx >= 4)
        .count();
    assert!(east > 0, "no monsters in freshly loaded eastern chunks");

    // Safe zone suppresses ambient spawns around the player.
    let mut quiet = Spawner::new(SEED);
    quiet.set_safe_zone(new_center.x * 32 + 16, 16, 40);
    for tick in 1..=300 {
        quiet.update(&mgr, tick);
    }
    for m in quiet.monsters.iter().filter(|m| m.is_alive()) {
        let d = (m.tile_x - (new_center.x * 32 + 16)).abs().max((m.tile_y - 16).abs());
        assert!(d > 40, "monster inside safe zone at distance {d}");
    }
}

fn build_stronghold(seed: u64) -> (Mgr, Spawner, TerritoryDefense<16>, i32, i32) {
    let mut mgr = Mgr::new(seed, 3);
    mgr.update_center(ChunkCoord::new(0, 0), 0);
    let (sx, sy) = mgr.find_open_spawn(64, 64, DEFAULT_SPAWN_MIN_OPEN_AREA);
    let (hx, hy) = (sx as i32, sy as i32);
    let mut spawner = Spawner::new(seed);
    spawner.set_safe_zone(hx, hy, DEFAULT_TERRITORY_RADIUS + 8);
    let mut defense = TerritoryDefense::<16>::new(seed, hx, hy, DEFAULT_TERRITORY_RADIUS, 2_000);
    defense.set_schedule(20, 120, 6, 3);
    (mgr, spawner, defense, hx, hy)
}

#[test]
fn test_territory_wave_defense_deterministic() {
    with_big_stack(test_territory_wave_defense_deterministic_body);
}

fn test_territory_wave_defense_deterministic_body() {
    let (mgr, mut spawner, mut defense, hx, hy) = build_stronghold(SEED);
    // Four posts around the heart, 5-tile reach, 30 damage every 2 ticks.
    for (dx, dy) in [(3, 0), (-3, 0), (0, 3), (0, -3)] {
        assert!(defense.add_post(hx + dx, hy + dy, 30, 5, 2));
    }

    let mut waves = 0u32;
    let mut shots = 0u32;
    let mut max_intruders = 0u16;
    let mut hunting_seen = false;
    for tick in 1..=900u64 {
        spawner.update(&mgr, tick);
        let r = defense.step(&mut spawner, &mgr, tick);
        if r.wave_spawned > 0 {
            waves += 1;
            assert!(r.wave_spawned <= defense.wave_size(defense.wave_index - 1));
        }
        shots += r.post_shots as u32;
        max_intruders = max_intruders.max(r.intruders);
        for m in spawner.monsters.iter().filter(|m| m.is_alive()) {
            if m.state == MonsterState::Hunting {
                hunting_seen = true;
                // Hunters only ever stand on tiles their habitat allows.
                assert!(m.archetype.accepts_tile(mgr.tile_at(m.tile_x as i64, m.tile_y as i64)));
            }
        }
    }

    assert!(waves >= 7, "only {waves} waves spawned");
    assert_eq!(defense.total_waves, waves);
    assert!(hunting_seen);
    assert!(max_intruders > 0, "no wave member ever entered the territory");
    assert!(shots > 0, "posts never fired");
    assert!(defense.kills > 0, "posts never killed anything");
    assert_eq!(spawner.total_killed, defense.kills as u64);
    assert!(!defense.breached, "heart fell with hp {}", defense.hp);
    assert_eq!(defense.hp + defense.damage_taken as u32, defense.max_hp);

    // Replay: bit-identical population and stronghold state.
    let (mgr2, mut spawner2, mut defense2, _, _) = build_stronghold(SEED);
    for (dx, dy) in [(3, 0), (-3, 0), (0, 3), (0, -3)] {
        defense2.add_post(hx + dx, hy + dy, 30, 5, 2);
    }
    for tick in 1..=900u64 {
        spawner2.update(&mgr2, tick);
        defense2.step(&mut spawner2, &mgr2, tick);
    }
    assert_eq!(spawner.compute_hash(), spawner2.compute_hash());
    assert_eq!(defense.compute_hash(), defense2.compute_hash());
    assert_eq!(defense, defense2);
}

#[test]
fn test_undefended_heart_is_breached() {
    with_big_stack(test_undefended_heart_is_breached_body);
}

fn test_undefended_heart_is_breached_body() {
    let (mgr, mut spawner, mut defense, _, _) = build_stronghold(SEED);
    let mut breach_tick = None;
    for tick in 1..=3_000u64 {
        spawner.update(&mgr, tick);
        let r = defense.step(&mut spawner, &mgr, tick);
        if r.heart_damage > 0 {
            assert!(r.intruders > 0);
        }
        if defense.breached {
            breach_tick = Some(tick);
            break;
        }
    }
    let t = breach_tick.expect("an undefended heart must eventually fall");
    assert_eq!(defense.hp, 0);
    assert_eq!(defense.damage_taken, defense.max_hp as u64);
    assert_eq!(defense.kills, 0);

    // No further waves are scheduled once the heart has fallen.
    let waves_at_breach = defense.total_waves;
    for tick in (t + 1)..=(t + 400) {
        defense.step(&mut spawner, &mgr, tick);
    }
    assert_eq!(defense.total_waves, waves_at_breach);
    assert_eq!(defense.hp, 0);
}
