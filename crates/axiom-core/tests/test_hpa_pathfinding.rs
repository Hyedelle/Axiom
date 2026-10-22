//! Phase 3 integration tests: hierarchical portal-graph pathfinding (HPA*) over the
//! infinite chunk world (T-072).

use axiom_core::{
    deterministic_hash, ChunkCoord, ChunkManager, ChunkPortalCache, HierarchicalPathfinder,
    Portal, TileType, HPA_UNREACHABLE, PORTAL_EDGE_NORTH, PORTAL_EDGE_SOUTH,
};
use std::time::Instant;

const S: usize = 32;
type Hpa = HierarchicalPathfinder<32, 64, 1024>;

/// Chunk managers are hundreds of KB of stack; run bodies on a roomy thread.
fn with_big_stack<F: FnOnce() + Send + 'static>(f: F) {
    std::thread::Builder::new()
        .stack_size(64 << 20)
        .spawn(f)
        .expect("spawn test thread")
        .join()
        .expect("test body panicked");
}

fn chunk_of(t: (i64, i64)) -> ChunkCoord {
    ChunkCoord::from_world_tile(t.0, t.1, S).0
}

fn manhattan(a: (i64, i64), b: (i64, i64)) -> i64 {
    (a.0 - b.0).abs() + (a.1 - b.1).abs()
}

/// Ground truth: flat 4-connected BFS over a rectangular world window.
struct FlatGrid {
    x0: i64,
    y0: i64,
    w: usize,
    h: usize,
    walk: Vec<bool>,
}

impl FlatGrid {
    fn new<const MC: usize, const DC: usize>(
        chunks: &ChunkManager<32, MC, DC>,
        x0: i64,
        y0: i64,
        w: usize,
        h: usize,
    ) -> Self {
        let mut walk = vec![false; w * h];
        for y in 0..h {
            for x in 0..w {
                walk[y * w + x] = chunks.tile_at(x0 + x as i64, y0 + y as i64).is_walkable();
            }
        }
        Self { x0, y0, w, h, walk }
    }

    fn idx(&self, t: (i64, i64)) -> Option<usize> {
        let x = t.0 - self.x0;
        let y = t.1 - self.y0;
        if x < 0 || y < 0 || x >= self.w as i64 || y >= self.h as i64 {
            None
        } else {
            Some(y as usize * self.w + x as usize)
        }
    }

    fn tile(&self, idx: usize) -> (i64, i64) {
        (self.x0 + (idx % self.w) as i64, self.y0 + (idx / self.w) as i64)
    }

    /// BFS distances from `start` (u32::MAX = unreachable).
    fn bfs(&self, start: (i64, i64)) -> Vec<u32> {
        let mut dist = vec![u32::MAX; self.w * self.h];
        let Some(s) = self.idx(start) else { return dist };
        if !self.walk[s] {
            return dist;
        }
        let mut queue = std::collections::VecDeque::new();
        dist[s] = 0;
        queue.push_back(s);
        while let Some(i) = queue.pop_front() {
            let (x, y) = (i % self.w, i / self.w);
            let d = dist[i] + 1;
            let mut push = |nx: usize, ny: usize| {
                if nx < self.w && ny < self.h {
                    let n = ny * self.w + nx;
                    if self.walk[n] && dist[n] == u32::MAX {
                        dist[n] = d;
                        queue.push_back(n);
                    }
                }
            };
            push(x.wrapping_sub(1), y);
            push(x + 1, y);
            push(x, y.wrapping_sub(1));
            push(x, y + 1);
        }
        dist
    }

