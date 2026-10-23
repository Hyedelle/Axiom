//! Biome monster spawner & territory defence throughput benchmark (T-071)
//!
//! Validates:
//! - Ambient spawn rolls over a 7x7 loaded chunk window + roaming for a 256-slot slab
//! - Territory defence with 16 posts stepping every tick against assault waves
//! - Per-tick cost stays far below the 16.6ms 60Hz budget even in debug builds

use axiom_core::{
    ChunkCoord, ChunkManager, MonsterSpawner, TerritoryDefense, DEFAULT_SPAWN_MIN_OPEN_AREA,
};
use std::time::Instant;

#[test]
fn test_monster_spawn_and_defense_throughput() {
    std::thread::Builder::new()
        .stack_size(32 << 20)
        .spawn(bench_body)
        .expect("spawn bench thread")
        .join()
        .expect("bench body panicked");
}

fn bench_body() {
    const TICKS: u64 = 2_000;
    let seed = 0xBE4C_4A11_0071u64;

    let mut mgr = ChunkManager::<32, 64>::new(seed, 3);
    mgr.update_center(ChunkCoord::new(0, 0), 0);
    let (sx, sy) = mgr.find_open_spawn(64, 64, DEFAULT_SPAWN_MIN_OPEN_AREA);
    let (hx, hy) = (sx as i32, sy as i32);

    let mut spawner = MonsterSpawner::<256>::new(seed);
    spawner.set_safe_zone(hx, hy, 20);
    let mut defense = TerritoryDefense::<16>::new(seed, hx, hy, 12, 5_000);
    defense.set_schedule(30, 90, 6, 2);
    let mut p = 0;
    for dy in [-4, -2, 2, 4] {
        for dx in [-4, -2, 2, 4] {
            if (dx as i32).abs() + (dy as i32).abs() >= 6 {
                p += 1;
                defense.add_post(hx + dx, hy + dy, 25, 6, 2);
            }
        }
    }
    assert!(p > 0);

    let mut spawn_total_ns = 0u128;
    let mut defense_total_ns = 0u128;
    let mut spawn_peak_ns = 0u128;
    let mut defense_peak_ns = 0u128;
    let mut spawn_peak_tick = 0u64;
    let mut defense_peak_tick = 0u64;
    let mut over_1ms = 0u32;
    let mut over_8ms = 0u32;
    let mut peak_alive = 0usize;
    let mut waves = 0u32;
    let mut shots = 0u64;

    for tick in 1..=TICKS {
        let t0 = Instant::now();
        spawner.update(&mgr, tick);
        let s_ns = t0.elapsed().as_nanos();
        let t1 = Instant::now();
        let r = defense.step(&mut spawner, &mgr, tick);
        let d_ns = t1.elapsed().as_nanos();

        spawn_total_ns += s_ns;
        defense_total_ns += d_ns;
        if s_ns > spawn_peak_ns {
            spawn_peak_ns = s_ns;
            spawn_peak_tick = tick;
        }
        if d_ns > defense_peak_ns {
            defense_peak_ns = d_ns;
            defense_peak_tick = tick;
        }
        if s_ns + d_ns > 1_000_000 {
            over_1ms += 1;
        }
        if s_ns > 8_000_000 || d_ns > 8_000_000 {
            over_8ms += 1;
        }
        peak_alive = peak_alive.max(spawner.alive());
        if r.wave_spawned > 0 {
            waves += 1;
        }
        shots += r.post_shots as u64;
    }

    println!(
        "\n==> [MONSTER SPAWNER] {TICKS} ticks: avg {:.1}us / peak {:.1}us per update, peak alive {peak_alive}, spawned {}, despawned {}, peak at tick {spawn_peak_tick}",
        spawn_total_ns as f64 / TICKS as f64 / 1_000.0,
        spawn_peak_ns as f64 / 1_000.0,
        spawner.total_spawned,
        spawner.total_despawned
    );
    println!(
        "==> [TERRITORY DEFENSE] avg {:.1}us / peak {:.1}us per step, {waves} waves, {shots} post shots, {} kills, heart {}/{} hp, peak at tick {defense_peak_tick}, ticks over 1ms: {over_1ms}",
        defense_total_ns as f64 / TICKS as f64 / 1_000.0,
        defense_peak_ns as f64 / 1_000.0,
        defense.kills,
        defense.hp,
        defense.max_hp
    );

    assert!(peak_alive > 0);
    assert!(waves >= 20, "only {waves} waves in {TICKS} ticks");
    assert!(defense.kills > 0);
    // Debug-build budget: average spawn roll + defence step stay far under one 60Hz frame.
    // Absolute single-tick peaks are dominated by OS scheduler jitter on a shared box, so
    // only a handful of ticks may ever cross half a frame.
    let spawn_avg_ns = spawn_total_ns / TICKS as u128;
    let defense_avg_ns = defense_total_ns / TICKS as u128;
    assert!(spawn_avg_ns < 1_000_000, "spawn update avg {}us exceeds 1ms", spawn_avg_ns / 1_000);
    assert!(defense_avg_ns < 1_000_000, "defense step avg {}us exceeds 1ms", defense_avg_ns / 1_000);
    assert!(over_8ms <= 15, "{over_8ms} ticks exceeded 8ms (peak {}us)", spawn_peak_ns.max(defense_peak_ns) / 1_000);
}
