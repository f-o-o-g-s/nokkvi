//! macOS Now Playing / media-key backend.
//!
//! Speaks the same [`MprisEvent`] / [`MprisCommand`] / [`MprisConnection`]
//! types as the Linux D-Bus service, so `update/mpris.rs` and the tick's push
//! in `update/playback.rs` stay shared. `souvlaki` publishes to
//! `MPNowPlayingInfoCenter` and routes `MPRemoteCommandCenter` commands back
//! as events. Control Center, the menu-bar Now Playing widget and the
//! keyboard media keys only treat the app as "now playing" while it has
//! published info and at least one enabled command, which `attach` does.

use std::{
    sync::mpsc as std_mpsc,
    time::{Duration, Instant},
};

use iced::task::{Never, Sipper, sipper};
use mpris_server::PlaybackStatus;
use souvlaki::{
    MediaControlEvent, MediaControls, MediaMetadata, MediaPlayback, MediaPosition, PlatformConfig,
    SeekDirection,
};
use tokio::sync::mpsc as tokio_mpsc;
use tracing::{debug, trace, warn};

use super::mpris::{MprisCommand, MprisConnection, MprisEvent};

const EVENT_CHANNEL_DEPTH: usize = 100;
const EVENT_POLL_TIMEOUT: Duration = Duration::from_millis(100);
/// How long the worker waits for a command before checking the subscription
/// is still alive.
const CMD_WAIT: Duration = Duration::from_millis(100);
/// macOS extrapolates the elapsed time from the last published value, so a
/// new one is pushed only when the real position drifts this far from that
/// extrapolation (a seek or a stall), plus on status changes.
const PROGRESS_DRIFT_US: i64 = 1_500_000;
/// Step for a bare `Seek(direction)` with no amount.
const BARE_SEEK_US: i64 = 5_000_000;

/// Run the macOS media-control service as an Iced subscription.
pub(crate) fn run() -> impl Sipper<Never, MprisEvent> {
    sipper(async |mut output| {
        let (event_tx, mut event_rx) = tokio_mpsc::channel::<MprisEvent>(EVENT_CHANNEL_DEPTH);
        let (cmd_tx, cmd_rx) = std_mpsc::channel::<MprisCommand>();

        let worker = std::thread::spawn(move || run_worker(event_tx, cmd_rx));

        output
            .send(MprisEvent::Connected(MprisConnection::from_sender(cmd_tx)))
            .await;

        loop {
            match tokio::time::timeout(EVENT_POLL_TIMEOUT, event_rx.recv()).await {
                Ok(Some(event)) => output.send(event).await,
                Ok(None) => {
                    debug!(" Now Playing event channel closed, subscription ending");
                    if worker.join().is_err() {
                        warn!(" Now Playing thread panicked");
                    }
                    break;
                }
                Err(_timeout) => {}
            }
        }

        std::future::pending::<Never>().await
    })
}

/// What the worker last told the system, to extrapolate against.
struct Published {
    status: PlaybackStatus,
    position_us: i64,
    /// When `position_us` was pushed, to extrapolate while playing.
    pushed_at: Instant,
    /// The `mpris:trackid` of the last metadata, which a Control Center
    /// scrubber position is judged against (macOS sends no track id).
    track_id: String,
    /// The last metadata sent (id, title, artist, album, duration, art), to
    /// drop the tick's every-tick resends. A radio's ICY title changes under
    /// one station id, so the id alone is not enough.
    metadata_key: (String, String, String, String, i64, Option<String>),
}

