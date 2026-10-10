//! MPRIS D-Bus service for Linux desktop media player integration.
//!
//! Uses `mpris-server` with the ready-made `Player` API to expose the app
//! as `org.mpris.MediaPlayer2.nokkvi.instance<pid>` on the session bus.
//! The per-pid suffix follows the MPRIS spec for multi-instance apps and
//! avoids silent name-claim contention when multiple nokkvi binaries run.
//!
//! Since `Player` uses `LocalServer` (not Send), the MPRIS server runs on a
//! dedicated thread with its own tokio runtime. Communication happens via
//! thread-safe channels.

use std::{sync::mpsc as std_mpsc, time::Duration};

use iced::task::{Never, Sipper, sipper};
use mpris_server::{LoopStatus, Metadata, PlaybackStatus, Player, Time, TrackId, Volume};
use tokio::sync::mpsc as tokio_mpsc;
use tracing::{debug, error, trace, warn};

/// MPRIS gets a deeper event channel because zbus method calls can burst
/// (Seek + SetPosition + SetVolume from a single playerctl invocation).
const MPRIS_EVENT_CHANNEL_DEPTH: usize = 100;

/// Event-poll fallback timeout so the cmd loop wakes even when no MPRIS
/// events are ready.
const MPRIS_EVENT_POLL_TIMEOUT: Duration = Duration::from_millis(100);

/// MPRIS cmd loop polls quickly so set_metadata / Seeked land within
/// one tick.
const MPRIS_CMD_IDLE_SLEEP: Duration = Duration::from_millis(10);

/// Events sent from MPRIS callbacks to the Iced app.
#[derive(Debug, Clone)]
pub enum MprisEvent {
    /// MPRIS server connected and ready
    Connected(MprisConnection),
    /// PlayPause method called
    PlayPause,
    /// Play method called
    Play,
    /// Pause method called
    Pause,
    /// Stop method called
    Stop,
    /// Raise method called (root `org.mpris.MediaPlayer2` interface)
    Raise,
    /// Next method called
    Next,
    /// Previous method called
    Previous,
    /// Seek method called (offset in microseconds, can be negative)
    Seek(i64),
    /// SetPosition method called: the `mpris:trackid` the client aims at
    /// and the absolute position in microseconds. The app ignores it unless
    /// [`PublishedTrack::set_position_target`] accepts it.
    SetPosition { track_id: String, position_us: i64 },
    /// Volume property set (0.0–1.0)
    SetVolume(f64),
    /// LoopStatus property set
    SetLoopStatus(LoopStatus),
    /// Shuffle property set
    SetShuffle(bool),
}

/// Commands sent from the app to the MPRIS task to update state.
#[derive(Debug, Clone)]
pub(crate) enum MprisCommand {
    /// Update playback status (Playing/Paused/Stopped)
    SetPlaybackStatus(PlaybackStatus),
    /// Update metadata (track id, title, artist, album, duration_us, art_url)
    SetMetadata {
        /// A D-Bus object path from [`track_id_path`].
        track_id: String,
        title: String,
        artist: String,
        album: String,
        duration_us: i64,
        art_url: Option<String>,
    },
    /// Update position (microseconds) — sets internal state for D-Bus polling
    SetPosition(i64),
    /// Emit a Seeked signal (position in microseconds)
    Seeked(i64),
    /// Update volume (0.0–1.0)
    SetVolume(f64),
    /// Update loop status
    SetLoopStatus(LoopStatus),
    /// Update shuffle status
    SetShuffle(bool),
    /// Update the CanSeek property
    SetCanSeek(bool),
}

