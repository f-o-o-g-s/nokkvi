//! Desktop notification service for the rate-this-track reminder.
//!
//! Sends a freedesktop notification (`org.freedesktop.Notifications.Notify`)
//! reminding the user to rate the current track. The notification is purely
//! informational — the user rates in Nokkvi itself (hotkey / click). Action
//! buttons were dropped because notification daemons cap how many they render
//! (noctalia, for one, shows only two), so a 1–5 star button row is not
//! portable; a plain reminder behaves identically on every daemon.
//!
//! Unlike [`crate::services::mpris`] (whose `Player` is `!Send` and so needs a
//! dedicated thread), zbus's `Connection` is `Send`, so the service runs as a
//! pure-async Iced subscription on iced's tokio runtime — the same shape as
//! [`crate::services::navidrome_sse`]. The proxy is hand-rolled because zbus is
//! only a transitive dependency elsewhere; promoting it to a direct dep
//! (Cargo.toml) costs no new compiled code.
//!
//! Reminders coalesce: each new reminder reuses the previous notification id as
//! `replaces_id`, so at most one rate-reminder is ever live. Delivery is
//! best-effort — a missing notification daemon (headless / minimal WM) is
//! logged at `warn` and the service idles, never crashing.

use std::collections::HashMap;

use iced::task::{Never, Sipper, sipper};
use tokio::sync::mpsc;
use tracing::{debug, warn};
use zbus::zvariant::Value;

/// App icon name — matches the MPRIS `desktop_entry` and the installed
/// `.desktop` id, so the daemon shows the Nokkvi icon.
const NOTIFICATION_APP_ICON: &str = "org.nokkvi.nokkvi";
/// App name reported to the notification daemon.
const NOTIFICATION_APP_NAME: &str = "Nokkvi";
/// Summary line of the reminder.
const NOTIFICATION_SUMMARY: &str = "Rate this track";
/// How long (ms) the reminder stays up before the daemon auto-dismisses it.
/// Generous on purpose — the feature exists to catch a user who has drifted to
/// another task. Daemons may override this with their own policy.
const RATING_REMINDER_EXPIRE_MS: i32 = 30_000;

/// Hand-rolled proxy for the freedesktop notification spec. zbus generates the
/// `notify` call from this trait.
#[zbus::proxy(
    interface = "org.freedesktop.Notifications",
    default_service = "org.freedesktop.Notifications",
    default_path = "/org/freedesktop/Notifications"
)]
trait Notifications {
    #[allow(clippy::too_many_arguments)]
    fn notify(
        &self,
        app_name: &str,
        replaces_id: u32,
        app_icon: &str,
        summary: &str,
        body: &str,
        actions: &[&str],
        hints: HashMap<&str, Value<'_>>,
        expire_timeout: i32,
    ) -> zbus::Result<u32>;
}

/// Commands sent from the app to the notification service.
#[derive(Debug, Clone)]
pub(crate) enum NotificationCommand {
    /// Show (or coalesce-replace) the rate-this-track reminder.
    ShowRatingReminder { title: String, artist: String },
}

/// Handle used by the app to push reminders to the notification service.
///
/// Clone freely; sends are non-blocking. Held as `Option<_>` on the root state
/// and populated by [`NotificationEvent::Connected`].
#[derive(Debug, Clone)]
pub struct NotificationConnection {
    sender: mpsc::UnboundedSender<NotificationCommand>,
}

impl NotificationConnection {
    /// Show (or coalesce-replace) the rate reminder for the given track.
    pub(crate) fn show_rating_reminder(&self, title: String, artist: String) {
        let _ = self
            .sender
            .send(NotificationCommand::ShowRatingReminder { title, artist });
    }
}

/// Events emitted from the notification service back to the app.
#[derive(Debug, Clone)]
pub enum NotificationEvent {
    /// Service connected; carries the handle for pushing reminders.
    Connected(NotificationConnection),
}

/// Build the reminder body from the track's title and artist.
fn reminder_body(title: &str, artist: &str) -> String {
    if artist.is_empty() {
        title.to_string()
    } else {
        format!("{title} · {artist}")
    }
}

/// Run the notification service as an Iced subscription.
pub(crate) fn run() -> impl Sipper<Never, NotificationEvent> {
    sipper(async |mut output| {
        let conn = match zbus::Connection::session().await {
            Ok(c) => c,
            Err(e) => {
                warn!(" [NOTIFY] No session bus; rating reminders unavailable: {e}");
                return std::future::pending::<Never>().await;
            }
        };
        let proxy = match NotificationsProxy::new(&conn).await {
            Ok(p) => p,
            Err(e) => {
                warn!(" [NOTIFY] Could not bind notification proxy: {e}");
                return std::future::pending::<Never>().await;
            }
        };

        let (cmd_tx, mut cmd_rx) = mpsc::unbounded_channel::<NotificationCommand>();
        output
            .send(NotificationEvent::Connected(NotificationConnection {
                sender: cmd_tx,
            }))
            .await;
        debug!(" [NOTIFY] Rating-reminder service ready");

        // Id of the live reminder, reused as `replaces_id` so reminders
        // coalesce — only one is ever on screen.
        let mut live_id = 0u32;

        while let Some(NotificationCommand::ShowRatingReminder { title, artist }) =
            cmd_rx.recv().await
        {
            let body = reminder_body(&title, &artist);
            let hints: HashMap<&str, Value> = HashMap::new();
            match proxy
                .notify(
                    NOTIFICATION_APP_NAME,
                    live_id,
                    NOTIFICATION_APP_ICON,
                    NOTIFICATION_SUMMARY,
                    &body,
                    &[],
                    hints,
                    RATING_REMINDER_EXPIRE_MS,
                )
                .await
            {
                Ok(id) => live_id = id,
                Err(e) => warn!(" [NOTIFY] Notify failed: {e}"),
            }
        }

        debug!(" [NOTIFY] Command channel closed; service ending");
        std::future::pending::<Never>().await
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reminder_body_joins_title_and_artist() {
        assert_eq!(reminder_body("Song", "Artist"), "Song · Artist");
    }

    #[test]
    fn reminder_body_omits_separator_when_artist_blank() {
        assert_eq!(reminder_body("Song", ""), "Song");
    }
}
