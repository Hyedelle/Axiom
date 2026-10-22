//! # Allocation-Free Pipe Fluid Simulator (T-074)
//!
//! Integer fluid transport for the factory layer:
//!
//! - [`FluidType`]: water, crude oil, steam, petroleum gas (or empty).
//! - [`PipeKind`]: plain pipe, storage tank, directional pump, one-way check valve.
//! - [`PipeNode`]: 16-byte POD segment (coordinates, fluid, volume, capacity, pressure,
//!   temperature).
//! - [`FluidNetwork`]: fixed-slab, 64-byte aligned network with an open-addressing position
//!   index and a precomputed 4-neighbour table (rebuilt only after topology edits).
//!
//! Per tick, in deterministic slot order:
//! 1. **Pumps** actively move up to `capacity * efficiency / 100` units from the segment behind
//!    them to the segment in front of them, regardless of level difference. Pumps hold no fluid
//!    and never take part in passive flow, so nothing can flow backwards through them.
//! 2. **Passive equalisation** between each pair of 4-adjacent non-pump segments (each pair
//!    visited once via the east / south neighbour): the fill levels are equalised with
//!    `flow = (v1 * c2 - v2 * c1) / (c1 + c2)`, which reduces to the prescribed `delta / 2` for
//!    equal capacities. A check valve only accepts inflow from its back side and only emits
//!    toward its `direction`. Fluids of different types never mix: the receiver must be empty
//!    or already hold the same fluid (an empty receiver adopts the incoming type and
//!    temperature; otherwise temperatures are volume-weighted integer averages).
//!    Integer dead zone: the division truncates toward zero, so two equal-capacity neighbours
//!    stop exchanging once they differ by a single unit (a chain of `n` segments can therefore
//!    hold a staircase of at most `n - 1` units). Scale capacities up for finer resolution.
//! 3. **Pressure telemetry**: `pressure = volume * 1000 / capacity` (per-mille fill); for pumps
//!    `pressure` is the throughput of the last tick.
//!
//! Every transfer subtracts exactly what it adds and is clamped by the sender's volume and the
//! receiver's free capacity, so total volume is conserved bit-for-bit:
//! `Σ volume + drained_total == initial + injected_total`.

use core::hash::Hasher;

use crate::hash::Fnv1aHasher;
use crate::logistics::{DIR_EAST, DIR_NORTH, DIR_SOUTH};

/// Default segment slab capacity.
pub const DEFAULT_MAX_PIPES: usize = 512;
/// Open-addressing buckets of the position index (`MAX_PIPES` must not exceed this).
pub const FLUID_INDEX_BUCKETS: usize = 1024;
/// "No segment" sentinel for neighbour / index tables.
pub const FLUID_NONE: u16 = u16::MAX;
/// Pressure scale: a full segment reports this pressure.
pub const PRESSURE_FULL: u16 = 1000;

const POS_SALT: u32 = 0x9E37_79B9;

/// Fluid carried by a segment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u8)]
pub enum FluidType {
    #[default]
    None = 0,
    Water = 1,
    CrudeOil = 2,
    Steam = 3,
    PetroleumGas = 4,
}

impl FluidType {
    #[inline(always)]
    pub const fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::None),
            1 => Some(Self::Water),
            2 => Some(Self::CrudeOil),
            3 => Some(Self::Steam),
            4 => Some(Self::PetroleumGas),
            _ => None,
        }
    }

    /// Gases are compressible and lighter; exposed for game-layer rules (not used by the core
    /// equalisation, which is level based).
    #[inline(always)]
    pub const fn is_gas(self) -> bool {
        matches!(self, Self::Steam | Self::PetroleumGas)
    }
}

/// Behaviour class of a segment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u8)]
pub enum PipeKind {
    /// Passive segment.
    #[default]
    Pipe = 0,
    /// Passive segment with (typically) large capacity.
    Tank = 1,
    /// Active one-way mover: back neighbour ➔ front neighbour, `capacity` units per tick.
    Pump = 2,
    /// Passive one-way segment: inflow from the back side only, outflow toward `direction` only.
    CheckValve = 3,
}

impl PipeKind {
    #[inline(always)]
    pub const fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Pipe),
            1 => Some(Self::Tank),
            2 => Some(Self::Pump),
            3 => Some(Self::CheckValve),
            _ => None,
        }
    }
}

