//! Iced subscription that converts repeat-one loop events from the audio engine
//! into `ScrobbleMessage::TrackLooped` messages.
//!
//! `PlaybackController` sends song IDs on an `UnboundedSender<String>` whenever
//! the same track loops in repeat-one mode. At login time, the UI takes the
//! corresponding receiver via `AppService::take_loop_receiver()` and registers it
//! here. The `run()` function is then used as an iced subscription that yields
//! `Message::Scrobble(ScrobbleMessage::TrackLooped(song_id))` events.
//!
//! Only one receiver is active at a time. Logging out replaces it via
//! `register_receiver`, triggering a fresh subscription identity.

use iced::task::{Never, Sipper};
use tokio::sync::mpsc::UnboundedReceiver;

use super::subscription_slot::SubscriptionSlot;

static SLOT: SubscriptionSlot<String> = SubscriptionSlot::new("LOOP_SUB");

/// Register a loop receiver at login time. Replaces any previously registered
/// receiver. Called once from `handle_login_result` after a successful login.
pub(crate) fn register_receiver(rx: UnboundedReceiver<String>) {
    SLOT.register(rx);
}

/// Run the repeat-one loop subscription as an iced subscription. Yields one
/// `String` (song ID) per loop event. Stalls on channel close (app exit).
pub(crate) fn run() -> impl Sipper<Never, String> {
    SLOT.run()
}
