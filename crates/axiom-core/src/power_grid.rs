//! # Pure-Integer Power Grid (T-074)
//!
//! Production / consumption / storage electricity simulation for the factory layer:
//!
//! - [`PowerRole`]: generator, consumer, accumulator or pole.
//! - [`PowerNode`]: 16-byte POD record (4 per cache line) describing one powered structure.
//! - [`PowerNetwork`]: 64-byte per-network ledger (production, demand, delivered energy,
//!   accumulator charge / discharge, satisfaction percentage, brownout counter).
//! - [`PowerGridManager`]: fixed-slab, 64-byte aligned manager that clusters poles into
//!   independent networks (union-find over wire reach / supply coverage) and settles every
//!   network each tick with integer arithmetic only.
//!
//! Settlement rules (per network, per tick):
//! 1. `demand = Σ consumer.capacity`, `generation = Σ generator.capacity`.
//! 2. Surplus (`generation >= demand`): consumers are fully served; the surplus charges
//!    accumulators in slot order, each limited by its `flow_rate` and free storage.
//!    Energy that no accumulator can absorb is reported as `wasted`.
//! 3. Deficit (`generation < demand`): accumulators discharge in slot order (limited by
//!    `flow_rate` and stored energy). `satisfaction_pct = min(100, available * 100 / demand)`.
//!    Each consumer receives `capacity * available / demand`; the integer remainder is handed
//!    out one unit at a time in slot order so that `Σ delivered == available` exactly.
//!    The network is in **brownout** and consumers run at `satisfaction_pct` efficiency.
//!
//! Axioms: no floating point, zero heap allocation, deterministic slot-order iteration,
//! FNV-1a state hash.

use core::hash::Hasher;

use crate::hash::Fnv1aHasher;

/// Default number of independent networks tracked by a manager.
pub const DEFAULT_MAX_POWER_NETS: usize = 16;
/// Default number of powered structures tracked by a manager.
pub const DEFAULT_MAX_POWER_NODES: usize = 256;
/// Network id meaning "not connected to any powered network".
pub const NO_NET: u8 = u8::MAX;
/// Hard ceiling for `MAX_NETS` (ids are `u8`, `NO_NET` is reserved).
pub const POWER_NET_ID_LIMIT: usize = NO_NET as usize;

/// Role of a structure inside the electrical network.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u8)]
pub enum PowerRole {
    /// Produces up to `capacity` energy units per tick (steam engine, solar panel).
    #[default]
    Generator = 0,
    /// Demands `capacity` energy units per tick (drill, smelter, laser turret).
    Consumer = 1,
    /// Stores up to `capacity` units, charging / discharging at most `flow_rate` per tick.
    Accumulator = 2,
    /// Distributes power: `capacity` = supply radius (Chebyshev tiles), `flow_rate` = wire reach.
    Pole = 3,
}

impl PowerRole {
    /// Decode a raw discriminant (used by deserialisation); `None` for invalid values.
    #[inline(always)]
    pub const fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Generator),
            1 => Some(Self::Consumer),
            2 => Some(Self::Accumulator),
            3 => Some(Self::Pole),
            _ => None,
        }
    }
}

/// One powered structure (16 bytes, POD, four per cache line).
///
/// Field semantics depend on [`PowerRole`]:
///
/// | role        | `capacity`              | `flow_rate`                        | `energy_stored` |
/// |-------------|-------------------------|------------------------------------|-----------------|
/// | Generator   | max output / tick       | output actually drawn last tick    | 0               |
/// | Consumer    | demand / tick           | energy delivered last tick         | 0               |
/// | Accumulator | storage capacity        | max charge / discharge per tick    | stored energy   |
/// | Pole        | supply radius (tiles)   | wire reach (tiles)                 | 0               |
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(C)]
pub struct PowerNode {
    pub x: i32,
    pub y: i32,
    pub role: PowerRole,
    /// Network id assigned by the last rebuild ([`NO_NET`] when unpowered).
    pub net: u8,
    pub capacity: u16,
    pub flow_rate: u16,
    pub energy_stored: u16,
}

