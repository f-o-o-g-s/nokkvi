//! Task supervisor for centralized background task tracking and lifecycle management.
//!
//! Provides:
//! - Unique task IDs for debugging
//! - Error logging with context
//! - Graceful shutdown via shared `CancellationToken`
//! - Bounded async shutdown that waits for in-flight tasks up to a budget
//!
//! ## Usage
//!
//! ```ignore
//! // Fire-and-forget task with error logging
//! task_manager.spawn_result("persist_settings", || async move {
//!     settings.save().await
//! });
//!
//! // Bounded shutdown: signal + await all tasks with a 500 ms budget
//! let clean = task_manager.shutdown_all(Duration::from_millis(500)).await;
//! info!("shutdown: {clean} tasks finished cleanly");
//! ```

use std::{
    future::Future,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

use tokio::{sync::Mutex, time::timeout};
use tokio_util::{sync::CancellationToken, task::TaskTracker};
use tracing::{debug, error, info, warn};

/// Status of a background task
#[derive(Debug, Clone, PartialEq)]
pub enum TaskStatus {
    Running,
    Completed,
    Failed(String),
    Cancelled,
}

/// A handle to a spawned task
#[derive(Debug, Clone)]
pub struct TaskHandle {
    pub id: u64,
    pub name: String,
}

pub type TaskStatusReceiver = tokio::sync::mpsc::UnboundedReceiver<(TaskHandle, TaskStatus)>;

/// Task supervisor with bounded async shutdown support.
///
/// Every task is spawned onto a [`TaskTracker`], which counts it before
/// `spawn_*` returns, so `shutdown_all()` can never miss a task that was
/// spawned just ahead of it. Every task also watches the shared
/// cancellation token, so firing it ends each one at its next await.
pub struct TaskManager {
    next_id: AtomicU64,
    cancellation_token: CancellationToken,
    tracker: TaskTracker,
    status_tx: tokio::sync::mpsc::UnboundedSender<(TaskHandle, TaskStatus)>,
    status_rx: Mutex<Option<TaskStatusReceiver>>,
}

impl TaskManager {
    pub fn new() -> Self {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        Self {
            next_id: AtomicU64::new(1),
            cancellation_token: CancellationToken::new(),
            tracker: TaskTracker::new(),
            status_tx: tx,
            status_rx: Mutex::new(Some(rx)),
        }
    }

    /// Take the status receiver (once) for UI integration
    pub fn take_status_receiver(&self) -> Option<TaskStatusReceiver> {
        match self.status_rx.try_lock() {
            Ok(mut guard) => {
                if guard.is_none() {
                    warn!(
                        "[TASK MANAGER] take_status_receiver called after receiver already taken"
                    );
                }
                guard.take()
            }
            Err(_) => None,
        }
    }

    /// Signal all tasks to shut down and wait for them within `budget`.
    ///
    /// 1. Fires the shared cancellation token: each task ends at its next
    ///    await.
    /// 2. Waits until every tracked task has exited, or until the budget
    ///    runs out.
    ///
    /// A task still running at the budget is stuck in synchronous code,
    /// where neither the token nor an abort could reach it, so it is left
    /// to finish on its own; this only stops waiting for it.
    ///
    /// Returns how many of the tasks in flight at the call exited within the
    /// budget (informational). Safe to call more than once: a later call
    /// with nothing in flight returns 0 at once.
    pub async fn shutdown_all(&self, budget: Duration) -> usize {
        self.cancellation_token.cancel();
        // Closing lets `wait()` resolve once the tracker is empty. Tasks
        // spawned after this are still tracked and still waited for.
        self.tracker.close();

        let in_flight = self.tracker.len();
        if in_flight == 0 {
            debug!("[TASK MANAGER] No tasks in flight at shutdown");
            return 0;
        }
        info!(
            "[TASK MANAGER] Awaiting {in_flight} task(s) (budget: {}ms)...",
            budget.as_millis()
        );

        if timeout(budget, self.tracker.wait()).await.is_ok() {
            info!("[TASK MANAGER] All {in_flight} task(s) finished within budget");
            return in_flight;
        }
        let remaining = self.tracker.len();
        warn!(
            "[TASK MANAGER] Shutdown budget elapsed with {remaining} task(s) still running; no longer waiting for them"
        );
        in_flight.saturating_sub(remaining)
    }

    /// Spawn a tracked task with automatic error logging.
    ///
    /// For fire-and-forget tasks. The shared cancellation token is wired
    /// via `select!` so the task exits as soon as the token fires.
    pub fn spawn<F, Fut>(&self, name: &'static str, future: F) -> TaskHandle
    where
        F: FnOnce() -> Fut + Send + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        self.spawn_tracked(name, move |task_name, token| async move {
            tokio::select! {
                _ = token.cancelled() => {
                    debug!("[TASK {}] cancelled before completion", task_name);
                    TaskStatus::Cancelled
                }
                _ = future() => TaskStatus::Completed,
            }
        })
    }

    /// Spawn a tracked task that returns a `Result`, with automatic error logging.
    ///
    /// Errors are logged with the task name for easy debugging. The shared
    /// cancellation token is wired via `select!`.
    pub fn spawn_result<F, Fut, T, E>(&self, name: &'static str, future: F) -> TaskHandle
    where
        F: FnOnce() -> Fut + Send + 'static,
        Fut: Future<Output = Result<T, E>> + Send + 'static,
        E: std::fmt::Display + Send,
        T: Send + 'static,
    {
        self.spawn_tracked(name, move |task_name, token| async move {
            tokio::select! {
                _ = token.cancelled() => {
                    debug!("[TASK {}] cancelled before completion", task_name);
                    TaskStatus::Cancelled
                }
                result = future() => match result {
                    Ok(_) => TaskStatus::Completed,
                    Err(e) => {
                        error!("[TASK] {} failed: {}", task_name, e);
                        TaskStatus::Failed(e.to_string())
                    }
                },
            }
        })
    }

    /// Spawn a cancellable long-lived task.
    ///
    /// The task receives the shared `CancellationToken` and is responsible for
    /// polling `token.is_cancelled()` (or `token.cancelled().await`) at each
    /// blocking point. The token is also the shared app-wide token, so this
    /// task exits automatically when `shutdown_all()` fires.
    #[cfg(test)]
    pub fn spawn_cancellable<F, Fut>(&self, name: &'static str, future: F) -> TaskHandle
    where
        F: FnOnce(CancellationToken) -> Fut + Send + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        self.spawn_tracked(name, move |task_name, token| async move {
            debug!("[TASK] Started: {}", task_name);
            future(token.clone()).await;
            if token.is_cancelled() {
                info!("[TASK] Cancelled: {}", task_name);
                TaskStatus::Cancelled
            } else {
                info!("[TASK] Completed: {}", task_name);
                TaskStatus::Completed
            }
        })
    }

    /// The one spawn body: number the task, track it, report `Running`,
    /// run `body` (which owns how the task meets the token and how its
    /// outcome maps to a status), then report that status.
    fn spawn_tracked<B, Fut>(&self, name: &'static str, body: B) -> TaskHandle
    where
        B: FnOnce(String, CancellationToken) -> Fut + Send + 'static,
        Fut: Future<Output = TaskStatus> + Send + 'static,
    {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let task_name = format!("{name}#{id}");
        let handle = TaskHandle {
            id,
            name: task_name.clone(),
        };

        let token = self.cancellation_token.clone();
        let status_tx = self.status_tx.clone();
        let reported = handle.clone();
        // `TaskTracker::spawn` counts the task before it returns, so a
        // `shutdown_all` right after this call already waits for it.
        self.tracker.spawn(async move {
            let _ = status_tx.send((reported.clone(), TaskStatus::Running));
            let status = body(task_name, token).await;
            let _ = status_tx.send((reported, status));
        });

        handle
    }
}

impl Default for TaskManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
        time::{Duration, Instant},
    };

    use super::*;

    /// Spawn a few tasks that sleep N ms; shutdown_all with 4N budget → all finish cleanly.
    #[tokio::test]
    async fn task_manager_shutdown_awaits_in_flight_tasks() {
        let tm = TaskManager::new();
        let n = 3usize;
        let sleep_ms = 50u64;

        for i in 0..n {
            let label: &'static str = match i {
                0 => "t0",
                1 => "t1",
                _ => "t2",
            };
            tm.spawn_result(label, move || async move {
                tokio::time::sleep(Duration::from_millis(sleep_ms)).await;
                Ok::<(), anyhow::Error>(())
            });
        }

        let clean = tm.shutdown_all(Duration::from_millis(sleep_ms * 4)).await;
        assert_eq!(
            clean, n,
            "all {n} tasks should finish cleanly within 4× sleep budget"
        );
    }

    /// Calling shutdown_all twice must not panic and the second call is a no-op.
    #[tokio::test]
    async fn task_manager_shutdown_is_idempotent() {
        let tm = TaskManager::new();

        tm.spawn_result("quick", || async move { Ok::<(), anyhow::Error>(()) });

        let _first = tm.shutdown_all(Duration::from_millis(200)).await;
        // Second call with nothing in flight must not panic.
        let second = tm.shutdown_all(Duration::from_millis(200)).await;
        assert_eq!(second, 0, "nothing in flight should report 0 clean tasks");
    }

    /// A task that performs a synchronous (uncancellable) redb write AFTER its
    /// last `.await` must have that write completed before `shutdown_all`
    /// returns — pinning the happens-before edge the logout teardown relies on
    /// to order `clear_session` strictly after the persistence drain (N3).
    ///
    /// Multi-thread flavour: the in-flight task performs a blocking
    /// (uncancellable) commit, which would stall a single-worker runtime.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn shutdown_all_completes_in_flight_redb_write_before_returning() {
        use crate::services::state_storage::StateStorage;

        let temp = tempfile::TempDir::new().unwrap();
        let db_path = temp.path().join("drain.redb");
        let storage = StateStorage::new(db_path.clone()).unwrap();
        let storage_clone = storage.clone();

        let tm = TaskManager::new();

        // In-flight task: completes its last `.await` quickly, then enters a
        // synchronous (uncancellable) region — a blocking sleep stands in for
        // redb's begin_write/insert/commit — and only then commits the
        // sentinel. shutdown_all firing mid-commit must still await it to
        // completion rather than abandoning the post-await blocking write.
        tm.spawn_result("persist_sentinel", move || async move {
            tokio::time::sleep(Duration::from_millis(5)).await;
            // No further `.await` past this point: the select! future branch
            // is now driven to Ready synchronously, so cancellation cannot
            // pre-empt the blocking commit below.
            std::thread::sleep(Duration::from_millis(40));
            storage_clone.save_binary("sentinel", &42u64)?;
            Ok::<(), anyhow::Error>(())
        });

        // Let the task pass its last await into the synchronous commit
        // region before the token fires.
        tokio::time::sleep(Duration::from_millis(15)).await;

        let _clean = tm.shutdown_all(Duration::from_millis(500)).await;

        // Fresh handle proves the write committed during the drain.
        drop(storage);
        let storage2 = StateStorage::new(db_path).unwrap();
        let loaded: Option<u64> = storage2.load_binary("sentinel").unwrap();
        assert_eq!(
            loaded,
            Some(42u64),
            "shutdown_all must not return before the task's post-await blocking \
             redb commit finishes"
        );
    }

    /// A task is tracked from the moment `spawn_*` returns: a `shutdown_all`
    /// issued right after the spawn, before the runtime has polled anything,
    /// still waits for it. The task here ignores the token for a moment, so
    /// only a tracked wait can see it finish.
    #[tokio::test]
    async fn shutdown_all_waits_for_a_task_spawned_just_before() {
        let tm = TaskManager::new();
        let finished = Arc::new(AtomicBool::new(false));
        let finished2 = finished.clone();

        tm.spawn_cancellable("late", move |_token| async move {
            tokio::time::sleep(Duration::from_millis(30)).await;
            finished2.store(true, Ordering::SeqCst);
        });

        let clean = tm.shutdown_all(Duration::from_millis(500)).await;

        assert!(
            finished.load(Ordering::SeqCst),
            "shutdown_all returned before the task it was handed had finished"
        );
        assert_eq!(clean, 1);
    }

    /// A task that ignores cancellation is waited for only as long as the
    /// budget: `shutdown_all` returns on time and does not count it.
    #[tokio::test]
    async fn shutdown_all_stops_waiting_at_the_budget() {
        let tm = TaskManager::new();

        tm.spawn_cancellable("stuck", |_token| async move {
            tokio::time::sleep(Duration::from_secs(10)).await;
        });

        let started = Instant::now();
        let clean = tm.shutdown_all(Duration::from_millis(100)).await;
        let elapsed = started.elapsed();

        assert!(
            elapsed >= Duration::from_millis(100),
            "shutdown_all returned before the budget while a task was still running ({elapsed:?})"
        );
        assert!(
            elapsed < Duration::from_millis(300),
            "shutdown_all overran its budget: {elapsed:?}"
        );
        assert_eq!(clean, 0, "the straggler did not finish cleanly");
    }

    /// spawn_cancellable registers its handle; shutdown_all cancels it cleanly.
    #[tokio::test]
    async fn task_manager_spawn_cancellable_registers_handle() {
        let tm = TaskManager::new();
        let finished = Arc::new(AtomicBool::new(false));
        let finished2 = finished.clone();

        tm.spawn_cancellable("long-lived", move |token| async move {
            tokio::select! {
                _ = token.cancelled() => {}
                _ = tokio::time::sleep(Duration::from_secs(30)) => {}
            }
            finished2.store(true, Ordering::SeqCst);
        });

        tm.shutdown_all(Duration::from_millis(200)).await;

        assert!(
            finished.load(Ordering::SeqCst),
            "spawn_cancellable task should exit after shutdown_all cancels the token"
        );
    }
}
