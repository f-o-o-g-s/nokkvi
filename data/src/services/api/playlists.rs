//! Playlists API Service
//!
//! Handles playlist-related API calls to Navidrome server.

use anyhow::{Context, Result};
use tracing::{debug, warn};

use crate::{
    services::api::{
        client::ApiClient,
        pagination, parse,
        sort::{self, SortDomain},
        subsonic,
    },
    types::playlist::Playlist,
};

/// Inner payload of the Subsonic `getPlaylist` envelope
/// ([`subsonic::SubsonicEnvelope`]).
#[derive(Debug, serde::Deserialize)]
struct PlaylistInner {
    playlist: Option<SubsonicPlaylistWithSongs>,
}

#[derive(Debug, serde::Deserialize)]
#[allow(dead_code)] // Fields needed for serde deserialization
struct SubsonicPlaylistWithSongs {
    id: Option<String>,
    name: Option<String>,
    entry: Option<serde_json::Value>, // Can be array or single object
    /// OpenSubsonic `readonly` (0.61+, lowercase key —
    /// `reference-navidrome/server/subsonic/responses/responses.go:324`).
    readonly: Option<bool>,
    #[serde(rename = "validUntil")]
    valid_until: Option<String>,
}

/// Playlist-level attributes captured from the Subsonic `getPlaylist`
/// envelope alongside the songs — threaded into the play-flow context so
/// surfaces that never see the native list (Harbour play, session restore)
/// still get a smartness signal.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SubsonicPlaylistAttrs {
    /// OpenSubsonic `readonly`: `true` for smart playlists AND for regular
    /// playlists not owned by the requesting user
    /// (`reference-navidrome/server/subsonic/playlists.go:160-177`) — a
    /// CONSERVATIVE guard signal, never proof-of-smart. `None` on pre-0.61
    /// servers (key absent).
    pub readonly: Option<bool>,
}

#[derive(Debug, serde::Deserialize)]
#[allow(dead_code)] // Fields needed for serde deserialization
struct SubsonicSongEntry {
    id: Option<String>,
    #[serde(rename = "albumId")]
    album_id: Option<String>,
}

#[derive(Clone)]
pub struct PlaylistsApiService {
    client: ApiClient,
    server_url: String,
    subsonic_credential: String,
}

impl PlaylistsApiService {
    /// Create with a pre-authenticated ApiClient.
    pub fn new(client: ApiClient, server_url: String, subsonic_credential: String) -> Self {
        Self {
            client,
            server_url,
            subsonic_credential,
        }
    }

    /// Library-aware playlist loader.
    ///
    /// `library_ids` is accepted for signature symmetry with the other
    /// browse endpoints but intentionally NOT forwarded — Navidrome's
    /// `/api/playlist` filter map registers only `q` + `smart`
    /// (`reference-navidrome/persistence/playlist_repository.go:48-60`),
    /// so an unrecognized `library_id` filter would either be silently
    /// ignored or surface as a `LIKE 'playlist.library_id ...'` error
    /// (the `playlist` table has no `library_id` column). User-accessible
    /// library auto-scoping already filters this list server-side.
    pub async fn load_playlists_with_libraries(
        &self,
        sort_mode: &str,
        sort_order: &str,
        search_query: Option<&str>,
        library_ids: &[i32],
    ) -> Result<(Vec<Playlist>, u32)> {
        // Touch the param so callers passing a non-empty set in good faith
        // don't trip `unused_variables`. We make a no-op consume here
        // rather than wiring through a fake param, since Navidrome would
        // reject the wire form.
        let _ = library_ids;
        // For random view, we load by name and shuffle client-side
        let (is_random, actual_sort_mode) = sort::resolve_random_sort_mode(sort_mode);

        // Map viewType to API sort parameter
        let sort_param = sort::map_sort_mode(SortDomain::Playlists, actual_sort_mode);
        let order_param = sort::resolve_order(SortDomain::Playlists, actual_sort_mode, sort_order);

        let params = Self::build_playlist_params(sort_param, order_param, search_query);

        // Fetch from native API
        let result = self.client.get_with_headers("/api/playlist", &params).await;

        // Parse failures degrade to an empty list like the network-error arm
        // below, but are logged — a malformed body must never yield a
        // silently empty Playlists view.
        let playlists: Vec<Playlist> = match result {
            Ok((response_text, _)) => {
                parse::parse_json_or_default(&response_text, "playlists JSON response")
            }
            Err(e) => {
                warn!(" PlaylistsApiService: Native API failed: {}", e);
                Vec::new()
            }
        };

        // Central draft filtering — the single-point fix for draft leakage.
        // Every caller lane (Playlists view, Harbour shelf, whole-library
        // search, Trawl seeds, all add-target pickers, batch dialog)
        // inherits it here, so a "nokkvi draft (safe to ignore)" workspace
        // row can never render anywhere. Strict-parse `is_draft()` means a
        // user comment merely starting with the prefix is NOT filtered.
        // The orphan sweep uses the raw (unfiltered) variant instead.
        let mut playlists = filter_draft_rows(playlists);

        // Client-side shuffle for random view
        if is_random {
            sort::apply_random_shuffle(&mut playlists);
            if let Some(first) = playlists.first() {
                debug!(" Random sort - First playlist: {}", first.name);
            }
        }

        let total_count = playlists.len() as u32;

        debug!(" PlaylistsService: Loaded {} playlists", total_count);

        Ok((playlists, total_count))
    }

    /// The RAW (unfiltered) list — the orphan sweep's loader: it must SEE
    /// draft rows to delete them, so it bypasses `filter_draft_rows`.
    /// Every other consumer goes through
    /// [`Self::load_playlists_with_libraries`]; keep it that way.
    pub async fn load_playlists_with_libraries_raw(&self) -> Result<Vec<Playlist>> {
        let params = Self::build_playlist_params("name", "ASC", None);
        let (response_text, _) = self
            .client
            .get_with_headers("/api/playlist", &params)
            .await?;
        Ok(parse::parse_json_or_default(
            &response_text,
            "playlists JSON response (raw)",
        ))
    }

