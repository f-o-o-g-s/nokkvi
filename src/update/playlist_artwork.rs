//! Custom (user-uploaded) playlist artwork handlers.
//!
//! Playlists normally render a client-side collage (2×2 row quad, 3×3 panel)
//! assembled from album covers. A playlist whose `uploaded_image` is set has
//! a real server-side cover instead, served through `getCoverArt?id=pl-<id>`
//! at any size — these handlers fetch it into the dedicated
//! `playlist_custom_art` / `playlist_custom_large_art` caches (the view gives
//! them display precedence over the collage) and run the Set/Reset upload
//! flows. Mirrors `update/radio_artwork.rs`.

use iced::{Task, widget::image};
use nokkvi_data::utils::artwork_url::THUMBNAIL_SIZE;

use crate::{
    Nokkvi,
    app_message::{ArtworkMessage, CustomArtworkOutcome, Message, MiniArt},
    update::components::custom_artwork,
};

/// Kind-prefixed `getCoverArt` id for a playlist's server-side cover.
fn playlist_art_id(playlist_id: &str) -> String {
    format!("pl-{playlist_id}")
}

impl Nokkvi {
    /// Decide which viewport playlists need a custom-cover mini fetch.
    /// Pure over state (no task construction) so the gating is unit-testable
    /// without an `app_service`. Returns `(playlist_id, updated_at)` pairs —
    /// the `updated_at` doubles as the fetch's `_u=` cache-buster and the
    /// version recorded on completion.
    ///
    /// Gates, in order: `uploaded_image` set, no fetch already in flight
    /// (`playlist_custom_art_pending`), then the shared
    /// [`should_refetch`](crate::update::components::should_refetch)
    /// membership + version + negative-cache check (a cover replaced in the
    /// web UI bumps `updated_at` → version-aware refetch; a known-dead id is
    /// not re-queued on every scroll step).
    pub(crate) fn playlist_custom_minis_to_fetch(&self) -> Vec<(String, Option<String>)> {
        use std::collections::HashSet;

        let total = self.library.playlists.len();
        if total == 0 {
            return Vec::new();
        }
        let cached: HashSet<&String> = self
            .artwork
            .playlist_custom_art
            .iter()
            .map(|(k, _)| k)
            .collect();
        let mut out = Vec::new();
        for idx in self.playlists_page.common.slot_list.prefetch_indices(total) {
            let Some(playlist) = self.library.playlists.get(idx) else {
                continue;
            };
            if playlist.uploaded_image.is_none() {
                continue;
            }
            if self
                .artwork
                .playlist_custom_art_pending
                .contains(&playlist.id)
            {
                continue;
            }
            let version = Some(playlist.updated_at.clone());
            if !crate::update::components::should_refetch(
                &cached,
                &self.artwork.playlist_custom_art_versions,
                &self.artwork.playlist_custom_art_failed,
                &playlist.id,
                &version,
            ) {
                continue;
            }
            out.push((playlist.id.clone(), version));
        }
        out
    }

    /// Viewport-window 80px prefetch of CUSTOM playlist covers — the task
    /// side of [`Self::playlist_custom_minis_to_fetch`]. Dispatched from the
    /// viewport-driven `LoadArtwork` pass that also drives the collage
    /// fetches, so scroll, view-enter, and the post-load pass all warm it.
    /// Uses the same parent-list index space as the collage prefetch
    /// (expansion skew accepted status quo).
    pub(crate) fn prefetch_playlist_custom_art_tasks(&mut self) -> Task<Message> {
        if self.app_service.is_none() {
            return Task::none();
        }
        let to_fetch = self.playlist_custom_minis_to_fetch();
        let tasks: Vec<_> = to_fetch
            .into_iter()
            .map(|(id, version)| self.dispatch_playlist_custom_mini_fetch(id, version))
            .collect();
        Task::batch(tasks)
    }

    /// Build one custom-cover mini fetch AND mark it in flight — the single
    /// entry point that keeps the pending set in lockstep with dispatched
    /// tasks (used by the viewport prefetch and the post-upload refetch).
    fn dispatch_playlist_custom_mini_fetch(
        &mut self,
        playlist_id: String,
        cache_buster: Option<String>,
    ) -> Task<Message> {
        self.artwork
            .playlist_custom_art_pending
            .insert(playlist_id.clone());
        self.fetch_playlist_custom_mini_task(playlist_id, cache_buster)
    }

