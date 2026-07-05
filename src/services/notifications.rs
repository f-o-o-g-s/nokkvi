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
/// Summary line of the rate-change confirmation. Distinct from the reminder so
/// the two notifications read differently even though they coalesce.
const NOTIFICATION_RATING_CHANGED_SUMMARY: &str = "Rating updated";
/// Summary line of the love-change confirmation. Distinct from the rate-change
/// summary so the two read differently even though they share the service.
const NOTIFICATION_LOVE_CHANGED_SUMMARY: &str = "Favorite updated";
/// How long (ms) the reminder stays up before the daemon auto-dismisses it.
/// Generous on purpose — the feature exists to catch a user who has drifted to
/// another task. Daemons may override this with their own policy.
const RATING_REMINDER_EXPIRE_MS: i32 = 30_000;
/// How long (ms) the rate-change confirmation stays up. Shorter than the
/// reminder: a confirmation is a transient acknowledgement, not a lingering
/// call to action.
const RATING_CHANGED_EXPIRE_MS: i32 = 5_000;
/// How long (ms) the love-change confirmation stays up. Same rationale as the
/// rate-change confirmation — a transient acknowledgement.
const LOVE_CHANGED_EXPIRE_MS: i32 = 5_000;

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
// The `Show` prefix is a deliberate command-verb convention that pairs each
// variant with its `show_*` connection method; keep it rather than stripping the
// shared prefix the lint flags.
#[allow(clippy::enum_variant_names)]
#[derive(Debug, Clone)]
pub(crate) enum NotificationCommand {
    /// Show (or coalesce-replace) the rate-this-track reminder.
    ShowRatingReminder { title: String, artist: String },
    /// Show (or coalesce-replace) a confirmation of the new rating, fired when
    /// the user rates via a hotkey or the `nokkvi rate` IPC verb.
    ShowRatingChanged {
        title: String,
        artist: String,
        rating: u32,
    },
    /// Show (or coalesce-replace) a confirmation that a track was loved or
    /// unloved, fired when the user toggles love via the love hotkey or the
    /// `nokkvi love` IPC verb.
    ShowLoveChanged {
        title: String,
        artist: String,
        loved: bool,
    },
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

    /// Show (or coalesce-replace) a confirmation of the new 0..=5 rating for
    /// the given track.
    pub(crate) fn show_rating_changed(&self, title: String, artist: String, rating: u32) {
        let _ = self.sender.send(NotificationCommand::ShowRatingChanged {
            title,
            artist,
            rating,
        });
    }

    /// Show (or coalesce-replace) a confirmation that the given track was loved
    /// (`loved == true`) or unloved (`loved == false`).
    pub(crate) fn show_love_changed(&self, title: String, artist: String, loved: bool) {
        let _ = self.sender.send(NotificationCommand::ShowLoveChanged {
            title,
            artist,
            loved,
        });
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

/// Build the rate-change confirmation body from the track, its artist, and the
/// new 0..=5 rating. A `rating` of 0 means the rating was cleared, which reads
/// as "Rating cleared" rather than "0/5" so an away user does not mistake it
/// for a never-rated track. The non-zero form pairs a star glyph row with the
/// explicit `N/5` count: the glyphs give at-a-glance shape, the number stays
/// unambiguous on daemons that render ★/☆ alike and for screen readers.
fn rating_changed_body(title: &str, artist: &str, rating: u32) -> String {
    let tail = reminder_body(title, artist);
    if rating == 0 {
        return format!("☆☆☆☆☆ · Rating cleared · {tail}");
    }
    let filled = rating.min(5) as usize;
    let stars: String = "★".repeat(filled) + &"☆".repeat(5 - filled);
    format!("{stars} · {rating}/5 · {tail}")
}

/// Build the love-change confirmation body from the track, its artist, and the
/// new love state. Pairs a heart glyph with the explicit "Loved"/"Unloved"
/// word: the glyph gives at-a-glance shape, the word stays unambiguous on
/// daemons that render ♥/♡ alike and for screen readers.
fn love_changed_body(title: &str, artist: &str, loved: bool) -> String {
    let tail = reminder_body(title, artist);
    if loved {
        format!("♥ Loved · {tail}")
    } else {
        format!("♡ Unloved · {tail}")
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

        while let Some(cmd) = cmd_rx.recv().await {
            // Each command resolves to its own summary/body/expire; both then
            // share the single notify call with `live_id` as `replaces_id` so a
            // fresh reminder or confirmation coalesces over the previous one.
            let (summary, body, expire) = match cmd {
                NotificationCommand::ShowRatingReminder { title, artist } => (
                    NOTIFICATION_SUMMARY,
                    reminder_body(&title, &artist),
                    RATING_REMINDER_EXPIRE_MS,
                ),
                NotificationCommand::ShowRatingChanged {
                    title,
                    artist,
                    rating,
                } => (
                    NOTIFICATION_RATING_CHANGED_SUMMARY,
                    rating_changed_body(&title, &artist, rating),
                    RATING_CHANGED_EXPIRE_MS,
                ),
                NotificationCommand::ShowLoveChanged {
                    title,
                    artist,
                    loved,
                } => (
                    NOTIFICATION_LOVE_CHANGED_SUMMARY,
                    love_changed_body(&title, &artist, loved),
                    LOVE_CHANGED_EXPIRE_MS,
                ),
            };
            let hints: HashMap<&str, Value> = HashMap::new();
            match proxy
                .notify(
                    NOTIFICATION_APP_NAME,
                    live_id,
                    NOTIFICATION_APP_ICON,
                    summary,
                    &body,
                    &[],
                    hints,
                    expire,
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

    #[test]
    fn rating_changed_body_renders_stars_and_count() {
        assert_eq!(
            rating_changed_body("Song", "Artist", 4),
            "★★★★☆ · 4/5 · Song · Artist"
        );
    }

    #[test]
    fn rating_changed_body_full_marks() {
        assert_eq!(
            rating_changed_body("Song", "Artist", 5),
            "★★★★★ · 5/5 · Song · Artist"
        );
    }

    #[test]
    fn rating_changed_body_cleared_reads_as_cleared_not_zero() {
        assert_eq!(
            rating_changed_body("Song", "Artist", 0),
            "☆☆☆☆☆ · Rating cleared · Song · Artist"
        );
    }

    #[test]
    fn rating_changed_body_omits_artist_when_blank() {
        // An artist item (rated via hotkey) carries an empty artist field.
        assert_eq!(
            rating_changed_body("Radiohead", "", 5),
            "★★★★★ · 5/5 · Radiohead"
        );
    }

    #[test]
    fn love_changed_body_loved_reads_as_loved() {
        assert_eq!(
            love_changed_body("Song", "Artist", true),
            "♥ Loved · Song · Artist"
        );
    }

    #[test]
    fn love_changed_body_unloved_reads_as_unloved() {
        assert_eq!(
            love_changed_body("Song", "Artist", false),
            "♡ Unloved · Song · Artist"
        );
    }

    #[test]
    fn love_changed_body_omits_artist_when_blank() {
        // An artist item (loved via hotkey) carries an empty artist field.
        assert_eq!(
            love_changed_body("Radiohead", "", true),
            "♥ Loved · Radiohead"
        );
    }
}
