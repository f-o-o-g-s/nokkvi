use anyhow::{Context, Result};
use serde::Deserialize;

use crate::{
    services::api::{
        client::ApiClient,
        pagination::{self, FULL_LOAD_PAGE_SIZE},
        parse,
        sort::{self, SortDomain},
    },
    types::song::Song,
};

pub struct SongsApiService {
    client: ApiClient,
}

/// Shared shape of every `/api/song` query: sort/order/filter/search/mode.
/// Split out so the per-page and paginated helpers can take a single
/// borrow instead of five identical scalars (which trips
/// `clippy::too_many_arguments`). The lifetime is the caller's stack
/// frame — every borrow is short-lived alongside the helper call.
struct SongQueryShape<'a> {
    sort_param: &'a str,
    order: &'a str,
    search_query: Option<&'a str>,
    filter: Option<&'a crate::types::filter::LibraryFilter>,
    sort_mode: &'a str,
}

impl SongsApiService {
    pub fn new(client: ApiClient, _server_url: String, _subsonic_credential: String) -> Self {
        Self { client }
    }

    pub fn new_with_client(
        client: ApiClient,
        server_url: String,
        subsonic_credential: String,
    ) -> Self {
        Self::new(client, server_url, subsonic_credential)
    }

    /// Load songs with sorting, filtering, and pagination.
    ///
    /// # Arguments
    /// * `sort_mode` — Sort/filter type: `"recentlyAdded"`, `"random"`, `"title"`, etc.
    /// * `sort_order` — `"ASC"` or `"DESC"`. Empty falls back to the per-mode default.
    /// * `search_query` — Optional title-substring search.
    /// * `filter` — Optional `LibraryFilter` (artist / album / genre scope).
    /// * `offset` — Optional starting index (defaults to 0).
    /// * `limit` — `Some(n)` issues a single page of `n` rows; `None` paginates
    ///   internally in `FULL_LOAD_PAGE_SIZE` chunks until the server reports a
    ///   short page or the cumulative count meets `X-Total-Count`. The latter
    ///   replaced the legacy `_end=50000` ceiling that silently truncated
    ///   libraries with more than 50_000 songs.
    pub async fn load_songs(
        &self,
        sort_mode: &str,
        sort_order: &str,
        search_query: Option<&str>,
        filter: Option<&crate::types::filter::LibraryFilter>,
        offset: Option<usize>,
        limit: Option<usize>,
    ) -> Result<(Vec<Song>, usize)> {
        let sort_param = sort::map_sort_mode(SortDomain::Songs, sort_mode);
        let order = if sort_order.is_empty() {
            sort::default_order(SortDomain::Songs, sort_mode)
        } else {
            sort_order
        };
        let offset_val = offset.unwrap_or(0) as u32;
        let shape = SongQueryShape {
            sort_param,
            order,
            search_query,
            filter,
            sort_mode,
        };

        match limit {
            Some(l) => {
                self.load_songs_single_page(&shape, offset_val, l as u32)
                    .await
            }
            None => self.load_songs_all_pages(&shape, offset_val).await,
        }
    }

