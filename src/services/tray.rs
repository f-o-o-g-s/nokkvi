//! StatusNotifierItem (system tray) integration.
//!
//! Uses `ksni`, a pure-Rust SNI implementation over `zbus`. Mirrors the MPRIS
//! service pattern: a dedicated thread owns its own tokio runtime + the ksni
//! `Handle`, while the iced subscription bridges callbacks into `Message::Tray`.
//!
//! The compositor must host an SNI tray for the icon to render. On Hyprland
//! that means `waybar` with the `tray` module; KDE Plasma works out of the
//! box; GNOME needs the AppIndicator extension.
//!
//! ## Lifecycle
//!
//! 1. `Subscription::run(tray::run)` on the iced side calls into `run()`.
//! 2. `run()` spawns a dedicated `std::thread` and emits
//!    `TrayEvent::Connected(TrayConnection)` so the app can store the handle
//!    for state pushes.
//! 3. ksni callbacks (left-click, menu items) translate into `TrayEvent`s sent
//!    over the event channel back to iced.
//! 4. When the subscription is dropped (e.g. user disabled the tray toggle),
//!    its event receiver goes with it. The thread sees the event channel
//!    closed, breaks out of its loop and shuts the ksni service down, which
//!    removes the icon. It must not wait for the command channel instead:
//!    the app keeps its `TrayConnection` after the toggle goes off.

use std::{sync::mpsc as std_mpsc, time::Duration};

use iced::task::{Never, Sipper, sipper};
use ksni::{
    Category, Icon, Status, Tray, TrayMethods,
    menu::{MenuItem, StandardItem},
};
use tokio::sync::mpsc as tokio_mpsc;
use tracing::{debug, error, warn};

/// Tray event channel is shallow — menu activations are user-paced.
const TRAY_EVENT_CHANNEL_DEPTH: usize = 32;

/// Event-poll fallback timeout so the cmd loop wakes even when no tray
/// events are ready.
const TRAY_EVENT_POLL_TIMEOUT: Duration = Duration::from_millis(100);

/// Tray cmd loop can idle longer — only set_playing_state is sent, and
/// only on track changes.
const TRAY_CMD_IDLE_SLEEP: Duration = Duration::from_millis(50);

/// Events emitted by tray menu activations / icon clicks.
#[derive(Debug, Clone)]
pub enum TrayEvent {
    /// Tray service is up; here is the handle for state pushes.
    Connected(TrayConnection),
    /// Left-click on the tray icon (or "Show / Hide" menu item) — toggle
    /// window visibility.
    Activate,
    /// Play/Pause menu item.
    PlayPause,
    /// Next menu item.
    Next,
    /// Previous menu item.
    Previous,
    /// Quit menu item.
    Quit,
}

/// Commands the app can send to mutate tray state.
#[derive(Debug, Clone)]
pub(crate) enum TrayCommand {
    /// Update the Play/Pause label and the tooltip title.
    SetPlayingState { is_playing: bool, title: String },
}

/// Handle for pushing state updates to the tray. Cheap to clone.
#[derive(Debug, Clone)]
pub struct TrayConnection {
    sender: std_mpsc::Sender<TrayCommand>,
}

impl TrayConnection {
    pub fn set_playing_state(&self, is_playing: bool, title: impl Into<String>) {
        let _ = self.sender.send(TrayCommand::SetPlayingState {
            is_playing,
            title: title.into(),
        });
    }
}

/// Internal tray state owned on the ksni thread.
struct NokkviTray {
    event_tx: tokio_mpsc::Sender<TrayEvent>,
    is_playing: bool,
    title: String,
}

impl NokkviTray {
    fn emit(&self, event: TrayEvent) {
        let _ = self.event_tx.try_send(event);
    }
}

impl Tray for NokkviTray {
    fn id(&self) -> String {
        "org.nokkvi.nokkvi".to_string()
    }

    fn title(&self) -> String {
        "Nokkvi".to_string()
    }

    fn icon_name(&self) -> String {
        // Falls back to the freedesktop icon if installed; the embedded
        // pixmap below is what most hosts will actually display.
        "org.nokkvi.nokkvi".to_string()
    }

    fn icon_pixmap(&self) -> Vec<Icon> {
        load_icon_pixmap().map(|i| vec![i]).unwrap_or_default()
    }