    /// Westmost and eastmost tiles of the largest connected component, plus the
    /// exact tile distance between them.
    fn span_of_largest_component(&self) -> ((i64, i64), (i64, i64), u32) {
        let mut seen = vec![false; self.w * self.h];
        let mut best: Option<(usize, usize, usize)> = None; // (size, west, east)
        for i in 0..self.w * self.h {
            if seen[i] || !self.walk[i] {
                continue;
            }
            let dist = self.bfs(self.tile(i));
            let mut size = 0usize;
            let mut west = i;
            let mut east = i;
            for (j, &d) in dist.iter().enumerate() {
                if d == u32::MAX {
                    continue;
                }
                seen[j] = true;
                size += 1;
                let (jx, jy) = (j % self.w, j / self.w);
                let (wx, wy) = (west % self.w, west / self.w);
                let (ex, ey) = (east % self.w, east / self.w);
                if (jx, jy) < (wx, wy) {
                    west = j;
                }
                if (jx, jy) > (ex, ey) {
                    east = j;
                }
            }
            if best.is_none_or(|(bs, _, _)| size > bs) {
                best = Some((size, west, east));
            }
        }
        let (_, west, east) = best.expect("window has walkable tiles");
        let start = self.tile(west);
        let goal = self.tile(east);
        let opt = self.bfs(start)[east];
        (start, goal, opt)
    }
}

fn assert_tile_path_valid<const MC: usize, const DC: usize>(
    chunks: &ChunkManager<32, MC, DC>,
    tiles: &[(i64, i64)],
    start: (i64, i64),
    goal: (i64, i64),
) {
    assert!(tiles.len() >= 2, "tile path must contain start and goal");
    assert_eq!(tiles[0], start);
    assert_eq!(tiles[tiles.len() - 1], goal);
    for (i, &t) in tiles.iter().enumerate() {
        assert!(
            chunks.tile_at(t.0, t.1).is_walkable(),
            "tile #{i} {t:?} is not walkable"
        );
        if i > 0 {
            assert_eq!(manhattan(tiles[i - 1], t), 1, "tiles #{} and #{i} are not adjacent", i - 1);
        }
    }
}

fn assert_waypoints_valid<const MC: usize, const DC: usize>(
    chunks: &ChunkManager<32, MC, DC>,
    wps: &[(i64, i64)],
    start: (i64, i64),
    goal: (i64, i64),
) {
    assert!(wps.len() >= 2);
    assert_eq!(wps[0], start);
    assert_eq!(wps[wps.len() - 1], goal);
    for (i, &w) in wps.iter().enumerate() {
        assert!(chunks.tile_at(w.0, w.1).is_walkable(), "waypoint #{i} {w:?} not walkable");
        if i > 0 {
            let a = wps[i - 1];
            assert!(
                chunk_of(a) == chunk_of(w) || manhattan(a, w) == 1,
                "waypoints #{} {a:?} and #{i} {w:?} are neither in one chunk nor adjacent",
                i - 1
            );
        }
    }
}