impl PowerNode {
    /// Generator producing `output` units per tick.
    #[inline]
    pub const fn generator(x: i32, y: i32, output: u16) -> Self {
        Self { x, y, role: PowerRole::Generator, net: NO_NET, capacity: output, flow_rate: 0, energy_stored: 0 }
    }

    /// Consumer demanding `demand` units per tick.
    #[inline]
    pub const fn consumer(x: i32, y: i32, demand: u16) -> Self {
        Self { x, y, role: PowerRole::Consumer, net: NO_NET, capacity: demand, flow_rate: 0, energy_stored: 0 }
    }

    /// Accumulator storing up to `storage` units, transferring at most `rate` per tick.
    #[inline]
    pub const fn accumulator(x: i32, y: i32, storage: u16, rate: u16) -> Self {
        Self { x, y, role: PowerRole::Accumulator, net: NO_NET, capacity: storage, flow_rate: rate, energy_stored: 0 }
    }

    /// Pole covering structures within `supply_radius` and wiring to poles within `wire_reach`.
    #[inline]
    pub const fn pole(x: i32, y: i32, supply_radius: u16, wire_reach: u16) -> Self {
        Self { x, y, role: PowerRole::Pole, net: NO_NET, capacity: supply_radius, flow_rate: wire_reach, energy_stored: 0 }
    }

    #[inline(always)]
    fn hash_into(&self, h: &mut Fnv1aHasher) {
        h.write(&self.x.to_le_bytes());
        h.write(&self.y.to_le_bytes());
        h.write(&[self.role as u8, self.net]);
        h.write(&self.capacity.to_le_bytes());
        h.write(&self.flow_rate.to_le_bytes());
        h.write(&self.energy_stored.to_le_bytes());
    }
}

/// Chebyshev distance between two nodes (never overflows: widened to `i64`).
#[inline(always)]
fn chebyshev(a: &PowerNode, b: &PowerNode) -> i64 {
    let dx = (a.x as i64 - b.x as i64).abs();
    let dy = (a.y as i64 - b.y as i64).abs();
    if dx > dy { dx } else { dy }
}

/// Per-network settlement ledger for the last tick (64 bytes, one cache line).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(C, align(64))]
pub struct PowerNetwork {
    pub id: u8,
    /// `min(100, available * 100 / demand)`; 100 when demand is zero.
    pub satisfaction_pct: u8,
    pub node_count: u16,
    pub generator_count: u16,
    pub consumer_count: u16,
    pub accumulator_count: u16,
    pub pole_count: u16,
    /// Σ generator capacity (nameplate).
    pub generation_capacity: u32,
    /// Energy actually drawn from generators this tick.
    pub produced: u32,
    /// Σ consumer demand.
    pub demand: u32,
    /// Energy delivered to consumers this tick.
    pub delivered: u32,
    /// Energy pushed into accumulators this tick.
    pub charged: u32,
    /// Energy pulled from accumulators this tick.
    pub discharged: u32,
    /// Σ accumulator stored energy after settlement.
    pub stored: u32,
    /// Σ accumulator capacity.
    pub storage_capacity: u32,
    /// Surplus generation that neither consumers nor accumulators could take.
    pub wasted: u32,
    /// Unmet demand this tick.
    pub deficit: u32,
    /// Ticks this network has spent below 100% satisfaction (since last rebuild).
    pub brownout_ticks: u32,
    _reserved: u32,
}

impl PowerNetwork {
    #[inline(always)]
    fn reset_tick(&mut self) {
        self.generation_capacity = 0;
        self.produced = 0;
        self.demand = 0;
        self.delivered = 0;
        self.charged = 0;
        self.discharged = 0;
        self.stored = 0;
        self.storage_capacity = 0;
        self.wasted = 0;
        self.deficit = 0;
        self.satisfaction_pct = 100;
    }