/// Opposite of a `DIR_*` constant.
#[inline(always)]
pub const fn opposite_dir(dir: u8) -> u8 {
    (dir + 2) & 3
}

/// Unit offset of a `DIR_*` constant (north is `-y`).
#[inline(always)]
pub const fn dir_offset(dir: u8) -> (i16, i16) {
    match dir & 3 {
        DIR_NORTH => (0, -1),
        DIR_EAST => (1, 0),
        DIR_SOUTH => (0, 1),
        _ => (-1, 0),
    }
}

/// One pipe segment (16 bytes, POD, four per cache line).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(C)]
pub struct PipeNode {
    pub x: i16,
    pub y: i16,
    pub fluid_type: FluidType,
    pub kind: PipeKind,
    /// Flow direction for pumps / check valves (`DIR_*`); ignored for pipes and tanks.
    pub direction: u8,
    /// Pump operating efficiency 0..=100 (brownout coupling); 100 for other kinds.
    pub efficiency_pct: u8,
    pub volume: u16,
    pub capacity: u16,
    /// Per-mille fill level (pumps: last tick throughput).
    pub pressure: u16,
    pub temperature: u16,
}

impl PipeNode {
    pub const fn new(x: i16, y: i16, kind: PipeKind, direction: u8, capacity: u16) -> Self {
        Self {
            x,
            y,
            fluid_type: FluidType::None,
            kind,
            direction: direction & 3,
            efficiency_pct: 100,
            volume: 0,
            capacity: if capacity == 0 { 1 } else { capacity },
            pressure: 0,
            temperature: 0,
        }
    }

    #[inline]
    pub const fn pipe(x: i16, y: i16, capacity: u16) -> Self {
        Self::new(x, y, PipeKind::Pipe, DIR_EAST, capacity)
    }

    #[inline]
    pub const fn tank(x: i16, y: i16, capacity: u16) -> Self {
        Self::new(x, y, PipeKind::Tank, DIR_EAST, capacity)
    }

    #[inline]
    pub const fn pump(x: i16, y: i16, direction: u8, rate: u16) -> Self {
        Self::new(x, y, PipeKind::Pump, direction, rate)
    }

    #[inline]
    pub const fn check_valve(x: i16, y: i16, direction: u8, capacity: u16) -> Self {
        Self::new(x, y, PipeKind::CheckValve, direction, capacity)
    }

    /// May this segment emit fluid toward `dir`?
    #[inline(always)]
    fn allows_out(&self, dir: u8) -> bool {
        match self.kind {
            PipeKind::CheckValve => self.direction == dir,
            PipeKind::Pump => false,
            _ => true,
        }
    }

    /// May this segment accept fluid that is travelling in direction `dir`?
    #[inline(always)]
    fn allows_in(&self, dir: u8) -> bool {
        match self.kind {
            PipeKind::CheckValve => self.direction == dir,
            PipeKind::Pump => false,
            _ => true,
        }
    }

    #[inline(always)]
    fn hash_into(&self, h: &mut Fnv1aHasher) {
        h.write(&self.x.to_le_bytes());
        h.write(&self.y.to_le_bytes());
        h.write(&[self.fluid_type as u8, self.kind as u8, self.direction, self.efficiency_pct]);
        h.write(&self.volume.to_le_bytes());
        h.write(&self.capacity.to_le_bytes());
        h.write(&self.pressure.to_le_bytes());
        h.write(&self.temperature.to_le_bytes());
    }
}

/// Snapshot of network-wide counters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(C)]
pub struct FluidStats {
    pub tick: u64,
    pub injected_total: u64,
    pub drained_total: u64,
    /// Σ volume over all segments.
    pub total_volume: u32,
    /// Units moved by passive equalisation last tick.
    pub moved_last_tick: u32,
    /// Units moved by pumps last tick.
    pub pumped_last_tick: u32,
    /// Transfers refused last tick because the fluids differed.
    pub blocked_last_tick: u32,
    pub pipe_count: u16,
    pub pump_count: u16,
}

