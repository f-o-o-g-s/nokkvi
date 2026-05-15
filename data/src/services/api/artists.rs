use std::sync::Arc;

use anyhow::{Context, Result};
use tracing::debug;

use crate::{
    services::api::{
        client::ApiClient,
        pagination, parse,
        sort::{self, SortDomain},
    },
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
    #[allow(clippy::too_many_arguments)]
    pub async fn load_artists(
        &self,
        sort_mode: &str,
        sort_order: &str,
        search_query: Option<&str>,
        filter: Option<&crate::types::filter::LibraryFilter>,
        album_artists_only: bool,
        offset: Option<usize>,
        limit: Option<usize>,
    ) -> Result<(Vec<Artist>, u32)> {
        // For random view, we load by name and shuffle client-side
        // (Navidrome doesn't support random sorting for artists)
        let is_random = sort_mode == "random";
        let actual_sort_mode = if is_random { "name" } else { sort_mode };

        // Map viewType to API sort parameter
        let sort_param = sort::map_sort_mode(SortDomain::Artists, actual_sort_mode);
        let default_order = sort::default_order(SortDomain::Artists, actual_sort_mode);
        let order_param = if sort_order.is_empty() {
            default_order
        } else {
            sort_order
        };

        // Build query parameters
        let mut params = vec![("_sort", sort_param), ("_order", order_param)];

        if album_artists_only {
            params.push(("role", "albumartist"));
        }

        // Add pagination parameters.
        let range = pagination::paged_range(offset.unwrap_or(0) as u32, limit.map(|x| x as u32));
        params.push(("_start", &range.start));
        params.push(("_end", &range.end));

        // Apply ID filter if present
        if let Some(f) = filter {
            if let crate::types::filter::LibraryFilter::ArtistId { id, .. } = f {
                params.push(("id", id));
            }
        } else if let Some(query) = search_query
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
        let mut artists: Vec<Artist> =
            parse::parse_json_with_preview(&response_text, "artists JSON response")?;

        // Client-side shuffle for random view
        if is_random {
            sort::apply_random_shuffle(&mut artists);
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
            ("_end", pagination::NO_LIMIT_END_STR),
            ("artist_id", artist_id),
        ];

        // Make API request
        let (response_text, _) = self
            .client
            .get_with_headers("/api/album", &params)
            .await
            .context("Failed to fetch artist albums from API")?;

        // Parse JSON response as array of albums
        let albums: Vec<Album> =
            parse::parse_json_with_preview(&response_text, "artist albums JSON response")?;

        debug!(
            " ArtistsService: Loaded {} albums for artist {}",
            albums.len(),
            artist_id
        );

        Ok(albums)
    }
}

impl Clone for ArtistsApiService {
    fn clone(&self) -> Self {
        Self {
            client: self.client.clone(),
        }
    }
}
