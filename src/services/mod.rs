//! Frontend services — collage artwork assembly and MPRIS D-Bus integration

pub(crate) mod collage_artwork;
pub(crate) mod ipc;
pub(crate) mod loop_subscription;
pub(crate) mod mpris;
pub(crate) mod mpris_art_writer;
pub(crate) mod navidrome_sse;
pub(crate) mod notifications;
pub(crate) mod queue_changed_subscription;
pub(crate) mod subscription_slot;
pub(crate) mod task_subscription;
pub(crate) mod tray;

/// The app icon (512x512 RGBA PNG), embedded once. The tray renders it, and
/// MPRIS publishes it as the placeholder art for an item with no art of its own.
pub(crate) const APP_ICON_PNG: &[u8] = include_bytes!("../../assets/org.nokkvi.nokkvi.png");
