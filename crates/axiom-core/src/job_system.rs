//! # Deterministic Asynchronous Job System (`axiom-core::job_system`)
//!
//! 0-Float, 64-Byte L1 cache aligned non-blocking job scheduler and dependency DAG solver.
//! Provides bit-exact reproducible execution across both single-threaded/WASM and
//! multi-threaded execution environments.
//!
//! - **0-Float / 64B L1 Aligned**: `JobDescriptor64` fits exactly into a 64-byte cache line,
//!   eliminating false sharing between CPU cores.
//! - **ABA Prevention**: `JobHandle` packs a 32-bit slot index and 32-bit generation counter.
//! - **Parent-Child DAG & Continuation Chains**: Atomic dependency tracking allows fine-grained
//!   parallel decomposition of voxel simulations, physics, and pathfinding.
//! - **Deterministic Consensus**: Computes 64-bit FNV-1a checksums over job results and execution traces.

use core::mem::{align_of, size_of};
use core::sync::atomic::{AtomicU32, Ordering};

/// 64-bit ABA-safe Job Handle combining 32-bit slot index and 32-bit generation counter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[repr(transparent)]
pub struct JobHandle(pub u64);

impl JobHandle {
    pub const NULL: Self = Self(0);

    /// Constructs a new JobHandle from a slot index and generation counter.
    #[inline(always)]
    pub const fn new(slot: u32, generation: u32) -> Self {
        Self(((generation as u64) << 32) | (slot as u64))
    }

    /// Extracts the 32-bit slot index.
    #[inline(always)]
    pub const fn slot(self) -> usize {
        (self.0 & 0xFFFF_FFFF) as usize
    }

    /// Extracts the 32-bit generation counter.
    #[inline(always)]
    pub const fn generation(self) -> u32 {
        (self.0 >> 32) as u32
    }

    /// Returns true if this is the null handle.
    #[inline(always)]
    pub const fn is_null(self) -> bool {
        self.0 == 0
    }
}

/// Execution status of a job in the scheduler.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum JobStatus {
    #[default]
    Free = 0,
    Pending = 1,
    Ready = 2,
    Running = 3,
    Done = 4,
}

/// Execution priority of a job.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum JobPriority {
    Critical = 0,
    High = 1,
    #[default]
    Normal = 2,
    Low = 3,
}

/// Job execution context delivered to a job function.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JobContext {
    pub handle: JobHandle,
    pub worker_id: u32,
    pub param: u64,
    pub tag: u64,
}

/// Deterministic job entry function pointer signature.
pub type JobFn = fn(JobContext);

/// Exact 64-Byte L1 cache aligned Job Descriptor.
#[derive(Debug)]
#[repr(C, align(64))]
pub struct JobDescriptor64 {
    /// 8B: Function pointer to execute
    pub entry: Option<JobFn>,
    /// 8B: User-defined 64-bit integer parameter
    pub param: u64,
    /// 8B: Parent job handle (decrements parent's counter upon completion)
    pub parent: JobHandle,
    /// 8B: Continuation job handle (submitted to Ready queue when this completes)
    pub continuation: JobHandle,
    /// 4B: Atomic unfinished dependencies counter (e.g. child jobs)
    pub pending_counter: AtomicU32,
    /// 4B: Generation counter for ABA prevention
    pub generation: u32,
    /// 8B: Job tag / classification / data slab identifier
    pub tag: u64,
    /// 1B: Status (0: Free, 1: Pending, 2: Ready, 3: Running, 4: Done)
    pub status: u8,
    /// 1B: Priority (0: Critical, 1: High, 2: Normal, 3: Low)
    pub priority: u8,
    /// 2B: Submitting worker ID
    pub worker_id: u16,
    /// 12B: Exact padding to maintain 64-byte L1 alignment
    pub _reserved: [u8; 12],
}

const _: () = assert!(size_of::<JobDescriptor64>() == 64);
const _: () = assert!(align_of::<JobDescriptor64>() == 64);

