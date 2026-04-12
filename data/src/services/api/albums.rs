use std::sync::Arc;

use anyhow::{Context, Result};
use tracing::{debug, trace};

use crate::{services::api::client::ApiClient, types::album::Album};

pub struct AlbumsApiService {
    client: Arc<ApiClient>,
    server_url: String,
    subsonic_credential: String,
}

impl AlbumsApiService {
    pub fn new(client: ApiClient, server_url: String, subsonic_credential: String) -> Self {
        Self {
            client: Arc::new(client),
            server_url,
            subsonic_credential,
        }
    }

    /// Get the HTTP client for making raw requests (e.g., image downloads)
    pub fn get_http_client(&self) -> Arc<reqwest::Client> {
        self.client.http_client()
    }

    /// Load albums from the API
    /// sort_mode: Sort mode (recentlyAdded, recentlyPlayed, mostPlayed, favorited, random, name, albumArtist, artist, year, songCount, duration, rating, genre)
    /// sort_order: Sort order (ASC or DESC)
    /// search_query: Optional search query
    pub async fn load_albums(
        &self,
        sort_mode: &str,
        sort_order: &str,
        search_query: Option<&str>,
        filter: Option<&crate::types::filter::LibraryFilter>,
        offset: Option<usize>,
        limit: Option<usize>,
    ) -> Result<(Vec<Album>, u32)> {
        // Map viewType to API sort parameter
        let sort_param = Self::map_sort_mode_to_sort_param(sort_mode);
        let default_order = Self::get_default_order(sort_mode);
        let order_param = if sort_order.is_empty() {
            default_order
        } else {
            sort_order
        };

        // Build query parameters
        let mut params = vec![
            ("_sort", sort_param.as_str()),
            ("_order", order_param),
            ("filter", "{}"),
        ];

        // Add pagination parameters (Navidrome uses _start and _end, not _offset and _limit)
        // For unlimited: use a very large number (999999) as the end value
        let offset_value = offset.unwrap_or(0);
        let limit_value = limit.unwrap_or(999999); // Use very large number for "unlimited"
        let start_value = offset_value;
        let end_value = offset_value + limit_value;
        let start_str = start_value.to_string();
        let end_str = end_value.to_string();
        params.push(("_start", &start_str));
        params.push(("_end", &end_str));

        // Apply ID filter if present
        if let Some(f) = filter {
            match f {
                crate::types::filter::LibraryFilter::ArtistId { id, .. } => {
                    params.push(("artist_id", id));
                }
                crate::types::filter::LibraryFilter::GenreId { name, .. } => {
                    params.push(("genre_id", name));
                }
                crate::types::filter::LibraryFilter::AlbumId { id, .. } => params.push(("id", id)),
            }
        } else if let Some(query) = search_query
            && !query.is_empty()
        {
            // Only fall back to text search if no ID filter is active
            params.push(("name", query));
        }

        // Make API request
        let (response_text, total_count_header) = self
            .client
            .get_with_headers("/api/album", &params)
            .await
            .context("Failed to fetch albums from API")?;

        // Parse JSON response as array of albums
        let albums: Vec<Album> = serde_json::from_str(&response_text).with_context(|| {
            // Provide better error message with response preview
            let preview = response_text.chars().take(500).collect::<String>();
            format!("Failed to parse albums JSON response. Response preview: {preview}")
        })?;

        // Get total count from X-Total-Count header, fallback to albums length
        let total_count = total_count_header.unwrap_or(albums.len() as u32);

        // Debug: check if updatedAt is being parsed
        if let Some(first_album) = albums.first() {
            trace!(
                " DEBUG: First album updatedAt = {:?}",
                first_album.updated_at
            );
        }

        debug!(
            " AlbumService: Loaded {} albums, X-Total-Count header: {:?}, using total_count: {}",
            albums.len(),
            total_count_header,
            total_count
        );

        Ok((albums, total_count))
    }

    /// Map viewType to sort parameter
    fn map_sort_mode_to_sort_param(sort_mode: &str) -> String {
        match sort_mode {
            "recentlyAdded" => "recently_added".to_string(),
            "recentlyPlayed" => "play_date".to_string(),
            "mostPlayed" => "play_count".to_string(),
            "favorited" => "starred_at".to_string(),
            "random" => "random".to_string(),
            "name" => "name".to_string(),
            "albumArtist" => "album_artist".to_string(),
            "artist" => "artist".to_string(),
            "year" => "max_year".to_string(),
            "songCount" => "songCount".to_string(),
            "duration" => "duration".to_string(),
            "rating" => "rating".to_string(),
            "genre" => "genre".to_string(),
            "all" => "name".to_string(),
            _ => "recently_added".to_string(),
        }
    }

    /// Get default sort order for sort mode
    fn get_default_order(sort_mode: &str) -> &'static str {
        match sort_mode {
            "recentlyAdded" | "recentlyPlayed" | "mostPlayed" | "favorited" | "year"
            | "songCount" | "duration" | "rating" => "DESC",
            _ => "ASC",
        }
    }
}

impl Clone for AlbumsApiService {
    fn clone(&self) -> Self {
        Self {
            client: self.client.clone(),
            server_url: self.server_url.clone(),
            subsonic_credential: self.subsonic_credential.clone(),
        }
    }
}
