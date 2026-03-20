//! Iced subscription that converts queue-changed events from the completion
//! callback into `Message::LoadQueue` messages.
//!
//! `PlaybackController` sends `()` on an `UnboundedSender<()>` whenever a track
//! auto-advances (after consume + `refresh_from_queue()`). At login time, the
//! UI takes the corresponding receiver via `AppService::take_queue_changed_receiver()`
//! and registers it here. The `run()` function is then used as an iced subscription
//! that yields `()` events, each mapped to `Message::LoadQueue`.
//!
//! Only one receiver is active at a time. Logging out replaces it via
//! `register_receiver`, triggering a fresh subscription identity.

use std::sync::OnceLock;

use iced::task::{Never, Sipper, sipper};
use tokio::sync::{Mutex, mpsc::UnboundedReceiver};
use tracing::debug;

/// Global slot for the queue-changed receiver, set at login time.
///
/// `OnceLock` is used so the slot can be set once per process life.
/// Wrap in `Mutex` to allow async `recv()` calls from the sipper.
static QUEUE_CHANGED_RX: OnceLock<Mutex<Option<UnboundedReceiver<()>>>> = OnceLock::new();

/// Register a queue-changed receiver at login time.
///
/// Replaces any previously registered receiver. Called once from
/// `handle_login_result` after a successful login.
pub(crate) fn register_receiver(rx: UnboundedReceiver<()>) {
    let slot = QUEUE_CHANGED_RX.get_or_init(|| Mutex::new(None));
    // Temporarily block to store the new receiver synchronously.
    // Safe: no other tokio thread can hold this lock at login time.
    if let Ok(mut guard) = slot.try_lock() {
        *guard = Some(rx);
        debug!(" [QUEUE_CHANGED_SUB] Queue-changed receiver registered");
    }
}

/// Run the queue-changed subscription as an iced subscription.
///
/// Yields one `()` per queue change event. Terminates only when the
/// sender side is dropped (app exit).
pub(crate) fn run() -> impl Sipper<Never, ()> {
    sipper(async |mut output| {
        loop {
            // Block until either a signal arrives or the channel is closed.
            let event = {
                let slot = QUEUE_CHANGED_RX.get_or_init(|| Mutex::new(None));
                let mut guard = slot.lock().await;
                match guard.as_mut() {
                    Some(rx) => rx.recv().await,
                    None => {
                        // No receiver yet — wait a bit and retry.
                        drop(guard);
                        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                        continue;
                    }
                }
            };

            match event {
                Some(()) => {
                    debug!(" [QUEUE_CHANGED_SUB] Queue changed event received");
                    output.send(()).await;
                }
                None => {
                    // Channel closed (app exit), stop subscription.
                    debug!(" [QUEUE_CHANGED_SUB] Channel closed, subscription ending");
                    break;
                }
            }
        }

        std::future::pending::<Never>().await
    })
}
