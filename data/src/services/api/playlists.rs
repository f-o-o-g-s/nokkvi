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

/// Subsonic API response for getPlaylist (with songs)
#[derive(Debug, serde::Deserialize)]
struct SubsonicPlaylistResponse {
    #[serde(rename = "subsonic-response")]
    subsonic_response: SubsonicResponseInner,
}

#[derive(Debug, serde::Deserialize)]
struct SubsonicResponseInner {
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

    /// Load playlists from the API
    ///
    /// sort_mode: Sort mode (name, songCount, duration, updatedAt, random)
    /// sort_order: Sort order (ASC or DESC)
    /// search_query: Optional search query
    ///
    /// Shim that forwards an empty `library_ids` slice — preserved for
    /// existing UI handler call sites. New library-aware code paths
    /// should call [`load_playlists_with_libraries`] directly (even
    /// though the parameter is currently a no-op for this endpoint, see
    /// that method's doc-comment).
    pub async fn load_playlists(
        &self,
        sort_mode: &str,
        sort_order: &str,
        search_query: Option<&str>,
    ) -> Result<(Vec<Playlist>, u32)> {
        self.load_playlists_with_libraries(sort_mode, sort_order, search_query, &[])
            .await
    }

    /// Library-aware variant of [`load_playlists`].
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
        let is_random = sort_mode == "random";
        let actual_sort_mode = if is_random { "name" } else { sort_mode };

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
        let response = subsonic::subsonic_post(
            &self.client.http_client(),
            &self.server_url,
            "getPlaylist",
            &self.subsonic_credential,
            &[("id", playlist_id)],
        )
        .await
        .context("Failed to fetch playlist songs from Subsonic API")?;

        let body = response
            .text()
            .await
            .context("Failed to read Subsonic response")?;

        let parsed: serde_json::Value =
            parse::parse_json_with_preview(&body, "Subsonic playlist response")?;

        let mut songs = Vec::new();

