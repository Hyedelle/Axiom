//! # Multi-Threaded Work-Stealing Job Queue & Worker Pool Coordinator (`axiom-core::worker_pool`)
//!
//! 0-Float, 64-Byte L1 cache aligned lock-free Chase-Lev work-stealing deques and
//! multi-worker pool coordinator. Enables high-throughput parallel task execution across
//! cores with zero lock contention and bit-exact reproducibility.
//!
//! - **Chase-Lev Lock-Free Deque**: Owner pushes/pops from bottom (LIFO, warm cache);
//!   idle stealers steal from top (FIFO, large/early tasks).
//! - **64B L1 Cache Aligned**: Per-worker state structures aligned to 64 bytes to eliminate false sharing.
//! - **Zero Heap Allocation**: Flat fixed-capacity ring arrays.
//! - **Bit-Exact Deterministic Coordination**: Predictable round-robin stealing with consensus hashing.

use core::mem::{align_of, size_of};
use core::sync::atomic::{fence, AtomicIsize, AtomicU64, Ordering};
use crate::job_system::{JobContext, JobHandle, JobSystem};

/// Capacity for a single worker's Chase-Lev deque (must be a power of 2).
pub const DEQUE_CAPACITY: usize = 256;
const _: () = assert!(DEQUE_CAPACITY.is_power_of_two());

/// Lock-free Single-Producer Multi-Consumer (SPMC) Chase-Lev Work-Stealing Deque.
/// Owner pushes and pops from the bottom (LIFO). Stealers pop from the top (FIFO).
#[repr(C, align(64))]
pub struct WorkStealingDeque<const CAP: usize = DEQUE_CAPACITY> {
    /// Atomic top index (read/written by stealers and owner)
    pub top: AtomicIsize,
    /// Atomic bottom index (written by owner only)
    pub bottom: AtomicIsize,
    /// Fixed-capacity ring buffer of JobHandles
    pub buffer: [JobHandle; CAP],
}

impl<const CAP: usize> Default for WorkStealingDeque<CAP> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const CAP: usize> WorkStealingDeque<CAP> {
    /// Creates a new, empty work-stealing deque.
    pub const fn new() -> Self {
        Self {
            top: AtomicIsize::new(0),
            bottom: AtomicIsize::new(0),
            buffer: [JobHandle::NULL; CAP],
        }
    }

    /// Pushes a job to the bottom of the deque (Owner only).
    /// Returns true if successful, or false if the deque is full.
    pub fn push(&mut self, handle: JobHandle) -> bool {
        let b = self.bottom.load(Ordering::Relaxed);
        let t = self.top.load(Ordering::Acquire);
        if b - t >= CAP as isize {
            return false;
        }

        let slot = (b as usize) & (CAP - 1);
        self.buffer[slot] = handle;
        fence(Ordering::Release);
        self.bottom.store(b + 1, Ordering::Relaxed);
        true
    }

    /// Pops a job from the bottom of the deque (Owner only, LIFO).
    pub fn pop(&mut self) -> Option<JobHandle> {
        let b = self.bottom.load(Ordering::Relaxed) - 1;
        self.bottom.store(b, Ordering::Relaxed);
        fence(Ordering::SeqCst);
        let t = self.top.load(Ordering::Relaxed);

        if t <= b {
            let slot = (b as usize) & (CAP - 1);
            let item = self.buffer[slot];
            if t == b {
                // Last item: compete with concurrent stealers
                if self.top.compare_exchange(t, t + 1, Ordering::SeqCst, Ordering::Relaxed).is_ok() {
                    self.bottom.store(t + 1, Ordering::Relaxed);
                    Some(item)
                } else {
                    self.bottom.store(t + 1, Ordering::Relaxed);
                    None // Lost race to stealer
                }
            } else {
                Some(item)
            }
        } else {
            // Queue empty
            self.bottom.store(t, Ordering::Relaxed);
            None
        }
    }

    /// Steals a job from the top of the deque (Stealers, FIFO).
    pub fn steal(&self) -> Option<JobHandle> {
        let t = self.top.load(Ordering::Acquire);
        fence(Ordering::SeqCst);
        let b = self.bottom.load(Ordering::Acquire);

        if t < b {
            let slot = (t as usize) & (CAP - 1);
            let item = self.buffer[slot];
            if self.top.compare_exchange(t, t + 1, Ordering::SeqCst, Ordering::Relaxed).is_ok() {
                Some(item)
            } else {
                None // Lost race to another stealer or owner
            }
        } else {
            None // Empty
        }
    }

