//! Shared infrastructure for "one mpsc receiver, parked behind a global slot,
//! drained by an iced [`Sipper`] subscription" wrappers.
//!
//! Three subscription modules in this crate (`loop_subscription`,
//! `queue_changed_subscription`, `task_subscription`) all follow the same
//! shape: a `static` slot holds an `UnboundedReceiver<T>` registered at login
//! time, and `iced::Subscription::run(module::run)` drains it forever. This
//! type consolidates that shape so each module collapses to ~12 lines that
//! delegate to a `static SLOT: SubscriptionSlot<T>`.
//!
//! # Semantics
//!
//! - **Registration** is synchronous (`pub fn register`). The
//!   [`parking_lot::Mutex`] cannot fail, so registrations cannot be silently
//!   dropped under contention — replacing the previous `OnceLock<tokio::Mutex>`
//!   + `try_lock` combination closes that latent race (audit Finding 13).
//! - **Subscription wakeup** is event-driven via [`tokio::sync::Notify`].
//!   When the sipper starts before the first `register` call, it parks on
//!   `Notify::notified()` and is woken in O(1) by `Notify::notify_one()`,
//!   replacing the 100 ms-sleep polling fallback the old wrappers used
//!   (audit Finding 8).
//! - **Channel close** stalls the sipper on `std::future::pending::<Never>()`
//!   forever. This preserves the original observable behavior: each module
//!   used to `break` out of its recv loop and then await `pending::<Never>()`
//!   to keep the iced subscription identity alive. Re-registering after a
//!   close is intentionally **not** supported here; logging back in produces
//!   a fresh subscription identity in iced, which restarts the sipper.

use iced::task::{Never, Sipper, sipper};
use parking_lot::Mutex;
use tokio::sync::{Notify, mpsc::UnboundedReceiver};
use tracing::debug;

/// Shared slot for the "single mpsc receiver, drained as an iced sipper"
/// pattern. See module docs.
pub(crate) struct SubscriptionSlot<T> {
    rx: Mutex<Option<UnboundedReceiver<T>>>,
    notify: Notify,
    tag: &'static str,
}

impl<T> SubscriptionSlot<T> {
    /// Construct an empty slot. `tag` is included in the `debug!` logs the
    /// slot emits at registration and channel-close time.
    ///
    /// `const` so callers can write
    /// `static SLOT: SubscriptionSlot<T> = SubscriptionSlot::new("…");`.
    pub(crate) const fn new(tag: &'static str) -> Self {
        Self {
            rx: Mutex::new(None),
            notify: Notify::const_new(),
            tag,
        }
    }

    /// Store the receiver in the slot and wake any sipper parked on the first
    /// registration. Replaces any previously registered receiver.
    ///
    /// The [`parking_lot::Mutex`] guarantees we always succeed; there is no
    /// silent-drop path (unlike the previous `tokio::Mutex::try_lock` shape).
    pub(crate) fn register(&self, rx: UnboundedReceiver<T>) {
        *self.rx.lock() = Some(rx);
        self.notify.notify_one();
        debug!(" [{}] Receiver registered", self.tag);
    }

    /// Drain the registered receiver as an iced sipper.
    ///
    /// - Waits for the first registration via `Notify::notified()`.
    /// - For each item received, sends it through the sipper output.
    /// - When the channel closes (sender dropped), parks forever on
    ///   `std::future::pending::<Never>()` so the iced subscription identity
    ///   stays alive but emits nothing further.
    pub(crate) fn run(&'static self) -> impl Sipper<Never, T>
    where
        T: Send + 'static,
    {
        sipper(async |mut output| {
            // Wait for the first `register()` to land a receiver in the slot.
            // Loop because the `register()` call may happen before we ever
            // park on `notified()`; the `parking_lot` lock check covers that
            // race-free.
            let mut rx = loop {
                if let Some(rx) = self.rx.lock().take() {
                    break rx;
                }
                self.notify.notified().await;
            };

            while let Some(value) = rx.recv().await {
                output.send(value).await;
            }

            debug!(" [{}] Channel closed, subscription ending", self.tag);
            std::future::pending::<Never>().await
        })
    }

    /// Test-only helper: reports whether the slot currently holds a receiver.
    /// Used by the unit tests in this module to verify `register` synchronously
    /// updates state.
    #[cfg(test)]
    pub(crate) fn slot_is_set(&self) -> bool {
        self.rx.lock().is_some()
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use futures::StreamExt;
    use tokio::sync::mpsc;

    use super::*;

    /// Helper to obtain a `&'static SubscriptionSlot<T>` inside a test.
    /// `Box::leak` is acceptable here because each test gets its own slot
    /// (no shared global state across tests) and the leak is bounded by
    /// the test process lifetime.
    fn leak_slot<T: Send + 'static>(tag: &'static str) -> &'static SubscriptionSlot<T> {
        Box::leak(Box::new(SubscriptionSlot::<T>::new(tag)))
    }