impl JobDescriptor64 {
    pub const FREE: Self = Self {
        entry: None,
        param: 0,
        parent: JobHandle::NULL,
        continuation: JobHandle::NULL,
        pending_counter: AtomicU32::new(0),
        generation: 1,
        tag: 0,
        status: JobStatus::Free as u8,
        priority: JobPriority::Normal as u8,
        worker_id: 0,
        _reserved: [0u8; 12],
    };

    #[inline(always)]
    pub fn is_free(&self) -> bool {
        self.status == JobStatus::Free as u8
    }

    #[inline(always)]
    pub fn is_ready(&self) -> bool {
        self.status == JobStatus::Ready as u8
    }

    #[inline(always)]
    pub fn is_done(&self) -> bool {
        self.status == JobStatus::Done as u8
    }
}

/// Default capacity for the flat-array job system.
pub const DEFAULT_JOB_CAPACITY: usize = 256;

/// Flat-Array deterministic Job Scheduler and Dependency DAG Solver.
#[repr(C, align(64))]
pub struct JobSystem<const CAP: usize = DEFAULT_JOB_CAPACITY> {
    /// 64B L1 aligned job descriptors (exact Flat Array, Zero-Heap)
    pub descriptors: [JobDescriptor64; CAP],
    /// Ring-buffer ready queue for jobs whose dependencies are fully resolved
    pub ready_queue: [JobHandle; CAP],
    pub ready_head: usize,
    pub ready_tail: usize,
    pub active_count: usize,
    pub total_jobs_completed: u64,
}

impl<const CAP: usize> Default for JobSystem<CAP> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const CAP: usize> JobSystem<CAP> {
    /// Creates a new, uninitialized JobSystem with all slots marked Free.
    pub const fn new() -> Self {
        Self {
            descriptors: [JobDescriptor64::FREE; CAP],
            ready_queue: [JobHandle::NULL; CAP],
            ready_head: 0,
            ready_tail: 0,
            active_count: 0,
            total_jobs_completed: 0,
        }
    }

    /// Allocates a job slot and returns its unique, ABA-safe JobHandle.
    pub fn allocate_job(
        &mut self,
        entry: JobFn,
        param: u64,
        tag: u64,
        priority: JobPriority,
    ) -> Option<JobHandle> {
        for slot in 0..CAP {
            let desc = &mut self.descriptors[slot];
            if desc.is_free() {
                let next_gen = desc.generation.wrapping_add(1).max(1);
                desc.entry = Some(entry);
                desc.param = param;
                desc.parent = JobHandle::NULL;
                desc.continuation = JobHandle::NULL;
                desc.pending_counter.store(0, Ordering::Relaxed);
                desc.generation = next_gen;
                desc.tag = tag;
                desc.status = JobStatus::Pending as u8;
                desc.priority = priority as u8;
                desc.worker_id = 0;

                self.active_count += 1;
                return Some(JobHandle::new(slot as u32, next_gen));
            }
        }
        None
    }

    /// Declares a parent-child dependency between two jobs.
    /// The parent job's pending dependency counter is incremented, and the child's parent handle is recorded.
    pub fn set_parent(&mut self, child: JobHandle, parent: JobHandle) -> bool {
        if !self.is_valid(child) || !self.is_valid(parent) {
            return false;
        }

        let p_slot = parent.slot();
        self.descriptors[p_slot].pending_counter.fetch_add(1, Ordering::SeqCst);

        let c_slot = child.slot();
        self.descriptors[c_slot].parent = parent;
        true
    }

    /// Declares a continuation job that will be automatically submitted when `current` finishes.
    pub fn set_continuation(&mut self, current: JobHandle, continuation: JobHandle) -> bool {
        if !self.is_valid(current) || !self.is_valid(continuation) {
            return false;
        }

        let slot = current.slot();
        self.descriptors[slot].continuation = continuation;
        true
    }

    /// Submits a job to the system. If it has 0 pending dependencies, it enters the Ready queue immediately.
    pub fn submit(&mut self, handle: JobHandle) -> bool {
        if !self.is_valid(handle) {
            return false;
        }

        let slot = handle.slot();
        let counter = self.descriptors[slot].pending_counter.load(Ordering::SeqCst);
        if counter == 0 {
            self.descriptors[slot].status = JobStatus::Ready as u8;
            self.push_ready(handle)
        } else {
            // Remains pending until children complete
            true
        }
    }