    #[inline(always)]
    fn hash_into(&self, h: &mut Fnv1aHasher) {
        h.write(&[self.id, self.satisfaction_pct]);
        h.write(&self.node_count.to_le_bytes());
        h.write(&self.generator_count.to_le_bytes());
        h.write(&self.consumer_count.to_le_bytes());
        h.write(&self.accumulator_count.to_le_bytes());
        h.write(&self.pole_count.to_le_bytes());
        h.write(&self.generation_capacity.to_le_bytes());
        h.write(&self.produced.to_le_bytes());
        h.write(&self.demand.to_le_bytes());
        h.write(&self.delivered.to_le_bytes());
        h.write(&self.charged.to_le_bytes());
        h.write(&self.discharged.to_le_bytes());
        h.write(&self.stored.to_le_bytes());
        h.write(&self.storage_capacity.to_le_bytes());
        h.write(&self.wasted.to_le_bytes());
        h.write(&self.deficit.to_le_bytes());
        h.write(&self.brownout_ticks.to_le_bytes());
    }
}

/// Fixed-slab power grid manager (64-byte aligned, zero heap).
///
/// Networks are rebuilt lazily on the next [`tick`](Self::tick) after any topology change
/// (`add_node` / `remove_node` / `set_position`). Two poles are wired when their Chebyshev
/// distance is within the larger of their wire reaches; a non-pole structure joins every
/// pole whose supply radius covers it (bridging those poles into one network). Clusters
/// without a pole are unpowered ([`NO_NET`]).
#[derive(Debug, Clone)]
#[repr(C, align(64))]
pub struct PowerGridManager<const MAX_NETS: usize = DEFAULT_MAX_POWER_NETS, const MAX_NODES: usize = DEFAULT_MAX_POWER_NODES> {
    nodes: [PowerNode; MAX_NODES],
    nets: [PowerNetwork; MAX_NETS],
    /// Union-find scratch (rebuilt on demand).
    parent: [u16; MAX_NODES],
    node_count: u16,
    net_count: u8,
    dirty: bool,
    /// Clusters that could not receive a network id because `MAX_NETS` was exhausted.
    overflow_clusters: u16,
    tick: u64,
}

impl<const MAX_NETS: usize, const MAX_NODES: usize> Default for PowerGridManager<MAX_NETS, MAX_NODES> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const MAX_NETS: usize, const MAX_NODES: usize> PowerGridManager<MAX_NETS, MAX_NODES> {
    pub fn new() -> Self {
        assert!(MAX_NETS <= POWER_NET_ID_LIMIT, "MAX_NETS must fit in a u8 below NO_NET");
        assert!(MAX_NODES <= u16::MAX as usize, "MAX_NODES must fit in u16 indices");
        Self {
            nodes: [PowerNode::default(); MAX_NODES],
            nets: [PowerNetwork::default(); MAX_NETS],
            parent: [0u16; MAX_NODES],
            node_count: 0,
            net_count: 0,
            dirty: false,
            overflow_clusters: 0,
            tick: 0,
        }
    }

    // -- topology ---------------------------------------------------------------------------

    /// Insert a node; returns its slot index, or `None` when the slab is full.
    pub fn add_node(&mut self, mut node: PowerNode) -> Option<u16> {
        let idx = self.node_count as usize;
        if idx >= MAX_NODES {
            return None;
        }
        node.net = NO_NET;
        if node.role == PowerRole::Accumulator && node.energy_stored > node.capacity {
            node.energy_stored = node.capacity;
        }
        if node.role != PowerRole::Accumulator {
            node.energy_stored = 0;
        }
        if node.role == PowerRole::Generator || node.role == PowerRole::Consumer {
            node.flow_rate = 0;
        }
        self.nodes[idx] = node;
        self.node_count += 1;
        self.dirty = true;
        Some(idx as u16)
    }

    /// Remove a node by swapping the last slot into its place. Returns the removed node.
    /// Indices of other nodes stay valid except the former last slot, which moves to `idx`.
    pub fn remove_node(&mut self, idx: u16) -> Option<PowerNode> {
        let i = idx as usize;
        let n = self.node_count as usize;
        if i >= n {
            return None;
        }
        let removed = self.nodes[i];
        let last = n - 1;
        if i != last {
            self.nodes[i] = self.nodes[last];
        }
        self.nodes[last] = PowerNode::default();
        self.node_count -= 1;
        self.dirty = true;
        Some(removed)
    }