fn run_worker(event_tx: tokio_mpsc::Sender<MprisEvent>, cmd_rx: std_mpsc::Receiver<MprisCommand>) {
    let mut controls = match MediaControls::new(PlatformConfig {
        display_name: "Nokkvi",
        dbus_name: "nokkvi",
        hwnd: None,
    }) {
        Ok(c) => c,
        Err(e) => {
            warn!(" Failed to create macOS media controls: {e}");
            return;
        }
    };

    let handler_tx = event_tx.clone();
    // Filled on the first SetMetadata; the handler reads it for SetPosition.
    let track_id = std::sync::Arc::new(parking_lot::Mutex::new(String::new()));
    let handler_track = std::sync::Arc::clone(&track_id);
    if let Err(e) = controls.attach(move |event| {
        if let Some(event) = translate(event, &handler_track.lock())
            && let Err(e) = handler_tx.try_send(event)
        {
            warn!(" Now Playing: dropped a media command: {e}");
        }
    }) {
        warn!(" Failed to attach macOS media-key handler: {e}");
        return;
    }
    debug!(" macOS Now Playing / media keys attached");

    let mut state = Published {
        status: PlaybackStatus::Stopped,
        position_us: 0,
        pushed_at: Instant::now(),
        track_id: String::new(),
        metadata_key: Default::default(),
    };

    loop {
        match cmd_rx.recv_timeout(CMD_WAIT) {
            Ok(command) => apply(&mut controls, &mut state, &track_id, command),
            Err(std_mpsc::RecvTimeoutError::Timeout) => {
                if event_tx.is_closed() {
                    debug!(" Now Playing subscription gone, ending worker");
                    break;
                }
            }
            Err(std_mpsc::RecvTimeoutError::Disconnected) => {
                debug!(" Now Playing command channel disconnected");
                break;
            }
        }
    }
}

/// Map a system media command onto an app event. `track_id` is the published
/// track, so a scrubber drag lands through `PublishedTrack::set_position_target`.
fn translate(event: MediaControlEvent, track_id: &str) -> Option<MprisEvent> {
    let micros = |d: Duration| i64::try_from(d.as_micros()).unwrap_or(i64::MAX);
    match event {
        MediaControlEvent::Play => Some(MprisEvent::Play),
        MediaControlEvent::Pause => Some(MprisEvent::Pause),
        MediaControlEvent::Toggle => Some(MprisEvent::PlayPause),
        MediaControlEvent::Next => Some(MprisEvent::Next),
        MediaControlEvent::Previous => Some(MprisEvent::Previous),
        MediaControlEvent::Stop => Some(MprisEvent::Stop),
        MediaControlEvent::Raise => Some(MprisEvent::Raise),
        MediaControlEvent::SetPosition(MediaPosition(d)) => Some(MprisEvent::SetPosition {
            track_id: track_id.to_string(),
            position_us: micros(d),
        }),
        MediaControlEvent::SeekBy(direction, d) => Some(MprisEvent::Seek(match direction {
            SeekDirection::Forward => micros(d),
            SeekDirection::Backward => -micros(d),
        })),
        MediaControlEvent::Seek(direction) => Some(MprisEvent::Seek(match direction {
            SeekDirection::Forward => BARE_SEEK_US,
            SeekDirection::Backward => -BARE_SEEK_US,
        })),
        MediaControlEvent::SetShuffle(on) => Some(MprisEvent::SetShuffle(on)),
        // Volume is the system's on macOS and Quit/OpenUri have no handler;
        // `MediaControlEvent` is non-exhaustive.
        _ => None,
    }
}

fn playback_for(state: &Published) -> MediaPlayback {
    let progress = Some(MediaPosition(Duration::from_micros(
        u64::try_from(state.position_us).unwrap_or(0),
    )));
    match state.status {
        PlaybackStatus::Playing => MediaPlayback::Playing { progress },
        PlaybackStatus::Paused => MediaPlayback::Paused { progress },
        PlaybackStatus::Stopped => MediaPlayback::Stopped,
    }
}

fn push_playback(controls: &mut MediaControls, state: &mut Published) {
    state.pushed_at = Instant::now();
    if let Err(e) = controls.set_playback(playback_for(state)) {
        debug!(" Now Playing playback update failed: {e}");
    }
}