    /// Single 80px custom-cover fetch for one playlist. `cache_buster` rides
    /// the `_u=` query param (the playlist's `updated_at`, or a fresh token
    /// right after an upload) so intermediary HTTP caches can't serve a stale
    /// image.
    pub(crate) fn fetch_playlist_custom_mini_task(
        &self,
        playlist_id: String,
        cache_buster: Option<String>,
    ) -> Task<Message> {
        let Some(shell) = &self.app_service else {
            return Task::none();
        };
        let albums_vm = shell.albums().clone();
        Task::perform(
            async move {
                let art = MiniArt::from_fetch(
                    albums_vm
                        .fetch_album_artwork(
                            &playlist_art_id(&playlist_id),
                            Some(THUMBNAIL_SIZE),
                            cache_buster.as_deref(),
                        )
                        .await,
                );
                (playlist_id, cache_buster, art)
            },
            |(playlist_id, version, art)| {
                Message::Artwork(ArtworkMessage::PlaylistCustomMiniLoaded(
                    playlist_id,
                    version,
                    art,
                ))
            },
        )
    }

    /// Store a fetched mini custom cover into `playlist_custom_art` and keep
    /// the prefetch-gate bookkeeping in lockstep: release the in-flight slot
    /// on EVERY outcome (so throttled/transient fetches can retry), record
    /// the warming version on success, and negatively cache a deterministic
    /// Missing (a stale `uploaded_image` whose file is gone server-side must
    /// not re-fire on every scroll step; a changed `updated_at` bypasses it).
    pub(crate) fn handle_playlist_custom_mini_loaded(
        &mut self,
        playlist_id: String,
        version: Option<String>,
        art: MiniArt,
    ) -> Task<Message> {
        self.artwork
            .playlist_custom_art_pending
            .remove(&playlist_id);
        match art {
            MiniArt::Loaded(h) => {
                self.artwork.playlist_custom_art.put(playlist_id.clone(), h);
                self.artwork
                    .playlist_custom_art_versions
                    .insert(playlist_id.clone(), version);
                self.artwork.playlist_custom_art_failed.remove(&playlist_id);
            }
            MiniArt::Missing => {
                self.artwork
                    .playlist_custom_art_failed
                    .insert(playlist_id, version);
            }
            // Transient: record nothing — the next viewport pass re-attempts.
            MiniArt::Transient => {}
        }
        Task::none()
    }

    /// Fetch the resolution-sized custom cover for a playlist. Gated on the
    /// live `uploaded_image` field, so callers can dispatch unconditionally
    /// for the centered playlist. Mirrors [`Nokkvi::handle_load_radio_large`].
    pub(crate) fn handle_load_playlist_custom_large(
        &mut self,
        playlist_id: String,
    ) -> Task<Message> {
        // Only custom-cover playlists have anything to fetch; read the LIVE
        // library field so an SSE reload that cleared it stops the fetch.
        let cache_buster = match self.library.playlists.iter().find(|p| p.id == playlist_id) {
            Some(p) if p.uploaded_image.is_some() => p.updated_at.clone(),
            _ => return Task::none(),
        };
        // Serve from cache for instant back-navigation (the Loaded handler
        // re-puts, which is also the LRU recency promotion).
        if self.playlist_custom_large_is_current(&playlist_id, &cache_buster)
            && let Some(handle) = self
                .artwork
                .playlist_custom_large_art
                .peek(&playlist_id)
                .cloned()
        {
            return Task::done(Message::Artwork(ArtworkMessage::PlaylistCustomLargeLoaded(
                playlist_id,
                Some(handle),
            )));
        }
        self.fetch_playlist_custom_large_task(playlist_id, cache_buster)
    }

    /// Whether the cached large custom cover for `playlist_id` is still
    /// current: cached AND the recorded warming version (written by the mini
    /// completion — the mini always accompanies the large through the same
    /// viewport pass) still matches the live `updated_at`. A cover replaced
    /// in the web UI bumps `updated_at`, so the stale cached large refetches
    /// instead of being served forever. Pure over state so the gate is
    /// unit-testable.
    pub(crate) fn playlist_custom_large_is_current(
        &self,
        playlist_id: &str,
        live_updated_at: &str,
    ) -> bool {
        self.artwork
            .playlist_custom_large_art
            .contains(&playlist_id.to_string())
            && self
                .artwork
                .playlist_custom_art_versions
                .get(playlist_id)
                .is_some_and(|v| v.as_deref() == Some(live_updated_at))
    }