    /// Find the first node at `(x, y)` in slot order.
    pub fn find_node(&self, x: i32, y: i32) -> Option<u16> {
        let n = self.node_count as usize;
        let mut i = 0usize;
        while i < n {
            let nd = &self.nodes[i];
            if nd.x == x && nd.y == y {
                return Some(i as u16);
            }
            i += 1;
        }
        None
    }

    /// Move a node (marks the topology dirty).
    pub fn set_position(&mut self, idx: u16, x: i32, y: i32) -> bool {
        let i = idx as usize;
        if i >= self.node_count as usize {
            return false;
        }
        self.nodes[i].x = x;
        self.nodes[i].y = y;
        self.dirty = true;
        true
    }

    /// Change a node's `capacity` (demand / output / storage / supply radius) without a rebuild
    /// unless the node is a pole (coverage changes topology).
    pub fn set_capacity(&mut self, idx: u16, capacity: u16) -> bool {
        let i = idx as usize;
        if i >= self.node_count as usize {
            return false;
        }
        let nd = &mut self.nodes[i];
        nd.capacity = capacity;
        match nd.role {
            PowerRole::Pole => self.dirty = true,
            PowerRole::Accumulator => {
                if nd.energy_stored > capacity {
                    nd.energy_stored = capacity;
                }
            }
            _ => {}
        }
        true
    }

    #[inline(always)]
    pub fn node(&self, idx: u16) -> Option<&PowerNode> {
        if (idx as usize) < self.node_count as usize { Some(&self.nodes[idx as usize]) } else { None }
    }

    #[inline(always)]
    pub fn nodes(&self) -> &[PowerNode] {
        &self.nodes[..self.node_count as usize]
    }

    #[inline(always)]
    pub fn node_count(&self) -> usize {
        self.node_count as usize
    }

    #[inline(always)]
    pub fn network(&self, id: u8) -> Option<&PowerNetwork> {
        if (id as usize) < self.net_count as usize { Some(&self.nets[id as usize]) } else { None }
    }

    #[inline(always)]
    pub fn networks(&self) -> &[PowerNetwork] {
        &self.nets[..self.net_count as usize]
    }

    #[inline(always)]
    pub fn network_count(&self) -> usize {
        self.net_count as usize
    }

    /// Clusters dropped because `MAX_NETS` was exhausted during the last rebuild.
    #[inline(always)]
    pub fn overflow_clusters(&self) -> u16 {
        self.overflow_clusters
    }

    #[inline(always)]
    pub fn tick_count(&self) -> u64 {
        self.tick
    }

    #[inline(always)]
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Operating efficiency (0..=100) of the network a node belongs to.
    /// Consumers in brownout run at this percentage; unpowered nodes return 0.
    pub fn efficiency_pct(&self, idx: u16) -> u8 {
        match self.node(idx) {
            Some(nd) if nd.net != NO_NET => self.nets[nd.net as usize].satisfaction_pct,
            _ => 0,
        }
    }

    /// Network id of a node ([`NO_NET`] if unpowered or out of range).
    #[inline]
    pub fn net_of(&self, idx: u16) -> u8 {
        match self.node(idx) {
            Some(nd) => nd.net,
            None => NO_NET,
        }
    }

    // -- union-find --------------------------------------------------------------------------

    #[inline(always)]
    fn find_root(&mut self, mut i: usize) -> usize {
        while self.parent[i] as usize != i {
            let p = self.parent[i] as usize;
            let gp = self.parent[p];
            self.parent[i] = gp;
            i = p;
        }
        i
    }

    #[inline(always)]
    fn union(&mut self, a: usize, b: usize) {
        let ra = self.find_root(a);
        let rb = self.find_root(b);
        if ra == rb {
            return;
        }
        // Lower slot index becomes the root: deterministic, independent of union order.
        if ra < rb {
            self.parent[rb] = ra as u16;
        } else {
            self.parent[ra] = rb as u16;
        }
    }