fn apply(
    controls: &mut MediaControls,
    state: &mut Published,
    shared_track_id: &parking_lot::Mutex<String>,
    command: MprisCommand,
) {
    match command {
        MprisCommand::SetPlaybackStatus(status) => {
            if status != state.status {
                state.status = status;
                push_playback(controls, state);
            }
        }
        MprisCommand::SetMetadata {
            track_id,
            title,
            artist,
            album,
            duration_us,
            art_url,
        } => {
            // The tick resends metadata every tick; skip an unchanged one so
            // artwork is not reloaded at 10 Hz.
            let key = (
                track_id.clone(),
                title.clone(),
                artist.clone(),
                album.clone(),
                duration_us,
                art_url.clone(),
            );
            *shared_track_id.lock() = track_id.clone();
            if state.metadata_key == key {
                trace!(" Now Playing: metadata unchanged");
                return;
            }
            state.metadata_key = key;
            state.track_id = track_id;
            let duration = u64::try_from(duration_us)
                .ok()
                .filter(|d| *d > 0)
                .map(Duration::from_micros);
            let metadata = MediaMetadata {
                title: Some(&title),
                artist: Some(&artist),
                album: Some(&album),
                cover_url: art_url.as_deref(),
                duration,
            };
            if let Err(e) = controls.set_metadata(metadata) {
                debug!(" Now Playing metadata update failed: {e}");
            }
            // souvlaki rebuilds the info dictionary, dropping elapsed time
            // and state: republish them.
            push_playback(controls, state);
        }
        MprisCommand::SetPosition(position_us) => {
            let expected = if state.status == PlaybackStatus::Playing {
                state.position_us
                    + i64::try_from(state.pushed_at.elapsed().as_micros()).unwrap_or(i64::MAX)
            } else {
                state.position_us
            };
            let drifted = (position_us - expected).abs() > PROGRESS_DRIFT_US;
            if drifted && state.status != PlaybackStatus::Stopped {
                state.position_us = position_us;
                push_playback(controls, state);
            } else if state.status != PlaybackStatus::Playing {
                // Keep the base fresh while paused so a resume starts here.
                state.position_us = position_us;
            }
        }
        MprisCommand::Seeked(position_us) => {
            state.position_us = position_us;
            if state.status != PlaybackStatus::Stopped {
                push_playback(controls, state);
            }
        }
        MprisCommand::SetShuffle(shuffle) => {
            if let Err(e) = controls.set_shuffle(Some(shuffle)) {
                debug!(" Now Playing shuffle update failed: {e}");
            }
        }
        // The system owns volume on macOS; there is no loop-mode command
        // published, and the scrubber cannot be disabled (the app guards
        // SetPosition/Seek itself through `PublishedTrack`).
        MprisCommand::SetVolume(_)
        | MprisCommand::SetLoopStatus(_)
        | MprisCommand::SetCanSeek(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scrubber_position_carries_the_published_track_id() {
        let event = translate(
            MediaControlEvent::SetPosition(MediaPosition(Duration::from_secs(90))),
            "/org/nokkvi/track/abc",
        );
        assert!(matches!(
            event,
            Some(MprisEvent::SetPosition { ref track_id, position_us })
                if track_id == "/org/nokkvi/track/abc" && position_us == 90_000_000
        ));
    }

    #[test]
    fn seek_by_backward_is_negative() {
        let event = translate(
            MediaControlEvent::SeekBy(SeekDirection::Backward, Duration::from_secs(10)),
            "",
        );
        assert!(matches!(event, Some(MprisEvent::Seek(-10_000_000))));
    }

    #[test]
    fn media_keys_map_to_app_events() {
        assert!(matches!(
            translate(MediaControlEvent::Toggle, ""),
            Some(MprisEvent::PlayPause)
        ));
        assert!(matches!(
            translate(MediaControlEvent::Next, ""),
            Some(MprisEvent::Next)
        ));
        assert!(translate(MediaControlEvent::SetVolume(0.5), "").is_none());
    }
}