/// What a published `mpris:trackid` names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MprisItem<'a> {
    /// Nothing is loaded.
    Nothing,
    /// A queue song, by its server id.
    Song(&'a str),
    /// A radio station, by its server id.
    Station(&'a str),
}

/// The `mpris:trackid` object path for `item`: `/org/nokkvi/track/<id>` for a
/// song, `/org/nokkvi/station/<id>` for a station, the spec's `NoTrack` path
/// for nothing (or an empty id).
///
/// An object path element may hold only `[A-Za-z0-9_]`, so the id is escaped
/// injectively: ASCII letters and digits stay, every other byte (including
/// `_` itself) becomes `_` plus two lowercase hex digits. Two distinct ids
/// can never share a path, which `SetPosition`'s stale-id check relies on.
pub(crate) fn track_id_path(item: MprisItem<'_>) -> String {
    use std::fmt::Write as _;

    let (prefix, id) = match item {
        MprisItem::Song(id) if !id.is_empty() => ("/org/nokkvi/track/", id),
        MprisItem::Station(id) if !id.is_empty() => ("/org/nokkvi/station/", id),
        MprisItem::Nothing | MprisItem::Song(_) | MprisItem::Station(_) => {
            return TrackId::NO_TRACK.as_str().to_string();
        }
    };
    let mut path = String::with_capacity(prefix.len() + id.len());
    path.push_str(prefix);
    for byte in id.bytes() {
        if byte.is_ascii_alphanumeric() {
            path.push(char::from(byte));
        } else {
            // Writing to a String cannot fail.
            #[expect(
                clippy::let_underscore_must_use,
                reason = "fmt::Write for String is infallible"
            )]
            let _ = write!(path, "_{byte:02x}");
        }
    }
    path
}

/// What nokkvi last published for the current item: the state a
/// `SetPosition` call is judged against. Recorded by the tick's MPRIS push
/// whether or not a D-Bus connection exists, so tests can read it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PublishedTrack {
    /// The published `mpris:trackid` ([`track_id_path`]).
    pub track_id: String,
    /// The published `mpris:length`, in microseconds.
    pub length_us: i64,
    /// The published `CanSeek`.
    pub can_seek: bool,
}

impl Default for PublishedTrack {
    fn default() -> Self {
        Self {
            track_id: track_id_path(MprisItem::Nothing),
            length_us: 0,
            can_seek: false,
        }
    }
}

impl PublishedTrack {
    /// The record for one tick: `item`, its length, whether the engine has
    /// it loaded (playing or paused) and whether the engine can seek it.
    /// Seeking is possible only for a loaded, seekable queue song of known
    /// length: a station is a live stream, a song the server transcodes on
    /// the fly plays front to back, and a stopped queue (a restored one at
    /// login) names a song the engine has not loaded.
    pub(crate) fn for_item(
        item: MprisItem<'_>,
        length_us: i64,
        loaded: bool,
        seekable: bool,
    ) -> Self {
        Self {
            track_id: track_id_path(item),
            length_us,
            can_seek: loaded && seekable && matches!(item, MprisItem::Song(_)) && length_us > 0,
        }
    }

    /// Whether a relative `Seek` may run: the spec says it has no effect
    /// while CanSeek is false.
    pub(crate) fn accepts_seek(&self) -> bool {
        self.can_seek
    }

    /// The seek target, in seconds, for a `SetPosition(track_id, position)`
    /// call, or `None` when the MPRIS spec says to ignore it: the id is not
    /// the current track's (a stale request), seeking is not possible, or
    /// the position is negative or past the track's length.
    pub(crate) fn set_position_target(&self, track_id: &str, position_us: i64) -> Option<f32> {
        let accepted = self.can_seek
            && track_id == self.track_id
            && (0..=self.length_us).contains(&position_us);
        accepted.then(|| (position_us as f64 / 1_000_000.0) as f32)
    }
}

/// Handle to push state updates to the MPRIS server.
///
/// Clone this and use from anywhere to update MPRIS properties.
/// Commands are sent via thread-safe channel to the MPRIS thread.
#[derive(Debug, Clone)]
pub struct MprisConnection {
    sender: std_mpsc::Sender<MprisCommand>,
}