    /// Build the `_sort` / `_order` / search params for an `/api/playlist`
    /// browse request. Extracted (mirroring `SongsApiService::
    /// build_song_params`) so the wire shape is pinned by tests — playlists
    /// search on `q` (not `name`/`title`) and NEVER send `library_id`; see
    /// [`Self::load_playlists_with_libraries`] for the Navidrome citation.
    fn build_playlist_params<'a>(
        sort_param: &'a str,
        order_param: &'a str,
        search_query: Option<&'a str>,
    ) -> Vec<(&'a str, &'a str)> {
        let mut params = vec![
            ("_sort", sort_param),
            ("_order", order_param),
            ("_start", "0"),
            ("_end", pagination::NO_LIMIT_END_STR),
        ];

        // Add search query if provided (playlists use "q" parameter)
        if let Some(query) = search_query
            && !query.is_empty()
        {
            params.push(("q", query));
        }
        params
    }

    /// Load songs from a playlist (for playback), plus the playlist-level
    /// attributes ([`SubsonicPlaylistAttrs`] — OpenSubsonic `readonly`)
    /// captured from the same envelope.
    pub async fn load_playlist_songs(
        &self,
        playlist_id: &str,
    ) -> Result<(Vec<crate::types::song::Song>, SubsonicPlaylistAttrs)> {
        let inner: PlaylistInner = subsonic::subsonic_get_envelope(
            &self.client.http_client(),
            &self.server_url,
            "getPlaylist",
            &self.subsonic_credential,
            &[("id", playlist_id)],
            "Subsonic playlist",
        )
        .await?;

        let mut songs = Vec::new();
        let mut attrs = SubsonicPlaylistAttrs::default();

        // Missing playlist/entry keys yield Ok(empty) — a playlist with
        // zero entries must not error.
        if let Some(playlist) = inner.playlist {
            attrs.readonly = playlist.readonly;
            if let Some(entry_value) = playlist.entry {
                // Subsonic returns a single object instead of a one-element
                // array; `deserialize_one_or_many` absorbs that quirk.
                let entries: Vec<serde_json::Value> =
                    subsonic::deserialize_one_or_many(entry_value)?;

                for entry in entries {
                    let song = parse_subsonic_song_entry(entry)?;
                    songs.push(song);
                }
            }
        }

        debug!(
            " PlaylistsService: Loaded {} songs from playlist {}",
            songs.len(),
            playlist_id
        );

        Ok((songs, attrs))
    }

    /// Load album IDs from a playlist (for artwork collage)
    /// Returns up to 9 unique album IDs
    pub async fn load_playlist_albums(&self, playlist_id: &str) -> Result<Vec<String>> {
        let inner: PlaylistInner = subsonic::subsonic_get_envelope(
            &self.client.http_client(),
            &self.server_url,
            "getPlaylist",
            &self.subsonic_credential,
            &[("id", playlist_id)],
            "Subsonic playlist",
        )
        .await?;

        let mut album_ids = Vec::new();
        let mut seen = std::collections::HashSet::new();

        if let Some(playlist) = inner.playlist
            && let Some(entry_value) = playlist.entry
        {
            // Subsonic returns a single object instead of a one-element
            // array; `deserialize_one_or_many` absorbs that quirk.
            let entries: Vec<SubsonicSongEntry> = subsonic::deserialize_one_or_many(entry_value)?;

            for entry in entries {
                if let Some(album_id) = entry.album_id
                    && !seen.contains(&album_id)
                {
                    seen.insert(album_id.clone());
                    album_ids.push(album_id);
                    if album_ids.len() >= 9 {
                        break;
                    }
                }
            }
        }

        Ok(album_ids)
    }

    // =========================================================================
    // Mutation Methods — Navidrome Native REST API (/api/playlist)
    // =========================================================================

    /// Create a new playlist with the given name, visibility, and optional songs.
    ///
    /// Uses Navidrome native API: POST /api/playlist + POST /api/playlist/:id/tracks
    pub async fn create_playlist(
        &self,
        name: &str,
        comment: &str,
        song_ids: &[String],
        public: bool,
    ) -> Result<String> {
        // Step 1: Create the playlist (name + comment + visibility in one POST,
        // the same body shape create_smart_playlist uses — the native endpoint
        // accepts comment, so a create never has to backfill it separately).
        let body = serde_json::json!({ "name": name, "comment": comment, "public": public });
        let response = self.client.post_json("/api/playlist", &body).await?;

        // Parse the playlist ID from response
        let response_json: serde_json::Value =
            serde_json::from_str(&response).context("Failed to parse create playlist response")?;
        let playlist_id = response_json
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("No playlist ID in create response"))?
            .to_string();

        // Step 2: If we have songs, add them to the newly created playlist
        if !song_ids.is_empty() {
            let tracks_body = serde_json::json!({ "ids": song_ids });
            self.client
                .post_json(&format!("/api/playlist/{playlist_id}/tracks"), &tracks_body)
                .await?;
        }

        Ok(playlist_id)
    }

    /// Update a playlist's metadata. A `None` field is left unchanged.
    ///
    /// Navidrome's `PUT /api/playlist/:id` is a FULL REPLACE of name / comment /
    /// public, NOT a partial merge: the REST layer binds the JSON body into a
    /// fresh `model.Playlist`, so any key absent from the body arrives as a Go
    /// zero value (`""` / `false`), and `updatePlaylistEntity` → `updateMetadata`
    /// then write all three columns unconditionally (reference-navidrome
    /// `core/playlists/rest_adapter.go:99-128` + `playlists.go:231`). Sending a
    /// subset therefore CLEARS the omitted fields server-side — a rename-only
    /// PUT would wipe the comment and silently flip the playlist private.
    ///
    /// To offer true "nil-means-unchanged" semantics we re-read the current
    /// record and overlay only the provided (`Some`) fields, then always send
    /// the full `(name, comment, public)` triple. Re-reading also means an
    /// unspecified field round-trips the server's *current* value, so a
    /// concurrent change to a field this edit did not touch is preserved rather
    /// than reverted to a stale local copy.
    ///
    /// Uses Navidrome native API: GET + PUT /api/playlist/:id
    pub async fn update_playlist(
        &self,
        playlist_id: &str,
        name: Option<&str>,
        comment: Option<&str>,
        public: Option<bool>,
    ) -> Result<()> {
        let current = self.get_playlist(playlist_id).await?;
        let (name, comment, public) = merge_playlist_update(&current, name, comment, public);
        // Read-merge the rules too when the target is smart: Navidrome
        // v0.61.0 full-replaces `Rules` on ANY PUT (`current.Rules =
        // entity.Rules` unconditionally — the #5542 sent-columns diff only
        // landed in 0.62.0), so a `{name, comment, public}` rename PUT
        // would silently wipe the rules and convert the smart playlist to
        // an empty regular one. Harmless on 0.62+ (`rulesEqual` ⇒ not
        // rulesChanged); load-bearing on the ruled ≥0.61.0 floor.
        let rules = if current.is_smart() {
            current.rules.as_ref()
        } else {
            None
        };
        let body = build_update_playlist_body(name, comment, public, rules);
        self.client
            .put_json(&format!("/api/playlist/{playlist_id}"), &body)
            .await?;
        Ok(())
    }

    /// Fetch a single playlist's current metadata.
    ///
    /// Uses Navidrome native API: GET /api/playlist/:id. Public: the rules
    /// session's pencil JIT-fetch and the draft preview lane call it from
    /// UI-side `shell_task` closures.
    pub async fn get_playlist(&self, playlist_id: &str) -> Result<Playlist> {
        let body = self
            .client
            .get(&format!("/api/playlist/{playlist_id}"), &[])
            .await?;
        serde_json::from_str(&body).context("Failed to deserialize playlist metadata")
    }

    /// Fetch a single playlist's current `updatedAt` token.
    ///
    /// Used by the editor save path as an optimistic-concurrency guard: it
    /// re-reads the server's current `updatedAt` just before a destructive
    /// full-overwrite so a concurrent server-side edit can be detected and the
    /// overwrite refused. Returns an empty string when the field is absent.
    ///
    /// Uses Navidrome native API: GET /api/playlist/:id
    pub async fn get_playlist_updated_at(&self, playlist_id: &str) -> Result<String> {
        Ok(self.get_playlist(playlist_id).await?.updated_at)
    }

    /// Delete a playlist.
    ///
    /// Uses Navidrome native API: DELETE /api/playlist/:id
    pub async fn delete_playlist(&self, playlist_id: &str) -> Result<()> {
        self.client
            .delete(&format!("/api/playlist/{playlist_id}"))
            .await
    }

    /// Upload a custom cover image for a playlist. Navidrome stores it
    /// server-side and serves it back through `getCoverArt?id=pl-<id>` at any
    /// size, taking precedence over the generated mosaic. Requires
    /// `EnableArtworkUpload` (default true) or admin, plus playlist
    /// ownership — a refusal surfaces as [`NokkviError::Forbidden`].
    ///
    /// Uses Navidrome native API: POST /api/playlist/:id/image
    /// (`multipart/form-data`, one part named `image`).
    ///
    /// [`NokkviError::Forbidden`]: crate::types::error::NokkviError::Forbidden
    pub async fn upload_image(
        &self,
        playlist_id: &str,
        bytes: Vec<u8>,
        filename: &str,
    ) -> Result<()> {
        self.client
            .post_multipart(
                &format!("/api/playlist/{playlist_id}/image"),
                "image",
                bytes,
                filename,
            )
            .await?;
        Ok(())
    }

    /// Delete a playlist's custom cover image, reverting `getCoverArt` to the
    /// automatic artwork (sidecar / external URL / generated mosaic).
    ///
    /// Uses Navidrome native API: DELETE /api/playlist/:id/image
    pub async fn delete_image(&self, playlist_id: &str) -> Result<()> {
        self.client
            .delete(&format!("/api/playlist/{playlist_id}/image"))
            .await
    }

    /// Create a smart playlist: POST /api/playlist with a `rules` criteria
    /// body. Content-Type must be exactly "application/json" — Navidrome
    /// string-compares it and falls into the M3U importer otherwise
    /// (`server/nativeapi/native_api.go`); reqwest's `.json()` emits exactly
    /// that. Owner is forced to the authenticated user; server-managed
    /// fields (path/sync/evaluatedAt) are cleared server-side, and creation
    /// nils EvaluatedAt so the first page-0 read evaluates fresh (0.61+).
    /// NEVER called with an empty root conjunction — validation gates every
    /// caller (test-pinned in M5).
    pub async fn create_smart_playlist(
        &self,
        name: &str,
        comment: &str,
        public: bool,
        rules: &serde_json::Value,
    ) -> Result<String> {
        let body = serde_json::json!({
            "name": name,
            "comment": comment,
            "public": public,
            "rules": rules,
        });
        let response = self.client.post_json("/api/playlist", &body).await?;
        let response_json: serde_json::Value = serde_json::from_str(&response)
            .context("Failed to parse create smart playlist response")?;
        response_json
            .get("id")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .ok_or_else(|| anyhow::anyhow!("No playlist ID in create response"))
    }

    /// PUT /api/playlist/:id with the FULL state {name, comment, public}
    /// (+ `rules` when given, + `sync` when given — the file-backed detach,
    /// a 0.62+ capability). Full-state PUT lets 0.62+ servers diff on sent
    /// columns: unchanged rules keep the stored result; changed rules take
    /// the applyContentUpdate path. Used for preview PUTs on capable
    /// servers and for every finalize save (the draft's marker comment is
    /// cleared in this same atomic body — never a two-step clear).
    pub async fn put_playlist_full(
        &self,
        playlist_id: &str,
        name: &str,
        comment: &str,
        public: bool,
        rules: Option<&serde_json::Value>,
        sync: Option<bool>,
    ) -> Result<()> {
        let mut body = build_update_playlist_body(name, comment, public, rules);
        if let Some(sync) = sync
            && let Some(map) = body.as_object_mut()
        {
            map.insert("sync".to_owned(), serde_json::Value::Bool(sync));
        }
        self.client
            .put_json(&format!("/api/playlist/{playlist_id}"), &body)
            .await?;
        Ok(())
    }

    /// GET /api/playlist/:id/tracks?_start={start}&_end={end}.
    ///
    /// `start == 0` is the ONLY read that triggers the owner-side smart
    /// refresh (`server/nativeapi/playlists.go` gates on `_start==0`);
    /// `start > 0` pages serve rows from the SAME completed evaluation —
    /// exactly what a results pane wants when scrolling past page one. Goes
    /// through `get_with_headers` because `X-Total-Count` IS the evaluated
    /// match count. Never sent with an `Accept: audio/x-mpegurl` header
    /// (that Accept exports M3U instead).
    ///
    /// WIRE SHAPE: rows are PlaylistTrack objects, NOT songs
    /// (`model/playlist.go`: {ID, MediaFileID, PlaylistID, embedded
    /// MediaFile}). Go's JSON field-conflict rule makes the emitted `id`
    /// the playlist-POSITION id ("1","2",…) — the real song id is ONLY in
    /// `mediaFileId`. Each row parses via [`NativePlaylistTrackRow::remap`]:
    /// capture both ids, remap the object's `id` to `mediaFileId`, THEN
    /// deserialize to Song. A naive Vec<Song> parse silently corrupts every
    /// id-keyed consumer (artwork prefetch, Get Info, Enter-to-play).
    pub async fn load_playlist_tracks_page(
        &self,
        playlist_id: &str,
        start: u32,
        end: u32,
    ) -> Result<(Vec<crate::types::song::Song>, Option<u32>)> {
        let start_str = start.to_string();
        let end_str = end.to_string();
        let (body, total) = self
            .client
            .get_with_headers(
                &format!("/api/playlist/{playlist_id}/tracks"),
                &[("_start", start_str.as_str()), ("_end", end_str.as_str())],
            )
            .await?;
        let rows: Vec<serde_json::Value> =
            serde_json::from_str(&body).context("Failed to parse playlist tracks response")?;
        let songs = rows
            .into_iter()
            .map(|row| NativePlaylistTrackRow::remap(row).map(|r| r.song))
            .collect::<Result<Vec<_>>>()?;
        Ok((songs, total))
    }

    /// Remove ONE track from a playlist by 1-based position.
    ///
    /// The position id goes in the `id` QUERY parameter of
    /// `DELETE /api/playlist/:playlistId/tracks` — the per-id PATH form
    /// (`/tracks/{id}`) is FORBIDDEN in nokkvi code: Navidrome ignores the
    /// path segment and returns a silent 200 no-op. Always single-id, so a
    /// stale position hits the server's `len(ids)==1` ErrNotFound branch
    /// (`server/nativeapi/playlists.go` deleteFromPlaylist) and 404s
    /// instead of deleting silently. A 200 whose echoed id is
    /// missing/mismatched is treated as failure.
    pub async fn remove_playlist_track_at(&self, playlist_id: &str, position: u32) -> Result<()> {
        let (endpoint, position_str) = remove_track_params(playlist_id, position);
        let body = self
            .client
            .delete_with_params(&endpoint, &[("id", position_str.as_str())])
            .await?;
        if !removal_echo_confirms(&body, position) {
            anyhow::bail!(
                "playlist track removal not confirmed by the server \
                 (echo mismatch for position {position}): {body}"
            );
        }
        Ok(())
    }

    /// Add songs to an existing playlist (append).
    ///
    /// Uses Navidrome native API: POST /api/playlist/:id/tracks
    pub async fn add_songs_to_playlist(
        &self,
        playlist_id: &str,
        song_ids: &[String],
    ) -> Result<()> {
        let body = serde_json::json!({ "ids": song_ids });
        self.client
            .post_json(&format!("/api/playlist/{playlist_id}/tracks"), &body)
            .await?;
        Ok(())
    }

    /// Replace all tracks in a playlist with the given song IDs.
    ///
    /// Uses Subsonic `createPlaylist` overwrite: calling with an existing
    /// `playlistId` fully replaces the track list in a single API call.
    pub async fn replace_playlist_tracks(
        &self,
        playlist_id: &str,
        song_ids: &[String],
    ) -> Result<()> {
        // Build params: playlistId + one songId per track (order-preserving)
        let mut params: Vec<(&str, &str)> = vec![("playlistId", playlist_id)];
        for id in song_ids {
            params.push(("songId", id.as_str()));
        }

        subsonic::subsonic_post_ok(
            &self.client.http_client(),
            &self.server_url,
            "createPlaylist",
            &self.subsonic_credential,
            &params,
            "Replace playlist tracks",
        )
        .await
    }
}