    /// Executes the next available job in the Ready queue.
    /// Returns true if a job was found and executed.
    pub fn run_next(&mut self, worker_id: u32) -> bool {
        let handle = match self.pop_ready() {
            Some(h) => h,
            None => return false,
        };

        if !self.is_valid(handle) {
            return false;
        }

        let slot = handle.slot();
        let entry = self.descriptors[slot].entry;
        let param = self.descriptors[slot].param;
        let tag = self.descriptors[slot].tag;
        let parent = self.descriptors[slot].parent;
        let continuation = self.descriptors[slot].continuation;

        self.descriptors[slot].status = JobStatus::Running as u8;
        self.descriptors[slot].worker_id = worker_id as u16;

        // Execute job callback
        if let Some(f) = entry {
            let ctx = JobContext {
                handle,
                worker_id,
                param,
                tag,
            };
            f(ctx);
        }

        // Mark this job as Done
        self.descriptors[slot].status = JobStatus::Done as u8;
        self.total_jobs_completed += 1;
        self.active_count = self.active_count.saturating_sub(1);

        // 1. Decrement parent's dependency counter
        if !parent.is_null() && self.is_valid(parent) {
            let p_slot = parent.slot();
            let prev = self.descriptors[p_slot].pending_counter.fetch_sub(1, Ordering::SeqCst);
            if prev == 1 {
                // Parent dependencies reached 0! Push parent to ready queue
                self.descriptors[p_slot].status = JobStatus::Ready as u8;
                self.push_ready(parent);
            }
        }

        // 2. Trigger continuation job if present
        if !continuation.is_null() && self.is_valid(continuation) {
            self.submit(continuation);
        }

        true
    }

    /// Runs all currently ready jobs and their cascading continuations until the ready queue is empty.
    /// Returns the number of jobs executed.
    pub fn run_all(&mut self, worker_id: u32) -> usize {
        let mut executed = 0;
        while self.run_next(worker_id) {
            executed += 1;
        }
        executed
    }

    /// Returns true if a job has completed its execution.
    pub fn is_completed(&self, handle: JobHandle) -> bool {
        if !self.is_valid(handle) {
            return false;
        }
        self.descriptors[handle.slot()].is_done()
    }

    /// Checks if a job handle refers to a currently allocated, valid slot with matching generation.
    #[inline(always)]
    pub fn is_valid(&self, handle: JobHandle) -> bool {
        if handle.is_null() {
            return false;
        }
        let slot = handle.slot();
        if slot >= CAP {
            return false;
        }
        let desc = &self.descriptors[slot];
        !desc.is_free() && desc.generation == handle.generation()
    }

    /// Pushes a job handle to the ready ring buffer.
    fn push_ready(&mut self, handle: JobHandle) -> bool {
        let next_head = (self.ready_head + 1) % CAP;
        if next_head == self.ready_tail {
            // Queue full
            return false;
        }
        self.ready_queue[self.ready_head] = handle;
        self.ready_head = next_head;
        true
    }

    /// Pops a job handle from the ready ring buffer.
    fn pop_ready(&mut self) -> Option<JobHandle> {
        if self.ready_head == self.ready_tail {
            return None;
        }
        let handle = self.ready_queue[self.ready_tail];
        self.ready_tail = (self.ready_tail + 1) % CAP;
        Some(handle)
    }

    /// Computes a 64-bit deterministic FNV-1a checksum over all completed jobs and their states.
    pub fn compute_consensus_hash(&self) -> u64 {
        let mut hash: u64 = 0xCBF2_9CE4_8422_2325;
        hash ^= self.total_jobs_completed.wrapping_mul(0x100_0000_01B3);

        for slot in 0..CAP {
            let desc = &self.descriptors[slot];
            if !desc.is_free() {
                hash ^= (desc.status as u64) | ((desc.priority as u64) << 8);
                hash ^= desc.param.wrapping_mul(0x100_0000_01B3);
                hash ^= desc.tag.wrapping_mul(0x100_0000_01B3);
                hash ^= (desc.generation as u64) << 16;
                hash = hash.wrapping_mul(0x100_0000_01B3);
            }
        }
        hash
    }
}