/// Fixed-slab fluid network (64-byte aligned, zero heap).
#[derive(Debug, Clone)]
#[repr(C, align(64))]
pub struct FluidNetwork<const MAX_PIPES: usize = DEFAULT_MAX_PIPES> {
    pipes: [PipeNode; MAX_PIPES],
    /// 4-neighbour slot table (`FLUID_NONE` when absent), rebuilt when dirty.
    neigh: [[u16; 4]; MAX_PIPES],
    /// Open-addressing position index: slot or `FLUID_NONE`.
    index: [u16; FLUID_INDEX_BUCKETS],
    count: u16,
    dirty: bool,
    moved_last_tick: u32,
    pumped_last_tick: u32,
    blocked_last_tick: u32,
    injected_total: u64,
    drained_total: u64,
    tick: u64,
}

impl<const MAX_PIPES: usize> Default for FluidNetwork<MAX_PIPES> {
    fn default() -> Self {
        Self::new()
    }
}

#[inline(always)]
fn pos_hash(x: i16, y: i16) -> usize {
    let key = ((x as u16 as u32) << 16) | (y as u16 as u32);
    let h = key.wrapping_mul(POS_SALT) ^ key.rotate_left(13);
    (h as usize) & (FLUID_INDEX_BUCKETS - 1)
}

impl<const MAX_PIPES: usize> FluidNetwork<MAX_PIPES> {
    pub fn new() -> Self {
        assert!(MAX_PIPES <= FLUID_INDEX_BUCKETS / 2, "MAX_PIPES must leave the index at most half full");
        Self {
            pipes: [PipeNode::default(); MAX_PIPES],
            neigh: [[FLUID_NONE; 4]; MAX_PIPES],
            index: [FLUID_NONE; FLUID_INDEX_BUCKETS],
            count: 0,
            dirty: false,
            moved_last_tick: 0,
            pumped_last_tick: 0,
            blocked_last_tick: 0,
            injected_total: 0,
            drained_total: 0,
            tick: 0,
        }
    }

    // -- topology ---------------------------------------------------------------------------

    /// Insert a segment. Fails when the slab is full or the tile is already occupied.
    pub fn add_pipe(&mut self, mut node: PipeNode) -> Option<u16> {
        let idx = self.count as usize;
        if idx >= MAX_PIPES || self.find_pipe(node.x, node.y).is_some() {
            return None;
        }
        if node.capacity == 0 {
            node.capacity = 1;
        }
        if node.kind == PipeKind::Pump {
            node.volume = 0;
        } else {
            node.efficiency_pct = 100;
            if node.volume > node.capacity {
                node.volume = node.capacity;
            }
        }
        node.direction &= 3;
        node.pressure = 0;
        self.pipes[idx] = node;
        self.count += 1;
        self.dirty = true;
        Some(idx as u16)
    }

    /// Remove a segment (swap-remove: the former last slot moves to `idx`). Its fluid is lost
    /// and counted in `drained_total` so the conservation ledger stays balanced.
    pub fn remove_pipe(&mut self, idx: u16) -> Option<PipeNode> {
        let i = idx as usize;
        let n = self.count as usize;
        if i >= n {
            return None;
        }
        let removed = self.pipes[i];
        self.drained_total += removed.volume as u64;
        let last = n - 1;
        if i != last {
            self.pipes[i] = self.pipes[last];
        }
        self.pipes[last] = PipeNode::default();
        self.count -= 1;
        self.dirty = true;
        Some(removed)
    }

    /// Slot of the segment at `(x, y)`.
    pub fn find_pipe(&self, x: i16, y: i16) -> Option<u16> {
        if self.dirty {
            // Index may be stale: linear scan (only after edits, never on the hot path).
            let n = self.count as usize;
            let mut i = 0usize;
            while i < n {
                if self.pipes[i].x == x && self.pipes[i].y == y {
                    return Some(i as u16);
                }
                i += 1;
            }
            return None;
        }
        let mut b = pos_hash(x, y);
        let mut probes = 0usize;
        while probes < FLUID_INDEX_BUCKETS {
            let slot = self.index[b];
            if slot == FLUID_NONE {
                return None;
            }
            let p = &self.pipes[slot as usize];
            if p.x == x && p.y == y {
                return Some(slot);
            }
            b = (b + 1) & (FLUID_INDEX_BUCKETS - 1);
            probes += 1;
        }
        None
    }