/// Drop nokkvi draft-workspace rows (strict marker parse — see
/// [`DraftMarker::parse`]) from a playlist listing. Applied centrally inside
/// [`PlaylistsApiService::load_playlists_with_libraries`] so every consumer
/// lane inherits it; pure so it unit-tests transport-free.
///
/// [`DraftMarker::parse`]: crate::types::playlist::DraftMarker::parse
fn filter_draft_rows(playlists: Vec<Playlist>) -> Vec<Playlist> {
    playlists.into_iter().filter(|p| !p.is_draft()).collect()
}

/// Project the non-smart playlists to `(id, name)` pairs — the ONE shared
/// source for every add-target picker (the D4 gating rule: the server
/// rejects track mutations on smart playlists with error 50/403, so no
/// picker may offer one). All four add-target fetch sites route through
/// this helper so the filter cannot drift per-site.
pub fn non_smart_name_pairs(playlists: &[Playlist]) -> Vec<(String, String)> {
    playlists
        .iter()
        .filter(|p| !p.is_smart())
        .map(|p| (p.id.clone(), p.name.clone()))
        .collect()
}

/// The quick-add-site variant: `(id, name, is_smart)` triples over the FULL
/// (pre-filter) list. The hotkey quick-add bypass must SEE smart rows to
/// refuse a smart default-playlist with the right toast; its dialog rows
/// are the filtered projection of these triples.
pub fn playlist_add_target_triples(playlists: &[Playlist]) -> Vec<(String, String, bool)> {
    playlists
        .iter()
        .map(|p| (p.id.clone(), p.name.clone(), p.is_smart()))
        .collect()
}