impl MprisConnection {
    /// A connection whose commands land on the returned receiver instead of
    /// an MPRIS thread, so tests can read what a push sent.
    #[cfg(test)]
    pub(crate) fn test_pair() -> (Self, std_mpsc::Receiver<MprisCommand>) {
        let (sender, receiver) = std_mpsc::channel();
        (Self { sender }, receiver)
    }

    /// Queue a command for the MPRIS thread. A closed channel means that
    /// thread has ended (no D-Bus session, or shutdown) and the app runs on
    /// without MPRIS, so the update is dropped with a trace line.
    fn send(&self, command: MprisCommand) {
        if self.sender.send(command).is_err() {
            trace!(" MPRIS thread gone; dropped a state update");
        }
    }

    /// Update playback status (Playing/Paused/Stopped)
    pub fn set_playback_status(&self, status: PlaybackStatus) {
        self.send(MprisCommand::SetPlaybackStatus(status));
    }

    /// Update current track metadata
    pub fn set_metadata(
        &self,
        track_id: &str,
        title: &str,
        artist: &str,
        album: &str,
        duration_us: i64,
        art_url: Option<&str>,
    ) {
        self.send(MprisCommand::SetMetadata {
            track_id: track_id.to_string(),
            title: title.to_string(),
            artist: artist.to_string(),
            album: album.to_string(),
            duration_us,
            art_url: art_url.map(|s| s.to_string()),
        });
    }

    /// Update position (microseconds) — keeps internal state fresh for D-Bus polling
    pub fn set_position(&self, position_us: i64) {
        self.send(MprisCommand::SetPosition(position_us));
    }

    /// Emit Seeked signal (position in microseconds)
    pub fn seeked(&self, position_us: i64) {
        self.send(MprisCommand::Seeked(position_us));
    }

    /// Update volume (0.0–1.0)
    pub fn set_volume(&self, volume: f64) {
        self.send(MprisCommand::SetVolume(volume));
    }

    /// Update loop status
    pub fn set_loop_status(&self, status: LoopStatus) {
        self.send(MprisCommand::SetLoopStatus(status));
    }

    /// Update shuffle status
    pub fn set_shuffle(&self, shuffle: bool) {
        self.send(MprisCommand::SetShuffle(shuffle));
    }

    /// Update the CanSeek property
    pub fn set_can_seek(&self, can_seek: bool) {
        self.send(MprisCommand::SetCanSeek(can_seek));
    }
}

/// Forward a D-Bus method call to the app. `try_send` fails when the app
/// is `MPRIS_EVENT_CHANNEL_DEPTH` events behind (the call is dropped) or
/// gone, so a failure is a lost media-key press and is logged.
fn forward(tx: &tokio_mpsc::Sender<MprisEvent>, event: MprisEvent) {
    if let Err(e) = tx.try_send(event) {
        warn!(" MPRIS: dropped a D-Bus command: {e}");
    }
}

/// Log a failed D-Bus property update or signal. The next update resends
/// the state, so a failure only leaves a client briefly stale.
fn log_failed<E: std::fmt::Display>(what: &str, result: Result<(), E>) {
    if let Err(e) = result {
        debug!(" MPRIS: {what} failed: {e}");
    }
}