    /// Resolution-sized custom-cover fetch for one playlist. `cache_buster`
    /// rides the `_u=` param — the playlist's `updated_at` on viewport-driven
    /// loads, or a fresh token right after an upload (same convention as the
    /// mini fetch, so neither path can be served a stale intermediary-cached
    /// image).
    fn fetch_playlist_custom_large_task(
        &self,
        playlist_id: String,
        cache_buster: String,
    ) -> Task<Message> {
        let Some(shell) = &self.app_service else {
            return Task::none();
        };
        let albums_vm = shell.albums().clone();
        let artwork_size = self.settings.artwork_resolution.to_size();
        Task::perform(
            async move {
                let bytes = albums_vm
                    .fetch_album_artwork(
                        &playlist_art_id(&playlist_id),
                        artwork_size,
                        Some(&cache_buster),
                    )
                    .await
                    .ok();
                (playlist_id, bytes.map(image::Handle::from_bytes))
            },
            |(playlist_id, handle)| {
                Message::Artwork(ArtworkMessage::PlaylistCustomLargeLoaded(
                    playlist_id,
                    handle,
                ))
            },
        )
    }

    /// Store a fetched large custom cover into `playlist_custom_large_art`.
    pub(crate) fn handle_playlist_custom_large_loaded(
        &mut self,
        playlist_id: String,
        handle: Option<image::Handle>,
    ) -> Task<Message> {
        if let Some(h) = handle {
            self.artwork.playlist_custom_large_art.put(playlist_id, h);
        }
        Task::none()
    }

    /// "Set Custom Artwork…" on a playlist: open the native file picker, read
    /// the chosen image, and upload it to Navidrome's
    /// `POST /api/playlist/{id}/image` — all inside one async task. The
    /// completion lands as [`ArtworkMessage::PlaylistCustomArtworkSet`].
    pub(crate) fn handle_set_playlist_artwork(
        &mut self,
        playlist_id: String,
        playlist_name: String,
    ) -> Task<Message> {
        let id_for_upload = playlist_id.clone();
        self.shell_task(
            move |shell| async move {
                let outcome = custom_artwork::pick_and_upload(|bytes, filename| async move {
                    shell
                        .playlists_api()
                        .await?
                        .upload_image(&id_for_upload, bytes, &filename)
                        .await
                })
                .await;
                (playlist_id, playlist_name, outcome)
            },
            |(id, name, outcome)| {
                Message::Artwork(ArtworkMessage::PlaylistCustomArtworkSet(id, name, outcome))
            },
        )
    }

    /// "Reset Artwork" on a playlist: `DELETE /api/playlist/{id}/image`.
    /// Completion lands as [`ArtworkMessage::PlaylistCustomArtworkReset`].
    pub(crate) fn handle_reset_playlist_artwork(
        &mut self,
        playlist_id: String,
        playlist_name: String,
    ) -> Task<Message> {
        let id_for_delete = playlist_id.clone();
        self.shell_task(
            move |shell| async move {
                let outcome = custom_artwork::outcome_from_result(
                    async {
                        shell
                            .playlists_api()
                            .await?
                            .delete_image(&id_for_delete)
                            .await
                    }
                    .await,
                );
                (playlist_id, playlist_name, outcome)
            },
            |(id, name, outcome)| {
                Message::Artwork(ArtworkMessage::PlaylistCustomArtworkReset(
                    id, name, outcome,
                ))
            },
        )
    }

