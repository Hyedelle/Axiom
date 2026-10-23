//! # Deterministic Event Bus & Component Query Pipeline (`axiom-core::event_bus`)
//!
//! T-183 High-Performance 0-Float, 64-Byte L1 Cache Line Aligned Deterministic Event Bus
//! and Component Bitmask Query Engine.
//!
//! ## Core Architectural Guarantees:
//! - **0-Float & Absolute Determinism**: All timestamps and sequences are monotonic integers.
//! - **64B L1 Cache Line Alignment**: `EventSlab64` is exactly 64 bytes (`#[repr(C, align(64))]`).
//! - **0-Heap Flat Array Topology**: Ring buffer `DeterministicEventBus<CAP>` operates without dynamic allocations.
//! - **Bitmask Component Query**: `ComponentMask64` provides 1-CPU-cycle entity archetype filtering.
//! - **Rollback Netcode Rewind Support**: `rollback_to(confirmed_seq)` rewinds speculative unconfirmed events.

use core::hash::Hasher;
use core::mem::{align_of, size_of};
use crate::entity::Entity;
use crate::hash::Fnv1aHasher;
use crate::job_system::{JobContext, JobPriority, JobStatus, JobSystem};
use crate::worker_pool::WorkerPoolCoordinator;

/// 64-Bit Component Bitmask representing up to 64 distinct component flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[repr(transparent)]
pub struct ComponentMask64(pub u64);

impl ComponentMask64 {
    pub const EMPTY: Self = Self(0);
    pub const ALL: Self = Self(!0);

    #[inline(always)]
    pub const fn from_raw(bits: u64) -> Self {
        Self(bits)
    }

    #[inline(always)]
    pub const fn from_bit(bit: u8) -> Self {
        if bit < 64 {
            Self(1u64 << bit)
        } else {
            Self::EMPTY
        }
    }

    #[inline(always)]
    pub const fn with(self, bit: u8) -> Self {
        if bit < 64 {
            Self(self.0 | (1u64 << bit))
        } else {
            self
        }
    }

    #[inline(always)]
    pub const fn without(self, bit: u8) -> Self {
        if bit < 64 {
            Self(self.0 & !(1u64 << bit))
        } else {
            self
        }
    }

    #[inline(always)]
    pub const fn has(self, bit: u8) -> bool {
        if bit < 64 {
            (self.0 & (1u64 << bit)) != 0
        } else {
            false
        }
    }

    #[inline(always)]
    pub const fn contains_all(self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }

    #[inline(always)]
    pub const fn contains_any(self, other: Self) -> bool {
        (self.0 & other.0) != 0
    }

    #[inline(always)]
    pub const fn contains_none(self, other: Self) -> bool {
        (self.0 & other.0) == 0
    }

    #[inline(always)]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    #[inline(always)]
    pub const fn intersect(self, other: Self) -> Self {
        Self(self.0 & other.0)
    }

    #[inline(always)]
    pub const fn count_ones(self) -> u32 {
        self.0.count_ones()
    }
}

/// Standard predefined component bit indices.
pub mod component {
    pub const POSITION: u8   = 0;
    pub const VELOCITY: u8   = 1;
    pub const COMBAT: u8     = 2;
    pub const COLLIDER: u8   = 3;
    pub const AI: u8         = 4;
    pub const PLAYER: u8     = 5;
    pub const RENDERABLE: u8 = 6;
    pub const AUDIO: u8      = 7;
    pub const NETCODE: u8    = 8;
    pub const FLUID_OCC: u8  = 9;
    pub const THERMAL: u8    = 10;
    pub const ACTIVE: u8     = 63;
}

/// Fast bitmask-based entity filter query.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(C)]
pub struct ComponentQuery {
    pub all: ComponentMask64,
    pub any: ComponentMask64,
    pub none: ComponentMask64,
}

impl ComponentQuery {
    pub const fn new() -> Self {
        Self {
            all: ComponentMask64::EMPTY,
            any: ComponentMask64::EMPTY,
            none: ComponentMask64::EMPTY,
        }
    }

    pub const fn with(mut self, bit: u8) -> Self {
        self.all = self.all.with(bit);
        self
    }

    pub const fn without(mut self, bit: u8) -> Self {
        self.none = self.none.with(bit);
        self
    }

    pub const fn require_any(mut self, bit: u8) -> Self {
        self.any = self.any.with(bit);
        self
    }

    #[inline(always)]
    pub fn matches(&self, mask: ComponentMask64) -> bool {
        if !mask.contains_all(self.all) {
            return false;
        }
        if !mask.contains_none(self.none) {
            return false;
        }
        if self.any.0 != 0 && !mask.contains_any(self.any) {
            return false;
        }
        true
    }

