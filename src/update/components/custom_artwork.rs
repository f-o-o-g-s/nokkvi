//! Shared plumbing for the "Set Custom Artwork…" / "Reset Artwork" flows
//! (playlists + radio stations): the native file picker with its local
//! read/size guards, the shared pick-then-upload front half, the flattening
//! of API results into [`CustomArtworkOutcome`], and the friendly-toast
//! mapping for flattened SERVER failure details.
//!
//! Failure origins are typed apart on purpose: only the upload/DELETE API
//! call may produce [`CustomArtworkOutcome::Failed`] (classified by the
//! Unauthorized / Forbidden / status-400 matchers); every local pick/read
//! problem lands in [`CustomArtworkOutcome::LocalFailed`] and is surfaced
//! verbatim — its detail embeds the user-picked path, which must never feed
//! the substring classifiers.

use std::future::Future;

use nokkvi_data::types::error::NokkviError;

use crate::app_message::CustomArtworkOutcome;

/// Extensions offered by the picker filter — the four formats Navidrome's
/// image-upload endpoints decode. Both cases are listed because rfd 0.17's
/// xdg-portal backend forwards these as literal, case-sensitive globs: a
/// lowercase-only filter makes `COVER.JPG` invisible in the dialog.
const IMAGE_EXTENSIONS: [&str; 10] = [
    "png", "PNG", "jpg", "JPG", "jpeg", "JPEG", "gif", "GIF", "webp", "WEBP",
];

/// Ceiling on the picked file's size, checked via metadata BEFORE reading it
/// into memory. Navidrome's `MaxImageUploadSize` defaults to 10 MB but is
/// server-configurable, so a tight client cap would wrongly block raised-cap
/// servers — 32 MiB sits comfortably above any sane cover while stopping an
/// accidental multi-hundred-MB pick from ballooning RAM and dying as an
/// opaque connection error instead of this friendly message. Server-side
/// rejects within the ceiling still map to the status-400 toast.
const MAX_IMAGE_FILE_BYTES: u64 = 32 * 1024 * 1024;

/// Result of the native pick + local read step, typed by ORIGIN.
pub(crate) enum PickOutcome {
    /// The user dismissed the dialog (or the portal produced no file).
    Cancelled,
    /// The picked file could not be used (unreadable / over the ceiling).
    /// The detail embeds the picked path — plain-toast material only.
    LocalError(String),
    /// Ready to upload.
    Picked { bytes: Vec<u8>, filename: String },
}

/// The oversize refusal line — pure so the wording is unit-testable.
fn oversize_message(filename: &str, len: u64) -> String {
    let mib = len as f64 / (1024.0 * 1024.0);
    let ceiling_mib = MAX_IMAGE_FILE_BYTES / (1024 * 1024);
    format!("{filename} is {mib:.1} MiB, over nokkvi's {ceiling_mib} MiB upload ceiling")
}

/// Open the native (xdg-portal) file picker filtered to uploadable image
/// formats, then size-check and read the picked file off the UI thread.
///
/// The portal serializes dialog presentation itself, so no client-side
/// "picker already open" guard is kept — a second request simply resolves
/// after the first, and each completion carries its own target entity.
pub(crate) async fn pick_image_file() -> PickOutcome {
    let Some(handle) = rfd::AsyncFileDialog::new()
        .set_title("Choose artwork image")
        .add_filter("Images", &IMAGE_EXTENSIONS)
        .pick_file()
        .await
    else {
        // rfd resolves `None` for a portal FAILURE as well as a user cancel,
        // and it reports the difference only through the `log` crate (which
        // nokkvi does not route). Keep the silent no-op UX — cancel is the
        // overwhelmingly common case — but leave one breadcrumb for the
        // portal-less setup where the menu item would otherwise look dead.
        tracing::info!(
            "artwork picker returned no file: user cancelled, or (if no dialog ever \
             appeared) a FileChooser portal implementation such as \
             xdg-desktop-portal-gtk may be missing"
        );
        return PickOutcome::Cancelled;
    };
    let path = handle.path().to_path_buf();
    let filename = path.file_name().map_or_else(
        || "artwork".to_string(),
        |n| n.to_string_lossy().into_owned(),
    );
    // Size guard BEFORE the read — see MAX_IMAGE_FILE_BYTES.
    match tokio::fs::metadata(&path).await {
        Ok(meta) if meta.len() > MAX_IMAGE_FILE_BYTES => {
            return PickOutcome::LocalError(oversize_message(&filename, meta.len()));
        }
        Ok(_) => {}
        Err(e) => {
            return PickOutcome::LocalError(format!("could not read {}: {e}", path.display()));
        }
    }
    match tokio::fs::read(&path).await {
        Ok(bytes) => PickOutcome::Picked { bytes, filename },
        Err(e) => PickOutcome::LocalError(format!("could not read {}: {e}", path.display())),
    }
}