    /// Recluster nodes into networks. Called automatically by [`tick`](Self::tick) when dirty.
    pub fn rebuild_networks(&mut self) {
        let n = self.node_count as usize;
        let mut i = 0usize;
        while i < n {
            self.parent[i] = i as u16;
            i += 1;
        }

        // Pole <-> pole wiring.
        i = 0;
        while i < n {
            if self.nodes[i].role == PowerRole::Pole {
                let mut j = i + 1;
                while j < n {
                    if self.nodes[j].role == PowerRole::Pole {
                        let reach_i = self.nodes[i].flow_rate as i64;
                        let reach_j = self.nodes[j].flow_rate as i64;
                        let reach = if reach_i > reach_j { reach_i } else { reach_j };
                        if chebyshev(&self.nodes[i], &self.nodes[j]) <= reach {
                            self.union(i, j);
                        }
                    }
                    j += 1;
                }
            }
            i += 1;
        }

        // Structure -> covering poles.
        i = 0;
        while i < n {
            if self.nodes[i].role != PowerRole::Pole {
                let mut p = 0usize;
                while p < n {
                    if self.nodes[p].role == PowerRole::Pole
                        && chebyshev(&self.nodes[i], &self.nodes[p]) <= self.nodes[p].capacity as i64
                    {
                        self.union(i, p);
                    }
                    p += 1;
                }
            }
            i += 1;
        }

        // Assign ids in slot order of the first member of each root. A cluster is a powered
        // network only if it contains at least one pole.
        let mut k = 0usize;
        while k < MAX_NETS {
            self.nets[k] = PowerNetwork::default();
            self.nets[k].id = k as u8;
            k += 1;
        }
        self.net_count = 0;
        self.overflow_clusters = 0;

        // First pass: mark roots that own a pole by storing a flag in the node `net` field.
        i = 0;
        while i < n {
            self.nodes[i].net = NO_NET;
            i += 1;
        }
        i = 0;
        while i < n {
            if self.nodes[i].role == PowerRole::Pole {
                let r = self.find_root(i);
                self.nodes[r].net = 0; // marker: "root has a pole"
            }
            i += 1;
        }
        // Second pass: assign ids. The root is always the lowest slot of its cluster, so it is
        // visited before any member; members simply copy the root's id.
        i = 0;
        while i < n {
            let r = self.find_root(i);
            let assigned = if r == i {
                if self.nodes[r].net == 0 {
                    // First visit of a powered cluster root.
                    if (self.net_count as usize) < MAX_NETS {
                        let id = self.net_count;
                        self.net_count += 1;
                        id
                    } else {
                        self.overflow_clusters = self.overflow_clusters.saturating_add(1);
                        NO_NET
                    }
                } else {
                    NO_NET
                }
            } else {
                self.nodes[r].net
            };
            self.nodes[i].net = assigned;
            if assigned != NO_NET {
                let net = &mut self.nets[assigned as usize];
                net.node_count += 1;
                match self.nodes[i].role {
                    PowerRole::Generator => net.generator_count += 1,
                    PowerRole::Consumer => net.consumer_count += 1,
                    PowerRole::Accumulator => net.accumulator_count += 1,
                    PowerRole::Pole => net.pole_count += 1,
                }
            }
            i += 1;
        }
        self.dirty = false;
    }

    // -- settlement --------------------------------------------------------------------------

