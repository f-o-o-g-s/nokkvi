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
                // Parse each entry as a Song matching Song model fields
                let song = crate::types::song::Song {
                    id: entry
                        .get("id")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string(),
                    title: entry
                        .get("title")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string(),
                    artist: entry
                        .get("artist")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string(),
                    artist_id: entry
                        .get("artistId")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string()),
                    album: entry
                        .get("album")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string(),
                    album_id: entry
                        .get("albumId")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string()),
                    cover_art: entry
                        .get("coverArt")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string()),
                    duration: entry.get("duration").and_then(|v| v.as_i64()).unwrap_or(0) as u32,
                    track: entry
                        .get("track")
                        .and_then(|v| v.as_i64())
                        .map(|v| v as u32),
                    disc: entry
                        .get("discNumber")
                        .and_then(|v| v.as_i64())
                        .map(|v| v as u32),
                    year: entry.get("year").and_then(|v| v.as_i64()).map(|v| v as u32),
                    genre: entry
                        .get("genre")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string()),
                    path: entry
                        .get("path")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string(),
                    size: entry.get("size").and_then(|v| v.as_u64()).unwrap_or(0),
                    bitrate: entry
                        .get("bitRate")
                        .and_then(|v| v.as_i64())
                        .map(|v| v as u32),
                    starred: entry.get("starred").and_then(|v| v.as_str()).is_some(),
                    play_count: entry
                        .get("playCount")
                        .and_then(|v| v.as_i64())
                        .map(|v| v as u32),
                    bpm: entry.get("bpm").and_then(|v| v.as_i64()).map(|v| v as u32),
                    channels: entry
                        .get("channelCount")
                        .and_then(|v| v.as_i64())
                        .map(|v| v as u32),
                    comment: entry
                        .get("comment")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string()),
                    rating: entry
                        .get("userRating")
                        .and_then(|v| v.as_i64())
                        .map(|v| v as u32),
                    album_artist: entry
                        .get("albumArtist")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string()),
                    suffix: entry
                        .get("suffix")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string()),
                    sample_rate: entry
                        .get("samplingRate")
                        .and_then(|v| v.as_i64())
                        .map(|v| v as u32),
                    created_at: entry
                        .get("createdAt")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string()),
                    play_date: entry
                        .get("playDate")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string()),
                    compilation: entry.get("compilation").and_then(|v| v.as_bool()),
                    bit_depth: entry
                        .get("bitDepth")
                        .and_then(|v| v.as_i64())
                        .map(|v| v as u32),
                    updated_at: entry
                        .get("updatedAt")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string()),
                    replay_gain: entry
                        .get("replayGain")
                        .map(|rg| crate::types::song::ReplayGain {
                            album_gain: rg.get("albumGain").and_then(|v| v.as_f64()),
                            track_gain: rg.get("trackGain").and_then(|v| v.as_f64()),
                            album_peak: rg.get("albumPeak").and_then(|v| v.as_f64()),
                            track_peak: rg.get("trackPeak").and_then(|v| v.as_f64()),
                        }),
                    original_position: None, // Set by QueueManager::set_queue/add_songs
                    tags: None,
                    participants: None,
                };
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

    /// Create a new playlist with the given name and optional songs.
    ///
    /// Uses Navidrome native API: POST /api/playlist + POST /api/playlist/:id/tracks
    pub async fn create_playlist(&self, name: &str, song_ids: &[String]) -> Result<String> {
        // Step 1: Create the playlist
        let body = serde_json::json!({ "name": name });
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

    /// Update a playlist's name and/or comment.
    ///
    /// Uses Navidrome native API: PUT /api/playlist/:id
    pub async fn update_playlist(
        &self,
        playlist_id: &str,
        name: &str,
        comment: Option<&str>,
    ) -> Result<()> {
        let body = if let Some(c) = comment {
            serde_json::json!({ "name": name, "comment": c })
        } else {
            serde_json::json!({ "name": name })
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