    /// Completion of the playlist "Set Custom Artwork…" upload. On success:
    /// drop any stale cached custom art, optimistically mark the library row
    /// (`uploaded_image = Some(marker)`) so display/menu gating flips without
    /// a full list reload (which would reset the playlists viewport to 0),
    /// then refetch mini + large with a fresh cache-buster. The marker value
    /// is a placeholder — nokkvi only ever treats the field as a presence
    /// flag (fetches key on `pl-<id>`), and the next list load (SSE, manual
    /// refresh, view re-enter) replaces it with the server's real reference.
    pub(crate) fn handle_playlist_custom_artwork_set(
        &mut self,
        playlist_id: String,
        playlist_name: String,
        outcome: CustomArtworkOutcome,
    ) -> Task<Message> {
        match outcome {
            CustomArtworkOutcome::Cancelled => Task::none(),
            CustomArtworkOutcome::LocalFailed(detail) => {
                self.playlist_custom_artwork_local_failure(&playlist_id, "Artwork upload", detail)
            }
            CustomArtworkOutcome::Failed(detail) => {
                self.playlist_custom_artwork_failure(&playlist_id, "Artwork upload", detail)
            }
            CustomArtworkOutcome::Applied => {
                self.toast_success(format!("Custom artwork set for '{playlist_name}'"));
                self.artwork.playlist_custom_art.pop(&playlist_id);
                self.artwork.playlist_custom_large_art.pop(&playlist_id);
                self.artwork
                    .playlist_custom_art_versions
                    .remove(&playlist_id);
                self.artwork.playlist_custom_art_failed.remove(&playlist_id);
                // ONE fresh timestamp: bumped onto the row's updated_at (so
                // every LATER refetch — viewport prefetch, post-eviction
                // large load — derives a fresh `_u=` buster instead of
                // re-sending the pre-upload value to an intermediary HTTP
                // cache) AND used directly by the immediate refetch pair.
                // RFC 3339 keeps the "Updated:" pill and date column parsing.
                let fresh_updated_at = nokkvi_data::utils::formatters::now_rfc3339();
                self.library.playlists.update_by(
                    |p| p.id == playlist_id,
                    |p| {
                        p.uploaded_image = Some("uploaded".to_string());
                        p.updated_at = fresh_updated_at.clone();
                    },
                );
                Task::batch([
                    self.dispatch_playlist_custom_mini_fetch(
                        playlist_id.clone(),
                        Some(fresh_updated_at.clone()),
                    ),
                    self.fetch_playlist_custom_large_task(playlist_id, fresh_updated_at),
                ])
            }
        }
    }

    /// Completion of the playlist "Reset Artwork" delete. On success the
    /// optimistic field clear + cache drop bring the collage/quad straight
    /// back (their caches were never touched) — no refetch needed.
    pub(crate) fn handle_playlist_custom_artwork_reset(
        &mut self,
        playlist_id: String,
        playlist_name: String,
        outcome: CustomArtworkOutcome,
    ) -> Task<Message> {
        match outcome {
            CustomArtworkOutcome::Cancelled => Task::none(),
            CustomArtworkOutcome::LocalFailed(detail) => {
                self.playlist_custom_artwork_local_failure(&playlist_id, "Artwork reset", detail)
            }
            CustomArtworkOutcome::Failed(detail) => {
                self.playlist_custom_artwork_failure(&playlist_id, "Artwork reset", detail)
            }
            CustomArtworkOutcome::Applied => {
                self.toast_success(format!(
                    "Artwork reset for '{playlist_name}' — the collage returns"
                ));
                self.artwork.playlist_custom_art.pop(&playlist_id);
                self.artwork.playlist_custom_large_art.pop(&playlist_id);
                self.artwork
                    .playlist_custom_art_versions
                    .remove(&playlist_id);
                self.artwork.playlist_custom_art_failed.remove(&playlist_id);
                self.library
                    .playlists
                    .update_by(|p| p.id == playlist_id, |p| p.uploaded_image = None);
                Task::none()
            }
        }
    }

    /// LOCAL-failure tail: plain toast, verbatim. Deliberately bypasses the
    /// Unauthorized/Forbidden/400 classifiers — the detail embeds the
    /// user-picked path (see `CustomArtworkOutcome::LocalFailed`).
    fn playlist_custom_artwork_local_failure(
        &mut self,
        playlist_id: &str,
        action_label: &'static str,
        detail: String,
    ) -> Task<Message> {
        tracing::error!("{action_label} failed locally for playlist {playlist_id}: {detail}");
        self.toast_error(format!("{action_label} failed: {detail}"));
        Task::none()
    }

    /// Shared SERVER-failure tail for the playlist Set/Reset completions:
    /// 401 drops to login; everything else logs at this handling boundary and
    /// surfaces the friendly toast. State is left untouched — the server
    /// didn't change.
    fn playlist_custom_artwork_failure(
        &mut self,
        playlist_id: &str,
        action_label: &'static str,
        detail: String,
    ) -> Task<Message> {
        if nokkvi_data::types::error::NokkviError::is_unauthorized_str(&detail) {
            return self.handle_session_expired();
        }
        tracing::error!("{action_label} failed for playlist {playlist_id}: {detail}");
        self.toast_error(custom_artwork::custom_artwork_error_toast(
            action_label,
            &detail,
        ));
        Task::none()
    }
}
