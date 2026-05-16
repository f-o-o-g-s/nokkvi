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

use iced::task::{Never, Sipper};
use tokio::sync::mpsc::UnboundedReceiver;

use super::subscription_slot::SubscriptionSlot;

static SLOT: SubscriptionSlot<()> = SubscriptionSlot::new("QUEUE_CHANGED_SUB");

/// Register a queue-changed receiver at login time. Replaces any previously
/// registered receiver. Called once from `handle_login_result` after a
/// successful login.
pub(crate) fn register_receiver(rx: UnboundedReceiver<()>) {
    SLOT.register(rx);
}

/// Run the queue-changed subscription as an iced subscription. Yields one `()`
/// per queue change event. Stalls on channel close (app exit).
pub(crate) fn run() -> impl Sipper<Never, ()> {
    SLOT.run()
}
