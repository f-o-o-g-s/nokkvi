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
//!    the command channel is closed, the thread breaks out of its loop, and
//!    the tray icon is dropped.

use std::sync::mpsc as std_mpsc;

use iced::task::{Never, Sipper, sipper};
use ksni::{
    Category, Icon, Status, Tray, TrayMethods,
    menu::{MenuItem, StandardItem},
};
use tokio::sync::mpsc as tokio_mpsc;
use tracing::{debug, error, warn};

const TRAY_ICON_PNG: &[u8] = include_bytes!("../../assets/org.nokkvi.nokkvi.png");

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
    let img = match image::load_from_memory(TRAY_ICON_PNG) {
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
    for px in data.chunks_exact_mut(4) {
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
        let (event_tx, mut event_rx) = tokio_mpsc::channel::<TrayEvent>(32);
        let (cmd_tx, cmd_rx) = std_mpsc::channel::<TrayCommand>();

        let tray_thread = std::thread::spawn(move || {
            run_tray_thread(event_tx, cmd_rx);
        });

        let connection = TrayConnection { sender: cmd_tx };
        output.send(TrayEvent::Connected(connection)).await;

        loop {
            match tokio::time::timeout(tokio::time::Duration::from_millis(100), event_rx.recv())
                .await
            {
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
/// `Handle`, processes app-side commands, and tears down on channel close.
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

        loop {
            match cmd_rx.try_recv() {
                Ok(TrayCommand::SetPlayingState { is_playing, title }) => {
                    handle
                        .update(|t: &mut NokkviTray| {
                            t.is_playing = is_playing;
                            t.title = title;
                        })
                        .await;
                }
                Err(std_mpsc::TryRecvError::Empty) => {
                    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
                }
                Err(std_mpsc::TryRecvError::Disconnected) => {
                    debug!(" Tray command channel disconnected; shutting down tray");
                    handle.shutdown().await;
                    break;
                }
            }
        }
    });
}
