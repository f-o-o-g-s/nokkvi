//! Playlists API Service
//!
//! Handles playlist-related API calls to Navidrome server.

use std::sync::Arc;

use anyhow::{Context, Result};
use rand::seq::SliceRandom;
use tracing::{debug, warn};

use crate::{
    services::api::{client::ApiClient, subsonic},
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

pub struct PlaylistsApiService {
    client: Arc<ApiClient>,
    server_url: String,
    subsonic_credential: String,
}

impl PlaylistsApiService {
    /// Create with a pre-authenticated ApiClient
    pub fn new_with_client(
        client: ApiClient,
        server_url: String,
        subsonic_credential: String,
    ) -> Self {
        Self {
            client: Arc::new(client),
            server_url,
            subsonic_credential,
        }
    }

    /// Load playlists from the API
    ///
    /// sort_mode: Sort mode (name, songCount, duration, updatedAt, random)
    /// sort_order: Sort order (ASC or DESC)
    /// search_query: Optional search query
    pub async fn load_playlists(
        &self,
        sort_mode: &str,
        sort_order: &str,
        search_query: Option<&str>,
    ) -> Result<(Vec<Playlist>, u32)> {
        // For random view, we load by name and shuffle client-side
        let is_random = sort_mode == "random";
        let actual_sort_mode = if is_random { "name" } else { sort_mode };

        // Map viewType to API sort parameter
        let sort_param = Self::map_sort_mode_to_sort_param(actual_sort_mode);
        let default_order = Self::get_default_order(actual_sort_mode);
        let order_param = if sort_order.is_empty() {
            default_order
        } else {
            sort_order
        };

        // Build query parameters for native API
        let mut params = vec![
            ("_sort", sort_param.as_str()),
            ("_order", order_param),
            ("_start", "0"),
            ("_end", "999999"),
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
            let mut rng = rand::rng();
            playlists.shuffle(&mut rng);
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

        let parsed: serde_json::Value = serde_json::from_str(&body).with_context(|| {
            format!(
                "Failed to parse Subsonic playlist response: {}",
                &body[..body.len().min(200)]
            )
        })?;

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
        let response = self.client.post_json("api/playlist", &body).await?;

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
                .post_json(&format!("api/playlist/{playlist_id}/tracks"), &tracks_body)
                .await?;
        }

        Ok(playlist_id)
    }

    /// Update a playlist's name, optional comment, and visibility.
    ///
    /// `public` is always sent on the wire so non-owner edits surface as a 403
    /// rather than silently flipping visibility from a partial-update default.
    ///
    /// Uses Navidrome native API: PUT /api/playlist/:id
    pub async fn update_playlist(
        &self,
        playlist_id: &str,
        name: &str,
        comment: Option<&str>,
        public: bool,
    ) -> Result<()> {
        let body = match comment {
            Some(c) => serde_json::json!({ "name": name, "comment": c, "public": public }),
            None => serde_json::json!({ "name": name, "public": public }),
        };
        self.client
            .put_json(&format!("api/playlist/{playlist_id}"), &body)
            .await?;
        Ok(())
    }

    /// Delete a playlist.
    ///
    /// Uses Navidrome native API: DELETE /api/playlist/:id
    pub async fn delete_playlist(&self, playlist_id: &str) -> Result<()> {
        self.client
            .delete(&format!("api/playlist/{playlist_id}"))
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
            .post_json(&format!("api/playlist/{playlist_id}/tracks"), &body)
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

    // =========================================================================
    // Sort Helpers
    // =========================================================================

    /// Map viewType to sort parameter
    fn map_sort_mode_to_sort_param(sort_mode: &str) -> String {
        match sort_mode {
            "name" => "name".to_string(),
            "songCount" => "song_count".to_string(),
            "duration" => "duration".to_string(),
            "updatedAt" => "updated_at".to_string(),
            "random" => "name".to_string(), // Random is handled client-side
            _ => "name".to_string(),
        }
    }

    /// Get default sort order for sort mode
    fn get_default_order(sort_mode: &str) -> &'static str {
        match sort_mode {
            "songCount" | "duration" | "updatedAt" => "DESC",
            _ => "ASC",
        }
    }
}

impl Clone for PlaylistsApiService {
    fn clone(&self) -> Self {
        Self {
            client: self.client.clone(),
            server_url: self.server_url.clone(),
            subsonic_credential: self.subsonic_credential.clone(),
        }
    }
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
}
