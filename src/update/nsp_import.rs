//! M6 — .nsp import: absorb a smart-playlist rules file from disk.
//!
//! Two surfaces share the pick+parse front half ([`pick_nsp_file`]):
//! - the Playlists header create dropdown ("Import .nsp File") runs the
//!   SERVER-WRITE flow here — collision-routed through the shared text
//!   input dialog (update-existing / create-new / cancel), or a direct
//!   create when the name is unclaimed;
//! - the rules session's create empty state ("Import from .nsp file…")
//!   loads the parsed criteria INTO the open session like a disk-backed
//!   preset (handled in `rules_editor.rs`), writing nothing server-side.
//!
//! Parsing itself is the data crate's `parse_nsp_envelope` — the same
//! 100 KB cap + comment-strip + flat-envelope split the server applies.

use iced::Task;
use nokkvi_data::types::smart_criteria::{NSP_MAX_BYTES, parse_nsp_envelope};

use crate::{Nokkvi, app_message::Message, widgets::text_input_dialog::TextInputDialogAction};

/// The payload a successfully parsed .nsp yields: dialog-ready metadata
/// (fallbacks applied) + the wire-identical criteria value.
#[derive(Debug, Clone, PartialEq)]
pub struct NspImportPayload {
    /// Envelope `name`, or the picked file's stem when absent — mirroring
    /// the server's own filename fallback at scan time.
    pub name: String,
    pub comment: String,
    /// Envelope `public`, defaulting to `true` (the project's default-public
    /// policy; the server would default this to its own config instead).
    pub public: bool,
    /// The criteria object (envelope minus name/comment/public) — sent to
    /// the API verbatim as `rules`.
    pub rules: serde_json::Value,
}

/// Outcome of the pick + read + parse step.
#[derive(Debug, Clone, PartialEq)]
pub enum NspPickResult {
    /// The user dismissed the dialog (or the portal produced no file).
    Cancelled,
    /// The file couldn't be used — a user-facing reason for the failure
    /// toast ("couldn't parse JSON" / "file exceeds 100 KB" / …).
    Failed(String),
    Parsed(Box<NspImportPayload>),
}

/// Open the native (xdg-portal) picker filtered to .nsp files, then
/// size-check, read, and parse. Mirrors the artwork picker's shape: size
/// guard BEFORE the read, silent cancel with one tracing breadcrumb for
/// portal-less setups.
pub(crate) async fn pick_nsp_file() -> NspPickResult {
    let Some(handle) = rfd::AsyncFileDialog::new()
        .set_title("Choose a smart-playlist (.nsp) file")
        // Both cases: rfd's xdg-portal backend forwards these as literal,
        // case-sensitive globs.
        .add_filter("Smart playlists", &["nsp", "NSP"])
        .pick_file()
        .await
    else {
        tracing::info!(
            "nsp picker returned no file: user cancelled, or (if no dialog ever appeared) a \
             FileChooser portal implementation such as xdg-desktop-portal-gtk may be missing"
        );
        return NspPickResult::Cancelled;
    };
    let path = handle.path().to_path_buf();
    // Size guard BEFORE the read — the parse re-checks, but refusing here
    // keeps an accidental huge pick from ballooning RAM first.
    match tokio::fs::metadata(&path).await {
        Ok(meta) if meta.len() > NSP_MAX_BYTES as u64 => {
            return NspPickResult::Failed("file exceeds 100 KB".to_owned());
        }
        Ok(_) => {}
        Err(e) => {
            return NspPickResult::Failed(format!("could not read {}: {e}", path.display()));
        }
    }
    let bytes = match tokio::fs::read(&path).await {
        Ok(bytes) => bytes,
        Err(e) => {
            return NspPickResult::Failed(format!("could not read {}: {e}", path.display()));
        }
    };
    match parse_nsp_envelope(&bytes) {
        Ok(envelope) => {
            let stem = path.file_stem().map_or_else(
                || "Imported playlist".to_owned(),
                |s| s.to_string_lossy().into_owned(),
            );
            let name = envelope
                .name
                .as_deref()
                .map(str::trim)
                .filter(|n| !n.is_empty())
                .map_or(stem, str::to_owned);
            NspPickResult::Parsed(Box::new(NspImportPayload {
                name,
                comment: envelope.comment.unwrap_or_default(),
                public: envelope.public.unwrap_or(true),
                rules: envelope.criteria,
            }))
        }
        Err(reason) => NspPickResult::Failed(reason),
    }
}

impl Nokkvi {
    /// Header-dropdown entry point: caps-gate, then run the async pick.
    pub(crate) fn handle_import_nsp(&mut self) -> Task<Message> {
        if !self.caps_state.smart_available() {
            // Defensive backstop — the dropdown entry is caps-gated; reaching
            // here means a stale surface or a rebind race.
            self.toast_warn(
                "Smart playlists need Navidrome 0.61+ (or the server version is unknown)",
            );
            return Task::none();
        }
        Task::perform(pick_nsp_file(), Message::NspImportPicked)
    }

