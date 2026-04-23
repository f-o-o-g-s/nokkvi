//! Iced subscription that converts task status events from the task manager
//! into `Message::TaskStatusChanged` messages.
//!
//! `TaskManager` sends `(TaskHandle, TaskStatus)` whenever a background task changes status.
//! At login time, the UI takes the corresponding receiver via `AppService::take_task_status_receiver()`
//! and registers it here. The `run()` function is then used as an iced subscription that yields
//! these tuples to the root `update()` loop.

use std::sync::OnceLock;

use iced::task::{Never, Sipper, sipper};
use nokkvi_data::services::task_manager::{TaskHandle, TaskStatus, TaskStatusReceiver};
use tokio::sync::Mutex;
use tracing::debug;

/// Global slot for the task status receiver, set at login time.
static STATUS_RX: OnceLock<Mutex<Option<TaskStatusReceiver>>> = OnceLock::new();

/// Register a status receiver at login time.
pub(crate) fn register_receiver(rx: TaskStatusReceiver) {
    let slot = STATUS_RX.get_or_init(|| Mutex::new(None));
    if let Ok(mut guard) = slot.try_lock() {
        *guard = Some(rx);
        debug!(" [TASK_SUB] Task status receiver registered");
    }
}

/// Run the task status subscription.
pub(crate) fn run() -> impl Sipper<Never, (TaskHandle, TaskStatus)> {
    sipper(async |mut output| {
        loop {
            let event = {
                let slot = STATUS_RX.get_or_init(|| Mutex::new(None));
                let mut guard = slot.lock().await;
                match guard.as_mut() {
                    Some(rx) => rx.recv().await,
                    None => {
                        drop(guard);
                        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                        continue;
                    }
                }
            };

            match event {
                Some((handle, status)) => {
                    output.send((handle, status)).await;
                }
                None => {
                    debug!(" [TASK_SUB] Channel closed, subscription ending");
                    break;
                }
            }
        }

        std::future::pending::<Never>().await
    })
}