    /// Rebuild the position index and neighbour table (automatic on the next tick when dirty).
    pub fn rebuild_topology(&mut self) {
        let n = self.count as usize;
        self.index = [FLUID_NONE; FLUID_INDEX_BUCKETS];
        let mut i = 0usize;
        while i < n {
            let mut b = pos_hash(self.pipes[i].x, self.pipes[i].y);
            while self.index[b] != FLUID_NONE {
                b = (b + 1) & (FLUID_INDEX_BUCKETS - 1);
            }
            self.index[b] = i as u16;
            i += 1;
        }
        self.dirty = false;
        i = 0;
        while i < n {
            let (x, y) = (self.pipes[i].x, self.pipes[i].y);
            let mut d = 0u8;
            while d < 4 {
                let (dx, dy) = dir_offset(d);
                let nx = x.wrapping_add(dx);
                let ny = y.wrapping_add(dy);
                self.neigh[i][d as usize] = match self.find_pipe(nx, ny) {
                    Some(s) => s,
                    None => FLUID_NONE,
                };
                d += 1;
            }
            i += 1;
        }
    }

    #[inline(always)]
    pub fn pipe(&self, idx: u16) -> Option<&PipeNode> {
        if (idx as usize) < self.count as usize { Some(&self.pipes[idx as usize]) } else { None }
    }

    #[inline(always)]
    pub fn pipes(&self) -> &[PipeNode] {
        &self.pipes[..self.count as usize]
    }

    #[inline(always)]
    pub fn pipe_count(&self) -> usize {
        self.count as usize
    }

    #[inline(always)]
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Neighbour slot of `idx` toward `dir` (valid after the last tick / rebuild).
    #[inline(always)]
    pub fn neighbor(&self, idx: u16, dir: u8) -> Option<u16> {
        if (idx as usize) >= self.count as usize {
            return None;
        }
        let s = self.neigh[idx as usize][(dir & 3) as usize];
        if s == FLUID_NONE { None } else { Some(s) }
    }

    /// Set a pump's operating efficiency (0..=100), e.g. from the power grid satisfaction.
    pub fn set_pump_efficiency(&mut self, idx: u16, pct: u8) -> bool {
        let i = idx as usize;
        if i >= self.count as usize || self.pipes[i].kind != PipeKind::Pump {
            return false;
        }
        self.pipes[i].efficiency_pct = if pct > 100 { 100 } else { pct };
        true
    }

    // -- external exchange ---------------------------------------------------------------------

    /// Add `amount` units of `fluid` at `temperature` into a segment. Returns the accepted
    /// amount (0 when the segment is a pump, is full, or holds a different fluid).
    pub fn inject(&mut self, idx: u16, fluid: FluidType, amount: u16, temperature: u16) -> u16 {
        let i = idx as usize;
        if i >= self.count as usize || fluid == FluidType::None {
            return 0;
        }
        let p = &mut self.pipes[i];
        if p.kind == PipeKind::Pump {
            return 0;
        }
        if p.volume > 0 && p.fluid_type != fluid {
            self.blocked_last_tick += 1;
            return 0;
        }
        let room = p.capacity - p.volume;
        let accepted = if amount > room { room } else { amount };
        if accepted == 0 {
            return 0;
        }
        if p.volume == 0 {
            p.fluid_type = fluid;
            p.temperature = temperature;
        } else {
            p.temperature = mix_temperature(p.volume, p.temperature, accepted, temperature);
        }
        p.volume += accepted;
        self.injected_total += accepted as u64;
        accepted
    }

    /// Remove up to `amount` units from a segment; returns `(fluid, removed)`.
    pub fn drain(&mut self, idx: u16, amount: u16) -> (FluidType, u16) {
        let i = idx as usize;
        if i >= self.count as usize {
            return (FluidType::None, 0);
        }
        let p = &mut self.pipes[i];
        let removed = if amount > p.volume { p.volume } else { amount };
        p.volume -= removed;
        self.drained_total += removed as u64;
        (p.fluid_type, removed)
    }

    // -- simulation ---------------------------------------------------------------------------

