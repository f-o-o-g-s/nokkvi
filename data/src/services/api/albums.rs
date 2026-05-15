use anyhow::{Context, Result};
use tracing::{debug, trace};

use crate::{
    services::api::{
        client::ApiClient,
        pagination, parse,
        sort::{self, SortDomain},
    },
    types::album::Album,
};

#[derive(Clone)]
pub struct AlbumsApiService {
    client: ApiClient,
}

impl AlbumsApiService {
    pub fn new(client: ApiClient) -> Self {
        Self { client }
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
        let sort_param = sort::map_sort_mode(SortDomain::Albums, sort_mode);
        let default_order = sort::default_order(SortDomain::Albums, sort_mode);
        let order_param = if sort_order.is_empty() {
            default_order
        } else {
            sort_order
        };

        // Build query parameters
        let mut params = vec![
            ("_sort", sort_param),
            ("_order", order_param),
            ("filter", "{}"),
        ];

        // Add pagination parameters (Navidrome uses _start and _end, not _offset and _limit).
        let range = pagination::paged_range(offset.unwrap_or(0) as u32, limit.map(|x| x as u32));
        params.push(("_start", &range.start));
        params.push(("_end", &range.end));

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
        let albums: Vec<Album> =
            parse::parse_json_with_preview(&response_text, "albums JSON response")?;

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
}
