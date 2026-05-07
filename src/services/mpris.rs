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

use std::sync::mpsc as std_mpsc;

use iced::task::{Never, Sipper, sipper};
use mpris_server::{LoopStatus, Metadata, PlaybackStatus, Player, Time, Volume};
use tokio::sync::mpsc as tokio_mpsc;
use tracing::{debug, error, warn};

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
    /// Next method called
    Next,
    /// Previous method called
    Previous,
    /// Seek method called (offset in microseconds, can be negative)
    Seek(i64),
    /// SetPosition method called (absolute position in microseconds)
    SetPosition(i64),
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
    /// Update metadata (title, artist, album, duration_us, art_url)
    SetMetadata {
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
    /// Update playback status (Playing/Paused/Stopped)
    pub fn set_playback_status(&self, status: PlaybackStatus) {
        let _ = self.sender.send(MprisCommand::SetPlaybackStatus(status));
    }

    /// Update current track metadata
    pub fn set_metadata(
        &self,
        title: &str,
        artist: &str,
        album: &str,
        duration_us: i64,
        art_url: Option<&str>,
    ) {
        let _ = self.sender.send(MprisCommand::SetMetadata {
            title: title.to_string(),
            artist: artist.to_string(),
            album: album.to_string(),
            duration_us,
            art_url: art_url.map(|s| s.to_string()),
        });
    }

    /// Update position (microseconds) — keeps internal state fresh for D-Bus polling
    pub fn set_position(&self, position_us: i64) {
        let _ = self.sender.send(MprisCommand::SetPosition(position_us));
    }

    /// Emit Seeked signal (position in microseconds)
    pub fn seeked(&self, position_us: i64) {
        let _ = self.sender.send(MprisCommand::Seeked(position_us));
    }

    /// Update volume (0.0–1.0)
    pub fn set_volume(&self, volume: f64) {
        let _ = self.sender.send(MprisCommand::SetVolume(volume));
    }

    /// Update loop status
    pub fn set_loop_status(&self, status: LoopStatus) {
        let _ = self.sender.send(MprisCommand::SetLoopStatus(status));
    }

    /// Update shuffle status
    pub fn set_shuffle(&self, shuffle: bool) {
        let _ = self.sender.send(MprisCommand::SetShuffle(shuffle));
    }
}

/// Run the MPRIS server as an Iced subscription.
///
/// Spawns a dedicated thread with its own tokio runtime for the MPRIS server
/// (since `Player` is not Send), and relays events via channels.
pub(crate) fn run() -> impl Sipper<Never, MprisEvent> {
    sipper(async |mut output| {
        // Channels for communication with MPRIS thread
        let (event_tx, mut event_rx) = tokio_mpsc::channel::<MprisEvent>(100);
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
            match tokio::time::timeout(tokio::time::Duration::from_millis(100), event_rx.recv())
                .await
            {
                Ok(Some(event)) => {
                    output.send(event).await;
                }
                Ok(None) => {
                    // Channel closed, MPRIS thread died - cleanup and exit loop
                    debug!(" MPRIS event channel closed, subscription ending");
                    let _ = mpris_thread.join();
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
            .can_play(true)
            .can_pause(true)
            .can_go_next(true)
            .can_go_previous(true)
            .can_seek(true)
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
                let _ = tx.try_send(MprisEvent::PlayPause);
            });

            // Play
            let tx = event_tx.clone();
            player.connect_play(move |_| {
                let _ = tx.try_send(MprisEvent::Play);
            });

            // Pause
            let tx = event_tx.clone();
            player.connect_pause(move |_| {
                let _ = tx.try_send(MprisEvent::Pause);
            });

            // Stop
            let tx = event_tx.clone();
            player.connect_stop(move |_| {
                let _ = tx.try_send(MprisEvent::Stop);
            });

            // Next
            let tx = event_tx.clone();
            player.connect_next(move |_| {
                let _ = tx.try_send(MprisEvent::Next);
            });

            // Previous
            let tx = event_tx.clone();
            player.connect_previous(move |_| {
                let _ = tx.try_send(MprisEvent::Previous);
            });

            // Seek (offset)
            let tx = event_tx.clone();
            player.connect_seek(move |_, offset| {
                let _ = tx.try_send(MprisEvent::Seek(offset.as_micros()));
            });

            // SetPosition
            let tx = event_tx.clone();
            player.connect_set_position(move |_, _track_id, position| {
                let _ = tx.try_send(MprisEvent::SetPosition(position.as_micros()));
            });

            // Volume setter
            let tx = event_tx.clone();
            player.connect_set_volume(move |_, volume| {
                let _ = tx.try_send(MprisEvent::SetVolume(volume));
            });

            // LoopStatus setter
            let tx = event_tx.clone();
            player.connect_set_loop_status(move |_, status| {
                let _ = tx.try_send(MprisEvent::SetLoopStatus(status));
            });

            // Shuffle setter
            let tx = event_tx.clone();
            player.connect_set_shuffle(move |_, shuffle| {
                let _ = tx.try_send(MprisEvent::SetShuffle(shuffle));
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
                    let _ = player.set_playback_status(status).await;
                }
                Ok(MprisCommand::SetMetadata {
                    title,
                    artist,
                    album,
                    duration_us,
                    art_url,
                }) => {
                    let mut builder = Metadata::builder()
                        .title(title)
                        .artist([artist])
                        .album(album)
                        .length(Time::from_micros(duration_us));

                    if let Some(url) = art_url {
                        builder = builder.art_url(url);
                    }

                    let _ = player.set_metadata(builder.build()).await;
                }
                Ok(MprisCommand::SetPosition(position_us)) => {
                    player.set_position(Time::from_micros(position_us));
                }
                Ok(MprisCommand::Seeked(position_us)) => {
                    let _ = player.seeked(Time::from_micros(position_us)).await;
                }
                Ok(MprisCommand::SetVolume(volume)) => {
                    let _ = player.set_volume(Volume::from(volume)).await;
                }
                Ok(MprisCommand::SetLoopStatus(status)) => {
                    let _ = player.set_loop_status(status).await;
                }
                Ok(MprisCommand::SetShuffle(shuffle)) => {
                    let _ = player.set_shuffle(shuffle).await;
                }
                Err(std_mpsc::TryRecvError::Empty) => {
                    // No commands, yield for a bit
                    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
                }
                Err(std_mpsc::TryRecvError::Disconnected) => {
                    warn!(" MPRIS command channel disconnected");
                    break;
                }
            }
        }
    });
}