/// Run the MPRIS server as an Iced subscription.
///
/// Spawns a dedicated thread with its own tokio runtime for the MPRIS server
/// (since `Player` is not Send), and relays events via channels.
pub(crate) fn run() -> impl Sipper<Never, MprisEvent> {
    sipper(async |mut output| {
        // Channels for communication with MPRIS thread
        let (event_tx, mut event_rx) = tokio_mpsc::channel::<MprisEvent>(MPRIS_EVENT_CHANNEL_DEPTH);
        let (cmd_tx, cmd_rx) = std_mpsc::channel::<MprisCommand>();

        // Spawn MPRIS server on dedicated thread
        let mpris_thread = std::thread::spawn(move || {
            run_mpris_thread(event_tx, cmd_rx);
        });

        // Create connection handle for app
        let connection = MprisConnection { sender: cmd_tx };
        output.send(MprisEvent::Connected(connection)).await;

        // Relay events from MPRIS thread to Iced
        // Use timeout-based polling to detect shutdown faster
        loop {
            // Use a short timeout so we can detect when the runtime is shutting down
            match tokio::time::timeout(MPRIS_EVENT_POLL_TIMEOUT, event_rx.recv()).await {
                Ok(Some(event)) => {
                    output.send(event).await;
                }
                Ok(None) => {
                    // Channel closed, MPRIS thread died - cleanup and exit loop
                    debug!(" MPRIS event channel closed, subscription ending");
                    if mpris_thread.join().is_err() {
                        warn!(" MPRIS thread panicked");
                    }
                    break; // Exit the polling loop
                }
                Err(_timeout) => {
                    // Timeout - just continue polling
                    // This allows the subscription to be cancelled during shutdown
                }
            }
        }

        // Suspend forever - Iced will cancel this when shutting down
        // This satisfies the Never return type while allowing clean shutdown
        std::future::pending::<Never>().await
    })
}