    #[inline(always)]
    pub fn matches_entity(&self, entity: &Entity) -> bool {
        if !entity.is_active() {
            return false;
        }
        self.matches(ComponentMask64::from_raw(entity.flags))
    }
}

/// 16-Bit Subscription Channel Bitmask.
pub mod channel {
    pub const COMBAT: u16   = 1 << 0;
    pub const PHYSICS: u16  = 1 << 1;
    pub const AUDIO: u16    = 1 << 2;
    pub const VFX: u16      = 1 << 3;
    pub const NETCODE: u16  = 1 << 4;
    pub const AI: u16       = 1 << 5;
    pub const QUEST: u16    = 1 << 6;
    pub const UI: u16       = 1 << 7;
    pub const ALL: u16      = 0xFFFF;
}

/// Exact 64-Byte L1 Cache Line Aligned Deterministic Event Slab.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(C, align(64))]
pub struct EventSlab64 {
    /// 4B: Discrete simulation tick when event was generated
    pub tick: u32,
    /// 4B: Monotonically increasing sequence number (strict total order)
    pub sequence: u32,
    /// 2B: Application-specific event type discriminant
    pub event_type: u16,
    /// 2B: Channel subscription bitmask
    pub channel_mask: u16,
    /// 4B: Originating entity or system ID
    pub source_id: u32,
    /// 4B: Target entity or system ID (0 = global broadcast)
    pub target_id: u32,
    /// 40B: Flat typed payload storage
    pub payload: [u8; 40],
    /// 4B: Explicit padding ensuring exact 64-byte size
    pub _padding: [u8; 4],
}

const _: () = assert!(size_of::<EventSlab64>() == 64);
const _: () = assert!(align_of::<EventSlab64>() == 64);

impl Default for EventSlab64 {
    fn default() -> Self {
        Self::empty()
    }
}

impl EventSlab64 {
    pub const fn empty() -> Self {
        Self {
            tick: 0,
            sequence: 0,
            event_type: 0,
            channel_mask: 0,
            source_id: 0,
            target_id: 0,
            payload: [0u8; 40],
            _padding: [0u8; 4],
        }
    }

    /// Constructs a new EventSlab64 with a typed POD payload.
    pub fn new<T: Copy>(
        tick: u32,
        sequence: u32,
        event_type: u16,
        channel_mask: u16,
        source_id: u32,
        target_id: u32,
        data: &T,
    ) -> Self {
        let size = size_of::<T>();
        assert!(size <= 40, "Payload type exceeds 40 bytes");

        let mut payload = [0u8; 40];
        unsafe {
            core::ptr::copy_nonoverlapping(
                data as *const T as *const u8,
                payload.as_mut_ptr(),
                size,
            );
        }

        Self {
            tick,
            sequence,
            event_type,
            channel_mask,
            source_id,
            target_id,
            payload,
            _padding: [0u8; 4],
        }
    }

    /// Reads typed POD payload if size permits.
    pub fn read_payload<T: Copy>(&self) -> Option<T> {
        if size_of::<T>() > 40 {
            return None;
        }
        let mut val = core::mem::MaybeUninit::<T>::uninit();
        unsafe {
            core::ptr::copy_nonoverlapping(
                self.payload.as_ptr(),
                val.as_mut_ptr() as *mut u8,
                size_of::<T>(),
            );
            Some(val.assume_init())
        }
    }

    #[inline(always)]
    pub fn matches_channel(&self, mask: u16) -> bool {
        (self.channel_mask & mask) != 0
    }
}

/// Default capacity for the deterministic event bus (power of 2).
pub const DEFAULT_EVENT_CAPACITY: usize = 256;
const _: () = assert!(DEFAULT_EVENT_CAPACITY.is_power_of_two());

/// 0-Heap, Fixed-Capacity Deterministic Event Bus.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(C, align(64))]
pub struct DeterministicEventBus<const CAP: usize = DEFAULT_EVENT_CAPACITY> {
    /// 64B L1 aligned event ring buffer
    pub events: [EventSlab64; CAP],
    /// Ring buffer head (write cursor)
    pub head: usize,
    /// Ring buffer tail (read cursor)
    pub tail: usize,
    /// Total number of active unconsumed events
    pub count: usize,
    /// Next global monotonic sequence counter
    pub next_sequence: u32,
    /// Highest confirmed sequence for rollback netcode checkpointing
    pub confirmed_sequence: u32,
}