        // Navigate to subsonic-response.playlist.entry
        if let Some(entry_value) = parsed
            .get("subsonic-response")
            .and_then(|sr| sr.get("playlist"))
            .and_then(|pl| pl.get("entry"))
        {
            // Handle both array and single object cases
            let entries: Vec<serde_json::Value> = if entry_value.is_array() {
                entry_value.as_array().cloned().unwrap_or_default()
            } else {
                vec![entry_value.clone()]
            };

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
        let response = subsonic::subsonic_post(
            &self.client.http_client(),
            &self.server_url,
            "getPlaylist",
            &self.subsonic_credential,
            &[("id", playlist_id)],
        )
        .await
        .context("Failed to fetch playlist for album IDs")?;

        let body = response
            .text()
            .await
            .context("Failed to read Subsonic response")?;

        let parsed: SubsonicPlaylistResponse = serde_json::from_str(&body)
            .with_context(|| "Failed to parse Subsonic playlist response".to_string())?;

        let mut album_ids = Vec::new();
        let mut seen = std::collections::HashSet::new();

        if let Some(playlist) = parsed.subsonic_response.playlist
            && let Some(entry_value) = playlist.entry
        {
            // Handle both array and single object cases
            let entries: Vec<SubsonicSongEntry> = if entry_value.is_array() {
                serde_json::from_value(entry_value)?
            } else {
                vec![serde_json::from_value(entry_value)?]
            };

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

    /// Update a playlist's metadata, sending ONLY the dirty fields.
    ///
    /// Navidrome's native API follows a nil-means-leave-unchanged contract
    /// (see navidrome `playlists.go`): an absent key leaves that field as-is.
    /// Each of `name` / `comment` / `public` is therefore put on the wire only
    /// when `Some`, so a comment-only edit no longer re-writes the name and no
    /// longer replays a stale `public` flag (which could silently revert a
    /// concurrent visibility change).
    ///
    /// NOTE: the historical "always send `public`" behavior — kept as a 403
    /// probe so a non-owner edit surfaces an error rather than silently
    /// succeeding — is retained ONLY for the rename path
    /// (`update/text_input_dialog.rs`), which has no public-dirty concept and
    /// passes `public: Some(current_public)` deliberately.
    ///
    /// Uses Navidrome native API: PUT /api/playlist/:id
    pub async fn update_playlist(
        &self,
        playlist_id: &str,
        name: Option<&str>,
        comment: Option<&str>,
        public: Option<bool>,
    ) -> Result<()> {
        let body = build_update_playlist_body(name, comment, public);
        self.client
            .put_json(&format!("/api/playlist/{playlist_id}"), &body)
            .await?;
        Ok(())
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
        let body = self
            .client
            .get(&format!("/api/playlist/{playlist_id}"), &[])
            .await?;
        let playlist: Playlist = serde_json::from_str(&body)
            .context("Failed to deserialize playlist metadata for staleness check")?;
        Ok(playlist.updated_at)
    }

    /// Delete a playlist.
    ///
    /// Uses Navidrome native API: DELETE /api/playlist/:id
    pub async fn delete_playlist(&self, playlist_id: &str) -> Result<()> {
        self.client
            .delete(&format!("/api/playlist/{playlist_id}"))
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

/// Build the PUT body for `update_playlist`, inserting each metadata key only
/// when it is dirty (`Some`).
///
/// An omitted (`None`) field is left out of the JSON object entirely so
/// Navidrome leaves it unchanged — never emitted as a `null`, which Navidrome
/// would treat differently from an absent key.
fn build_update_playlist_body(
    name: Option<&str>,
    comment: Option<&str>,
    public: Option<bool>,
) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    if let Some(name) = name {
        map.insert("name".to_string(), serde_json::Value::from(name));
    }
    if let Some(comment) = comment {
        map.insert("comment".to_string(), serde_json::Value::from(comment));
    }
    if let Some(public) = public {
        map.insert("public".to_string(), serde_json::Value::from(public));
    }
    serde_json::Value::Object(map)
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

    // ---- update_playlist body: send only dirty fields (N21) ----

    #[test]
    fn build_body_omits_clean_fields() {
        // A comment-only edit must put ONLY the comment on the wire — no `name`
        // (so a clean name is not re-written) and no `public` (so a concurrent
        // visibility change is not silently reverted).
        let body = build_update_playlist_body(None, Some("c"), None);
        assert!(
            body.get("name").is_none(),
            "a clean name must be omitted entirely"
        );
        assert!(
            body.get("public").is_none(),
            "a clean public flag must be omitted entirely (no silent revert)"
        );
        assert_eq!(body["comment"], "c", "the dirty comment must be present");
    }

    #[test]
    fn build_body_includes_dirty_public() {
        // When the public flag is dirty it must be emitted with its value.
        let body = build_update_playlist_body(None, None, Some(false));
        assert_eq!(
            body.get("public"),
            Some(&json!(false)),
            "a dirty public flag must be emitted with its (false) value"
        );
        assert!(body.get("name").is_none());
        assert!(body.get("comment").is_none());
    }

    #[test]
    fn build_body_never_emits_null_for_omitted_field() {
        // An omitted field must be ABSENT, never present-as-null — Navidrome
        // treats a present null differently from an absent key.
        let body = build_update_playlist_body(Some("n"), None, None);
        let obj = body.as_object().expect("body must be a JSON object");
        assert_eq!(obj.len(), 1, "only the single dirty field may be present");
        assert!(
            obj.values().all(|v| !v.is_null()),
            "no field may be emitted as a JSON null"
        );
    }

    #[test]
    fn build_body_includes_all_when_all_dirty() {
        let body = build_update_playlist_body(Some("n"), Some("c"), Some(true));
        assert_eq!(body["name"], "n");
        assert_eq!(body["comment"], "c");
        assert_eq!(body["public"], json!(true));
    }
}
