//! Task Manager for centralized background task tracking
//!
//! Provides:
//! - Unique task IDs for debugging
//! - Error logging with context
//! - Graceful shutdown via CancellationToken
//!
//! ## Usage
//!
//! ```ignore
//! // Fire-and-forget task with error logging
//! task_manager.spawn("persist_settings", async move {
//!     settings.save().await
//! });
//!
//! // Long-lived task with cancellation support
//! task_manager.spawn_cancellable("artwork_prefetch", |token| async move {
//!     while !token.is_cancelled() {
//!         // ... do work
//!     }
//! });
//! ```
//!
//! ## Future Work (R7.5)
//!
//! TODO: UI notifications for task status
//! - Add `TaskStatus` enum (Running, Completed, Failed)
//! - Add `on_status_change` callback for UI integration
//! - Add `TaskProgress` struct for long-running tasks
//! - Expose active task list for UI display

use std::{
    future::Future,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};

/// A handle to a spawned task
#[derive(Clone)]
pub struct TaskHandle {
    pub id: u64,
    pub name: String,
    // TODO(R7.5): Add JoinHandle for awaiting completion
    // TODO(R7.5): Add status channel for UI updates
}

/// Lightweight task manager with shutdown support
pub struct TaskManager {
    next_id: AtomicU64,
    cancellation_token: CancellationToken,
    // Track active tasks for debugging
    active_tasks: Arc<Mutex<Vec<TaskHandle>>>,
}

impl TaskManager {
    pub fn new() -> Self {
        Self {
            next_id: AtomicU64::new(1),
            cancellation_token: CancellationToken::new(),
            active_tasks: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Get the cancellation token for checking shutdown status
    pub fn cancellation_token(&self) -> CancellationToken {
        self.cancellation_token.clone()
    }

    /// Signal all tasks to shut down gracefully
    pub fn shutdown(&self) {
        warn!(" [TASK MANAGER] Initiating graceful shutdown...");
        self.cancellation_token.cancel();
    }

    /// Spawn a tracked task with automatic error logging
    ///
    /// For fire-and-forget tasks that don't need cancellation support.
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

        let active_tasks = self.active_tasks.clone();
        let handle_clone = handle.clone();

        tokio::spawn(async move {
            // Register task
            {
                let mut tasks = active_tasks.lock().await;
                tasks.push(handle_clone.clone());
            }

            // Run task
            future().await;

            // Unregister task
            {
                let mut tasks = active_tasks.lock().await;
                tasks.retain(|t| t.id != handle_clone.id);
            }
        });

        handle
    }

    /// Spawn a tracked task that returns a Result, with automatic error logging
    ///
    /// Errors are logged with the task name for easy debugging.
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

        let active_tasks = self.active_tasks.clone();
        let handle_clone = handle.clone();

        tokio::spawn(async move {
            // Register task
            {
                let mut tasks = active_tasks.lock().await;
                tasks.push(handle_clone.clone());
            }

            // Run task and log errors
            match future().await {
                Ok(_) => {
                    // Success - no logging needed for routine tasks
                    // TODO(R7.5): Send success status to UI channel
                }
                Err(e) => {
                    error!(" [TASK] {} failed: {}", task_name, e);
                    // TODO(R7.5): Send error status to UI channel
                }
            }

            // Unregister task
            {
                let mut tasks = active_tasks.lock().await;
                tasks.retain(|t| t.id != handle_clone.id);
            }
        });

        handle
    }

    /// Spawn a cancellable task that respects shutdown signals
    ///
    /// The task receives a `CancellationToken` and should check `token.is_cancelled()`
    /// periodically to exit gracefully during shutdown.
    ///
    /// For long-running background tasks like artwork prefetching.
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
        let handle_clone = handle.clone();

        tokio::spawn(async move {
            // Register task
            {
                let mut tasks = active_tasks.lock().await;
                tasks.push(handle_clone.clone());
            }

            debug!(" [TASK] Started: {}", task_name);

            // Run task with cancellation token
            future(token).await;

            info!(" [TASK] Completed: {}", task_name);

            // Unregister task
            {
                let mut tasks = active_tasks.lock().await;
                tasks.retain(|t| t.id != handle_clone.id);
            }
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

// TODO(R7.5): Future UI notification enhancements
//
// pub enum TaskStatus {
//     Running,
//     Completed,
//     Failed(String),
//     Cancelled,
// }
//
// pub struct TaskProgress {
//     pub current: u64,
//     pub total: u64,
//     pub message: String,
// }
//
// impl TaskManager {
//     /// Subscribe to task status updates for UI display
//     pub fn subscribe_status(&self) -> mpsc::Receiver<(TaskHandle, TaskStatus)> {
//         // Returns a channel that receives status updates
//     }
//
//     /// Subscribe to task progress updates for progress bars
//     pub fn subscribe_progress(&self) -> mpsc::Receiver<(TaskHandle, TaskProgress)> {
//         // Returns a channel that receives progress updates
//     }
// }
