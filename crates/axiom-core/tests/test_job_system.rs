//! # Tests for Deterministic Asynchronous Job System (`axiom-core::job_system`)

use axiom_core::job_system::{
    JobContext, JobDescriptor64, JobHandle, JobPriority, JobStatus, JobSystem,
};
use core::sync::atomic::{AtomicU32, Ordering};

static SINGLE_COUNTER: AtomicU32 = AtomicU32::new(0);
static DAG_COUNTER: AtomicU32 = AtomicU32::new(0);
static CHAIN_COUNTER: AtomicU32 = AtomicU32::new(0);
static HASH_COUNTER: AtomicU32 = AtomicU32::new(0);

fn single_inc_job(ctx: JobContext) {
    SINGLE_COUNTER.fetch_add(ctx.param as u32, Ordering::SeqCst);
}

fn dag_inc_job(ctx: JobContext) {
    DAG_COUNTER.fetch_add(ctx.param as u32, Ordering::SeqCst);
}

fn chain_inc_job(ctx: JobContext) {
    CHAIN_COUNTER.fetch_add(ctx.param as u32, Ordering::SeqCst);
}

fn hash_mul_job(ctx: JobContext) {
    let val = (ctx.param as u32).wrapping_mul(ctx.tag as u32);
    HASH_COUNTER.fetch_add(val, Ordering::SeqCst);
}

#[test]
fn test_job_descriptor_size_and_alignment() {
    assert_eq!(core::mem::size_of::<JobDescriptor64>(), 64, "JobDescriptor64 must be exactly 64 bytes");
    assert_eq!(core::mem::align_of::<JobDescriptor64>(), 64, "JobDescriptor64 must be 64-byte L1 aligned");
    assert_eq!(core::mem::size_of::<JobHandle>(), 8, "JobHandle must be 8 bytes");
}

#[test]
fn test_single_job_allocation_and_execution() {
    SINGLE_COUNTER.store(0, Ordering::SeqCst);
    let mut js = JobSystem::<64>::new();

    let h = js.allocate_job(single_inc_job, 42, 100, JobPriority::High).expect("Allocation must succeed");
    assert!(!h.is_null());
    assert!(js.is_valid(h));
    assert!(!js.is_completed(h));

    // Submit to ready queue
    assert!(js.submit(h));

    // Run next
    let executed = js.run_next(0);
    assert!(executed);
    assert!(js.is_completed(h));
    assert_eq!(SINGLE_COUNTER.load(Ordering::SeqCst), 42);
    assert_eq!(js.total_jobs_completed, 1);
}

#[test]
fn test_parent_child_dag_dependencies() {
    DAG_COUNTER.store(0, Ordering::SeqCst);
    let mut js = JobSystem::<128>::new();

    // 1. Parent job: adds 1,000 to counter
    let parent = js.allocate_job(dag_inc_job, 1000, 1, JobPriority::Critical).unwrap();

    // 2. Three child jobs: add 10, 20, 30 to counter
    let child1 = js.allocate_job(dag_inc_job, 10, 2, JobPriority::Normal).unwrap();
    let child2 = js.allocate_job(dag_inc_job, 20, 3, JobPriority::Normal).unwrap();
    let child3 = js.allocate_job(dag_inc_job, 30, 4, JobPriority::Normal).unwrap();

    assert!(js.set_parent(child1, parent));
    assert!(js.set_parent(child2, parent));
    assert!(js.set_parent(child3, parent));

    // Submit parent first: has 3 dependencies, so must NOT enter Ready queue yet
    assert!(js.submit(parent));
    assert_eq!(js.descriptors[parent.slot()].status, JobStatus::Pending as u8);

    // Submit children
    assert!(js.submit(child1));
    assert!(js.submit(child2));
    assert!(js.submit(child3));

    // Execute first child
    assert!(js.run_next(0));
    assert!(js.is_completed(child1));
    assert!(!js.is_completed(parent));
    assert_eq!(DAG_COUNTER.load(Ordering::SeqCst), 10);

    // Execute second child
    assert!(js.run_next(0));
    assert!(js.is_completed(child2));
    assert!(!js.is_completed(parent));
    assert_eq!(DAG_COUNTER.load(Ordering::SeqCst), 30);

    // Execute third child: parent's dependencies reach 0, so parent is automatically moved to Ready!
    assert!(js.run_next(0));
    assert!(js.is_completed(child3));
    assert_eq!(DAG_COUNTER.load(Ordering::SeqCst), 60);

    // Execute parent job now that children are done
    assert!(js.run_next(0));
    assert!(js.is_completed(parent));
    assert_eq!(DAG_COUNTER.load(Ordering::SeqCst), 1060);
    assert_eq!(js.total_jobs_completed, 4);
}

#[test]
fn test_continuation_chain() {
    CHAIN_COUNTER.store(0, Ordering::SeqCst);
    let mut js = JobSystem::<64>::new();

    // Job A -> Job B continuation
    let job_b = js.allocate_job(chain_inc_job, 50, 2, JobPriority::Normal).unwrap();
    let job_a = js.allocate_job(chain_inc_job, 25, 1, JobPriority::Normal).unwrap();

    assert!(js.set_continuation(job_a, job_b));
    assert!(js.submit(job_a));

    // Run all
    let count = js.run_all(0);
    assert_eq!(count, 2, "Both Job A and continuation Job B must execute");
    assert!(js.is_completed(job_a));
    assert!(js.is_completed(job_b));
    assert_eq!(CHAIN_COUNTER.load(Ordering::SeqCst), 75);
}

#[test]
fn test_aba_prevention_via_generation() {
    let mut js = JobSystem::<8>::new();

    let h1 = js.allocate_job(single_inc_job, 1, 1, JobPriority::Normal).unwrap();
    let slot = h1.slot();
    let gen1 = h1.generation();
    assert_eq!(gen1, 2);

    // Free the slot by completing it and resetting
    js.descriptors[slot].status = JobStatus::Free as u8;

    // Allocate again into the same slot
    let h2 = js.allocate_job(single_inc_job, 2, 2, JobPriority::Normal).unwrap();
    assert_eq!(h2.slot(), slot);
    assert_eq!(h2.generation(), 3);

    // Old handle h1 must be invalid!
    assert!(!js.is_valid(h1));
    assert!(js.is_valid(h2));
}

#[test]
fn test_deterministic_consensus_hash() {
    let mut js = JobSystem::<128>::new();

    for i in 0..50 {
        let h = js.allocate_job(hash_mul_job, (i + 1) as u64, ((i * 3) + 7) as u64, JobPriority::Normal).unwrap();
        js.submit(h);
    }

    let ran = js.run_all(0);
    assert_eq!(ran, 50);
    let hash1 = js.compute_consensus_hash();
    assert_ne!(hash1, 0);

    // Second independent run with same sequence
    let mut js2 = JobSystem::<128>::new();
    for i in 0..50 {
        let h = js2.allocate_job(hash_mul_job, (i + 1) as u64, ((i * 3) + 7) as u64, JobPriority::Normal).unwrap();
        js2.submit(h);
    }
    let ran2 = js2.run_all(0);
    assert_eq!(ran2, 50);
    let hash2 = js2.compute_consensus_hash();

    assert_eq!(hash1, hash2, "Identical job DAG execution must produce 100% bit-exact consensus hash");
}