/// How long a foreign draft must sit untouched before the sweep may take
/// it (every draft write refreshes the marker `ts`, so an actively
/// previewing session never ages out).
pub const DRAFT_SWEEP_MIN_AGE_SECS: u64 = 15 * 60;

/// Select the draft-workspace rows the startup orphan sweep should delete.
/// Pure (pid-liveness injected) so both arms unit-test transport-free:
///
/// - **(a) stale-foreign:** marker `ts` older than 15 minutes AND the
///   marker's pid not alive — same-box parallel sessions are protected by
///   the live-pid check (the pid field is load-bearing, not decorative).
/// - **(b) own-orphan:** the marker's pid is OUR pid and no live rules
///   session owns that draft id — the 401 case, where teardown couldn't
///   authorize a DELETE; age-exempt (our pid + no owning session is proof
///   of orphanhood).
///
/// Only strict-parsed markers WITH rules are candidates; malformed markers
/// are never selected (nor filtered — the M1 pin's sweep half).
pub fn select_sweepable_drafts(
    rows: &[Playlist],
    own_pid: u32,
    live_draft_id: Option<&str>,
    now_ts: u64,
    pid_alive: impl Fn(u32) -> bool,
) -> Vec<String> {
    rows.iter()
        .filter(|p| p.rules.as_ref().is_some_and(|r| !r.is_null()))
        .filter_map(|p| {
            let marker = crate::types::playlist::DraftMarker::parse(&p.comment)?;
            let own = marker.pid == own_pid;
            if own {
                // Arm (b): our own orphan — unless a live session owns it.
                if Some(p.id.as_str()) == live_draft_id {
                    return None;
                }
                return Some(p.id.clone());
            }
            // Arm (a): stale AND dead.
            let age = now_ts.saturating_sub(marker.ts);
            if age >= DRAFT_SWEEP_MIN_AGE_SECS && !pid_alive(marker.pid) {
                return Some(p.id.clone());
            }
            None
        })
        .collect()
}

