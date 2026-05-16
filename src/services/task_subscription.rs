//! Iced subscription that converts task status events from the task manager
//! into `Message::TaskStatusChanged` messages.
//!
//! `TaskManager` sends `(TaskHandle, TaskStatus)` whenever a background task
//! changes status. At login time, the UI takes the corresponding receiver via
//! `AppService::take_task_status_receiver()` and registers it here. The `run()`
//! function is then used as an iced subscription that yields these tuples to
//! the root `update()` loop.

use iced::task::{Never, Sipper};
use nokkvi_data::services::task_manager::{TaskHandle, TaskStatus, TaskStatusReceiver};

use super::subscription_slot::SubscriptionSlot;

static SLOT: SubscriptionSlot<(TaskHandle, TaskStatus)> = SubscriptionSlot::new("TASK_SUB");

/// Register a status receiver at login time. Replaces any previously registered
/// receiver.
pub(crate) fn register_receiver(rx: TaskStatusReceiver) {
    SLOT.register(rx);
}

/// Run the task status subscription. Yields `(TaskHandle, TaskStatus)` for each
/// task lifecycle event. Stalls on channel close (app exit).
pub(crate) fn run() -> impl Sipper<Never, (TaskHandle, TaskStatus)> {
    SLOT.run()
}