/// Run the MPRIS server on its dedicated thread with its own tokio runtime.
fn run_mpris_thread(
    event_tx: tokio_mpsc::Sender<MprisEvent>,
    cmd_rx: std_mpsc::Receiver<MprisCommand>,
) {
    // Create single-threaded runtime for this thread
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("Failed to create MPRIS tokio runtime");

    let local = tokio::task::LocalSet::new();

    local.block_on(&rt, async move {
        // Per the MPRIS spec, multi-instance apps must suffix the bus name with a
        // unique identifier so concurrent launches don't fight over the same
        // well-known name. zbus's default `RequestName` flags allow a contended
        // claim to silently land in the queue (`Ok(InQueue)`), which previously
        // left nokkvi running with no MPRIS visibility on the bus.
        let bus_suffix = format!("nokkvi.instance{}", std::process::id());
        let player = match Player::builder(&bus_suffix)
            .identity("Nokkvi")
            .desktop_entry("org.nokkvi.nokkvi")
            .can_raise(true)
            .can_play(true)
            .can_pause(true)
            .can_go_next(true)
            .can_go_previous(true)
            // Nothing is loaded yet; the first tick's push sets it.
            .can_seek(false)
            .can_control(true)
            .build()
            .await
        {
            Ok(p) => p,
            Err(e) => {
                error!(" Failed to create MPRIS player: {e}");
                return;
            }
        };

        debug!(" MPRIS server started: org.mpris.MediaPlayer2.{bus_suffix}");

        // Set up callbacks for MPRIS method calls
        {
            // PlayPause
            let tx = event_tx.clone();
            player.connect_play_pause(move |_| {
                forward(&tx, MprisEvent::PlayPause);
            });

            // Play
            let tx = event_tx.clone();
            player.connect_play(move |_| {
                forward(&tx, MprisEvent::Play);
            });

            // Pause
            let tx = event_tx.clone();
            player.connect_pause(move |_| {
                forward(&tx, MprisEvent::Pause);
            });

            // Stop
            let tx = event_tx.clone();
            player.connect_stop(move |_| {
                forward(&tx, MprisEvent::Stop);
            });

            // Raise
            let tx = event_tx.clone();
            player.connect_raise(move |_| {
                forward(&tx, MprisEvent::Raise);
            });

            // Next
            let tx = event_tx.clone();
            player.connect_next(move |_| {
                forward(&tx, MprisEvent::Next);
            });

            // Previous
            let tx = event_tx.clone();
            player.connect_previous(move |_| {
                forward(&tx, MprisEvent::Previous);
            });

            // Seek (offset)
            let tx = event_tx.clone();
            player.connect_seek(move |_, offset| {
                forward(&tx, MprisEvent::Seek(offset.as_micros()));
            });

            // SetPosition
            let tx = event_tx.clone();
            player.connect_set_position(move |_, track_id, position| {
                forward(
                    &tx,
                    MprisEvent::SetPosition {
                        track_id: track_id.as_str().to_string(),
                        position_us: position.as_micros(),
                    },
                );
            });

            // Volume setter
            let tx = event_tx.clone();
            player.connect_set_volume(move |_, volume| {
                forward(&tx, MprisEvent::SetVolume(volume));
            });

            // LoopStatus setter
            let tx = event_tx.clone();
            player.connect_set_loop_status(move |_, status| {
                forward(&tx, MprisEvent::SetLoopStatus(status));
            });

            // Shuffle setter
            let tx = event_tx.clone();
            player.connect_set_shuffle(move |_, shuffle| {
                forward(&tx, MprisEvent::SetShuffle(shuffle));
            });
        }

        // Spawn the player's D-Bus event loop
        // This is CRITICAL - without this, the player won't respond to D-Bus method calls!
        tokio::task::spawn_local(player.run());

        // Process commands from app in a loop
        loop {
            // Non-blocking check for commands
            match cmd_rx.try_recv() {
                Ok(MprisCommand::SetPlaybackStatus(status)) => {
                    log_failed(
                        "PlaybackStatus update",
                        player.set_playback_status(status).await,
                    );
                }
                Ok(MprisCommand::SetMetadata {
                    track_id,
                    title,
                    artist,
                    album,
                    duration_us,
                    art_url,
                }) => {
                    // `track_id_path` only builds valid object paths; the
                    // fallback is unreachable but keeps a bad one off D-Bus.
                    let track_id = TrackId::try_from(track_id).unwrap_or(TrackId::NO_TRACK);
                    let mut builder = Metadata::builder()
                        .trackid(track_id)
                        .title(title)
                        .artist([artist])
                        .album(album)
                        .length(Time::from_micros(duration_us));

                    if let Some(url) = art_url {
                        builder = builder.art_url(url);
                    }

                    log_failed(
                        "Metadata update",
                        player.set_metadata(builder.build()).await,
                    );
                }
                Ok(MprisCommand::SetPosition(position_us)) => {
                    player.set_position(Time::from_micros(position_us));
                }
                Ok(MprisCommand::Seeked(position_us)) => {
                    log_failed(
                        "Seeked signal",
                        player.seeked(Time::from_micros(position_us)).await,
                    );
                }
                Ok(MprisCommand::SetVolume(volume)) => {
                    log_failed(
                        "Volume update",
                        player.set_volume(Volume::from(volume)).await,
                    );
                }
                Ok(MprisCommand::SetLoopStatus(status)) => {
                    log_failed("LoopStatus update", player.set_loop_status(status).await);
                }
                Ok(MprisCommand::SetShuffle(shuffle)) => {
                    log_failed("Shuffle update", player.set_shuffle(shuffle).await);
                }
                Ok(MprisCommand::SetCanSeek(can_seek)) => {
                    log_failed("CanSeek update", player.set_can_seek(can_seek).await);
                }
                Err(std_mpsc::TryRecvError::Empty) => {
                    // No commands, yield for a bit
                    tokio::time::sleep(MPRIS_CMD_IDLE_SLEEP).await;
                }
                Err(std_mpsc::TryRecvError::Disconnected) => {
                    warn!(" MPRIS command channel disconnected");
                    break;
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// HIGH RISK byte-identity pin: MPRIS publishing latency is visible
    /// to `playerctl` and other zbus consumers. If a future change to
    /// any of these three values is intentional, update this test and
    /// note the user-visible behavior change in the commit body.
    #[test]
    fn mpris_timing_constants_byte_identity() {
        assert_eq!(MPRIS_EVENT_CHANNEL_DEPTH, 100);
        assert_eq!(MPRIS_EVENT_POLL_TIMEOUT, Duration::from_millis(100));
        assert_eq!(MPRIS_CMD_IDLE_SLEEP, Duration::from_millis(10));
    }
}