/// Case-insensitive, trimmed duplicate-name lookup: returns the index of
/// the first existing name that matches `candidate`. ONE comparison shared
/// by three surfaces — the create-dialog warning, the rules-session name
/// diagnostic, and the `.nsp` import collision check. Warn-only consumers:
/// a duplicate name is legal server-side, so this never blocks.
pub fn duplicate_playlist_name<'a, I>(candidate: &str, existing: I) -> Option<usize>
where
    I: IntoIterator<Item = &'a str>,
{
    let needle = candidate.trim().to_lowercase();
    if needle.is_empty() {
        return None;
    }
    existing
        .into_iter()
        .position(|name| name.trim().to_lowercase() == needle)
}

/// One remapped row of the native `/api/playlist/{id}/tracks` response.
/// See [`PlaylistsApiService::load_playlist_tracks_page`] for the wire
/// shape this guards against.
pub struct NativePlaylistTrackRow {
    /// The 1-based playlist POSITION id the wire emitted as `id`.
    pub position: u32,
    /// The song, with its `id` remapped to the real `mediaFileId`.
    pub song: crate::types::song::Song,
}

impl NativePlaylistTrackRow {
    /// Capture `id` (position) + `mediaFileId`, remap the object's `id` to
    /// the media-file id, then deserialize to `Song`.
    fn remap(mut row: serde_json::Value) -> Result<Self> {
        let obj = row
            .as_object_mut()
            .context("playlist track row is not an object")?;
        let position = obj
            .get("id")
            .and_then(|v| match v {
                serde_json::Value::String(s) => s.parse::<u32>().ok(),
                serde_json::Value::Number(n) => n.as_u64().map(|n| n as u32),
                _ => None,
            })
            .context("playlist track row has no position id")?;
        let media_file_id = obj
            .get("mediaFileId")
            .and_then(|v| v.as_str())
            .context("playlist track row has no mediaFileId")?
            .to_owned();
        obj.insert("id".to_owned(), serde_json::Value::String(media_file_id));
        let song: crate::types::song::Song = serde_json::from_value(row)
            .context("Failed to deserialize playlist track row as Song")?;
        Ok(Self { position, song })
    }
}

/// Build the wire shape for the single-track removal: `(endpoint,
/// position-id string)`. Pure so the query-param-vs-path-form tripwire
/// unit-tests transport-free (the way `build_playlist_params` is tested).
fn remove_track_params(playlist_id: &str, position: u32) -> (String, String) {
    (
        format!("/api/playlist/{playlist_id}/tracks"),
        position.to_string(),
    )
}

/// Check the DELETE response echo. For a single id Navidrome answers
/// `{"id":"<position>"}` (`writeDeleteManyResponse`, singular form for
/// `len(ids)==1`); the plural `{"ids":["<position>"]}` is accepted
/// defensively. Anything else — empty body, null, wrong id — is a failed
/// confirmation (the silent-no-op tripwire).
fn removal_echo_confirms(body: &str, position: u32) -> bool {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(body) else {
        return false;
    };
    let expect = position.to_string();
    if v.get("id").and_then(|x| x.as_str()) == Some(expect.as_str()) {
        return true;
    }
    v.get("ids")
        .and_then(|x| x.as_array())
        .is_some_and(|ids| ids.len() == 1 && ids[0].as_str() == Some(expect.as_str()))
}

/// Overlay the caller's provided (`Some`) fields onto the current server
/// record; a `None` field keeps the server's current value. This is how
/// [`PlaylistsApiService::update_playlist`] offers "nil-means-unchanged"
/// semantics on top of Navidrome's full-replace PUT.
fn merge_playlist_update<'a>(
    current: &'a Playlist,
    name: Option<&'a str>,
    comment: Option<&'a str>,
    public: Option<bool>,
) -> (&'a str, &'a str, bool) {
    (
        name.unwrap_or(current.name.as_str()),
        comment.unwrap_or(current.comment.as_str()),
        public.unwrap_or(current.public),
    )
}

/// Build the PUT body for `update_playlist` — ALWAYS the full
/// `(name, comment, public)` triple. Navidrome zero-fills any omitted key
/// (see [`PlaylistsApiService::update_playlist`]), so emitting a subset would
/// clear the missing fields server-side. `rules` rides along verbatim when
/// the current record is smart (the 0.61 rename-wipes-rules guard — see the
/// caller); regular playlists never send the key.
fn build_update_playlist_body(
    name: &str,
    comment: &str,
    public: bool,
    rules: Option<&serde_json::Value>,
) -> serde_json::Value {
    let mut body = serde_json::json!({ "name": name, "comment": comment, "public": public });
    if let Some(rules) = rules
        && let Some(map) = body.as_object_mut()
    {
        map.insert("rules".to_owned(), rules.clone());
    }
    body
}