    /// Returns the approximate count of elements currently in the deque.
    #[inline(always)]
    pub fn len(&self) -> usize {
        let b = self.bottom.load(Ordering::Relaxed);
        let t = self.top.load(Ordering::Relaxed);
        (b - t).max(0) as usize
    }

    /// Returns true if the deque is currently empty.
    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Exact 64-Byte L1 cache aligned worker telemetry slab.
#[repr(C, align(64))]
#[derive(Debug)]
pub struct WorkerTelemetry64 {
    pub worker_id: u32,                  // 4B
    pub _align_pad: u32,                 // 4B: explicit alignment padding
    pub jobs_executed: AtomicU64,        // 8B
    pub jobs_stolen: AtomicU64,          // 8B
    pub steal_attempts: AtomicU64,       // 8B
    pub steal_failures: AtomicU64,       // 8B
    pub _padding: [u8; 24],              // 24B: exact padding to 64 bytes
}

const _: () = assert!(size_of::<WorkerTelemetry64>() == 64);
const _: () = assert!(align_of::<WorkerTelemetry64>() == 64);

impl Default for WorkerTelemetry64 {
    fn default() -> Self {
        Self::new(0)
    }
}

impl WorkerTelemetry64 {
    pub const fn new(id: u32) -> Self {
        Self {
            worker_id: id,
            _align_pad: 0,
            jobs_executed: AtomicU64::new(0),
            jobs_stolen: AtomicU64::new(0),
            steal_attempts: AtomicU64::new(0),
            steal_failures: AtomicU64::new(0),
            _padding: [0u8; 24],
        }
    }
}

/// Multi-Worker Work-Stealing Pool Coordinator for up to `MAX_WORKERS` threads.
pub struct WorkerPoolCoordinator<
    const MAX_WORKERS: usize = 8,
    const DEQUE_CAP: usize = DEQUE_CAPACITY,
> {
    pub worker_count: usize,
    pub deques: [WorkStealingDeque<DEQUE_CAP>; MAX_WORKERS],
    pub telemetry: [WorkerTelemetry64; MAX_WORKERS],
}

impl<const MAX_WORKERS: usize, const DEQUE_CAP: usize> Default
    for WorkerPoolCoordinator<MAX_WORKERS, DEQUE_CAP>
{
    fn default() -> Self {
        Self::new(MAX_WORKERS)
    }
}

impl<const MAX_WORKERS: usize, const DEQUE_CAP: usize>
    WorkerPoolCoordinator<MAX_WORKERS, DEQUE_CAP>
{
    /// Creates a new worker pool coordinator for `count` active workers.
    pub fn new(count: usize) -> Self {
        Self {
            worker_count: count,
            deques: core::array::from_fn(|_| WorkStealingDeque::new()),
            telemetry: core::array::from_fn(|i| WorkerTelemetry64::new(i as u32)),
        }
    }

    /// Pushes a job handle to a worker's local deque.
    pub fn push_local(&mut self, worker_id: usize, handle: JobHandle) -> bool {
        if worker_id < self.worker_count {
            self.deques[worker_id].push(handle)
        } else {
            false
        }
    }

    /// Pops a job handle from a worker's local deque (LIFO).
    pub fn pop_local(&mut self, worker_id: usize) -> Option<JobHandle> {
        if worker_id < self.worker_count {
            self.deques[worker_id].pop()
        } else {
            None
        }
    }

    /// Attempts to steal a job from another worker (FIFO).
    pub fn steal_from(&self, stealer_id: usize, target_id: usize) -> Option<JobHandle> {
        if stealer_id >= self.worker_count || target_id >= self.worker_count || stealer_id == target_id {
            return None;
        }

        self.telemetry[stealer_id].steal_attempts.fetch_add(1, Ordering::Relaxed);
        let stolen = self.deques[target_id].steal();
        if stolen.is_some() {
            self.telemetry[stealer_id].jobs_stolen.fetch_add(1, Ordering::Relaxed);
        } else {
            self.telemetry[stealer_id].steal_failures.fetch_add(1, Ordering::Relaxed);
        }
        stolen
    }

    /// Round-robin steal attempt from any available peer worker.
    pub fn steal_any(&self, stealer_id: usize, seed: usize) -> Option<JobHandle> {
        if self.worker_count <= 1 {
            return None;
        }

        for offset in 1..self.worker_count {
            let target = (stealer_id + offset + seed) % self.worker_count;
            if let Some(item) = self.steal_from(stealer_id, target) {
                return Some(item);
            }
        }
        None
    }

    /// Executes one job on the specified worker:
    /// 1. Tries to pop from local deque (LIFO, warm cache).
    /// 2. If empty, attempts to steal from other workers (FIFO).
    /// Returns true if a job was found and executed.
    pub fn execute_one<const SYS_CAP: usize>(
        &mut self,
        worker_id: usize,
        job_system: &mut JobSystem<SYS_CAP>,
        seed: usize,
    ) -> bool {
        if worker_id >= self.worker_count {
            return false;
        }

        let job_handle = self.pop_local(worker_id)
            .or_else(|| self.steal_any(worker_id, seed));

        let handle = match job_handle {
            Some(h) => h,
            None => return false,
        };

        if !job_system.is_valid(handle) {
            return false;
        }

        let slot = handle.slot();
        let entry = job_system.descriptors[slot].entry;
        let param = job_system.descriptors[slot].param;
        let tag = job_system.descriptors[slot].tag;
        let parent = job_system.descriptors[slot].parent;
        let continuation = job_system.descriptors[slot].continuation;

        job_system.descriptors[slot].status = crate::job_system::JobStatus::Running as u8;
        job_system.descriptors[slot].worker_id = worker_id as u16;

        if let Some(f) = entry {
            let ctx = JobContext {
                handle,
                worker_id: worker_id as u32,
                param,
                tag,
            };
            f(ctx);
        }

        job_system.descriptors[slot].status = crate::job_system::JobStatus::Done as u8;
        job_system.total_jobs_completed += 1;
        job_system.active_count = job_system.active_count.saturating_sub(1);
        self.telemetry[worker_id].jobs_executed.fetch_add(1, Ordering::Relaxed);

        // Resolve dependencies:
        if !parent.is_null() && job_system.is_valid(parent) {
            let p_slot = parent.slot();
            let prev = job_system.descriptors[p_slot].pending_counter.fetch_sub(1, Ordering::SeqCst);
            if prev == 1 {
                job_system.descriptors[p_slot].status = crate::job_system::JobStatus::Ready as u8;
                // Push ready parent to local worker queue
                self.push_local(worker_id, parent);
            }
        }

        if !continuation.is_null() && job_system.is_valid(continuation) {
            let c_slot = continuation.slot();
            job_system.descriptors[c_slot].status = crate::job_system::JobStatus::Ready as u8;
            self.push_local(worker_id, continuation);
        }

        true
    }

    /// Drains all workers until no jobs remain in any local queue and no steals succeed.
    /// Simulates a cooperative multi-threaded step loop deterministically.
    pub fn drain_all<const SYS_CAP: usize>(
        &mut self,
        job_system: &mut JobSystem<SYS_CAP>,
    ) -> usize {
        let mut total_executed = 0;
        let mut step = 0;
        loop {
            let mut any_ran = false;
            for w in 0..self.worker_count {
                if self.execute_one(w, job_system, step) {
                    any_ran = true;
                    total_executed += 1;
                }
            }
            step += 1;
            if !any_ran {
                break;
            }
        }
        total_executed
    }

    /// Returns the sum of jobs executed across all workers.
    pub fn total_jobs_executed(&self) -> u64 {
        let mut sum = 0;
        for t in &self.telemetry[..self.worker_count] {
            sum += t.jobs_executed.load(Ordering::Relaxed);
        }
        sum
    }

    /// Returns the sum of jobs stolen across all workers.
    pub fn total_jobs_stolen(&self) -> u64 {
        let mut sum = 0;
        for t in &self.telemetry[..self.worker_count] {
            sum += t.jobs_stolen.load(Ordering::Relaxed);
        }
        sum
    }
}