    /// Build the `_sort` / `_order` / `_start` / `_end` / filter / favorited
    /// params for a single Songs request. Shared between the single-page and
    /// paginated paths so the per-request shape stays in lockstep.
    fn build_song_params<'a>(
        shape: &SongQueryShape<'a>,
        start_str: &'a str,
        end_str: &'a str,
    ) -> Vec<(&'a str, &'a str)> {
        let mut params: Vec<(&str, &str)> = vec![
            ("_sort", shape.sort_param),
            ("_order", shape.order),
            ("_start", start_str),
            ("_end", end_str),
        ];
        if let Some(f) = shape.filter {
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
        } else if let Some(query) = shape.search_query
            && !query.is_empty()
        {
            params.push(("title", query));
        }
        if shape.sort_mode == "favorited" {
            params.push(("starred", "true"));
        }
        params
    }

    /// Single `/api/song` request, used when the caller specified an explicit
    /// `limit`. Mirrors the previous (pre-pagination-loop) single-call shape.
    async fn load_songs_single_page(
        &self,
        shape: &SongQueryShape<'_>,
        offset: u32,
        limit: u32,
    ) -> Result<(Vec<Song>, usize)> {
        let range = pagination::paged_range(offset, Some(limit));
        let params = Self::build_song_params(shape, &range.start, &range.end);
        let response = self
            .client
            .get_with_headers("/api/song", &params)
            .await
            .context("Failed to fetch songs from API")?;
        Self::parse_response_with_total(&response.0, response.1)
    }

    /// Page through `/api/song` in `FULL_LOAD_PAGE_SIZE` chunks until the
    /// server returns a short page or the cumulative count meets the reported
    /// total. Used when the caller passes `limit = None` — i.e., "load every
    /// matching row". `starting_offset` is preserved relative to each
    /// per-page request so callers paginating from a non-zero base get
    /// consistent absolute indices on the wire.
    async fn load_songs_all_pages(
        &self,
        shape: &SongQueryShape<'_>,
        starting_offset: u32,
    ) -> Result<(Vec<Song>, usize)> {
        // Outer-closure captures: the Fn signature on `fetch_all_pages` means
        // anything referenced from inside must be re-clonable across calls.
        // Clone-on-entry to owned types here keeps the inner `async move`
        // body straightforward — borrowing through the shape into the closure
        // would require the shape to outlive an opaque Future and forces
        // lifetime gymnastics we don't need.
        let client = self.client.clone();
        let sort_param = shape.sort_param.to_string();
        let order = shape.order.to_string();
        let search_query = shape.search_query.map(str::to_string);
        let filter = shape.filter.cloned();
        let sort_mode = shape.sort_mode.to_string();

        pagination::fetch_all_pages(FULL_LOAD_PAGE_SIZE, |start, end| {
            let client = client.clone();
            let sort_param = sort_param.clone();
            let order = order.clone();
            let search_query = search_query.clone();
            let filter = filter.clone();
            let sort_mode = sort_mode.clone();
            async move {
                let absolute_start = starting_offset.saturating_add(start);
                let absolute_end = starting_offset.saturating_add(end);
                let start_str = absolute_start.to_string();
                let end_str = absolute_end.to_string();
                let shape = SongQueryShape {
                    sort_param: &sort_param,
                    order: &order,
                    search_query: search_query.as_deref(),
                    filter: filter.as_ref(),
                    sort_mode: &sort_mode,
                };
                let params = Self::build_song_params(&shape, &start_str, &end_str);
                let response = client
                    .get_with_headers("/api/song", &params)
                    .await
                    .context("Failed to fetch songs page from API")?;
                Self::parse_response_with_total(&response.0, response.1)
            }
        })
        .await
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

    /// Load every song for a specific genre.
    ///
    /// # Arguments
    /// * `genre_name` — The genre name to filter by.
    ///
    /// Pages through `/api/song` in `FULL_LOAD_PAGE_SIZE` chunks until
    /// exhausted. Replaces the legacy `_end=50000` ceiling, which silently
    /// truncated genres with more than 50_000 songs.
    pub async fn load_songs_by_genre(&self, genre_name: &str) -> Result<(Vec<Song>, usize)> {
        let client = self.client.clone();
        let genre_filter = genre_name.to_string();

        pagination::fetch_all_pages(FULL_LOAD_PAGE_SIZE, |start, end| {
            let client = client.clone();
            let genre_filter = genre_filter.clone();
            async move {
                let start_str = start.to_string();
                let end_str = end.to_string();
                let params = vec![
                    ("genre", genre_filter.as_str()),
                    ("_sort", "album"),
                    ("_order", "ASC"),
                    ("_start", start_str.as_str()),
                    ("_end", end_str.as_str()),
                ];
                let response = client
                    .get_with_headers("/api/song", &params)
                    .await
                    .context("Failed to fetch genre songs from API")?;
                Self::parse_response_with_total(&response.0, response.1)
            }
        })
        .await
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

        Err(anyhow::anyhow!(
            "Failed to parse songs response: {}",
            parse::preview(response_text)
        ))
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