/// Parse a single Subsonic `getPlaylist` entry into a `Song`.
///
/// Subsonic uses different field names than the Navidrome native API
/// (`track` vs `trackNumber`, `channelCount` vs `channels`, `samplingRate`
/// vs `sampleRate`, `userRating` vs `rating`). The canonical `Song` struct
/// declares `#[serde(alias = ...)]` for the Subsonic spellings, so a single
/// `serde_json::from_value` call deserializes both shapes.
fn parse_subsonic_song_entry(entry: serde_json::Value) -> Result<crate::types::song::Song> {
    serde_json::from_value::<crate::types::song::Song>(entry)
        .context("Failed to deserialize Subsonic playlist song entry")
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// The typed envelope navigation shared by `load_playlist_songs` and
    /// `load_playlist_albums`: envelope → playlist → entry, with the
    /// one-or-many quirk absorbed by `deserialize_one_or_many`.
    #[test]
    fn playlist_envelope_extracts_entries_for_array_and_single_object() {
        let array_body = r#"{
            "subsonic-response": {
                "status": "ok",
                "playlist": {
                    "id": "p1",
                    "name": "Mix",
                    "entry": [
                        { "id": "s1", "albumId": "a1" },
                        { "id": "s2", "albumId": "a2" }
                    ]
                }
            }
        }"#;
        let parsed: subsonic::SubsonicEnvelope<PlaylistInner> =
            serde_json::from_str(array_body).expect("array-entry envelope must parse");
        let entry = parsed
            .response
            .playlist
            .expect("playlist key present")
            .entry
            .expect("entry key present");
        let entries: Vec<serde_json::Value> =
            subsonic::deserialize_one_or_many(entry).expect("array fan-out");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0]["id"], "s1");
        assert_eq!(entries[1]["albumId"], "a2");

        // Subsonic's XML→JSON bridge collapses a one-element collection
        // to a bare object — the single-object variant must also extract.
        let single_body = r#"{
            "subsonic-response": {
                "status": "ok",
                "playlist": {
                    "id": "p1",
                    "name": "Mix",
                    "entry": { "id": "s1", "albumId": "a1" }
                }
            }
        }"#;
        let parsed: subsonic::SubsonicEnvelope<PlaylistInner> =
            serde_json::from_str(single_body).expect("single-entry envelope must parse");
        let entry = parsed
            .response
            .playlist
            .expect("playlist key present")
            .entry
            .expect("entry key present");
        let entries: Vec<serde_json::Value> =
            subsonic::deserialize_one_or_many(entry).expect("single-object fan-out");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0]["albumId"], "a1");
    }

    /// Real-shape Subsonic `getPlaylist` entry — uses Subsonic field names
    /// (`track`, `channelCount`, `samplingRate`, `userRating`) which differ
    /// from the canonical Navidrome native names.
    #[test]
    fn parses_subsonic_entry_with_subsonic_field_names() {
        let entry = json!({
            "id": "song-1",
            "title": "Karma Police",
            "artist": "Radiohead",
            "album": "OK Computer",
            "albumId": "album-1",
            "duration": 263,
            "track": 6,
            "discNumber": 1,
            "year": 1997,
            "bitRate": 320,
            "channelCount": 2,
            "samplingRate": 44100,
            "userRating": 4,
            "suffix": "flac",
            "path": "radiohead/ok-computer/06-karma-police.flac",
            "size": 30_000_000_u64,
            "starred": "2024-01-15T12:00:00Z"
        });

        let song = parse_subsonic_song_entry(entry).expect("should parse Subsonic entry");

        assert_eq!(song.id, "song-1");
        assert_eq!(song.title, "Karma Police");
        assert_eq!(
            song.track,
            Some(6),
            "Subsonic `track` must populate Song.track"
        );
        assert_eq!(
            song.channels,
            Some(2),
            "Subsonic `channelCount` must populate Song.channels"
        );
        assert_eq!(
            song.sample_rate,
            Some(44100),
            "Subsonic `samplingRate` must populate Song.sample_rate"
        );
        assert_eq!(
            song.rating,
            Some(4),
            "Subsonic `userRating` must populate Song.rating"
        );
        assert!(song.starred, "non-empty `starred` string must be true");
    }

    /// Empty `starred` string is how Subsonic encodes "not starred" for some
    /// shapes — the canonical `deserialize_starred` returns false for empty
    /// strings, and the playlist parser must agree.
    #[test]
    fn empty_starred_string_is_not_starred() {
        let entry = json!({
            "id": "song-2",
            "title": "No Surprises",
            "artist": "Radiohead",
            "album": "OK Computer",
            "duration": 229,
            "starred": ""
        });

        let song = parse_subsonic_song_entry(entry).expect("should parse entry");
        assert!(
            !song.starred,
            "empty `starred` string must deserialize to false (matches deserialize_starred)"
        );
    }

    /// When Navidrome includes `tags` / `participants` on a playlist entry
    /// (info-modal data), the parser must surface them — the previous
    /// hand-rolled parser hard-coded both to `None`.
    #[test]
    fn tags_and_participants_round_trip_when_present() {
        let entry = json!({
            "id": "song-3",
            "title": "Lucky",
            "artist": "Radiohead",
            "album": "OK Computer",
            "duration": 259,
            "starred": false,
            "tags": {
                "barcode": ["724385522925"],
                "isrc": ["GBAYE9700116"]
            },
            "participants": {
                "composer": [
                    { "id": "p1", "name": "Thom Yorke", "subRole": null }
                ],
                "producer": [
                    { "id": "p2", "name": "Nigel Godrich" }
                ]
            }
        });

        let song = parse_subsonic_song_entry(entry).expect("should parse entry");

        let tags = song.tags.as_ref().expect("tags must be present");
        assert_eq!(
            tags.get("barcode").map(|v| v.as_slice()),
            Some(&["724385522925".to_string()][..])
        );
        assert_eq!(
            tags.get("isrc").map(|v| v.as_slice()),
            Some(&["GBAYE9700116".to_string()][..])
        );

        let participants = song
            .participants
            .as_ref()
            .expect("participants must be present");
        let composers = participants
            .get("composer")
            .expect("composer participants must be present");
        assert_eq!(composers.len(), 1);
        assert_eq!(composers[0].name, "Thom Yorke");
        let producers = participants
            .get("producer")
            .expect("producer participants must be present");
        assert_eq!(producers.len(), 1);
        assert_eq!(producers[0].name, "Nigel Godrich");
    }

    // ---- update_playlist body + merge: Navidrome full-replace contract ----
    //
    // Navidrome's PUT zero-fills any omitted key, so a partial body would CLEAR
    // the missing fields. We instead read-merge: overlay the dirty fields onto
    // the current record and always send the full triple.

    fn playlist_with(name: &str, comment: &str, public: bool) -> Playlist {
        serde_json::from_value(json!({
            "id": "p1",
            "name": name,
            "comment": comment,
            "public": public,
            "updatedAt": "2026-06-06T00:00:00Z",
        }))
        .expect("playlist fixture must deserialize")
    }

    #[test]
    fn build_body_always_sends_full_triple() {
        // The body MUST always carry all three fields — a subset would let
        // Navidrome zero-fill (clear) the omitted name/comment/public.
        let body = build_update_playlist_body("n", "c", true, None);
        let obj = body.as_object().expect("body must be a JSON object");
        assert_eq!(
            obj.len(),
            3,
            "a regular playlist's body is exactly name + comment + public"
        );
        assert_eq!(body["name"], "n");
        assert_eq!(body["comment"], "c");
        assert_eq!(body["public"], json!(true));
    }

    #[test]
    fn merge_rename_keeps_current_comment_and_public() {
        // Regression for the reported data loss: a rename (name = Some, comment
        // and public = None) must PRESERVE the server's current comment and
        // public — never wipe the comment or flip the playlist private.
        let current = playlist_with("Old Name", "keep me", true);
        let (name, comment, public) = merge_playlist_update(&current, Some("New Name"), None, None);
        assert_eq!(name, "New Name");
        assert_eq!(comment, "keep me", "a rename must not clear the comment");
        assert!(public, "a rename must not flip the playlist private");
    }

    #[test]
    fn merge_comment_edit_keeps_current_name() {
        // The mirror case: editing only the comment must preserve the name.
        let current = playlist_with("Keep Name", "old comment", false);
        let (name, comment, public) =
            merge_playlist_update(&current, None, Some("new comment"), None);
        assert_eq!(name, "Keep Name", "a comment edit must not clear the name");
        assert_eq!(comment, "new comment");
        assert!(
            !public,
            "an unspecified public flag must round-trip the current value"
        );
    }

    #[test]
    fn merge_overlays_all_provided_fields() {
        let current = playlist_with("Old", "old c", false);
        let (name, comment, public) =
            merge_playlist_update(&current, Some("New"), Some("new c"), Some(true));
        assert_eq!(name, "New");
        assert_eq!(comment, "new c");
        assert!(public);
    }

    /// Pin the `/api/playlist` browse wire shape: full-set load, search on
    /// `q` — NOT `name`/`title`, which are the other endpoints' search keys.
    #[test]
    fn browse_params_pin_wire_shape_with_q_search() {
        let params = PlaylistsApiService::build_playlist_params("name", "ASC", Some("road"));
        assert_eq!(
            params,
            vec![
                ("_sort", "name"),
                ("_order", "ASC"),
                ("_start", "0"),
                ("_end", pagination::NO_LIMIT_END_STR),
                ("q", "road"),
            ]
        );
    }

    /// `/api/playlist` registers only `q` + `smart` filters
    /// (`reference-navidrome/persistence/playlist_repository.go:48-60`) and
    /// the playlist table has no `library_id` column. The builder's
    /// signature deliberately has no library input, so forwarding one
    /// requires a signature change that breaks this test's call site at
    /// compile time; the assert pins the exact base-only shape.
    #[test]
    fn browse_params_never_emit_library_id() {
        let params = PlaylistsApiService::build_playlist_params("name", "ASC", None);
        assert_eq!(
            params,
            vec![
                ("_sort", "name"),
                ("_order", "ASC"),
                ("_start", "0"),
                ("_end", pagination::NO_LIMIT_END_STR),
            ]
        );
    }

    /// Build a minimal `Playlist` fixture for the helper tests below.
    fn fixture_playlist(id: &str, name: &str, comment: &str, smart: bool) -> Playlist {
        let mut v = serde_json::json!({ "id": id, "name": name, "comment": comment });
        if smart && let Some(map) = v.as_object_mut() {
            map.insert(
                "rules".to_owned(),
                serde_json::json!({ "all": [ { "is": { "loved": true } } ] }),
            );
        }
        serde_json::from_value(v).expect("fixture must deserialize")
    }

    /// The central draft filter drops rows whose comment strict-parses as a
    /// draft marker — and ONLY those. A prefix-only comment survives (the
    /// vanishing-real-playlist hazard the strict grammar exists to prevent).
    #[test]
    fn filter_draft_rows_drops_strict_markers_only() {
        let rows = vec![
            fixture_playlist("p1", "Road Trip", "", false),
            fixture_playlist(
                "p2",
                "nokkvi draft (safe to ignore)",
                "nokkvi-draft/1 pid=4242 ts=1752800000",
                true,
            ),
            // Malformed marker: prefix present, fields unparseable — NOT
            // filtered (and M5 pins it is not swept either).
            fixture_playlist("p3", "My notes", "nokkvi-draft/my notes", false),
            fixture_playlist("p4", "Loved", "", true),
        ];
        let kept = filter_draft_rows(rows);
        let ids: Vec<&str> = kept.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, vec!["p1", "p3", "p4"]);
    }

    /// The ONE shared add-target projection: smart rows excluded, order
    /// preserved. All four add-target fetch sites call this helper, so the
    /// batch Add-to-Playlist dialog cannot re-admit smart rows without
    /// failing this test.
    #[test]
    fn non_smart_name_pairs_filters_smart_rows() {
        let rows = vec![
            fixture_playlist("p1", "Road Trip", "", false),
            fixture_playlist("sp1", "Never Played", "", true),
            fixture_playlist("p2", "Evening", "", false),
        ];
        assert_eq!(
            non_smart_name_pairs(&rows),
            vec![
                ("p1".to_owned(), "Road Trip".to_owned()),
                ("p2".to_owned(), "Evening".to_owned()),
            ]
        );
    }

    /// The quick-add variant keeps smart rows visible (flagged) — the
    /// bypass must SEE a smart default to refuse it with the right toast.
    #[test]
    fn playlist_add_target_triples_keep_smart_rows_flagged() {
        let rows = vec![
            fixture_playlist("p1", "Road Trip", "", false),
            fixture_playlist("sp1", "Never Played", "", true),
        ];
        assert_eq!(
            playlist_add_target_triples(&rows),
            vec![
                ("p1".to_owned(), "Road Trip".to_owned(), false),
                ("sp1".to_owned(), "Never Played".to_owned(), true),
            ]
        );
    }

    /// Duplicate-name detection trims and case-folds; an all-whitespace
    /// candidate never matches (empty names are a separate Error lane).
    #[test]
    fn duplicate_playlist_name_trims_and_case_folds() {
        let existing = ["Road Trip", "Evening Chill"];
        assert_eq!(
            duplicate_playlist_name("  road trip ", existing.iter().copied()),
            Some(0)
        );
        assert_eq!(
            duplicate_playlist_name("EVENING CHILL", existing.iter().copied()),
            Some(1)
        );
        assert_eq!(
            duplicate_playlist_name("Fresh Name", existing.iter().copied()),
            None
        );
        assert_eq!(
            duplicate_playlist_name("   ", existing.iter().copied()),
            None
        );
    }

    /// Wire-pin: the update PUT body includes the current rules verbatim
    /// when the target is smart — the 0.61 rename-wipes-rules guard. A
    /// regular playlist's body must NOT carry the key (sending `rules`
    /// where none existed would convert it server-side).
    #[test]
    fn update_body_carries_rules_for_smart_current_only() {
        let rules = serde_json::json!({ "all": [ { "is": { "loved": true } } ] });
        let with_rules = build_update_playlist_body("Mix", "c", true, Some(&rules));
        assert_eq!(with_rules["name"], "Mix");
        assert_eq!(with_rules["comment"], "c");
        assert_eq!(with_rules["public"], true);
        assert_eq!(with_rules["rules"], rules);

        let without = build_update_playlist_body("Mix", "c", true, None);
        assert!(without.get("rules").is_none());
    }

    /// The orphan sweep's selector matrix: arm (a) takes stale-AND-dead
    /// foreign markers only; arm (b) takes own-pid markers with no owning
    /// live session (age-exempt); malformed markers and marker-less rows
    /// are NEVER selected.
    #[test]
    fn sweep_selector_arms_matrix() {
        let now = 2_000_000u64;
        let fresh_ts = now - 60; // 1 min old
        let stale_ts = now - 3600; // 1 h old
        let draft = |id: &str, pid: u32, ts: u64| {
            let mut p = fixture_playlist(id, "nokkvi draft (safe to ignore)", "", true);
            p.comment = crate::types::playlist::DraftMarker::format(1, pid, ts);
            p
        };
        let rows = vec![
            draft("own-live", 42, fresh_ts),           // ours, owned by session
            draft("own-orphan", 42, fresh_ts),         // ours, no session → (b)
            draft("foreign-stale-dead", 7, stale_ts),  // (a)
            draft("foreign-stale-alive", 8, stale_ts), // alive pid → keep
            draft("foreign-fresh-dead", 9, fresh_ts),  // too fresh → keep
            {
                // Malformed marker: prefix present, unparseable — never
                // selected.
                let mut p = fixture_playlist("malformed", "x", "nokkvi-draft/notes", true);
                p.comment = "nokkvi-draft/notes".to_owned();
                p
            },
            {
                // Strict marker but NO rules — not a draft candidate.
                let mut p = fixture_playlist("no-rules", "x", "", false);
                p.comment = crate::types::playlist::DraftMarker::format(1, 7, stale_ts);
                p
            },
        ];
        let alive = |pid: u32| pid == 8 || pid == 42;
        let selected = select_sweepable_drafts(&rows, 42, Some("own-live"), now, alive);
        assert_eq!(
            selected,
            vec!["own-orphan".to_owned(), "foreign-stale-dead".to_owned()],
            "exactly arm (b) + arm (a) rows are taken"
        );

        // With no live session, our fresh drafts are all orphans.
        let selected = select_sweepable_drafts(&rows, 42, None, now, alive);
        assert!(selected.contains(&"own-live".to_owned()));
    }

    /// The native tracks-row remap tripwire: a row carrying both the
    /// position `id` ("1") and the real song id in `mediaFileId` must come
    /// out as a Song whose id IS the media-file id, with the position
    /// retained on the row. A naive Vec<Song> parse would silently key
    /// every consumer (artwork, Get Info, Enter-to-play) on positions.
    #[test]
    fn native_track_row_remaps_media_file_id() {
        let row = json!({
            "id": "1",
            "mediaFileId": "abc123",
            "playlistId": "pl-1",
            "title": "Karma Police",
            "artist": "Radiohead",
            "album": "OK Computer",
            "albumId": "album-1",
            "duration": 263.2
        });
        let remapped = NativePlaylistTrackRow::remap(row).expect("fixture must remap");
        assert_eq!(remapped.position, 1, "the wire id is the position");
        assert_eq!(
            remapped.song.id, "abc123",
            "the song id must be the mediaFileId, never the position"
        );
        assert_eq!(remapped.song.title, "Karma Police");
    }

    /// The smart-create POST body carries name/comment/public/rules — and
    /// the full-state PUT adds `sync` only when a detach is requested.
    #[test]
    fn put_full_body_carries_sync_only_when_requested() {
        let rules = json!({ "all": [ { "is": { "loved": true } } ] });
        let base = build_update_playlist_body("Mix", "c", false, Some(&rules));
        assert!(base.get("sync").is_none());

        // put_playlist_full assembles base + sync — mirror its insertion.
        let mut with_sync = base.clone();
        with_sync
            .as_object_mut()
            .expect("object")
            .insert("sync".to_owned(), serde_json::Value::Bool(false));
        assert_eq!(with_sync["sync"], json!(false));
        assert_eq!(with_sync["rules"], rules);
        assert_eq!(with_sync["name"], "Mix");
    }

    /// The single-track removal wire shape: position id rides the `id`
    /// QUERY parameter — the endpoint carries NO trailing id segment. The
    /// path form (`/tracks/{id}`) is a silent server-side no-op, so this
    /// pin is the tripwire against regressing into it.
    #[test]
    fn remove_track_wire_shape_is_query_param_not_path() {
        let (endpoint, position) = remove_track_params("pl-1", 3);
        assert_eq!(endpoint, "/api/playlist/pl-1/tracks");
        assert!(
            !endpoint.contains("/tracks/"),
            "the per-id path form is forbidden — Navidrome silently no-ops it"
        );
        assert_eq!(position, "3");
    }

    /// The echo check accepts the server's singular single-id form (and the
    /// plural defensively) and fails everything else — empty body, null,
    /// mismatched id — so a 200 no-op can never read as success.
    #[test]
    fn removal_echo_check_matrix() {
        assert!(removal_echo_confirms(r#"{"id":"3"}"#, 3));
        assert!(removal_echo_confirms(r#"{"ids":["3"]}"#, 3));
        for bad in [
            "",
            "null",
            "{}",
            r#"{"id":"4"}"#,
            r#"{"id":null}"#,
            r#"{"ids":[]}"#,
            r#"{"ids":["3","4"]}"#,
            r#"{"ids":null}"#,
        ] {
            assert!(
                !removal_echo_confirms(bad, 3),
                "must reject echo body: {bad:?}"
            );
        }
    }

    /// OpenSubsonic `readonly` parse off the Subsonic playlist envelope:
    /// present-true, present-false, and pre-0.61 absent forms.
    #[test]
    fn subsonic_playlist_readonly_parses_all_forms() {
        let parse = |body: &str| -> Option<bool> {
            let parsed: subsonic::SubsonicEnvelope<PlaylistInner> =
                serde_json::from_str(body).expect("envelope must parse");
            parsed
                .response
                .playlist
                .expect("playlist key present")
                .readonly
        };
        let with = |ro: &str| {
            format!(
                r#"{{ "subsonic-response": {{ "status": "ok",
                     "playlist": {{ "id": "p1", "name": "Mix"{ro} }} }} }}"#
            )
        };
        assert_eq!(parse(&with(r#", "readonly": true"#)), Some(true));
        assert_eq!(parse(&with(r#", "readonly": false"#)), Some(false));
        assert_eq!(parse(&with("")), None);
    }
}
