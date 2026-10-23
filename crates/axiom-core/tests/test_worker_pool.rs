//! # Tests for Chase-Lev Work-Stealing Deque and Worker Pool (`axiom-core::worker_pool`)

use axiom_core::job_system::{JobContext, JobHandle, JobPriority, JobSystem};
use axiom_core::worker_pool::{WorkStealingDeque, WorkerPoolCoordinator, WorkerTelemetry64};
use core::sync::atomic::{AtomicU32, Ordering};

static WORKER_SUM: AtomicU32 = AtomicU32::new(0);

fn accumulate_job(ctx: JobContext) {
    WORKER_SUM.fetch_add(ctx.param as u32, Ordering::SeqCst);
}

#[test]
fn test_worker_telemetry_size_and_alignment() {
    assert_eq!(core::mem::size_of::<WorkerTelemetry64>(), 64);
    assert_eq!(core::mem::align_of::<WorkerTelemetry64>(), 64);
}

#[test]
fn test_chase_lev_deque_owner_lifo() {
    let mut deque = WorkStealingDeque::<16>::new();
    assert!(deque.is_empty());
    assert_eq!(deque.len(), 0);

    let h1 = JobHandle::new(1, 1);
    let h2 = JobHandle::new(2, 1);
    let h3 = JobHandle::new(3, 1);

    assert!(deque.push(h1));
    assert!(deque.push(h2));
    assert!(deque.push(h3));
    assert_eq!(deque.len(), 3);

    // Owner pops from bottom (LIFO: h3, then h2, then h1)
    assert_eq!(deque.pop(), Some(h3));
    assert_eq!(deque.pop(), Some(h2));
    assert_eq!(deque.pop(), Some(h1));
    assert_eq!(deque.pop(), None);
    assert!(deque.is_empty());
}

#[test]
fn test_chase_lev_deque_stealer_fifo() {
    let mut deque = WorkStealingDeque::<16>::new();

    let h1 = JobHandle::new(10, 1);
    let h2 = JobHandle::new(20, 1);
    let h3 = JobHandle::new(30, 1);

    assert!(deque.push(h1));
    assert!(deque.push(h2));
    assert!(deque.push(h3));

    // Stealer steals from top (FIFO: h1, then h2, then h3)
    assert_eq!(deque.steal(), Some(h1));
    assert_eq!(deque.steal(), Some(h2));
    assert_eq!(deque.steal(), Some(h3));
    assert_eq!(deque.steal(), None);
    assert!(deque.is_empty());
}

#[test]
fn test_chase_lev_interleaved_owner_and_stealer() {
    let mut deque = WorkStealingDeque::<16>::new();

    let h1 = JobHandle::new(1, 1);
    let h2 = JobHandle::new(2, 1);
    let h3 = JobHandle::new(3, 1);
    let h4 = JobHandle::new(4, 1);

    assert!(deque.push(h1));
    assert!(deque.push(h2));
    assert!(deque.push(h3));
    assert!(deque.push(h4));

    // Stealer steals oldest from top (FIFO: h1)
    assert_eq!(deque.steal(), Some(h1));

    // Owner pops newest from bottom (LIFO: h4)
    assert_eq!(deque.pop(), Some(h4));

    // Stealer steals next oldest (h2)
    assert_eq!(deque.steal(), Some(h2));

    // Owner pops remaining (h3)
    assert_eq!(deque.pop(), Some(h3));

    // Now empty for both
    assert_eq!(deque.steal(), None);
    assert_eq!(deque.pop(), None);
}

#[test]
fn test_worker_pool_coordinator_work_stealing_drain() {
    WORKER_SUM.store(0, Ordering::SeqCst);

    let mut job_system = JobSystem::<128>::new();
    let mut coordinator = WorkerPoolCoordinator::<4, 64>::new(4);

    // Enqueue 40 jobs exclusively onto worker 0's local deque
    for i in 1..=40 {
        let handle = job_system.allocate_job(accumulate_job, i as u64, 0, JobPriority::Normal).unwrap();
        // Mark as Ready in job system and push to worker 0
        job_system.descriptors[handle.slot()].status = axiom_core::job_system::JobStatus::Ready as u8;
        assert!(coordinator.push_local(0, handle));
    }

    assert_eq!(coordinator.deques[0].len(), 40);
    assert_eq!(coordinator.deques[1].len(), 0);
    assert_eq!(coordinator.deques[2].len(), 0);
    assert_eq!(coordinator.deques[3].len(), 0);

    // Drain all workers: Workers 1, 2, 3 must steal tasks from Worker 0 and execute them!
    let total_executed = coordinator.drain_all(&mut job_system);
    assert_eq!(total_executed, 40);

    // Expected sum: 1 + 2 + ... + 40 = (40 * 41) / 2 = 820
    assert_eq!(WORKER_SUM.load(Ordering::SeqCst), 820);
    assert_eq!(job_system.total_jobs_completed, 40);

    // Verify that stealing actually occurred across workers!
    let total_stolen = coordinator.total_jobs_stolen();
    assert!(total_stolen > 0, "Idle workers 1..3 must have stolen tasks from worker 0");

    let total_stat_exec = coordinator.total_jobs_executed();
    assert_eq!(total_stat_exec, 40);
}
