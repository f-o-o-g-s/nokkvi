use std::sync::Arc;

use anyhow::{Context, Result};
use rand::seq::SliceRandom;
use tracing::debug;

use crate::{
    services::api::client::ApiClient,
    types::{album::Album, artist::Artist},
};

pub struct ArtistsApiService {
    client: Arc<ApiClient>,
}

impl ArtistsApiService {
    pub fn new(client: ApiClient, _server_url: String, _subsonic_credential: String) -> Self {
        Self {
            client: Arc::new(client),
        }
    }

    /// Load artists from the API
    /// sort_mode: Sort mode (name, favorited, albumCount, songCount, random)
    /// sort_order: Sort order (ASC or DESC)
    /// search_query: Optional search query
    pub async fn load_artists(
        &self,
        sort_mode: &str,
        sort_order: &str,
        search_query: Option<&str>,
        offset: Option<usize>,
        limit: Option<usize>,
    ) -> Result<(Vec<Artist>, u32)> {
        // For random view, we load by name and shuffle client-side
        // (Navidrome doesn't support random sorting for artists)
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

        // Build query parameters
        let mut params = vec![
            ("_sort", sort_param.as_str()),
            ("_order", order_param),
            ("role", "albumartist"), // Only show album artists (per QML reference)
        ];

        // Add pagination parameters
        let offset_val = offset.unwrap_or(0);
        let limit_val = limit.unwrap_or(999999);
        let start_str = offset_val.to_string();
        let end_str = (offset_val + limit_val).to_string();
        params.push(("_start", &start_str));
        params.push(("_end", &end_str));

        // Add search query if provided
        if let Some(query) = search_query
            && !query.is_empty()
        {
            params.push(("name", query));
        }

        // Make API request
        let (response_text, total_count_header) = self
            .client
            .get_with_headers("/api/artist", &params)
            .await
            .context("Failed to fetch artists from API")?;

        // Parse JSON response as array of artists
        let mut artists: Vec<Artist> = serde_json::from_str(&response_text).with_context(|| {
            // Provide better error message with response preview
            let preview = response_text.chars().take(500).collect::<String>();
            format!("Failed to parse artists JSON response. Response preview: {preview}")
        })?;

        // Client-side shuffle for random view
        if is_random {
            let mut rng = rand::rng();
            artists.shuffle(&mut rng);
            if let Some(first) = artists.first() {
                debug!(" Random sort - First artist: {}", first.name);
            }
        }

        // Get total count from X-Total-Count header, fallback to artists length
        let total_count = total_count_header.unwrap_or(artists.len() as u32);

        debug!(
            " ArtistsService: Loaded {} artists, X-Total-Count header: {:?}, using total_count: {}",
            artists.len(),
            total_count_header,
            total_count
        );

        Ok((artists, total_count))
    }

    /// Load albums for a specific artist
    pub async fn load_artist_albums(&self, artist_id: &str) -> Result<Vec<Album>> {
        // Build query parameters - filter by artist_id
        let _filter = format!("{{\"artist_id\":\"{artist_id}\"}}");
        let params = vec![
            ("_sort", "max_year"),
            ("_order", "DESC"),
            ("_start", "0"),
            ("_end", "999999"),
            ("artist_id", artist_id),
        ];

        // Make API request
        let (response_text, _) = self
            .client
            .get_with_headers("/api/album", &params)
            .await
            .context("Failed to fetch artist albums from API")?;

        // Parse JSON response as array of albums
        let albums: Vec<Album> = serde_json::from_str(&response_text).with_context(|| {
            let preview = response_text.chars().take(500).collect::<String>();
            format!("Failed to parse artist albums JSON response. Response preview: {preview}")
        })?;

        debug!(
            " ArtistsService: Loaded {} albums for artist {}",
            albums.len(),
            artist_id
        );

        Ok(albums)
    }

    /// Map viewType to sort parameter
    fn map_sort_mode_to_sort_param(sort_mode: &str) -> String {
        match sort_mode {
            "name" => "name".to_string(),
            "favorited" => "starred_at".to_string(),
            "albumCount" => "album_count".to_string(),
            "songCount" => "song_count".to_string(),
            "random" => "name".to_string(), // Random is handled client-side
            _ => "name".to_string(),
        }
    }

    /// Get default sort order for sort mode
    fn get_default_order(sort_mode: &str) -> &'static str {
        match sort_mode {
            "favorited" | "albumCount" | "songCount" => "DESC",
            _ => "ASC",
        }
    }
}

impl Clone for ArtistsApiService {
    fn clone(&self) -> Self {
        Self {
            client: self.client.clone(),
        }
    }
}
