use anyhow::{Context, Result};
use serde::Deserialize;

use crate::{services::api::client::ApiClient, types::song::Song};

pub struct SongsApiService {
    client: ApiClient,
}

impl SongsApiService {
    pub fn new(client: ApiClient, _server_url: String, _subsonic_credential: String) -> Self {
        Self { client }
    }

    /// Load all songs with sorting, filtering, and pagination
    ///
    /// # Arguments
    /// * `sort_mode` - Sort/filter type: "recentlyAdded", "random", "title", etc
    /// * `sort_order` - "ASC" or "DESC"
    /// * `search_query` - Optional search term
    /// * `offset` - Starting position for pagination
    /// * `limit` - Maximum number of songs to return (None = 500)
    pub async fn load_songs(
        &self,
        sort_mode: &str,
        sort_order: &str,
        search_query: Option<&str>,
        filter: Option<&crate::types::filter::LibraryFilter>,
        offset: Option<usize>,
        limit: Option<usize>,
    ) -> Result<(Vec<Song>, usize)> {
        let sort_param = Self::map_sort_mode_to_sort_param(sort_mode);
        let order = if sort_order.is_empty() {
            Self::get_default_order(sort_mode)
        } else {
            sort_order
        };

        let offset_val = offset.unwrap_or(0);
        let limit_val = limit.unwrap_or(50000); // No practical limit
        let start = offset_val.to_string();
        let end = (offset_val + limit_val).to_string();

        let mut params: Vec<(&str, &str)> = vec![
            ("_sort", sort_param),
            ("_order", order),
            ("_start", &start),
            ("_end", &end),
        ];

        // Apply ID filter if present
        let title_search: String;
        if let Some(f) = filter {
            match f {
                crate::types::filter::LibraryFilter::ArtistId { id, .. } => {
                    params.push(("artists_id", id));
                }
                crate::types::filter::LibraryFilter::GenreId { name, .. } => {
                    params.push(("genre_id", name));
                }
                crate::types::filter::LibraryFilter::AlbumId { id, .. } => {
                    params.push(("album_id", id));
                }
            }
        } else if let Some(query) = search_query
            && !query.is_empty()
        {
            title_search = query.to_string();
            params.push(("title", &title_search));
        }

        // Add starred filter for favorited view
        if sort_mode == "favorited" {
            params.push(("starred", "true"));
        }

        let response_text = self
            .client
            .get_with_headers("/api/song", &params)
            .await
            .context("Failed to fetch songs from API")?;

        let (body, total_count) =
            Self::parse_response_with_total(&response_text.0, response_text.1)?;

        Ok((body, total_count))
    }

    /// Load a single song by its ID
    pub async fn load_song_by_id(&self, song_id: &str) -> Result<Song> {
        let params = vec![("id", song_id), ("_start", "0"), ("_end", "1")];

        let response_text = self
            .client
            .get("/api/song", &params)
            .await
            .context("Failed to fetch song by ID from API")?;

        let songs = Self::parse_song_response(&response_text)?;
        songs
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("Song not found: {song_id}"))
    }

    /// Load songs for an album
    pub async fn load_album_songs(&self, album_id: &str) -> Result<Vec<Song>> {
        // Use "_sort=album" to leverage Navidrome's sort mapping which expands to:
        // "order_album_name, album_id, disc_number, track_number, order_artist_name, title"
        // The raw "_sort=disc_number,track_number" was silently dropped by Navidrome's
        // sanitizeSort because it treats comma-separated values as a single field name.
        let params = vec![
            ("album_id", album_id),
            ("_sort", "album"),
            ("_order", "ASC"),
            ("_start", "0"),
            ("_end", "500"),
        ];

        let response_text = self
            .client
            .get("/api/song", &params)
            .await
            .context("Failed to fetch album songs from API")?;

        let mut songs = Self::parse_song_response(&response_text)?;

        // Sort songs by disc number and track number
        songs.sort_by(|a, b| {
            let disc_a = a.disc.unwrap_or(1);
            let disc_b = b.disc.unwrap_or(1);
            if disc_a != disc_b {
                disc_a.cmp(&disc_b)
            } else {
                let track_a = a.track.unwrap_or(0);
                let track_b = b.track.unwrap_or(0);
                track_a.cmp(&track_b)
            }
        });

        Ok(songs)
    }

    /// Load all songs for a specific genre
    ///
    /// # Arguments
    /// * `genre_name` - The genre name to filter by
    pub async fn load_songs_by_genre(&self, genre_name: &str) -> Result<(Vec<Song>, usize)> {
        let genre_filter = genre_name.to_string();
        let params = vec![
            ("genre", genre_filter.as_str()),
            ("_sort", "album"),
            ("_order", "ASC"),
            ("_start", "0"),
            ("_end", "50000"), // No practical limit
        ];

        let response_text = self
            .client
            .get_with_headers("/api/song", &params)
            .await
            .context("Failed to fetch genre songs from API")?;

        let (songs, total_count) =
            Self::parse_response_with_total(&response_text.0, response_text.1)?;

        Ok((songs, total_count))
    }

    /// Map sort mode to Navidrome API sort parameter
    fn map_sort_mode_to_sort_param(sort_mode: &str) -> &'static str {
        match sort_mode {
            "recentlyAdded" => "createdAt",
            "recentlyPlayed" => "playDate",
            "mostPlayed" => "playCount",
            "favorited" => "starred_at",
            "random" => "random",
            "title" => "title",
            "album" => "album",
            "artist" => "artist",
            "albumArtist" => "order_album_artist_name",
            "year" => "year",
            "duration" => "duration",
            "bpm" => "bpm",
            "channels" => "channels",
            "genre" => "genre",
            "rating" => "rating",
            "comment" => "comment",
            _ => "createdAt",
        }
    }

    /// Get default sort order for sort mode
    fn get_default_order(sort_mode: &str) -> &'static str {
        match sort_mode {
            "recentlyAdded" | "recentlyPlayed" | "mostPlayed" | "favorited" | "year"
            | "duration" | "bpm" | "channels" | "rating" => "DESC",
            _ => "ASC",
        }
    }

    /// Parse response that may be array or object with content
    fn parse_song_response(response_text: &str) -> Result<Vec<Song>> {
        #[derive(Deserialize)]
        struct SongResponse {
            content: Option<Vec<Song>>,
        }

        // Try parsing as array first
        if let Ok(songs) = serde_json::from_str::<Vec<Song>>(response_text) {
            return Ok(songs);
        }

        // Try parsing as object with content array
        if let Ok(response) = serde_json::from_str::<SongResponse>(response_text)
            && let Some(songs) = response.content
        {
            return Ok(songs);
        }

        let preview = response_text.chars().take(300).collect::<String>();
        Err(anyhow::anyhow!("Failed to parse songs response: {preview}"))
    }

    /// Parse response with total count from headers
    fn parse_response_with_total(
        response_text: &str,
        total_header: Option<u32>,
    ) -> Result<(Vec<Song>, usize)> {
        let songs = Self::parse_song_response(response_text)?;
        let total = total_header.map_or(songs.len(), |t| t as usize);
        Ok((songs, total))
    }
}
