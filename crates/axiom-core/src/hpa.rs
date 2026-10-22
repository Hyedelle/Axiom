//! # Hierarchical Pathfinding A* (HPA*) over the infinite chunk world (T-072)
//!
//! Two-level, zero-heap, zero-float long-distance navigation on top of
//! [`ChunkManager`]:
//!
//! - **Level 1 — abstract portal graph.** Every chunk edge is scanned for maximal
//!   runs of tiles that are walkable on *both* sides of the boundary. Each run is a
//!   [`Portal`]. All portals of one chunk plus their pairwise intra-chunk BFS
//!   distances form a [`ChunkPortalCache`]. A portal and the portal of the adjacent
//!   chunk that shares the same opening are *twins* exactly one step apart. A* runs
//!   over this sparse graph (tens to a few hundred nodes) instead of hundreds of
//!   thousands of tiles.
//! - **Level 2 — local steering.** Inside the current chunk a unit walks to the next
//!   portal waypoint with an exact 4-connected BFS over the chunk's cached 1,024-bit
//!   walkability mask ([`HierarchicalPathfinder::local_route`]).
//!
//! Every distance is an integer Manhattan tile count. The cache slab, search scratch
//! and open heap are fixed-size arrays allocated once with the pathfinder; a query
//! performs no heap allocation. Caches are validated per query against the
//! modification counters of the owning chunk and its four neighbours, so edits on
//! loaded chunks are picked up automatically. Edits recorded against *unloaded*
//! chunks (pre-editing through the delta slab) must be followed by
//! [`HierarchicalPathfinder::invalidate_chunk`].

use crate::chunk::{ChunkCoord, ChunkManager};

/// Edge identifiers in clockwise order (`y-1`, `x+1`, `y+1`, `x-1`).
pub const PORTAL_EDGE_NORTH: u8 = 0;
pub const PORTAL_EDGE_EAST: u8 = 1;
pub const PORTAL_EDGE_SOUTH: u8 = 2;
pub const PORTAL_EDGE_WEST: u8 = 3;

/// Maximum portals cached per chunk (slots beyond this are counted in `overflow`).
pub const MAX_PORTALS_PER_CHUNK: usize = 16;
/// Sentinel for "no walkable route" in intra-chunk distance tables.
pub const HPA_UNREACHABLE: u16 = u16::MAX;
/// Largest chunk edge the bit-mask BFS supports (one `u32` row per tile row).
pub const HPA_MAX_CHUNK_SIZE: usize = 32;
/// Tile scratch capacity (`HPA_MAX_CHUNK_SIZE²`).
pub const HPA_CHUNK_TILES_MAX: usize = HPA_MAX_CHUNK_SIZE * HPA_MAX_CHUNK_SIZE;
/// Default chunk-cache slab size (matches `DEFAULT_MAX_CHUNKS`).
pub const DEFAULT_HPA_MAX_CHUNKS: usize = 64;
/// Default macro node capacity: one node per cache slot per portal slot.
pub const DEFAULT_HPA_MAX_PORTALS: usize = DEFAULT_HPA_MAX_CHUNKS * MAX_PORTALS_PER_CHUNK;
/// Open-addressing bucket count for the chunk-cache index.
pub const HPA_INDEX_BUCKETS: usize = 512;
/// Open-list capacity of the macro A*.
pub const HPA_HEAP_CAPACITY: usize = 2048;
/// Maximum macro waypoints (portal hops) in one reconstructed route.
pub const HPA_MAX_WAYPOINTS: usize = 256;
/// Sentinel node id ("no predecessor").
pub const HPA_NO_NODE: u16 = u16::MAX;
const INDEX_EMPTY: u16 = u16::MAX;

// ---------------------------------------------------------------------------
// Portal
// ---------------------------------------------------------------------------

/// One walkable opening on a chunk boundary (16-byte POD).
///
/// The run `[start_offset, end_offset]` (inclusive) is measured along the edge:
/// `x` offsets for north/south edges, `y` offsets for east/west edges. Both the
/// tiles inside this chunk and the facing tiles of the neighbour are walkable for
/// every offset of the run, so the neighbour owns an identical twin portal on the
/// opposite edge with the same offsets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(C)]
pub struct Portal {
    pub chunk: ChunkCoord,
    pub edge: u8,
    pub start_offset: u8,
    pub end_offset: u8,
    pub _reserved: [u8; 5],
}

impl Portal {
    pub const EMPTY: Portal = Portal {
        chunk: ChunkCoord::new(0, 0),
        edge: 0,
        start_offset: 0,
        end_offset: 0,
        _reserved: [0; 5],
    };

    #[inline(always)]
    pub const fn new(chunk: ChunkCoord, edge: u8, start_offset: u8, end_offset: u8) -> Self {
        Self {
            chunk,
            edge: edge & 3,
            start_offset,
            end_offset,
            _reserved: [0; 5],
        }
    }

    /// Number of tiles in the opening.
    #[inline(always)]
    pub const fn len(&self) -> usize {
        self.end_offset as usize - self.start_offset as usize + 1
    }

    /// Representative offset used as the graph node (midpoint of the run).
    #[inline(always)]
    pub const fn center_offset(&self) -> usize {
        (self.start_offset as usize + self.end_offset as usize) / 2
    }