    #[tokio::test]
    async fn register_then_run_emits_value() {
        let slot = leak_slot::<i32>("test_register_then_run");
        let (tx, rx) = mpsc::unbounded_channel();
        slot.register(rx);

        tx.send(1).unwrap();
        tx.send(2).unwrap();
        tx.send(3).unwrap();

        let mut sip = slot.run().pin();
        assert_eq!(sip.next().await, Some(1));
        assert_eq!(sip.next().await, Some(2));
        assert_eq!(sip.next().await, Some(3));
    }

    #[tokio::test]
    async fn run_then_register_picks_up_after_notify() {
        let slot = leak_slot::<i32>("test_run_then_register");

        // Start sipping first; the sipper task should park on `notified().await`
        // because the slot is empty.
        let driver = tokio::spawn(async move {
            let mut sip = slot.run().pin();
            sip.next().await
        });

        // Give the driver task a chance to park.
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(!driver.is_finished(), "sipper should be parked on notify");

        let (tx, rx) = mpsc::unbounded_channel();
        slot.register(rx);
        tx.send(42).unwrap();

        let got = tokio::time::timeout(Duration::from_secs(1), driver)
            .await
            .expect("driver should complete after registration")
            .expect("join handle");

        assert_eq!(got, Some(42));
    }

    #[tokio::test]
    async fn channel_close_stalls_sipper() {
        let slot = leak_slot::<i32>("test_channel_close_stalls");
        let (tx, rx) = mpsc::unbounded_channel();
        slot.register(rx);

        tx.send(7).unwrap();
        drop(tx);

        let mut sip = slot.run().pin();

        // First value still drains.
        assert_eq!(sip.next().await, Some(7));

        // Subsequent reads should hang forever (the sipper parks on
        // `pending::<Never>()`); bound the test on a timeout and assert
        // we did NOT yield further output and did NOT terminate.
        let stalled = tokio::time::timeout(Duration::from_millis(100), sip.next()).await;
        assert!(
            stalled.is_err(),
            "sipper should stall after channel close, not terminate or emit"
        );
    }

    #[tokio::test]
    async fn register_replaces_existing_slot() {
        let slot = leak_slot::<i32>("test_register_replaces");

        let (_tx_a, rx_a) = mpsc::unbounded_channel::<i32>();
        slot.register(rx_a);
        assert!(slot.slot_is_set());

        let (tx_b, rx_b) = mpsc::unbounded_channel::<i32>();
        slot.register(rx_b);
        assert!(slot.slot_is_set());

        // The second register must have replaced the first: sending on `tx_b`
        // (the second sender) should reach the sipper.
        tx_b.send(99).unwrap();

        let mut sip = slot.run().pin();
        assert_eq!(sip.next().await, Some(99));
    }

    #[tokio::test]
    async fn register_never_silently_drops_under_contention() {
        // Even if another task is briefly holding the parking_lot mutex,
        // `register` uses a blocking `lock()` (not `try_lock`), so it must
        // wait and complete. There is no silent-drop path.
        let slot = leak_slot::<i32>("test_no_silent_drop");

        let hold_handle = {
            let slot_ref = slot;
            tokio::task::spawn_blocking(move || {
                let _guard = slot_ref.rx.lock();
                std::thread::sleep(Duration::from_millis(30));
                // Lock released on drop here.
            })
        };

        // Yield long enough for the spawn_blocking task to grab the lock.
        tokio::time::sleep(Duration::from_millis(5)).await;

        let (_tx, rx) = mpsc::unbounded_channel::<i32>();
        // `register` blocks on the parking_lot mutex until the holder releases
        // it. The call cannot fail or no-op.
        slot.register(rx);

        hold_handle.await.unwrap();

        assert!(
            slot.slot_is_set(),
            "register must populate the slot even after contention"
        );
    }

    /// Compile-time check: the three concrete `T` types used by the existing
    /// subscription modules (`String`, `()`, and the task-status tuple) must
    /// all satisfy `SubscriptionSlot<T>`'s bounds. If any of these stop
    /// compiling, the corresponding subscription module would too.
    #[allow(dead_code)]
    fn _type_check_concrete_subscription_slot_types() {
        use nokkvi_data::services::task_manager::{TaskHandle, TaskStatus};

        let _: SubscriptionSlot<String> = SubscriptionSlot::new("loop");
        let _: SubscriptionSlot<()> = SubscriptionSlot::new("queue");
        let _: SubscriptionSlot<(TaskHandle, TaskStatus)> = SubscriptionSlot::new("task");
    }
}