#[test]
fn test_hpa_portal_extraction() {
    with_big_stack(|| {
        assert_eq!(core::mem::size_of::<Portal>(), 16);
        assert_eq!(core::mem::align_of::<ChunkPortalCache<32>>(), 64);
        assert_eq!(core::mem::align_of::<Hpa>(), 64);

        // --- Controlled boundary: chunk (0,0) over chunk (0,1), wall on row 31 with
        //     two openings (x 3..=5 and x 20).
        let mut mgr = ChunkManager::<32, 16, 16384>::new(0x5EED_7072, 1);
        mgr.update_center(ChunkCoord::new(0, 0), 0);
        for cy in 0..=1i64 {
            for ly in 0..S as i64 {
                for lx in 0..S as i64 {
                    assert!(mgr.set_tile(lx, cy * S as i64 + ly, TileType::Plains));
                }
            }
        }
        for lx in 0..S as i64 {
            if !(3..=5).contains(&lx) && lx != 20 {
                assert!(mgr.set_tile(lx, 31, TileType::Rock));
            }
        }

        let mut hpa = Box::new(Hpa::new());
        let cache = *hpa.build_cache(ChunkCoord::new(0, 0), &mgr).expect("cache slot");
        assert_eq!(cache.coord, ChunkCoord::new(0, 0));
        assert_eq!(cache.overflow, 0);
        assert_eq!(cache.walkable_tiles(), (S * S - 28) as u32);
        let south: Vec<Portal> =
            cache.portals().iter().copied().filter(|p| p.edge == PORTAL_EDGE_SOUTH).collect();
        assert_eq!(south.len(), 2, "exactly two openings on the south edge: {south:?}");
        assert_eq!((south[0].start_offset, south[0].end_offset), (3, 5));
        assert_eq!((south[1].start_offset, south[1].end_offset), (20, 20));
        assert_eq!(south[0].center_world(S), (4, 31));
        assert_eq!(south[1].center_world(S), (20, 31));
        let i0 = cache.portal_slot(PORTAL_EDGE_SOUTH, 3).unwrap();
        let i1 = cache.portal_slot(PORTAL_EDGE_SOUTH, 20).unwrap();
        // (4,31) -> (4,30) -> (20,30) -> (20,31): the wall forces the detour via row 30.
        assert_eq!(cache.intra_dist[i0][i1], 18);
        assert_eq!(cache.intra_dist[i1][i0], 18);
        assert_eq!(cache.intra_dist[i0][i0], 0);

        // Twin portals on the north edge of chunk (0,1), one step away.
        let twin_cache = *hpa.build_cache(ChunkCoord::new(0, 1), &mgr).expect("cache slot");
        let north: Vec<Portal> = twin_cache
            .portals()
            .iter()
            .copied()
            .filter(|p| p.edge == PORTAL_EDGE_NORTH)
            .collect();
        assert_eq!(north.len(), 2);
        let t0 = twin_cache.portal_slot(PORTAL_EDGE_NORTH, 3).expect("twin of 3..=5");
        assert_eq!(twin_cache.portals[t0].end_offset, 5);
        assert_eq!(twin_cache.portals[t0].center_world(S), (4, 32));
        assert_eq!(manhattan(south[0].center_world(S), twin_cache.portals[t0].center_world(S)), 1);
        assert_eq!(twin_cache.portals[t0], south[0].twin());
        assert!(twin_cache.portal_slot(PORTAL_EDGE_NORTH, 20).is_some());
        assert_eq!(hpa.cached_chunk_count(), 2);

        // Editing a loaded chunk is detected through the modification stamp: a
        // vertical wall at x = 12 separates the two south portals.
        for ly in 0..S as i64 {
            assert!(mgr.set_tile(12, ly, TileType::Rock));
        }
        let builds_before = hpa.stats.cache_builds;
        let cache = *hpa.build_cache(ChunkCoord::new(0, 0), &mgr).expect("cache slot");
        assert_eq!(hpa.stats.cache_builds, builds_before + 1, "stale cache must rebuild");
        let i0 = cache.portal_slot(PORTAL_EDGE_SOUTH, 3).unwrap();
        let i1 = cache.portal_slot(PORTAL_EDGE_SOUTH, 20).unwrap();
        assert_eq!(cache.intra_dist[i0][i1], HPA_UNREACHABLE);
        assert_eq!(hpa.cached_chunk_count(), 2);
        let builds_before = hpa.stats.cache_builds;
        let _ = hpa.build_cache(ChunkCoord::new(0, 0), &mgr);
        assert_eq!(hpa.stats.cache_builds, builds_before, "unchanged chunk must hit the cache");

        // --- Procedural terrain: every portal is a maximal run walkable on both sides
        //     of the boundary (water/rock never leaks in) and owns an identical twin.
        let mut mgr2 = ChunkManager::<32, 16>::new(0xC0FFEE_7072, 1);
        mgr2.update_center(ChunkCoord::new(0, 0), 0);
        let mut total_portals = 0usize;
        let mut max_per_chunk = 0usize;
        for cy in -1..=1 {
            for cx in -1..=1 {
                let coord = ChunkCoord::new(cx, cy);
                let c = *hpa.build_cache(coord, &mgr2).expect("cache slot");
                assert_eq!(c.overflow, 0, "chunk {coord:?} exceeded the portal budget");
                total_portals += c.portal_count as usize;
                max_per_chunk = max_per_chunk.max(c.portal_count as usize);
                for p in c.portals() {
                    assert_eq!(p.chunk, coord);
                    let tw = p.twin();
                    let both_open = |off: i64| -> bool {
                        if !(0..S as i64).contains(&off) {
                            return false;
                        }
                        let (lx, ly) = Portal::edge_local_tile(p.edge, off as usize, S);
                        let (tx, ty) = Portal::edge_local_tile(tw.edge, off as usize, S);
                        let inside = mgr2.tile_at(
                            coord.x as i64 * S as i64 + lx as i64,
                            coord.y as i64 * S as i64 + ly as i64,
                        );
                        let outside = mgr2.tile_at(
                            tw.chunk.x as i64 * S as i64 + tx as i64,
                            tw.chunk.y as i64 * S as i64 + ty as i64,
                        );
                        inside.is_walkable() && outside.is_walkable()
                    };
                    for off in p.start_offset as i64..=p.end_offset as i64 {
                        assert!(both_open(off), "portal {p:?} offset {off} is not open on both sides");
                    }
                    assert!(!both_open(p.start_offset as i64 - 1), "portal {p:?} is not maximal (start)");
                    assert!(!both_open(p.end_offset as i64 + 1), "portal {p:?} is not maximal (end)");
                    let tc = *hpa.build_cache(tw.chunk, &mgr2).expect("cache slot");
                    let ts = tc.portal_slot(tw.edge, tw.start_offset).expect("twin portal exists");
                    assert_eq!(tc.portals[ts], tw);
                    assert_eq!(manhattan(p.center_world(S), tw.center_world(S)), 1);
                }
            }
        }
        assert!(total_portals >= 9, "procedural 3x3 window should expose portals: {total_portals}");
        println!(
            "\n==> [HPA PORTALS] 3x3 procedural window: {total_portals} portals, max {max_per_chunk}/chunk, \
             {} caches resident, {} builds",
            hpa.cached_chunk_count(),
            hpa.stats.cache_builds
        );
    });
}