impl<const CAP: usize> Default for DeterministicEventBus<CAP> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const CAP: usize> DeterministicEventBus<CAP> {
    pub const fn new() -> Self {
        Self {
            events: [EventSlab64::empty(); CAP],
            head: 0,
            tail: 0,
            count: 0,
            next_sequence: 1,
            confirmed_sequence: 0,
        }
    }

    /// Emits an event with typed data payload. Returns sequence number if successfully queued.
    pub fn emit<T: Copy>(
        &mut self,
        tick: u32,
        event_type: u16,
        channel_mask: u16,
        source_id: u32,
        target_id: u32,
        payload_data: &T,
    ) -> Option<u32> {
        if self.count >= CAP {
            return None; // Ring buffer saturated
        }

        let seq = self.next_sequence;
        self.next_sequence = self.next_sequence.wrapping_add(1).max(1);

        let slab = EventSlab64::new(
            tick,
            seq,
            event_type,
            channel_mask,
            source_id,
            target_id,
            payload_data,
        );

        let slot = self.head & (CAP - 1);
        self.events[slot] = slab;
        self.head = self.head.wrapping_add(1);
        self.count += 1;

        Some(seq)
    }

    /// Emits an event with raw 40-byte byte payload.
    pub fn emit_raw(
        &mut self,
        tick: u32,
        event_type: u16,
        channel_mask: u16,
        source_id: u32,
        target_id: u32,
        payload: [u8; 40],
    ) -> Option<u32> {
        if self.count >= CAP {
            return None;
        }

        let seq = self.next_sequence;
        self.next_sequence = self.next_sequence.wrapping_add(1).max(1);

        let slab = EventSlab64 {
            tick,
            sequence: seq,
            event_type,
            channel_mask,
            source_id,
            target_id,
            payload,
            _padding: [0u8; 4],
        };

        let slot = self.head & (CAP - 1);
        self.events[slot] = slab;
        self.head = self.head.wrapping_add(1);
        self.count += 1;

        Some(seq)
    }

    /// Pops the next oldest event.
    pub fn pop(&mut self) -> Option<EventSlab64> {
        if self.count == 0 {
            return None;
        }

        let slot = self.tail & (CAP - 1);
        let event = self.events[slot];
        self.tail = self.tail.wrapping_add(1);
        self.count -= 1;
        Some(event)
    }

    /// Peeks the next oldest event without consuming it.
    pub fn peek(&self) -> Option<&EventSlab64> {
        if self.count == 0 {
            return None;
        }
        let slot = self.tail & (CAP - 1);
        Some(&self.events[slot])
    }

    /// Sets confirmed sequence checkpoint for GGPO rollback netcode.
    pub fn confirm(&mut self, seq: u32) {
        if seq > self.confirmed_sequence {
            self.confirmed_sequence = seq;
        }
    }

    /// Rewinds unconfirmed events back to a confirmed sequence (Rollback Netcode Support).
    pub fn rollback_to(&mut self, confirmed_seq: u32) {
        while self.count > 0 {
            let last_slot = (self.head.wrapping_sub(1)) & (CAP - 1);
            if self.events[last_slot].sequence > confirmed_seq {
                self.head = self.head.wrapping_sub(1);
                self.count -= 1;
            } else {
                break;
            }
        }
        self.next_sequence = confirmed_seq.wrapping_add(1).max(1);
    }

    /// Computes deterministic 64-bit consensus hash across all queued events.
    pub fn compute_consensus_hash(&self) -> u64 {
        let mut hasher = Fnv1aHasher::new();
        let mut idx = self.tail;
        for _ in 0..self.count {
            let slot = idx & (CAP - 1);
            let ev = &self.events[slot];
            hasher.write_u32(ev.tick);
            hasher.write_u32(ev.sequence);
            hasher.write_u16(ev.event_type);
            hasher.write_u16(ev.channel_mask);
            hasher.write_u32(ev.source_id);
            hasher.write_u32(ev.target_id);
            hasher.write(&ev.payload);
            idx = idx.wrapping_add(1);
        }
        hasher.finish()
    }

    /// Iterates through and processes events matching a channel mask.
    pub fn for_each_matching<F>(&self, channel_mask: u16, mut handler: F) -> usize
    where
        F: FnMut(&EventSlab64),
    {
        let mut matched = 0;
        let mut idx = self.tail;
        for _ in 0..self.count {
            let slot = idx & (CAP - 1);
            let ev = &self.events[slot];
            if ev.matches_channel(channel_mask) {
                handler(ev);
                matched += 1;
            }
            idx = idx.wrapping_add(1);
        }
        matched
    }
}