    /// Advance one tick: rebuild networks if dirty, then settle every network.
    pub fn tick(&mut self) {
        if self.dirty {
            self.rebuild_networks();
        }
        self.tick = self.tick.wrapping_add(1);
        let n = self.node_count as usize;
        let nets = self.net_count as usize;

        let mut k = 0usize;
        while k < nets {
            self.nets[k].reset_tick();
            k += 1;
        }

        // Pass 1: totals.
        let mut i = 0usize;
        while i < n {
            let nd = &mut self.nodes[i];
            if nd.net != NO_NET {
                let net = &mut self.nets[nd.net as usize];
                match nd.role {
                    PowerRole::Generator => {
                        net.generation_capacity += nd.capacity as u32;
                        nd.flow_rate = 0;
                    }
                    PowerRole::Consumer => {
                        net.demand += nd.capacity as u32;
                        nd.flow_rate = 0;
                    }
                    PowerRole::Accumulator => {
                        net.storage_capacity += nd.capacity as u32;
                    }
                    PowerRole::Pole => {}
                }
            }
            i += 1;
        }

        // Pass 2: per-network balance decision (surplus charges / deficit discharges).
        k = 0;
        while k < nets {
            let net = &mut self.nets[k];
            if net.generation_capacity >= net.demand {
                net.delivered = net.demand;
                net.produced = net.demand; // raised by charging below
            }
            k += 1;
        }

        // Pass 3: accumulators (slot order).
        i = 0;
        while i < n {
            let nd = &mut self.nodes[i];
            if nd.net != NO_NET && nd.role == PowerRole::Accumulator {
                let net = &mut self.nets[nd.net as usize];
                if net.generation_capacity >= net.demand {
                    let surplus = net.generation_capacity - net.demand - net.charged;
                    let room = (nd.capacity - nd.energy_stored) as u32;
                    let mut charge = nd.flow_rate as u32;
                    if charge > room { charge = room; }
                    if charge > surplus { charge = surplus; }
                    nd.energy_stored += charge as u16;
                    net.charged += charge;
                } else {
                    let deficit = net.demand - net.generation_capacity - net.discharged;
                    let mut discharge = nd.flow_rate as u32;
                    if discharge > nd.energy_stored as u32 { discharge = nd.energy_stored as u32; }
                    if discharge > deficit { discharge = deficit; }
                    nd.energy_stored -= discharge as u16;
                    net.discharged += discharge;
                }
                net.stored += nd.energy_stored as u32;
            }
            i += 1;
        }

        // Pass 4: finalise network ledgers.
        k = 0;
        while k < nets {
            let net = &mut self.nets[k];
            if net.generation_capacity >= net.demand {
                net.produced = net.demand + net.charged;
                net.wasted = net.generation_capacity - net.produced;
                net.satisfaction_pct = 100;
            } else {
                net.produced = net.generation_capacity;
                let available = net.generation_capacity + net.discharged;
                net.delivered = available;
                net.deficit = net.demand - available;
                let pct = (available as u64 * 100) / net.demand as u64;
                net.satisfaction_pct = if pct > 100 { 100 } else { pct as u8 };
                if net.satisfaction_pct < 100 {
                    net.brownout_ticks = net.brownout_ticks.saturating_add(1);
                }
            }
            k += 1;
        }

        // Pass 5: distribute delivered energy to consumers and drawn output to generators.
        // Floor shares first, then hand the remainder out one unit per node in slot order so
        // that Σ consumer.flow_rate == delivered and Σ generator.flow_rate == produced.
        i = 0;
        while i < n {
            let nd = &mut self.nodes[i];
            if nd.net == NO_NET {
                i += 1;
                continue;
            }
            let net = &self.nets[nd.net as usize];
            match nd.role {
                PowerRole::Consumer if net.demand > 0 => {
                    nd.flow_rate = ((nd.capacity as u64 * net.delivered as u64) / net.demand as u64) as u16;
                }
                PowerRole::Generator if net.generation_capacity > 0 => {
                    nd.flow_rate =
                        ((nd.capacity as u64 * net.produced as u64) / net.generation_capacity as u64) as u16;
                }
                _ => {}
            }
            i += 1;
        }
        k = 0;
        while k < nets {
            let (delivered, demand, produced, gen_cap) = {
                let net = &self.nets[k];
                (net.delivered, net.demand, net.produced, net.generation_capacity)
            };
            let mut consumer_left = if demand > 0 { delivered - self.sum_flow(k, PowerRole::Consumer) } else { 0 };
            let mut gen_left = if gen_cap > 0 { produced - self.sum_flow(k, PowerRole::Generator) } else { 0 };
            let mut i = 0usize;
            while i < n && (consumer_left > 0 || gen_left > 0) {
                let nd = &mut self.nodes[i];
                if nd.net as usize == k {
                    if nd.role == PowerRole::Consumer && consumer_left > 0 && nd.flow_rate < nd.capacity {
                        nd.flow_rate += 1;
                        consumer_left -= 1;
                    } else if nd.role == PowerRole::Generator && gen_left > 0 && nd.flow_rate < nd.capacity {
                        nd.flow_rate += 1;
                        gen_left -= 1;
                    }
                }
                i += 1;
            }
            k += 1;
        }
    }

