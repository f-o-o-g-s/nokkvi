//! Task supervisor for centralized background task tracking and lifecycle management.
//!
//! Provides:
//! - Unique task IDs for debugging
//! - Error logging with context
//! - Graceful shutdown via shared `CancellationToken`
//! - Bounded async shutdown that awaits in-flight tasks and aborts stragglers
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
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use tokio::{sync::Mutex, task::JoinSet, time::timeout};
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};

/// Status of a background task
#[derive(Debug, Clone, PartialEq)]
pub enum TaskStatus {
    Running,
    Completed,
    Failed(String),
    Cancelled,
}

/// Progress update for a background task
#[derive(Debug, Clone)]
pub struct TaskProgress {
    pub current: u64,
    pub total: u64,
    pub message: String,
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
/// Tracks all spawned tasks via a `JoinSet` so that `shutdown_all()` can
/// await their completion within a configurable time budget, aborting any
/// that exceed it. The synchronous `shutdown()` method is preserved for
/// call sites (logout flow) that only need to fire the cancellation signal
/// without waiting.
pub struct TaskManager {
    next_id: AtomicU64,
    cancellation_token: CancellationToken,
    /// Name-based active task list (debug / health checks).
    active_tasks: Arc<Mutex<Vec<TaskHandle>>>,
    /// JoinSet of all spawned task handles — used by `shutdown_all()`.
    join_set: Arc<Mutex<JoinSet<()>>>,
    status_tx: tokio::sync::mpsc::UnboundedSender<(TaskHandle, TaskStatus)>,
    status_rx: Arc<Mutex<Option<TaskStatusReceiver>>>,
}

impl TaskManager {
    pub fn new() -> Self {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        Self {
            next_id: AtomicU64::new(1),
            cancellation_token: CancellationToken::new(),
            active_tasks: Arc::new(Mutex::new(Vec::new())),
            join_set: Arc::new(Mutex::new(JoinSet::new())),
            status_tx: tx,
            status_rx: Arc::new(Mutex::new(Some(rx))),
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

    /// Get the cancellation token for checking shutdown status
    pub fn cancellation_token(&self) -> CancellationToken {
        self.cancellation_token.clone()
    }

    /// Signal all tasks to shut down gracefully (non-blocking).
    ///
    /// Fires the shared cancellation token so that any `select!`-guarded task
    /// begins unwinding. This returns immediately; use `shutdown_all()` if you
    /// need to wait for tasks to finish.
    pub fn shutdown(&self) {
        warn!("[TASK MANAGER] Initiating graceful shutdown (signal only)...");
        self.cancellation_token.cancel();
    }

    /// Signal all tasks to shut down and await their completion within `budget`.
    ///
    /// 1. Fires the shared cancellation token.
    /// 2. Awaits all tracked `JoinHandle`s with the total time budget.
    /// 3. Aborts any task that has not exited before the budget expires.
    ///
    /// Returns the number of tasks that finished cleanly (informational;
    /// aborted tasks are not counted). Safe to call multiple times — the
    /// second and subsequent calls are no-ops if the JoinSet is already drained.
    pub async fn shutdown_all(&self, budget: Duration) -> usize {
        self.cancellation_token.cancel();
        info!(
            "[TASK MANAGER] Awaiting all tasks (budget: {}ms)...",
            budget.as_millis()
        );

        let mut set = self.join_set.lock().await;

        if set.is_empty() {
            debug!("[TASK MANAGER] No tasks in flight at shutdown");
            return 0;
        }

        let total = set.len();
        let mut clean = 0usize;

        match timeout(budget, async {
            while let Some(result) = set.join_next().await {
                match result {
                    Ok(()) => clean += 1,
                    Err(e) if e.is_cancelled() => {
                        debug!("[TASK MANAGER] Task cancelled during shutdown");
                    }
                    Err(e) => {
                        warn!("[TASK MANAGER] Task panicked during shutdown: {e}");
                    }
                }
            }
        })
        .await
        {
            Ok(()) => {
                info!("[TASK MANAGER] All {total} tasks finished cleanly within budget");
            }
            Err(_elapsed) => {
                let remaining = set.len();
                warn!(
                    "[TASK MANAGER] Shutdown budget elapsed; aborting {remaining} remaining task(s)"
                );
                set.abort_all();
                // Drain abort results so the JoinSet is empty on next call.
                while set.join_next().await.is_some() {}
            }
        }

        clean
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
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let task_name = format!("{name}#{id}");
        let handle = TaskHandle {
            id,
            name: task_name.clone(),
        };

        let token = self.cancellation_token.clone();
        let active_tasks = self.active_tasks.clone();
        let join_set = self.join_set.clone();
        let handle_clone = handle.clone();
        let status_tx = self.status_tx.clone();

        // The task body runs the user future and updates bookkeeping.
        let task_fut = async move {
            {
                let mut tasks = active_tasks.lock().await;
                tasks.push(handle_clone.clone());
            }

            let _ = status_tx.send((handle_clone.clone(), TaskStatus::Running));

            tokio::select! {
                _ = token.cancelled() => {
                    debug!("[TASK {}] cancelled before completion", task_name);
                    let _ = status_tx.send((handle_clone.clone(), TaskStatus::Cancelled));
                }
                _ = future() => {
                    let _ = status_tx.send((handle_clone.clone(), TaskStatus::Completed));
                }
            }

            {
                let mut tasks = active_tasks.lock().await;
                tasks.retain(|t| t.id != handle_clone.id);
            }
        };

        // Spawn the task, then register its JoinHandle in the supervisor set.
        // The registration happens in a brief async step so the caller doesn't block.
        let join_handle = tokio::spawn(task_fut);
        tokio::spawn(async move {
            let mut set = join_set.lock().await;
            set.spawn(async move {
                let _ = join_handle.await;
            });
        });

        handle
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
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let task_name = format!("{name}#{id}");
        let handle = TaskHandle {
            id,
            name: task_name.clone(),
        };

        let token = self.cancellation_token.clone();
        let active_tasks = self.active_tasks.clone();
        let join_set = self.join_set.clone();
        let handle_clone = handle.clone();
        let status_tx = self.status_tx.clone();

        let task_fut = async move {
            {
                let mut tasks = active_tasks.lock().await;
                tasks.push(handle_clone.clone());
            }

            let _ = status_tx.send((handle_clone.clone(), TaskStatus::Running));

            tokio::select! {
                _ = token.cancelled() => {
                    debug!("[TASK {}] cancelled before completion", task_name);
                    let _ = status_tx.send((handle_clone.clone(), TaskStatus::Cancelled));
                }
                result = future() => {
                    match result {
                        Ok(_) => {
                            let _ = status_tx.send((handle_clone.clone(), TaskStatus::Completed));
                        }
                        Err(e) => {
                            error!("[TASK] {} failed: {}", task_name, e);
                            let _ = status_tx
                                .send((handle_clone.clone(), TaskStatus::Failed(e.to_string())));
                        }
                    }
                }
            }

            {
                let mut tasks = active_tasks.lock().await;
                tasks.retain(|t| t.id != handle_clone.id);
            }
        };

        let join_handle = tokio::spawn(task_fut);
        tokio::spawn(async move {
            let mut set = join_set.lock().await;
            set.spawn(async move {
                let _ = join_handle.await;
            });
        });

        handle
    }

    /// Spawn a cancellable long-lived task.
    ///
    /// The task receives the shared `CancellationToken` and is responsible for
    /// polling `token.is_cancelled()` (or `token.cancelled().await`) at each
    /// blocking point. The token is also the shared app-wide token, so this
    /// task exits automatically when `shutdown()` / `shutdown_all()` fires.
    pub fn spawn_cancellable<F, Fut>(&self, name: &'static str, future: F) -> TaskHandle
    where
        F: FnOnce(CancellationToken) -> Fut + Send + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let task_name = format!("{name}#{id}");
        let handle = TaskHandle {
            id,
            name: task_name.clone(),
        };

        let token = self.cancellation_token.clone();
        let active_tasks = self.active_tasks.clone();
        let join_set = self.join_set.clone();
        let handle_clone = handle.clone();
        let status_tx = self.status_tx.clone();

        let task_fut = async move {
            {
                let mut tasks = active_tasks.lock().await;
                tasks.push(handle_clone.clone());
            }

            debug!("[TASK] Started: {}", task_name);
            let _ = status_tx.send((handle_clone.clone(), TaskStatus::Running));

            future(token.clone()).await;

            if token.is_cancelled() {
                info!("[TASK] Cancelled: {}", task_name);
                let _ = status_tx.send((handle_clone.clone(), TaskStatus::Cancelled));
            } else {
                info!("[TASK] Completed: {}", task_name);
                let _ = status_tx.send((handle_clone.clone(), TaskStatus::Completed));
            }

            {
                let mut tasks = active_tasks.lock().await;
                tasks.retain(|t| t.id != handle_clone.id);
            }
        };

        let join_handle = tokio::spawn(task_fut);
        tokio::spawn(async move {
            let mut set = join_set.lock().await;
            set.spawn(async move {
                let _ = join_handle.await;
            });
        });

        handle
    }

    /// Get count of currently active tasks (for debugging/health checks)
    pub async fn active_task_count(&self) -> usize {
        self.active_tasks.lock().await.len()
    }

    /// Get names of currently active tasks (for debugging)
    pub async fn active_task_names(&self) -> Vec<String> {
        self.active_tasks
            .lock()
            .await
            .iter()
            .map(|h| h.name.clone())
            .collect()
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

        // Allow handle-registration tasks a tick to insert into the JoinSet.
        tokio::time::sleep(Duration::from_millis(10)).await;

        let clean = tm.shutdown_all(Duration::from_millis(sleep_ms * 4)).await;
        assert_eq!(
            clean, n,
            "all {n} tasks should finish cleanly within 4× sleep budget"
        );
    }

    /// Spawn a task that sleeps 10 s; shutdown_all with 100 ms budget → returns quickly.
    #[tokio::test]
    async fn task_manager_shutdown_aborts_when_over_budget() {
        let tm = TaskManager::new();

        tm.spawn_result("slow", || async move {
            tokio::time::sleep(Duration::from_secs(10)).await;
            Ok::<(), anyhow::Error>(())
        });

        // Allow handle registration.
        tokio::time::sleep(Duration::from_millis(10)).await;

        let budget = Duration::from_millis(100);
        let started = Instant::now();
        let _clean = tm.shutdown_all(budget).await;
        let elapsed = started.elapsed();

        assert!(
            elapsed < Duration::from_millis(300),
            "shutdown_all should return within ~200 ms of budget expiry, got {elapsed:?}"
        );
    }

    /// Calling shutdown_all twice must not panic and the second call is a no-op.
    #[tokio::test]
    async fn task_manager_shutdown_is_idempotent() {
        let tm = TaskManager::new();

        tm.spawn_result("quick", || async move { Ok::<(), anyhow::Error>(()) });

        tokio::time::sleep(Duration::from_millis(10)).await;

        let _first = tm.shutdown_all(Duration::from_millis(200)).await;
        // Second call on a drained JoinSet must not panic.
        let second = tm.shutdown_all(Duration::from_millis(200)).await;
        assert_eq!(second, 0, "drained JoinSet should report 0 clean tasks");
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

        tokio::time::sleep(Duration::from_millis(10)).await;

        tm.shutdown_all(Duration::from_millis(200)).await;

        assert!(
            finished.load(Ordering::SeqCst),
            "spawn_cancellable task should exit after shutdown_all cancels the token"
        );
    }
}