    /// Move up to `want` units from slot `from` to slot `to` (type / capacity clamped).
    /// Returns the amount actually moved.
    #[inline]
    fn transfer(&mut self, from: usize, to: usize, want: u32) -> u16 {
        if want == 0 || from == to {
            return 0;
        }
        let (src_vol, src_type, src_temp) = {
            let s = &self.pipes[from];
            (s.volume, s.fluid_type, s.temperature)
        };
        if src_vol == 0 || src_type == FluidType::None {
            return 0;
        }
        let dst = &mut self.pipes[to];
        if dst.volume > 0 && dst.fluid_type != src_type {
            self.blocked_last_tick += 1;
            return 0;
        }
        let room = (dst.capacity - dst.volume) as u32;
        let mut amount = want;
        if amount > src_vol as u32 {
            amount = src_vol as u32;
        }
        if amount > room {
            amount = room;
        }
        if amount == 0 {
            return 0;
        }
        let amount = amount as u16;
        if dst.volume == 0 {
            dst.fluid_type = src_type;
            dst.temperature = src_temp;
        } else {
            dst.temperature = mix_temperature(dst.volume, dst.temperature, amount, src_temp);
        }
        dst.volume += amount;
        self.pipes[from].volume -= amount;
        amount
    }

    /// Advance one tick (pumps ➔ passive equalisation ➔ pressure telemetry).
    pub fn tick(&mut self) {
        if self.dirty {
            self.rebuild_topology();
        }
        self.tick = self.tick.wrapping_add(1);
        self.moved_last_tick = 0;
        self.pumped_last_tick = 0;
        self.blocked_last_tick = 0;
        let n = self.count as usize;

        // Phase 1: pumps.
        let mut i = 0usize;
        while i < n {
            if self.pipes[i].kind == PipeKind::Pump {
                let dir = self.pipes[i].direction;
                let back = self.neigh[i][opposite_dir(dir) as usize];
                let front = self.neigh[i][dir as usize];
                let mut moved = 0u16;
                if back != FLUID_NONE && front != FLUID_NONE {
                    let (b, f) = (back as usize, front as usize);
                    let rate = (self.pipes[i].capacity as u32 * self.pipes[i].efficiency_pct as u32) / 100;
                    if self.pipes[b].allows_out(dir) && self.pipes[f].allows_in(dir) {
                        moved = self.transfer(b, f, rate);
                        if moved > 0 {
                            self.pipes[i].fluid_type = self.pipes[f].fluid_type;
                            self.pipes[i].temperature = self.pipes[f].temperature;
                        }
                    }
                }
                self.pipes[i].pressure = moved;
                self.pumped_last_tick += moved as u32;
            }
            i += 1;
        }

        // Phase 2: passive equalisation, each adjacent pair once (east and south links).
        i = 0;
        while i < n {
            if self.pipes[i].kind != PipeKind::Pump {
                let mut side = 0usize;
                while side < 2 {
                    let dir = if side == 0 { DIR_EAST } else { DIR_SOUTH };
                    let j = self.neigh[i][dir as usize];
                    if j != FLUID_NONE && self.pipes[j as usize].kind != PipeKind::Pump {
                        let j = j as usize;
                        let (vi, ci) = (self.pipes[i].volume as i64, self.pipes[i].capacity as i64);
                        let (vj, cj) = (self.pipes[j].volume as i64, self.pipes[j].capacity as i64);
                        let flow = (vi * cj - vj * ci) / (ci + cj);
                        if flow > 0 {
                            if self.pipes[i].allows_out(dir) && self.pipes[j].allows_in(dir) {
                                self.moved_last_tick += self.transfer(i, j, flow as u32) as u32;
                            }
                        } else if flow < 0 {
                            let back = opposite_dir(dir);
                            if self.pipes[j].allows_out(back) && self.pipes[i].allows_in(back) {
                                self.moved_last_tick += self.transfer(j, i, (-flow) as u32) as u32;
                            }
                        }
                    }
                    side += 1;
                }
            }
            i += 1;
        }

        // Phase 3: pressure telemetry.
        i = 0;
        while i < n {
            let p = &mut self.pipes[i];
            if p.kind != PipeKind::Pump {
                p.pressure = ((p.volume as u32 * PRESSURE_FULL as u32) / p.capacity as u32) as u16;
            }
            i += 1;
        }
    }

    /// Σ volume over all segments.
    pub fn total_volume(&self) -> u32 {
        let n = self.count as usize;
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < n {
            sum += self.pipes[i].volume as u32;
            i += 1;
        }
        sum
    }

    /// Σ volume of one fluid type.
    pub fn volume_of(&self, fluid: FluidType) -> u32 {
        let n = self.count as usize;
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < n {
            if self.pipes[i].fluid_type == fluid {
                sum += self.pipes[i].volume as u32;
            }
            i += 1;
        }
        sum
    }