#[test]
fn test_hpa_long_distance_planning() {
    with_big_stack(|| {
        const R: i32 = 5; // 11x11 chunks = 352x352 tiles, all loaded
        let mut picked = None;
        for seed in [0x4850_412A_0000_0001u64, 0x4850_412A_0000_0002, 0x4850_412A_0000_0003, 7, 99] {
            let mut mgr = Box::new(ChunkManager::<32, 128>::new(seed, R));
            mgr.update_center(ChunkCoord::new(0, 0), 0);
            assert_eq!(mgr.loaded_count(), 121);
            let grid = FlatGrid::new(&mgr, -160, -160, 352, 352);
            let (start, goal, opt) = grid.span_of_largest_component();
            let span = chunk_of(start).chebyshev_dist(chunk_of(goal));
            if span >= 5 {
                picked = Some((seed, mgr, start, goal, opt, span));
                break;
            }
        }
        let (seed, mgr, start, goal, opt, span) = picked.expect("a seed with a >= 5-chunk component");
        assert!(opt != u32::MAX && opt >= 5 * S as u32);

        let mut hpa = Box::new(Hpa::new());
        let mut wps = [(0i64, 0i64); 256];
        let t_cold = Instant::now();
        let n = hpa.find_path(start, goal, &mgr, &mut wps);
        let cold = t_cold.elapsed();
        assert!(n >= 3, "long route must cross at least one portal (got {n} waypoints)");
        assert_waypoints_valid(&mgr, &wps[..n], start, goal);
        let crossings = wps[..n]
            .windows(2)
            .filter(|w| chunk_of(w[0]) != chunk_of(w[1]))
            .count();
        assert!(crossings >= span as usize, "must cross at least {span} chunk boundaries: {crossings}");

        // Full tile path: walkable, contiguous, never shorter than the exact optimum,
        // and within the HPA* portal-centre approximation bound.
        let mut tiles = vec![(0i64, 0i64); 16_384];
        let m = hpa.find_tile_path(start, goal, &mgr, &mut tiles);
        assert!(m > 0, "tile expansion failed");
        assert_tile_path_valid(&mgr, &tiles[..m], start, goal);
        let hpa_len = (m - 1) as u32;
        assert!(hpa_len >= opt, "HPA route ({hpa_len}) cannot beat exact BFS ({opt})");
        assert!(
            hpa_len <= opt * 2 + 2 * S as u32,
            "HPA route {hpa_len} is too long versus optimum {opt}"
        );

        // Obstacles: no waypoint or tile may sit on rock/water (checked above) and
        // an impassable goal is rejected outright.
        let mut blocked = None;
        'scan: for y in -160..192 {
            for x in -160..192 {
                if !mgr.tile_at(x, y).is_walkable() {
                    blocked = Some((x, y));
                    break 'scan;
                }
            }
        }
        assert_eq!(hpa.find_path(start, blocked.unwrap(), &mgr, &mut wps), 0);

        // Warm-cache macro planning throughput.
        const ITERS: u32 = 100;
        let t_warm = Instant::now();
        for _ in 0..ITERS {
            let k = hpa.find_path(start, goal, &mgr, &mut wps);
            assert_eq!(k, n);
        }
        let warm = t_warm.elapsed() / ITERS;
        let expanded = hpa.stats.last_nodes_expanded;
        println!(
            "\n==> [HPA LONG RANGE] seed {seed:#x}: {start:?} -> {goal:?} ({span} chunks, optimum {opt} tiles)\n\
             \x20   {n} waypoints / {crossings} crossings / {hpa_len} tiles ({}% of optimum)\n\
             \x20   cold {cold:?} (incl. {} cache builds), warm {warm:?}/query, {expanded} nodes expanded, \
             {} caches resident, heap overflows {}",
            hpa_len * 100 / opt,
            hpa.stats.cache_builds,
            hpa.cached_chunk_count(),
            hpa.stats.heap_overflows
        );
        assert_eq!(hpa.stats.heap_overflows, 0);
        assert!(expanded <= 1024, "macro search must stay on the sparse graph: {expanded}");
        assert!(
            warm.as_micros() < 2_000,
            "warm macro query too slow for debug gate: {warm:?} (release target 50µs)"
        );

        // Streaming independence: the same query against a manager that has only a
        // 3x3 window loaded (every other chunk sampled procedurally) yields the
        // identical waypoint sequence.
        let mut small = ChunkManager::<32, 16>::new(seed, 1);
        small.update_center(chunk_of(start), 0);
        let mut hpa2 = Box::new(Hpa::new());
        let mut wps2 = [(0i64, 0i64); 256];
        let n2 = hpa2.find_path(start, goal, &small, &mut wps2);
        assert_eq!(n2, n);
        assert_eq!(&wps2[..n2], &wps[..n]);
    });
}