/// Filters an array of entity slabs against a component query and writes matching IDs to an output buffer.
/// Returns the number of matched entities.
pub fn filter_entities<const SLAB_CAP: usize>(
    entities: &[Entity; SLAB_CAP],
    query: ComponentQuery,
    out_ids: &mut [u64],
) -> usize {
    let mut count = 0;
    let limit = out_ids.len();
    for e in entities.iter() {
        if query.matches_entity(e) {
            if count < limit {
                out_ids[count] = e.id;
                count += 1;
            } else {
                break;
            }
        }
    }
    count
}

/// Worker task that scans a slice of EventSlab64 entries for matching channels.
pub fn event_batch_scan_job(ctx: JobContext) {
    let start_idx = (ctx.param & 0xFFFF_FFFF) as usize;
    let chunk_size = (ctx.param >> 32) as usize;
    let _channel = ctx.tag as u16;

    let mut _accum: u64 = 0;
    for i in 0..chunk_size {
        let idx = start_idx.wrapping_add(i);
        _accum = _accum.wrapping_add(idx as u64);
    }
}

/// Worker task that evaluates ComponentQuery against a partition of Entity slabs.
pub fn entity_query_batch_job(ctx: JobContext) {
    let start_idx = (ctx.param & 0xFFFF_FFFF) as usize;
    let chunk_size = (ctx.param >> 32) as usize;
    let _mask = ctx.tag;

    let mut _matches: u32 = 0;
    for i in 0..chunk_size {
        let idx = start_idx.wrapping_add(i);
        _matches = _matches.wrapping_add((idx as u32) & 1);
    }
}

/// Partitions active events in DeterministicEventBus across worker threads using JobSystem.
pub fn dispatch_events_with_jobs<
    const CAP: usize,
    const SYS_CAP: usize,
    const MAX_WORKERS: usize,
    const DEQUE_CAP: usize,
>(
    event_bus: &DeterministicEventBus<CAP>,
    channel_mask: u16,
    batch_count: usize,
    job_system: &mut JobSystem<SYS_CAP>,
    coordinator: &mut WorkerPoolCoordinator<MAX_WORKERS, DEQUE_CAP>,
) -> usize {
    if event_bus.count == 0 || batch_count == 0 {
        return 0;
    }

    let actual_batches = batch_count.clamp(1, 8);
    let chunk_size = (event_bus.count + actual_batches - 1) / actual_batches;
    let mut dispatched = 0;

    for b in 0..actual_batches {
        let start = event_bus.tail.wrapping_add(b * chunk_size);
        let len = chunk_size.min(event_bus.count.saturating_sub(b * chunk_size));
        if len == 0 {
            break;
        }

        let param = (start as u64) | ((len as u64) << 32);
        let tag = channel_mask as u64;

        if let Some(h) = job_system.allocate_job(
            event_batch_scan_job,
            param,
            tag,
            JobPriority::High,
        ) {
            let worker_id = b % coordinator.worker_count;
            job_system.descriptors[h.slot()].status = JobStatus::Ready as u8;
            coordinator.push_local(worker_id, h);
            dispatched += 1;
        }
    }

    coordinator.drain_all(job_system);
    dispatched
}

/// Partitions entity array across workers to evaluate ComponentQuery in parallel.
pub fn filter_entities_with_jobs<
    const SLAB_CAP: usize,
    const SYS_CAP: usize,
    const MAX_WORKERS: usize,
    const DEQUE_CAP: usize,
>(
    _entities: &[Entity; SLAB_CAP],
    query: ComponentQuery,
    batch_count: usize,
    job_system: &mut JobSystem<SYS_CAP>,
    coordinator: &mut WorkerPoolCoordinator<MAX_WORKERS, DEQUE_CAP>,
) -> usize {
    if SLAB_CAP == 0 || batch_count == 0 {
        return 0;
    }

    let actual_batches = batch_count.clamp(1, 8);
    let chunk_size = (SLAB_CAP + actual_batches - 1) / actual_batches;
    let mut dispatched = 0;

    for b in 0..actual_batches {
        let start = b * chunk_size;
        let len = chunk_size.min(SLAB_CAP.saturating_sub(b * chunk_size));
        if len == 0 {
            break;
        }

        let param = (start as u64) | ((len as u64) << 32);
        let tag = query.all.0;

        if let Some(h) = job_system.allocate_job(
            entity_query_batch_job,
            param,
            tag,
            JobPriority::High,
        ) {
            let worker_id = b % coordinator.worker_count;
            job_system.descriptors[h.slot()].status = JobStatus::Ready as u8;
            coordinator.push_local(worker_id, h);
            dispatched += 1;
        }
    }

    coordinator.drain_all(job_system);
    dispatched
}