    /// Edge facing `edge` from the other side of the boundary.
    #[inline(always)]
    pub const fn opposite_edge(edge: u8) -> u8 {
        (edge + 2) & 3
    }

    /// Chunk on the far side of this portal.
    #[inline(always)]
    pub const fn neighbour_chunk(&self) -> ChunkCoord {
        match self.edge {
            PORTAL_EDGE_NORTH => ChunkCoord::new(self.chunk.x, self.chunk.y.wrapping_sub(1)),
            PORTAL_EDGE_EAST => ChunkCoord::new(self.chunk.x.wrapping_add(1), self.chunk.y),
            PORTAL_EDGE_SOUTH => ChunkCoord::new(self.chunk.x, self.chunk.y.wrapping_add(1)),
            _ => ChunkCoord::new(self.chunk.x.wrapping_sub(1), self.chunk.y),
        }
    }

    /// Local `(lx, ly)` of the tile at `offset` along `edge` in a chunk of size `s`.
    #[inline(always)]
    pub const fn edge_local_tile(edge: u8, offset: usize, s: usize) -> (usize, usize) {
        match edge {
            PORTAL_EDGE_NORTH => (offset, 0),
            PORTAL_EDGE_EAST => (s - 1, offset),
            PORTAL_EDGE_SOUTH => (offset, s - 1),
            _ => (0, offset),
        }
    }

    /// Local tile of the portal's representative node.
    #[inline(always)]
    pub const fn center_local(&self, s: usize) -> (usize, usize) {
        Self::edge_local_tile(self.edge, self.center_offset(), s)
    }

    /// World tile of the portal's representative node.
    #[inline(always)]
    pub fn center_world(&self, s: usize) -> (i64, i64) {
        let (lx, ly) = self.center_local(s);
        (
            self.chunk.x as i64 * s as i64 + lx as i64,
            self.chunk.y as i64 * s as i64 + ly as i64,
        )
    }

    /// The mirrored portal owned by the neighbouring chunk.
    #[inline(always)]
    pub const fn twin(&self) -> Portal {
        Portal::new(
            self.neighbour_chunk(),
            Self::opposite_edge(self.edge),
            self.start_offset,
            self.end_offset,
        )
    }

    /// Slot-independent deterministic tie-break key for the macro A*.
    #[inline(always)]
    pub const fn tie_key(&self) -> u64 {
        self.chunk.bucket_hash() ^ (((self.edge as u64) << 8) | self.start_offset as u64)
    }
}

// ---------------------------------------------------------------------------
// Per-chunk portal cache
// ---------------------------------------------------------------------------

/// Portals of one chunk, their intra-chunk BFS distance matrix and the chunk's
/// walkability bit mask (64-byte aligned, fixed size).
#[derive(Debug, Clone, Copy)]
#[repr(C, align(64))]
pub struct ChunkPortalCache<const S: usize = 32> {
    pub coord: ChunkCoord,
    /// Modification stamp of the owning chunk and its 4 neighbours at build time.
    pub stamp: u64,
    /// Query counter at last use (LRU eviction key).
    pub last_used: u64,
    /// Search epoch that validated/pinned this cache (never evicted mid-query).
    pub search_epoch: u32,
    pub portal_count: u8,
    /// Openings dropped because the slot budget was exhausted.
    pub overflow: u8,
    pub active: bool,
    pub _pad: u8,
    pub portals: [Portal; MAX_PORTALS_PER_CHUNK],
    /// `intra_dist[i][j]` = 4-connected tile distance between portal centres `i` and
    /// `j` inside this chunk, or [`HPA_UNREACHABLE`].
    pub intra_dist: [[u16; MAX_PORTALS_PER_CHUNK]; MAX_PORTALS_PER_CHUNK],
    /// Row `ly`, bit `lx` set when the tile is walkable.
    pub walkable: [u32; S],
}

impl<const S: usize> Default for ChunkPortalCache<S> {
    fn default() -> Self {
        Self::empty()
    }
}

impl<const S: usize> ChunkPortalCache<S> {
    pub const fn empty() -> Self {
        Self {
            coord: ChunkCoord::new(0, 0),
            stamp: 0,
            last_used: 0,
            search_epoch: 0,
            portal_count: 0,
            overflow: 0,
            active: false,
            _pad: 0,
            portals: [Portal::EMPTY; MAX_PORTALS_PER_CHUNK],
            intra_dist: [[HPA_UNREACHABLE; MAX_PORTALS_PER_CHUNK]; MAX_PORTALS_PER_CHUNK],
            walkable: [0; S],
        }
    }

    #[inline(always)]
    pub fn is_walkable(&self, lx: usize, ly: usize) -> bool {
        lx < S && ly < S && (self.walkable[ly] >> lx) & 1 == 1
    }

    /// Portals as a slice.
    #[inline(always)]
    pub fn portals(&self) -> &[Portal] {
        &self.portals[..self.portal_count as usize]
    }

