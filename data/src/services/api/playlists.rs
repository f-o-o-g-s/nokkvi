//! Playlists API Service
//!
//! Handles playlist-related API calls to Navidrome server.

use anyhow::{Context, Result};
use tracing::{debug, warn};

use crate::{
    services::api::{
        client::ApiClient,
        pagination,
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
        let default_order = sort::default_order(SortDomain::Playlists, actual_sort_mode);
        let order_param = if sort_order.is_empty() {
            default_order
        } else {
            sort_order
        };

        // Build query parameters for native API
        let mut params = vec![
            ("_sort", sort_param),
            ("_order", order_param),
            ("_start", "0"),
            ("_end", pagination::NO_LIMIT_END_STR),
        ];

        // Add search query if provided (playlists use "q" parameter)
        let search_query_string: String;
        if let Some(query) = search_query
            && !query.is_empty()
        {
            search_query_string = query.to_string();
            params.push(("q", &search_query_string));
        }

        // Fetch from native API
        let result = self.client.get_with_headers("/api/playlist", &params).await;

        let mut playlists: Vec<Playlist> = match result {
            Ok((response_text, _)) => serde_json::from_str(&response_text).unwrap_or_default(),
            Err(e) => {
                warn!(" PlaylistsApiService: Native API failed: {}", e);
                Vec::new()
            }
        };

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

    /// Load songs from a playlist (for playback)
    /// Returns full Song objects
    pub async fn load_playlist_songs(
        &self,
        playlist_id: &str,
    ) -> Result<Vec<crate::types::song::Song>> {
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

        // Missing playlist/entry keys yield Ok(empty) — a playlist with
        // zero entries must not error.
        if let Some(playlist) = inner.playlist
            && let Some(entry_value) = playlist.entry
        {
            // Subsonic returns a single object instead of a one-element
            // array; `deserialize_one_or_many` absorbs that quirk.
            let entries: Vec<serde_json::Value> = subsonic::deserialize_one_or_many(entry_value)?;

            for entry in entries {
                let song = parse_subsonic_song_entry(entry)?;
                songs.push(song);
            }
        }

        debug!(
            " PlaylistsService: Loaded {} songs from playlist {}",
            songs.len(),
            playlist_id
        );

        Ok(songs)
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
        song_ids: &[String],
        public: bool,
    ) -> Result<String> {
        // Step 1: Create the playlist
        let body = serde_json::json!({ "name": name, "public": public });
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
        let body = build_update_playlist_body(name, comment, public);
        self.client
            .put_json(&format!("/api/playlist/{playlist_id}"), &body)
            .await?;
        Ok(())
    }

    /// Fetch a single playlist's current metadata.
    ///
    /// Uses Navidrome native API: GET /api/playlist/:id
    pub(crate) async fn get_playlist(&self, playlist_id: &str) -> Result<Playlist> {
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
/// clear the missing fields server-side.
fn build_update_playlist_body(name: &str, comment: &str, public: bool) -> serde_json::Value {
    serde_json::json!({ "name": name, "comment": comment, "public": public })
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
        let body = build_update_playlist_body("n", "c", true);
        let obj = body.as_object().expect("body must be a JSON object");
        assert_eq!(
            obj.len(),
            3,
            "must always send exactly name + comment + public"
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
}