#[test]
fn test_hpa_path_determinism() {
    with_big_stack(|| {
        const SEED: u64 = 0x4850_412A_DE7E_0072;
        let mut mgr = Box::new(ChunkManager::<32, 64>::new(SEED, 3));
        mgr.update_center(ChunkCoord::new(0, 0), 0);
        let start = mgr.find_open_spawn(64, 64, 1024);
        let grid = FlatGrid::new(&mgr, -96, -96, 224, 224);
        let dist = grid.bfs(start);
        // Farthest reachable tile (ties: smallest index) inside the loaded window.
        let mut goal = start;
        let mut best = 0i64;
        for (i, &d) in dist.iter().enumerate() {
            if d != u32::MAX {
                let t = grid.tile(i);
                let far = manhattan(start, t);
                if far > best {
                    best = far;
                    goal = t;
                }
            }
        }
        assert!(best >= 64, "need a long route for a meaningful replay: {best}");

        fn run<const MC: usize, const DC: usize>(
            hpa: &mut Hpa,
            mgr: &ChunkManager<32, MC, DC>,
            start: (i64, i64),
            goal: (i64, i64),
        ) -> (Vec<(i64, i64)>, Vec<(i64, i64)>, u64) {
            let mut wps = [(0i64, 0i64); 256];
            let n = hpa.find_path(start, goal, mgr, &mut wps);
            assert!(n >= 2);
            let mut tiles = vec![(0i64, 0i64); 8192];
            let m = hpa.find_tile_path(start, goal, mgr, &mut tiles);
            assert!(m >= 2);
            let w = wps[..n].to_vec();
            let t = tiles[..m].to_vec();
            let h = deterministic_hash(&(&w[..], &t[..]));
            (w, t, h)
        }

        let mut a = Box::new(Hpa::new());
        let r1 = run(&mut a, &mgr, start, goal);
        let r2 = run(&mut a, &mgr, start, goal); // warm caches
        let mut b = Box::new(Hpa::new());
        let r3 = run(&mut b, &mgr, start, goal); // cold, fresh slab
        assert_eq!(r1, r2, "warm replay diverged");
        assert_eq!(r1, r3, "fresh pathfinder diverged");
        assert_waypoints_valid(&mgr, &r1.0, start, goal);
        assert_tile_path_valid(&mgr, &r1.1, start, goal);

        // Churn the LRU slab with 200 pseudo-random queries, then replay again.
        let walkable: Vec<(i64, i64)> = dist
            .iter()
            .enumerate()
            .filter(|(_, &d)| d != u32::MAX)
            .map(|(i, _)| grid.tile(i))
            .collect();
        let mut lcg = SEED;
        let mut wps = [(0i64, 0i64); 256];
        let mut found = 0usize;
        for _ in 0..200 {
            lcg = lcg.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let s = walkable[(lcg >> 33) as usize % walkable.len()];
            lcg = lcg.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let g = walkable[(lcg >> 33) as usize % walkable.len()];
            let k = a.find_path(s, g, &mgr, &mut wps);
            assert!(k >= 1, "{s:?} -> {g:?} are in one component yet no route was found");
            assert_waypoints_valid(&mgr, &wps[..k], s, g);
            found += 1;
        }
        assert_eq!(found, 200);
        let r4 = run(&mut a, &mgr, start, goal);
        assert_eq!(r1, r4, "replay after slab churn diverged");

        // Blocking a tile in the middle of the route on a loaded chunk is detected
        // through the modification stamp: the new route avoids it (or is correctly
        // reported impossible when the tile was a choke point).
        let mid = r1.1[r1.1.len() / 2];
        assert_ne!(mid, start);
        assert_ne!(mid, goal);
        assert!(mgr.set_tile(mid.0, mid.1, TileType::Rock));
        let grid2 = FlatGrid::new(&mgr, -96, -96, 224, 224);
        let still_reachable = grid2.bfs(start)[grid2.idx(goal).unwrap()] != u32::MAX;
        let mut tiles = vec![(0i64, 0i64); 8192];
        let m = a.find_tile_path(start, goal, &mgr, &mut tiles);
        if still_reachable {
            assert!(m >= 2, "route must be re-planned around the new obstacle");
            assert_tile_path_valid(&mgr, &tiles[..m], start, goal);
            assert!(!tiles[..m].contains(&mid), "re-planned route still crosses the blocked tile");
        } else {
            assert_eq!(m, 0);
        }

        println!(
            "\n==> [HPA DETERMINISM] {start:?} -> {goal:?}: {} waypoints, {} tiles, replay hash {:#018x}, \
             {} queries / {} cache builds / {} evictions",
            r1.0.len(),
            r1.1.len(),
            r1.2,
            a.stats.queries,
            a.stats.cache_builds,
            a.stats.cache_evictions
        );
    });
}