    /// Finds the slot of the portal starting at `start_offset` on `edge`.
    #[inline]
    pub fn portal_slot(&self, edge: u8, start_offset: u8) -> Option<usize> {
        let n = self.portal_count as usize;
        let mut i = 0usize;
        while i < n {
            let p = &self.portals[i];
            if p.edge == edge && p.start_offset == start_offset {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// Number of walkable tiles in the mask.
    pub fn walkable_tiles(&self) -> u32 {
        let mut n = 0u32;
        let mut ly = 0usize;
        while ly < S {
            n += self.walkable[ly].count_ones();
            ly += 1;
        }
        n
    }

    /// Rebuilds the cache for `coord` from the current world state.
    ///
    /// Reads the owning chunk directly when it is loaded (otherwise through
    /// `ChunkManager::tile_at`, which folds in preserved deltas and procedural
    /// samples), scans the 4 edges for shared openings and fills the intra-chunk
    /// distance matrix with one BFS per portal. Stack scratch only.
    pub fn build<const MC: usize, const DC: usize>(
        &mut self,
        coord: ChunkCoord,
        chunks: &ChunkManager<S, MC, DC>,
    ) {
        assert!(S <= HPA_MAX_CHUNK_SIZE, "HPA chunk size must be <= 32");
        self.coord = coord;
        self.portal_count = 0;
        self.overflow = 0;
        let origin_x = coord.x as i64 * S as i64;
        let origin_y = coord.y as i64 * S as i64;

        // 1. Walkability mask.
        if let Some(chunk) = chunks.chunk_at(coord) {
            let mut ly = 0usize;
            while ly < S {
                let mut row = 0u32;
                let mut lx = 0usize;
                while lx < S {
                    if chunk.tiles[ly][lx].is_walkable() {
                        row |= 1u32 << lx;
                    }
                    lx += 1;
                }
                self.walkable[ly] = row;
                ly += 1;
            }
        } else {
            let mut ly = 0usize;
            while ly < S {
                let mut row = 0u32;
                let mut lx = 0usize;
                while lx < S {
                    if chunks
                        .tile_at(origin_x + lx as i64, origin_y + ly as i64)
                        .is_walkable()
                    {
                        row |= 1u32 << lx;
                    }
                    lx += 1;
                }
                self.walkable[ly] = row;
                ly += 1;
            }
        }

        // 2. Edge scan: maximal runs walkable on both sides of the boundary.
        let mut edge = PORTAL_EDGE_NORTH;
        while edge <= PORTAL_EDGE_WEST {
            let mut run_start: Option<usize> = None;
            let mut offset = 0usize;
            while offset <= S {
                let open = if offset < S {
                    let (lx, ly) = Portal::edge_local_tile(edge, offset, S);
                    let inside = self.is_walkable(lx, ly);
                    if inside {
                        let (ox, oy) = match edge {
                            PORTAL_EDGE_NORTH => (lx as i64, -1i64),
                            PORTAL_EDGE_EAST => (S as i64, ly as i64),
                            PORTAL_EDGE_SOUTH => (lx as i64, S as i64),
                            _ => (-1i64, ly as i64),
                        };
                        chunks.tile_at(origin_x + ox, origin_y + oy).is_walkable()
                    } else {
                        false
                    }
                } else {
                    false
                };
                match (run_start, open) {
                    (None, true) => run_start = Some(offset),
                    (Some(start), false) => {
                        if (self.portal_count as usize) < MAX_PORTALS_PER_CHUNK {
                            self.portals[self.portal_count as usize] =
                                Portal::new(coord, edge, start as u8, (offset - 1) as u8);
                            self.portal_count += 1;
                        } else {
                            self.overflow = self.overflow.saturating_add(1);
                        }
                        run_start = None;
                    }
                    _ => {}
                }
                offset += 1;
            }
            edge += 1;
        }

        // 3. Intra-chunk distance matrix (one BFS per portal centre).
        let n = self.portal_count as usize;
        let mut dist = [HPA_UNREACHABLE; HPA_CHUNK_TILES_MAX];
        let mut parent = [HPA_NO_NODE; HPA_CHUNK_TILES_MAX];
        let mut queue = [0u16; HPA_CHUNK_TILES_MAX];
        let mut i = 0usize;
        while i < MAX_PORTALS_PER_CHUNK {
            self.intra_dist[i] = [HPA_UNREACHABLE; MAX_PORTALS_PER_CHUNK];
            i += 1;
        }
        i = 0;
        while i < n {
            let (sx, sy) = self.portals[i].center_local(S);
            chunk_bfs::<S>(&self.walkable, sx, sy, &mut dist, &mut parent, &mut queue);
            let mut j = 0usize;
            while j < n {
                let (tx, ty) = self.portals[j].center_local(S);
                self.intra_dist[i][j] = dist[ty * S + tx];
                j += 1;
            }
            i += 1;
        }
    }
}

/// 4-connected unit-cost BFS over a chunk walkability mask.
///
/// Fills `dist` (tile distance or [`HPA_UNREACHABLE`]) and `parent` (predecessor
/// tile index or [`HPA_NO_NODE`]) for the first `S*S` entries.
pub fn chunk_bfs<const S: usize>(
    walkable: &[u32; S],
    sx: usize,
    sy: usize,
    dist: &mut [u16; HPA_CHUNK_TILES_MAX],
    parent: &mut [u16; HPA_CHUNK_TILES_MAX],
    queue: &mut [u16; HPA_CHUNK_TILES_MAX],
) {
    let total = S * S;
    let mut i = 0usize;
    while i < total {
        dist[i] = HPA_UNREACHABLE;
        parent[i] = HPA_NO_NODE;
        i += 1;
    }
    if sx >= S || sy >= S || (walkable[sy] >> sx) & 1 == 0 {
        return;
    }
    let start = sy * S + sx;
    dist[start] = 0;
    queue[0] = start as u16;
    let mut head = 0usize;
    let mut tail = 1usize;
    while head < tail {
        let idx = queue[head] as usize;
        head += 1;
        let (x, y) = (idx % S, idx / S);
        let d = dist[idx] + 1;
        let neighbours = [
            (x, y.wrapping_sub(1)),
            (x + 1, y),
            (x, y + 1),
            (x.wrapping_sub(1), y),
        ];
        let mut k = 0usize;
        while k < 4 {
            let (nx, ny) = neighbours[k];
            k += 1;
            if nx >= S || ny >= S || (walkable[ny] >> nx) & 1 == 0 {
                continue;
            }
            let nidx = ny * S + nx;
            if dist[nidx] != HPA_UNREACHABLE {
                continue;
            }
            dist[nidx] = d;
            parent[nidx] = idx as u16;
            queue[tail] = nidx as u16;
            tail += 1;
        }
    }
}

// ---------------------------------------------------------------------------
// Macro open list
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
struct HpaNode {
    f: u32,
    node: u16,
    _pad: u16,
    tie: u64,
}

impl HpaNode {
    #[inline(always)]
    fn less(&self, other: &HpaNode) -> bool {
        (self.f, self.tie, self.node) < (other.f, other.tie, other.node)
    }
}

#[derive(Debug, Clone, Copy)]
struct HpaHeap {
    nodes: [HpaNode; HPA_HEAP_CAPACITY],
    len: usize,
}

impl HpaHeap {
    const fn new() -> Self {
        Self {
            nodes: [HpaNode {
                f: 0,
                node: 0,
                _pad: 0,
                tie: 0,
            }; HPA_HEAP_CAPACITY],
            len: 0,
        }
    }

    #[inline(always)]
    fn clear(&mut self) {
        self.len = 0;
    }

    fn push(&mut self, node: HpaNode) -> bool {
        if self.len >= HPA_HEAP_CAPACITY {
            return false;
        }
        let mut idx = self.len;
        self.nodes[idx] = node;
        self.len += 1;
        while idx > 0 {
            let parent = (idx - 1) / 2;
            if self.nodes[idx].less(&self.nodes[parent]) {
                self.nodes.swap(idx, parent);
                idx = parent;
            } else {
                break;
            }
        }
        true
    }

    fn pop(&mut self) -> Option<HpaNode> {
        if self.len == 0 {
            return None;
        }
        let top = self.nodes[0];
        self.len -= 1;
        if self.len > 0 {
            self.nodes[0] = self.nodes[self.len];
            let mut idx = 0usize;
            loop {
                let left = 2 * idx + 1;
                let right = left + 1;
                let mut smallest = idx;
                if left < self.len && self.nodes[left].less(&self.nodes[smallest]) {
                    smallest = left;
                }
                if right < self.len && self.nodes[right].less(&self.nodes[smallest]) {
                    smallest = right;
                }
                if smallest == idx {
                    break;
                }
                self.nodes.swap(idx, smallest);
                idx = smallest;
            }
        }
        Some(top)
    }
}

// ---------------------------------------------------------------------------
// Hierarchical pathfinder
// ---------------------------------------------------------------------------

/// Running counters for diagnostics and benchmarks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(C)]
pub struct HpaStats {
    pub queries: u64,
    pub cache_builds: u64,
    pub cache_hits: u64,
    pub cache_evictions: u64,
    pub nodes_expanded: u64,
    pub heap_overflows: u64,
    pub failures: u64,
    pub last_nodes_expanded: u32,
    pub last_cost: u32,
}

/// Zero-heap two-level HPA* planner over a [`ChunkManager`].
///
/// `S` is the chunk edge (<= 32), `MAX_CHUNKS` the number of chunk caches held
/// simultaneously, `MAX_PORTALS` the macro node capacity which must be at least
/// `MAX_CHUNKS * MAX_PORTALS_PER_CHUNK` (node id = cache slot × 16 + portal slot).
#[derive(Debug, Clone)]
#[repr(C, align(64))]
pub struct HierarchicalPathfinder<
    const S: usize = 32,
    const MAX_CHUNKS: usize = DEFAULT_HPA_MAX_CHUNKS,
    const MAX_PORTALS: usize = DEFAULT_HPA_MAX_PORTALS,
> {
    pub caches: [ChunkPortalCache<S>; MAX_CHUNKS],
    pub stats: HpaStats,
    index: [u16; HPA_INDEX_BUCKETS],
    g_score: [u32; MAX_PORTALS],
    came_from: [u16; MAX_PORTALS],
    visited_epoch: [u32; MAX_PORTALS],
    closed_epoch: [u32; MAX_PORTALS],
    heap: HpaHeap,
    epoch: u32,
    query_counter: u64,
}

impl<const S: usize, const MAX_CHUNKS: usize, const MAX_PORTALS: usize> Default
    for HierarchicalPathfinder<S, MAX_CHUNKS, MAX_PORTALS>
{
    fn default() -> Self {
        Self::new()
    }
}

impl<const S: usize, const MAX_CHUNKS: usize, const MAX_PORTALS: usize>
    HierarchicalPathfinder<S, MAX_CHUNKS, MAX_PORTALS>
{
    /// Creates an empty planner (all slabs allocated inline, nothing on the heap).
    pub fn new() -> Self {
        assert!(S <= HPA_MAX_CHUNK_SIZE, "HPA chunk size must be <= 32");
        assert!(
            MAX_CHUNKS < HPA_INDEX_BUCKETS,
            "MAX_CHUNKS must be smaller than HPA_INDEX_BUCKETS"
        );
        assert!(
            MAX_PORTALS >= MAX_CHUNKS * MAX_PORTALS_PER_CHUNK && MAX_PORTALS < u16::MAX as usize,
            "MAX_PORTALS must cover MAX_CHUNKS * MAX_PORTALS_PER_CHUNK node ids"
        );
        Self {
            caches: [ChunkPortalCache::<S>::empty(); MAX_CHUNKS],
            stats: HpaStats::default(),
            index: [INDEX_EMPTY; HPA_INDEX_BUCKETS],
            g_score: [u32::MAX; MAX_PORTALS],
            came_from: [HPA_NO_NODE; MAX_PORTALS],
            visited_epoch: [0; MAX_PORTALS],
            closed_epoch: [0; MAX_PORTALS],
            heap: HpaHeap::new(),
            epoch: 1,
            query_counter: 0,
        }
    }

    /// Integer Manhattan distance between world tiles, saturated to `u32`.
    #[inline(always)]
    pub fn manhattan(a: (i64, i64), b: (i64, i64)) -> u32 {
        let d = (a.0 - b.0).unsigned_abs().saturating_add((a.1 - b.1).unsigned_abs());
        if d > u32::MAX as u64 {
            u32::MAX
        } else {
            d as u32
        }
    }

    // ---- cache slab --------------------------------------------------------

    #[inline(always)]
    fn bucket_of(coord: ChunkCoord) -> usize {
        (coord.bucket_hash() % (HPA_INDEX_BUCKETS as u64)) as usize
    }

    fn rebuild_index(&mut self) {
        self.index = [INDEX_EMPTY; HPA_INDEX_BUCKETS];
        let mut i = 0usize;
        while i < MAX_CHUNKS {
            if self.caches[i].active {
                let coord = self.caches[i].coord;
                self.index_insert(coord, i);
            }
            i += 1;
        }
    }

    fn index_insert(&mut self, coord: ChunkCoord, slot: usize) {
        let mut b = Self::bucket_of(coord);
        let mut probes = 0usize;
        while probes < HPA_INDEX_BUCKETS {
            if self.index[b] == INDEX_EMPTY {
                self.index[b] = slot as u16;
                return;
            }
            b = (b + 1) % HPA_INDEX_BUCKETS;
            probes += 1;
        }
    }

    /// Slot of the cache for `coord`, if resident.
    pub fn find_cache(&self, coord: ChunkCoord) -> Option<usize> {
        let mut b = Self::bucket_of(coord);
        let mut probes = 0usize;
        while probes < HPA_INDEX_BUCKETS {
            let e = self.index[b];
            if e == INDEX_EMPTY {
                return None;
            }
            let slot = e as usize;
            if slot < MAX_CHUNKS && self.caches[slot].active && self.caches[slot].coord == coord {
                return Some(slot);
            }
            b = (b + 1) % HPA_INDEX_BUCKETS;
            probes += 1;
        }
        None
    }

    /// Resident cache for `coord` (may be stale until the next query touches it).
    pub fn cache_for(&self, coord: ChunkCoord) -> Option<&ChunkPortalCache<S>> {
        self.find_cache(coord).map(|i| &self.caches[i])
    }

    /// Number of resident chunk caches.
    pub fn cached_chunk_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < MAX_CHUNKS {
            if self.caches[i].active {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// Drops the cache of `coord` (call after editing tiles of an unloaded chunk,
    /// or of a loaded chunk's neighbour that is itself unloaded).
    pub fn invalidate_chunk(&mut self, coord: ChunkCoord) {
        if let Some(i) = self.find_cache(coord) {
            self.caches[i].active = false;
            self.rebuild_index();
        }
    }

    /// Drops every cache and resets the search scratch (stats are kept).
    pub fn clear(&mut self) {
        let mut i = 0usize;
        while i < MAX_CHUNKS {
            self.caches[i].active = false;
            self.caches[i].search_epoch = 0;
            i += 1;
        }
        self.index = [INDEX_EMPTY; HPA_INDEX_BUCKETS];
        self.heap.clear();
    }

    /// Combined modification stamp of `coord` and its 4 neighbours (loaded chunks
    /// contribute their `modification_count`; unloaded chunks contribute 0, which
    /// equals a pristine loaded chunk so streaming alone never forces a rebuild).
    fn stamp<const MC: usize, const DC: usize>(
        coord: ChunkCoord,
        chunks: &ChunkManager<S, MC, DC>,
    ) -> u64 {
        let ring = [
            coord,
            ChunkCoord::new(coord.x, coord.y.wrapping_sub(1)),
            ChunkCoord::new(coord.x.wrapping_add(1), coord.y),
            ChunkCoord::new(coord.x, coord.y.wrapping_add(1)),
            ChunkCoord::new(coord.x.wrapping_sub(1), coord.y),
        ];
        let mut h = 0u64;
        let mut i = 0usize;
        while i < 5 {
            let m = match chunks.chunk_at(ring[i]) {
                Some(c) => c.header.modification_count as u64,
                None => 0,
            };
            h = h.wrapping_mul(0x100000001b3) ^ m;
            i += 1;
        }
        h
    }

    /// Returns the slot of an up-to-date cache for `coord`, building or
    /// revalidating it as needed. `None` only when every slot is pinned by the
    /// current query (slab exhausted).
    fn acquire_cache<const MC: usize, const DC: usize>(
        &mut self,
        coord: ChunkCoord,
        chunks: &ChunkManager<S, MC, DC>,
    ) -> Option<usize> {
        let epoch = self.epoch;
        if let Some(i) = self.find_cache(coord) {
            if self.caches[i].search_epoch != epoch {
                let stamp = Self::stamp(coord, chunks);
                if stamp != self.caches[i].stamp {
                    self.caches[i].build(coord, chunks);
                    self.caches[i].stamp = stamp;
                    self.stats.cache_builds += 1;
                } else {
                    self.stats.cache_hits += 1;
                }
                self.caches[i].search_epoch = epoch;
            }
            self.caches[i].last_used = self.query_counter;
            return Some(i);
        }

        // Allocate: first inactive slot, else least-recently-used unpinned slot.
        let mut slot = usize::MAX;
        let mut oldest = u64::MAX;
        let mut i = 0usize;
        while i < MAX_CHUNKS {
            let c = &self.caches[i];
            if !c.active {
                slot = i;
                break;
            }
            if c.search_epoch != epoch && c.last_used < oldest {
                oldest = c.last_used;
                slot = i;
            }
            i += 1;
        }
        if slot == usize::MAX {
            return None;
        }
        if self.caches[slot].active {
            self.caches[slot].active = false;
            self.rebuild_index();
            self.stats.cache_evictions += 1;
        }
        let stamp = Self::stamp(coord, chunks);
        self.caches[slot].build(coord, chunks);
        self.caches[slot].stamp = stamp;
        self.caches[slot].search_epoch = epoch;
        self.caches[slot].last_used = self.query_counter;
        self.caches[slot].active = true;
        self.stats.cache_builds += 1;
        self.index_insert(coord, slot);
        Some(slot)
    }

    /// Builds (or revalidates) the cache for `coord` and returns it.
    pub fn build_cache<const MC: usize, const DC: usize>(
        &mut self,
        coord: ChunkCoord,
        chunks: &ChunkManager<S, MC, DC>,
    ) -> Option<&ChunkPortalCache<S>> {
        self.begin_query();
        let slot = self.acquire_cache(coord, chunks)?;
        Some(&self.caches[slot])
    }

    // ---- search ------------------------------------------------------------

    fn begin_query(&mut self) {
        self.query_counter += 1;
        self.epoch = self.epoch.wrapping_add(1);
        if self.epoch == 0 {
            self.visited_epoch = [0; MAX_PORTALS];
            self.closed_epoch = [0; MAX_PORTALS];
            let mut i = 0usize;
            while i < MAX_CHUNKS {
                self.caches[i].search_epoch = 0;
                i += 1;
            }
            self.epoch = 1;
        }
        self.heap.clear();
    }

    #[inline]
    fn relax(&mut self, from: u16, to: usize, g: u32, h: u32, tie: u64) {
        let epoch = self.epoch;
        if self.visited_epoch[to] == epoch && g >= self.g_score[to] {
            return;
        }
        self.visited_epoch[to] = epoch;
        self.g_score[to] = g;
        self.came_from[to] = from;
        let pushed = self.heap.push(HpaNode {
            f: g.saturating_add(h),
            node: to as u16,
            _pad: 0,
            tie,
        });
        if !pushed {
            self.stats.heap_overflows += 1;
        }
    }

    /// Plans a route from world tile `start` to `goal`.
    ///
    /// Writes `start`, the centre tile of every portal crossed (twin portals appear
    /// as two adjacent waypoints) and `goal` into `out_waypoints`; consecutive
    /// waypoints are either in the same chunk and mutually reachable inside it, or
    /// one Manhattan step apart across a chunk boundary (see [`Self::local_route`]).
    /// Returns the waypoint count, or 0 when no route exists, an endpoint is not
    /// walkable, or `out_waypoints` is too small.
    pub fn find_path<const MC: usize, const DC: usize>(
        &mut self,
        start: (i64, i64),
        goal: (i64, i64),
        chunks: &ChunkManager<S, MC, DC>,
        out_waypoints: &mut [(i64, i64)],
    ) -> usize {
        self.stats.queries += 1;
        self.stats.last_nodes_expanded = 0;
        self.stats.last_cost = 0;
        self.begin_query();
        let epoch = self.epoch;

        if out_waypoints.is_empty()
            || !chunks.tile_at(start.0, start.1).is_walkable()
            || !chunks.tile_at(goal.0, goal.1).is_walkable()
        {
            self.stats.failures += 1;
            return 0;
        }
        if start == goal {
            out_waypoints[0] = start;
            return 1;
        }

        let (sc, slx, sly) = ChunkCoord::from_world_tile(start.0, start.1, S);
        let (gc, glx, gly) = ChunkCoord::from_world_tile(goal.0, goal.1, S);

        let Some(si) = self.acquire_cache(sc, chunks) else {
            self.stats.failures += 1;
            return 0;
        };
        let Some(gi) = self.acquire_cache(gc, chunks) else {
            self.stats.failures += 1;
            return 0;
        };

        // Distances from the goal to every tile of the goal chunk (undirected BFS).
        let mut goal_dist = [HPA_UNREACHABLE; HPA_CHUNK_TILES_MAX];
        let mut dist = [HPA_UNREACHABLE; HPA_CHUNK_TILES_MAX];
        let mut parent = [HPA_NO_NODE; HPA_CHUNK_TILES_MAX];
        let mut queue = [0u16; HPA_CHUNK_TILES_MAX];
        chunk_bfs::<S>(&self.caches[gi].walkable, glx, gly, &mut goal_dist, &mut parent, &mut queue);
        chunk_bfs::<S>(&self.caches[si].walkable, slx, sly, &mut dist, &mut parent, &mut queue);

        let mut best_goal_g = u32::MAX;
        let mut best_goal_pred = HPA_NO_NODE;
        if sc == gc {
            let d = goal_dist[sly * S + slx];
            if d != HPA_UNREACHABLE {
                best_goal_g = d as u32;
            }
        }

        // Seed: start -> every reachable portal of the start chunk.
        {
            let count = self.caches[si].portal_count as usize;
            let portals = self.caches[si].portals;
            let mut p = 0usize;
            while p < count {
                let (lx, ly) = portals[p].center_local(S);
                let d = dist[ly * S + lx];
                if d != HPA_UNREACHABLE {
                    let h = Self::manhattan(portals[p].center_world(S), goal);
                    self.relax(
                        HPA_NO_NODE,
                        si * MAX_PORTALS_PER_CHUNK + p,
                        d as u32,
                        h,
                        portals[p].tie_key(),
                    );
                }
                p += 1;
            }
        }

        let mut expanded = 0u32;
        while let Some(top) = self.heap.pop() {
            if top.f >= best_goal_g {
                break;
            }
            let node = top.node as usize;
            if self.closed_epoch[node] == epoch {
                continue;
            }
            self.closed_epoch[node] = epoch;
            expanded += 1;

            let ci = node / MAX_PORTALS_PER_CHUNK;
            let pi = node % MAX_PORTALS_PER_CHUNK;
            let g = self.g_score[node];
            let count = self.caches[ci].portal_count as usize;
            let portals = self.caches[ci].portals;
            let row = self.caches[ci].intra_dist[pi];
            let portal = portals[pi];

            // Goal link.
            if ci == gi {
                let (lx, ly) = portal.center_local(S);
                let d = goal_dist[ly * S + lx];
                if d != HPA_UNREACHABLE {
                    let total = g.saturating_add(d as u32);
                    if total < best_goal_g {
                        best_goal_g = total;
                        best_goal_pred = node as u16;
                    }
                }
            }

            // Intra-chunk edges.
            let mut q = 0usize;
            while q < count {
                let d = row[q];
                if q != pi && d != HPA_UNREACHABLE {
                    let h = Self::manhattan(portals[q].center_world(S), goal);
                    self.relax(
                        node as u16,
                        ci * MAX_PORTALS_PER_CHUNK + q,
                        g.saturating_add(d as u32),
                        h,
                        portals[q].tie_key(),
                    );
                }
                q += 1;
            }

            // Twin edge across the boundary (cost 1).
            let twin = portal.twin();
            if let Some(ni) = self.acquire_cache(twin.chunk, chunks) {
                if let Some(qs) = self.caches[ni].portal_slot(twin.edge, twin.start_offset) {
                    let tp = self.caches[ni].portals[qs];
                    let h = Self::manhattan(tp.center_world(S), goal);
                    self.relax(
                        node as u16,
                        ni * MAX_PORTALS_PER_CHUNK + qs,
                        g.saturating_add(1),
                        h,
                        tp.tie_key(),
                    );
                }
            }
        }

        self.stats.nodes_expanded += expanded as u64;
        self.stats.last_nodes_expanded = expanded;

        if best_goal_g == u32::MAX {
            self.stats.failures += 1;
            return 0;
        }
        self.stats.last_cost = best_goal_g;

        // Reconstruct portal chain (goal side first).
        let mut chain = [HPA_NO_NODE; HPA_MAX_WAYPOINTS];
        let mut chain_len = 0usize;
        let mut node = best_goal_pred;
        while node != HPA_NO_NODE {
            if chain_len >= HPA_MAX_WAYPOINTS {
                self.stats.failures += 1;
                return 0;
            }
            chain[chain_len] = node;
            chain_len += 1;
            node = self.came_from[node as usize];
        }

        let needed = chain_len + 2;
        if out_waypoints.len() < needed {
            self.stats.failures += 1;
            return 0;
        }
        out_waypoints[0] = start;
        let mut i = 0usize;
        while i < chain_len {
            let n = chain[chain_len - 1 - i] as usize;
            let p = self.caches[n / MAX_PORTALS_PER_CHUNK].portals[n % MAX_PORTALS_PER_CHUNK];
            out_waypoints[1 + i] = p.center_world(S);
            i += 1;
        }
        out_waypoints[needed - 1] = goal;
        needed
    }

    /// Phase 2 steering: exact 4-connected tile route between two waypoints.
    ///
    /// Both tiles must lie in the same chunk (BFS over the cached walkability
    /// mask), or be one Manhattan step apart across a boundary. Writes the tiles
    /// from `from` to `to` inclusive and returns the count (0 when unreachable,
    /// unsupported, or `out_tiles` is too small).
    pub fn local_route<const MC: usize, const DC: usize>(
        &mut self,
        from: (i64, i64),
        to: (i64, i64),
        chunks: &ChunkManager<S, MC, DC>,
        out_tiles: &mut [(i64, i64)],
    ) -> usize {
        if out_tiles.is_empty() {
            return 0;
        }
        if from == to {
            out_tiles[0] = from;
            return 1;
        }
        let (fc, flx, fly) = ChunkCoord::from_world_tile(from.0, from.1, S);
        let (tc, tlx, tly) = ChunkCoord::from_world_tile(to.0, to.1, S);
        if fc != tc {
            if Self::manhattan(from, to) == 1
                && out_tiles.len() >= 2
                && chunks.tile_at(from.0, from.1).is_walkable()
                && chunks.tile_at(to.0, to.1).is_walkable()
            {
                out_tiles[0] = from;
                out_tiles[1] = to;
                return 2;
            }
            return 0;
        }

        self.begin_query();
        let Some(ci) = self.acquire_cache(fc, chunks) else {
            return 0;
        };
        let mut dist = [HPA_UNREACHABLE; HPA_CHUNK_TILES_MAX];
        let mut parent = [HPA_NO_NODE; HPA_CHUNK_TILES_MAX];
        let mut queue = [0u16; HPA_CHUNK_TILES_MAX];
        // BFS from the target so parents point towards it; walk forward from `from`.
        chunk_bfs::<S>(&self.caches[ci].walkable, tlx, tly, &mut dist, &mut parent, &mut queue);
        let start_idx = fly * S + flx;
        let d = dist[start_idx];
        if d == HPA_UNREACHABLE {
            return 0;
        }
        let len = d as usize + 1;
        if out_tiles.len() < len {
            return 0;
        }
        let origin_x = fc.x as i64 * S as i64;
        let origin_y = fc.y as i64 * S as i64;
        let mut idx = start_idx;
        let mut i = 0usize;
        while i < len {
            out_tiles[i] = (origin_x + (idx % S) as i64, origin_y + (idx / S) as i64);
            if idx == tly * S + tlx {
                break;
            }
            idx = parent[idx] as usize;
            i += 1;
        }
        len
    }

    /// Full tile-level route: macro plan followed by local steering between every
    /// pair of waypoints. Returns the tile count written to `out_tiles` (0 on failure).
    pub fn find_tile_path<const MC: usize, const DC: usize>(
        &mut self,
        start: (i64, i64),
        goal: (i64, i64),
        chunks: &ChunkManager<S, MC, DC>,
        out_tiles: &mut [(i64, i64)],
    ) -> usize {
        let mut wps = [(0i64, 0i64); HPA_MAX_WAYPOINTS];
        let n = self.find_path(start, goal, chunks, &mut wps);
        if n == 0 || out_tiles.is_empty() {
            return 0;
        }
        if n == 1 {
            out_tiles[0] = start;
            return 1;
        }
        let mut written = 0usize;
        let mut i = 0usize;
        while i + 1 < n {
            let base = if written == 0 { 0 } else { written - 1 };
            let k = self.local_route(wps[i], wps[i + 1], chunks, &mut out_tiles[base..]);
            if k == 0 {
                return 0;
            }
            written = base + k;
            i += 1;
        }
        written
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_portal_geometry_and_twins() {
        assert_eq!(core::mem::size_of::<Portal>(), 16);
        assert_eq!(core::mem::align_of::<ChunkPortalCache<32>>(), 64);
        let p = Portal::new(ChunkCoord::new(3, -2), PORTAL_EDGE_SOUTH, 4, 9);
        assert_eq!(p.len(), 6);
        assert_eq!(p.center_offset(), 6);
        assert_eq!(p.center_world(32), (3 * 32 + 6, -2 * 32 + 31));
        let t = p.twin();
        assert_eq!(t.chunk, ChunkCoord::new(3, -1));
        assert_eq!(t.edge, PORTAL_EDGE_NORTH);
        assert_eq!(t.center_world(32), (3 * 32 + 6, -1 * 32));
        assert_eq!(t.twin(), p);
        let e = Portal::new(ChunkCoord::new(0, 0), PORTAL_EDGE_EAST, 31, 31);
        assert_eq!(e.center_world(32), (31, 31));
        assert_eq!(e.twin().center_world(32), (32, 31));
        let w = Portal::new(ChunkCoord::new(0, 0), PORTAL_EDGE_WEST, 0, 0);
        assert_eq!(w.twin().chunk, ChunkCoord::new(-1, 0));
        assert_eq!(w.twin().center_world(32), (-1, 0));
    }

    #[test]
    fn test_chunk_bfs_mask() {
        let mut walk = [u32::MAX; 32];
        walk[5] = 1 << 0; // row 5 blocked except column 0
        let mut dist = [HPA_UNREACHABLE; HPA_CHUNK_TILES_MAX];
        let mut parent = [HPA_NO_NODE; HPA_CHUNK_TILES_MAX];
        let mut queue = [0u16; HPA_CHUNK_TILES_MAX];
        chunk_bfs::<32>(&walk, 10, 0, &mut dist, &mut parent, &mut queue);
        assert_eq!(dist[0], 10);
        // Must detour through (0, 5): 10 left + 10 down + 10 right = 30.
        assert_eq!(dist[10 * 32 + 10], 30);
        assert_eq!(dist[5 * 32 + 10], HPA_UNREACHABLE);
    }
}