    fn category(&self) -> Category {
        Category::ApplicationStatus
    }

    fn status(&self) -> Status {
        Status::Active
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        self.emit(TrayEvent::Activate);
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        vec![
            StandardItem {
                label: "Show / Hide".to_string(),
                activate: Box::new(|t: &mut Self| t.emit(TrayEvent::Activate)),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: if self.is_playing {
                    "Pause".to_string()
                } else {
                    "Play".to_string()
                },
                activate: Box::new(|t: &mut Self| t.emit(TrayEvent::PlayPause)),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Next".to_string(),
                activate: Box::new(|t: &mut Self| t.emit(TrayEvent::Next)),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Previous".to_string(),
                activate: Box::new(|t: &mut Self| t.emit(TrayEvent::Previous)),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Quit".to_string(),
                activate: Box::new(|t: &mut Self| t.emit(TrayEvent::Quit)),
                ..Default::default()
            }
            .into(),
        ]
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        ksni::ToolTip {
            title: if self.title.is_empty() {
                "Nokkvi".to_string()
            } else {
                self.title.clone()
            },
            description: String::new(),
            icon_name: String::new(),
            icon_pixmap: Vec::new(),
        }
    }
}

/// Decode the embedded PNG into ksni's ARGB32 (network byte order) layout.
fn load_icon_pixmap() -> Option<Icon> {
    let img = match image::load_from_memory(super::APP_ICON_PNG) {
        Ok(img) => img,
        Err(e) => {
            warn!(" Tray icon decode failed: {e}");
            return None;
        }
    };

    let rgba = img.to_rgba8();
    let (width, height) = rgba.dimensions();
    let mut data = rgba.into_raw();
    // RGBA → ARGB (network byte order = big-endian = A,R,G,B in memory)
    for px in data.as_chunks_mut::<4>().0 {
        px.rotate_right(1);
    }
    Some(Icon {
        width: width as i32,
        height: height as i32,
        data,
    })
}

/// Iced subscription entrypoint — spawns the tray thread and bridges events.
pub(crate) fn run() -> impl Sipper<Never, TrayEvent> {
    sipper(async |mut output| {
        let (event_tx, mut event_rx) = tokio_mpsc::channel::<TrayEvent>(TRAY_EVENT_CHANNEL_DEPTH);
        let (cmd_tx, cmd_rx) = std_mpsc::channel::<TrayCommand>();

        let tray_thread = std::thread::spawn(move || {
            run_tray_thread(event_tx, cmd_rx);
        });

        let connection = TrayConnection { sender: cmd_tx };
        output.send(TrayEvent::Connected(connection)).await;

        loop {
            match tokio::time::timeout(TRAY_EVENT_POLL_TIMEOUT, event_rx.recv()).await {
                Ok(Some(event)) => output.send(event).await,
                Ok(None) => {
                    debug!(" Tray event channel closed; subscription ending");
                    let _ = tray_thread.join();
                    break;
                }
                Err(_timeout) => {}
            }
        }

        std::future::pending::<Never>().await
    })
}

/// Dedicated tray thread: owns a current-thread tokio runtime + the ksni
/// `Handle`, processes app-side commands, and tears down once
/// [`pump_commands`] returns.
fn run_tray_thread(
    event_tx: tokio_mpsc::Sender<TrayEvent>,
    cmd_rx: std_mpsc::Receiver<TrayCommand>,
) {
    let rt = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            error!(" Failed to create tray tokio runtime: {e}");
            return;
        }
    };

    rt.block_on(async move {
        let events = event_tx.clone();
        let tray = NokkviTray {
            event_tx,
            is_playing: false,
            title: String::new(),
        };

        let handle = match tray.spawn().await {
            Ok(h) => h,
            Err(e) => {
                warn!(
                    " Tray service failed to register on session bus ({e}); \
                     compositor may lack a StatusNotifierItem host"
                );
                return;
            }
        };

        debug!(" Tray service started: org.nokkvi.nokkvi");

        let exit = pump_commands(&cmd_rx, &events, async |cmd| match cmd {
            TrayCommand::SetPlayingState { is_playing, title } => {
                handle
                    .update(|t: &mut NokkviTray| {
                        t.is_playing = is_playing;
                        t.title = title;
                    })
                    .await;
            }
        })
        .await;
        debug!(" Tray shutting down ({exit:?})");
        handle.shutdown().await;
    });
}