    fn sum_flow(&self, net_id: usize, role: PowerRole) -> u32 {
        let n = self.node_count as usize;
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < n {
            let nd = &self.nodes[i];
            if nd.net as usize == net_id && nd.role == role {
                sum += nd.flow_rate as u32;
            }
            i += 1;
        }
        sum
    }

    /// Σ stored energy across all accumulators (including unpowered ones).
    pub fn total_stored(&self) -> u64 {
        let n = self.node_count as usize;
        let mut sum = 0u64;
        let mut i = 0usize;
        while i < n {
            if self.nodes[i].role == PowerRole::Accumulator {
                sum += self.nodes[i].energy_stored as u64;
            }
            i += 1;
        }
        sum
    }

    /// FNV-1a hash of the complete grid state (nodes, network ledgers, tick).
    pub fn compute_hash(&self) -> u64 {
        let mut h = Fnv1aHasher::new();
        h.write(&self.tick.to_le_bytes());
        h.write(&self.node_count.to_le_bytes());
        h.write(&[self.net_count, self.dirty as u8]);
        h.write(&self.overflow_clusters.to_le_bytes());
        let n = self.node_count as usize;
        let mut i = 0usize;
        while i < n {
            self.nodes[i].hash_into(&mut h);
            i += 1;
        }
        let nets = self.net_count as usize;
        let mut k = 0usize;
        while k < nets {
            self.nets[k].hash_into(&mut h);
            k += 1;
        }
        h.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_is_cache_line_friendly() {
        assert_eq!(core::mem::size_of::<PowerNode>(), 16);
        assert_eq!(core::mem::size_of::<PowerNetwork>(), 64);
        assert_eq!(core::mem::align_of::<PowerNetwork>(), 64);
        assert_eq!(core::mem::align_of::<PowerGridManager<16, 256>>(), 64);
    }

    #[test]
    fn poles_cluster_and_bridge() {
        let mut g: PowerGridManager<4, 32> = PowerGridManager::new();
        let p0 = g.add_node(PowerNode::pole(0, 0, 2, 7)).unwrap();
        let p1 = g.add_node(PowerNode::pole(6, 0, 2, 7)).unwrap();
        let p2 = g.add_node(PowerNode::pole(40, 0, 2, 7)).unwrap();
        let gen = g.add_node(PowerNode::generator(1, 1, 50)).unwrap();
        let lonely = g.add_node(PowerNode::consumer(100, 100, 10)).unwrap();
        g.tick();
        assert_eq!(g.net_of(p0), g.net_of(p1));
        assert_ne!(g.net_of(p0), g.net_of(p2));
        assert_eq!(g.net_of(gen), g.net_of(p0));
        assert_eq!(g.net_of(lonely), NO_NET);
        assert_eq!(g.network_count(), 2);
        // A consumer covered by two poles that cannot wire directly bridges them.
        let p3 = g.add_node(PowerNode::pole(14, 0, 7, 1)).unwrap();
        g.tick();
        assert_ne!(g.net_of(p3), g.net_of(p1), "8 tiles apart, reach 7: no direct wire");
        assert_eq!(g.network_count(), 3);
        let bridge = g.add_node(PowerNode::consumer(8, 0, 10)).unwrap();
        g.tick();
        assert_eq!(g.net_of(p3), g.net_of(p0));
        assert_eq!(g.net_of(bridge), g.net_of(p1));
        assert_ne!(g.net_of(p2), g.net_of(p0));
        assert_eq!(g.network_count(), 2);
    }
}