/// The shared front half of every "Set Custom Artwork…" flow: pick + local
/// read, then hand `(bytes, filename)` to the caller's upload future. Only
/// the UPLOAD result flows into [`CustomArtworkOutcome::Failed`] (the
/// server-classified arm); pick/read problems land in `LocalFailed`.
pub(crate) async fn pick_and_upload<F, Fut>(upload: F) -> CustomArtworkOutcome
where
    F: FnOnce(Vec<u8>, String) -> Fut,
    Fut: Future<Output = anyhow::Result<()>>,
{
    match pick_image_file().await {
        PickOutcome::Cancelled => CustomArtworkOutcome::Cancelled,
        PickOutcome::LocalError(detail) => CustomArtworkOutcome::LocalFailed(detail),
        PickOutcome::Picked { bytes, filename } => {
            outcome_from_result(upload(bytes, filename).await)
        }
    }
}

/// Flatten an upload/delete API result into the Clone-able completion
/// outcome (`{e:#}` keeps the full context chain, including the typed
/// `Unauthorized` / `Forbidden` Display markers).
pub(crate) fn outcome_from_result(result: anyhow::Result<()>) -> CustomArtworkOutcome {
    match result {
        Ok(()) => CustomArtworkOutcome::Applied,
        Err(e) => CustomArtworkOutcome::Failed(format!("{e:#}")),
    }
}

/// Map a flattened SERVER failure detail onto the user-facing toast line.
///
/// `action` is the leading noun phrase ("Artwork upload" / "Artwork reset");
/// the 401 session-expiry case is handled by the caller BEFORE this (it drops
/// to login rather than toasting), and local pick/read failures never reach
/// here (see [`CustomArtworkOutcome::LocalFailed`]).
///
/// - 403 (`Forbidden` marker): the server refused — artwork uploads disabled
///   (`EnableArtworkUpload=false` + non-admin) or, for playlists, not the
///   owner. One honest message covers both server-side causes.
/// - 400: Navidrome rejects undecodable images and bodies over its
///   `MaxImageUploadSize` (default 10 MB) with a Bad Request.
/// - anything else: generic, with the detail preserved.
pub(crate) fn custom_artwork_error_toast(action: &str, detail: &str) -> String {
    if NokkviError::is_forbidden_str(detail) {
        format!(
            "{action} not allowed: the server refused (artwork upload disabled, or you lack permission)"
        )
    } else if detail.contains("status 400") {
        format!("{action} failed: not a valid image, or larger than the server allows")
    } else {
        format!("{action} failed: {detail}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forbidden_detail_maps_to_permission_toast() {
        let detail = "Forbidden: API POST /api/playlist/p1/image failed with status 403: denied";
        let toast = custom_artwork_error_toast("Artwork upload", detail);
        assert!(toast.starts_with("Artwork upload not allowed"), "{toast}");
        assert!(toast.contains("permission"), "{toast}");
    }

    #[test]
    fn bad_request_detail_maps_to_invalid_image_toast() {
        let detail = "API POST /api/radio/r1/image failed with status 400 Bad Request: unable to decode image";
        let toast = custom_artwork_error_toast("Artwork upload", detail);
        assert!(toast.contains("not a valid image"), "{toast}");
    }

    #[test]
    fn other_details_pass_through_generically() {
        let toast = custom_artwork_error_toast("Artwork reset", "connection refused");
        assert_eq!(toast, "Artwork reset failed: connection refused");
    }

    #[test]
    fn outcome_from_result_flattens_both_arms() {
        assert!(matches!(
            outcome_from_result(Ok(())),
            CustomArtworkOutcome::Applied
        ));
        let outcome = outcome_from_result(Err(nokkvi_data::types::error::NokkviError::Forbidden(
            "ctx: 403".into(),
        )
        .into()));
        let CustomArtworkOutcome::Failed(detail) = outcome else {
            panic!("Err must flatten to Failed");
        };
        assert!(NokkviError::is_forbidden_str(&detail), "{detail}");
    }

    /// rfd's portal backend matches these globs case-sensitively, so every
    /// lowercase extension must ship with its uppercase twin.
    #[test]
    fn image_extensions_cover_both_cases() {
        for ext in ["png", "jpg", "jpeg", "gif", "webp"] {
            assert!(IMAGE_EXTENSIONS.contains(&ext), "missing lowercase {ext}");
            let upper = ext.to_ascii_uppercase();
            assert!(
                IMAGE_EXTENSIONS.contains(&upper.as_str()),
                "missing uppercase twin {upper} (portal globs are case-sensitive)"
            );
        }
    }

    #[test]
    fn oversize_message_names_file_size_and_ceiling() {
        let msg = oversize_message("huge.png", 300 * 1024 * 1024);
        assert!(msg.contains("huge.png"), "{msg}");
        assert!(msg.contains("300.0 MiB"), "{msg}");
        assert!(msg.contains("32 MiB"), "{msg}");
    }
}
