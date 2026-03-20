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

use std::sync::OnceLock;

use iced::task::{Never, Sipper, sipper};
use tokio::sync::{Mutex, mpsc::UnboundedReceiver};
use tracing::debug;

/// Global slot for the loop receiver, set at login time.
///
/// `OnceLock` is used so the slot can be set once per process life.
/// Wrap in `Mutex` to allow async `recv()` calls from the sipper.
static LOOP_RX: OnceLock<Mutex<Option<UnboundedReceiver<String>>>> = OnceLock::new();

/// Register a loop receiver at login time.
///
/// Replaces any previously registered receiver. Called once from
/// `handle_login_result` after a successful login.
pub(crate) fn register_receiver(rx: UnboundedReceiver<String>) {
    let slot = LOOP_RX.get_or_init(|| Mutex::new(None));
    // Temporarily block to store the new receiver synchronously.
    // Safe: no other tokio thread can hold this lock at login time.
    if let Ok(mut guard) = slot.try_lock() {
        *guard = Some(rx);
        debug!(" [LOOP_SUB] Loop receiver registered");
    }
}

/// Run the repeat-one loop subscription as an iced subscription.
///
/// Yields one `String` (song ID) per loop event. Terminates only when the
/// sender side is dropped (app exit).
pub(crate) fn run() -> impl Sipper<Never, String> {
    sipper(async |mut output| {
        loop {
            // Block until either a song ID arrives or the channel is closed.
            let song_id = {
                let slot = LOOP_RX.get_or_init(|| Mutex::new(None));
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

            match song_id {
                Some(id) => {
                    debug!(" [LOOP_SUB] Loop event for song: {}", id);
                    output.send(id).await;
                }
                None => {
                    // Channel closed (app exit), stop subscription.
                    debug!(" [LOOP_SUB] Loop channel closed, subscription ending");
                    break;
                }
            }
        }

        std::future::pending::<Never>().await
    })
}