    /// Route the parsed file: no name collision → direct create; collision
    /// with an OWNED SMART row → three-way dialog (Update / Create new /
    /// Cancel); any other collision (unowned, or an owned ordinary
    /// playlist whose rules a PUT would silently convert) → create-only
    /// dialog with a dimmed honesty note.
    pub(crate) fn handle_nsp_import_picked(&mut self, result: NspPickResult) -> Task<Message> {
        let payload = match result {
            NspPickResult::Cancelled => return Task::none(),
            NspPickResult::Failed(reason) => {
                self.toast_error(format!("Not a valid smart-playlist file — {reason}"));
                return Task::none();
            }
            NspPickResult::Parsed(payload) => payload,
        };

        let collision = nokkvi_data::services::api::playlists::duplicate_playlist_name(
            &payload.name,
            self.library.playlists.iter().map(|p| p.name.as_str()),
        )
        .map(|i| &self.library.playlists[i]);

        let Some(row) = collision else {
            // Unclaimed name — create directly, no dialog.
            return self.import_create_task(*payload);
        };

        let owned =
            crate::views::playlists::view::playlist_is_owned(&row.owner_id, &self.session_user_id);
        if owned && row.is_smart {
            // Three-way: Update replaces the existing row's rules (the
            // overwriting action sits LAST as the primary button), Create
            // new makes a separate playlist from the input's name.
            let mut note = format!(
                "You already own a smart playlist named \"{}\". Update replaces its rules; \
                 Create new makes a separate playlist.",
                row.name
            );
            let detach_sync = row.is_file_backed && row.sync && self.caps_state.caps().sync_via_put;
            if detach_sync {
                note.push_str(" Updating detaches it from its server-side file.");
            } else if row.is_file_backed && row.sync {
                // 0.61: no sync PUT — the file re-syncs over the update.
                note.push_str(" Its server-side file re-syncs the rules on every scan.");
            }
            self.text_input_dialog.open(
                "Import Smart Playlist",
                payload.name.clone(),
                "Playlist name...",
                TextInputDialogAction::ImportNspUpdate {
                    playlist_id: row.id.clone(),
                    detach_sync,
                    // Preserve the TARGET's metadata — a rules-only update
                    // (the file's comment/public would silently overwrite,
                    // e.g. flip a private playlist public when the file omits
                    // `public`). The public toggle stays hidden for Update.
                    comment: row.comment.clone(),
                    public: row.public,
                    rules: payload.rules.clone(),
                },
            );
            self.text_input_dialog.extra_action = Some((
                "Create new".to_owned(),
                TextInputDialogAction::ImportNspCreate {
                    comment: payload.comment,
                    rules: payload.rules,
                },
            ));
            // Seeds the "Create new" alternative (its toggle is hidden while
            // Update is primary, but SubmitExtra reads this value).
            self.text_input_dialog.public = payload.public;
            self.text_input_dialog.set_note(note);
        } else {
            let note = if owned {
                format!(
                    "You already own an ordinary playlist named \"{}\" — this creates a \
                     separate smart playlist.",
                    row.name
                )
            } else {
                let owner = if row.owner_name.is_empty() {
                    "another user".to_owned()
                } else {
                    row.owner_name.clone()
                };
                format!(
                    "A playlist named \"{}\" already exists but belongs to {owner}.",
                    row.name
                )
            };
            self.text_input_dialog.open(
                "Import Smart Playlist",
                payload.name.clone(),
                "Playlist name...",
                TextInputDialogAction::ImportNspCreate {
                    comment: payload.comment,
                    rules: payload.rules,
                },
            );
            self.text_input_dialog.public = payload.public;
            self.text_input_dialog.set_note(note);
        }
        Task::none()
    }

    /// The direct-create lane (unclaimed name, no dialog). Success rides
    /// the standard `PlaylistMutated(Created)` toast + list refresh.
    pub(crate) fn import_create_task(&mut self, payload: NspImportPayload) -> Task<Message> {
        let NspImportPayload {
            name,
            comment,
            public,
            rules,
        } = payload;
        let toast_name = name.clone();
        self.shell_task(
            move |shell| async move {
                let service = shell.playlists_api().await?;
                service
                    .create_smart_playlist(&name, &comment, public, &rules)
                    .await
            },
            move |result: Result<String, anyhow::Error>| match result {
                // `None` id on purpose (the M7 Trawl-save reasoning): an
                // imported playlist has no relation to the current queue, so
                // it must NOT seize the queue's playlist-context banner /
                // persist a false "playing from" context. The Created toast +
                // list refresh still fire.
                Ok(_playlist_id) => Message::PlaylistMutated(
                    crate::app_message::PlaylistMutation::Created(toast_name, None),
                ),
                Err(e) => {
                    tracing::error!(" Failed to import smart playlist: {e}");
                    Message::Toast(crate::app_message::ToastMessage::Push(
                        nokkvi_data::types::toast::Toast::new(
                            format!("Failed to import smart playlist: {e}"),
                            nokkvi_data::types::toast::ToastLevel::Error,
                        ),
                    ))
                }
            },
        )
    }
}