/// Why [`pump_commands`] stopped.
#[derive(Debug, PartialEq, Eq)]
enum LoopExit {
    /// Every `TrayConnection` was dropped.
    CommandsClosed,
    /// The iced subscription that owns the event receiver was cancelled.
    SubscriptionGone,
}

/// Feed app-side commands to `apply` until the tray should shut down: the
/// subscription is gone (`events` reports its receiver dropped), or every
/// `TrayConnection` is.
///
/// The subscription check is the one that fires in practice. `Nokkvi`
/// keeps its `TrayConnection` after Show Tray Icon goes off, so waiting for
/// the command channel alone left the thread, and the icon, running with
/// nothing to deliver its clicks to.
async fn pump_commands(
    cmd_rx: &std_mpsc::Receiver<TrayCommand>,
    events: &tokio_mpsc::Sender<TrayEvent>,
    mut apply: impl AsyncFnMut(TrayCommand),
) -> LoopExit {
    loop {
        if events.is_closed() {
            return LoopExit::SubscriptionGone;
        }
        match cmd_rx.try_recv() {
            Ok(cmd) => apply(cmd).await,
            Err(std_mpsc::TryRecvError::Empty) => {
                tokio::time::sleep(TRAY_CMD_IDLE_SLEEP).await;
            }
            Err(std_mpsc::TryRecvError::Disconnected) => return LoopExit::CommandsClosed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// HIGH RISK byte-identity pin: tray-menu cadence is visible to
    /// `ksni` system-tray hosts. If a future change to any of these
    /// three values is intentional, update this test and note the
    /// user-visible behavior change in the commit body.
    #[test]
    fn tray_timing_constants_byte_identity() {
        assert_eq!(TRAY_EVENT_CHANNEL_DEPTH, 32);
        assert_eq!(TRAY_EVENT_POLL_TIMEOUT, Duration::from_millis(100));
        assert_eq!(TRAY_CMD_IDLE_SLEEP, Duration::from_millis(50));
    }

    /// Upper bound for a loop that should end on its own; far above the
    /// 50 ms idle sleep, so a pass is never a timing fluke.
    const LOOP_DEADLINE: Duration = Duration::from_secs(2);

    #[tokio::test]
    async fn the_command_loop_ends_when_the_subscription_is_gone() {
        // Show Tray Icon off: iced cancels the subscription, which drops the
        // event receiver, but the app still holds its `TrayConnection`. The
        // loop must end anyway, or the icon outlives the setting.
        let (event_tx, event_rx) = tokio_mpsc::channel::<TrayEvent>(1);
        let (cmd_tx, cmd_rx) = std_mpsc::channel();
        let _still_held_by_the_app = TrayConnection { sender: cmd_tx };
        drop(event_rx);

        let exit = tokio::time::timeout(
            LOOP_DEADLINE,
            pump_commands(&cmd_rx, &event_tx, async |_| {}),
        )
        .await
        .expect("the tray must shut down once its subscription is gone");
        assert_eq!(exit, LoopExit::SubscriptionGone);
    }

    #[tokio::test]
    async fn commands_are_applied_in_order_until_the_app_drops_its_handle() {
        let (event_tx, _event_rx) = tokio_mpsc::channel::<TrayEvent>(1);
        let (cmd_tx, cmd_rx) = std_mpsc::channel();
        let connection = TrayConnection { sender: cmd_tx };
        connection.set_playing_state(true, "A");
        connection.set_playing_state(false, "B");
        drop(connection);

        let mut seen = Vec::new();
        let exit = tokio::time::timeout(
            LOOP_DEADLINE,
            pump_commands(&cmd_rx, &event_tx, async |cmd| match cmd {
                TrayCommand::SetPlayingState { is_playing, title } => {
                    seen.push((is_playing, title));
                }
            }),
        )
        .await
        .expect("the loop must end once the app drops its handle");
        assert_eq!(exit, LoopExit::CommandsClosed);
        assert_eq!(
            seen,
            vec![(true, "A".to_string()), (false, "B".to_string())]
        );
    }
}