    pub fn stats(&self) -> FluidStats {
        let n = self.count as usize;
        let mut pumps = 0u16;
        let mut i = 0usize;
        while i < n {
            if self.pipes[i].kind == PipeKind::Pump {
                pumps += 1;
            }
            i += 1;
        }
        FluidStats {
            tick: self.tick,
            injected_total: self.injected_total,
            drained_total: self.drained_total,
            total_volume: self.total_volume(),
            moved_last_tick: self.moved_last_tick,
            pumped_last_tick: self.pumped_last_tick,
            blocked_last_tick: self.blocked_last_tick,
            pipe_count: self.count,
            pump_count: pumps,
        }
    }

    /// FNV-1a hash of the complete network state.
    pub fn compute_hash(&self) -> u64 {
        let mut h = Fnv1aHasher::new();
        h.write(&self.tick.to_le_bytes());
        h.write(&self.count.to_le_bytes());
        h.write(&[self.dirty as u8]);
        h.write(&self.moved_last_tick.to_le_bytes());
        h.write(&self.pumped_last_tick.to_le_bytes());
        h.write(&self.blocked_last_tick.to_le_bytes());
        h.write(&self.injected_total.to_le_bytes());
        h.write(&self.drained_total.to_le_bytes());
        let n = self.count as usize;
        let mut i = 0usize;
        while i < n {
            self.pipes[i].hash_into(&mut h);
            i += 1;
        }
        h.finish()
    }
}

/// Volume-weighted integer temperature of a mix (`u64` intermediate, never overflows).
#[inline(always)]
fn mix_temperature(v_old: u16, t_old: u16, v_new: u16, t_new: u16) -> u16 {
    let total = v_old as u64 + v_new as u64;
    if total == 0 {
        return t_new;
    }
    ((v_old as u64 * t_old as u64 + v_new as u64 * t_new as u64) / total) as u16
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logistics::DIR_WEST;

    #[test]
    fn layout_is_cache_line_friendly() {
        assert_eq!(core::mem::size_of::<PipeNode>(), 16);
        assert_eq!(core::mem::align_of::<FluidNetwork<64>>(), 64);
    }

    #[test]
    fn equal_capacity_pair_halves_delta() {
        let mut net: FluidNetwork<8> = FluidNetwork::new();
        let a = net.add_pipe(PipeNode::pipe(0, 0, 100)).unwrap();
        let b = net.add_pipe(PipeNode::pipe(1, 0, 100)).unwrap();
        assert_eq!(net.inject(a, FluidType::Water, 80, 15), 80);
        net.tick();
        assert_eq!(net.pipe(a).unwrap().volume, 40);
        assert_eq!(net.pipe(b).unwrap().volume, 40);
        assert_eq!(net.pipe(b).unwrap().fluid_type, FluidType::Water);
        assert_eq!(net.pipe(b).unwrap().pressure, 400);
        assert_eq!(net.total_volume(), 80);
    }

    #[test]
    fn different_fluids_never_mix() {
        let mut net: FluidNetwork<8> = FluidNetwork::new();
        let a = net.add_pipe(PipeNode::pipe(0, 0, 100)).unwrap();
        let b = net.add_pipe(PipeNode::pipe(1, 0, 100)).unwrap();
        net.inject(a, FluidType::Water, 100, 15);
        net.inject(b, FluidType::CrudeOil, 10, 15);
        net.tick();
        assert_eq!(net.volume_of(FluidType::Water), 100);
        assert_eq!(net.volume_of(FluidType::CrudeOil), 10);
        assert_eq!(net.stats().blocked_last_tick, 1);
    }

    #[test]
    fn index_survives_removal() {
        let mut net: FluidNetwork<16> = FluidNetwork::new();
        for x in 0..10i16 {
            net.add_pipe(PipeNode::pipe(x, 0, 50)).unwrap();
        }
        net.tick();
        assert_eq!(net.find_pipe(9, 0), Some(9));
        net.remove_pipe(0);
        net.tick();
        assert_eq!(net.find_pipe(9, 0), Some(0));
        assert_eq!(net.find_pipe(0, 0), None);
        assert_eq!(net.neighbor(0, DIR_WEST), net.find_pipe(8, 0));
    }
}
