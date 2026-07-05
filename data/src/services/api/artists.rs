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

#[derive(Clone)]
pub struct ArtistsApiService {
    client: ApiClient,
}

impl ArtistsApiService {
    pub fn new(client: ApiClient) -> Self {
        Self { client }
    }

    /// Load artists from the API
    /// sort_mode: Sort mode (name, favorited, albumCount, songCount, random)
    /// sort_order: Sort order (ASC or DESC)
    /// search_query: Optional search query
    /// library_ids: When non-empty, restrict results to the given library
    /// (music folder) IDs by appending repeatable `library_id` params. An
    /// empty slice omits the param entirely — Navidrome's auto-scoping
    /// already limits to libraries the user has access to.
    #[allow(clippy::too_many_arguments)]
    pub async fn load_artists(
        &self,
        sort_mode: &str,
        sort_order: &str,
        search_query: Option<&str>,
        filter: Option<&crate::types::filter::LibraryFilter>,
        library_ids: &[i32],
        album_artists_only: bool,
        offset: Option<usize>,
        limit: Option<usize>,
    ) -> Result<(Vec<Artist>, u32)> {
        // For random view, we load by name and shuffle client-side
        // (Navidrome doesn't support random sorting for artists)
        let (is_random, actual_sort_mode) = sort::resolve_random_sort_mode(sort_mode);

        // Map viewType to API sort parameter
        let sort_param = sort::map_sort_mode(SortDomain::Artists, actual_sort_mode);
        let order_param = sort::resolve_order(SortDomain::Artists, actual_sort_mode, sort_order);

        // Pagination range and owned `library_id` strings — both owned
        // alongside `params` so the `&str` borrows built below outlive the
        // call to `get_with_headers`. See `albums.rs` for the companion
        // comment.
        let range = pagination::paged_range(offset.unwrap_or(0) as u32, limit.map(|x| x as u32));
        let library_id_strings =
            crate::services::api::songs::collect_library_id_strings(library_ids, filter);

        let params = Self::build_artist_params(
            sort_param,
            order_param,
            &range,
            album_artists_only,
            search_query,
            filter,
            &library_id_strings,
        );

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

    /// Build the `_sort` / `_order` / role / filter / search / pagination /
    /// `library_id` params for an `/api/artist` browse request. Extracted
    /// (mirroring `SongsApiService::build_song_params`) so the wire shape is
    /// pinned by tests — including the bare `id` key for the ArtistId
    /// filter, which is this endpoint's own primary key rather than a join
    /// column like `/api/album`'s `artist_id`.
    fn build_artist_params<'a>(
        sort_param: &'a str,
        order_param: &'a str,
        range: &'a pagination::PagedRange,
        album_artists_only: bool,
        search_query: Option<&'a str>,
        filter: Option<&'a crate::types::filter::LibraryFilter>,
        library_id_strings: &'a [String],
    ) -> Vec<(&'a str, &'a str)> {
        let mut params = vec![("_sort", sort_param), ("_order", order_param)];

        if album_artists_only {
            params.push(("role", "albumartist"));
        }

        params.push(("_start", range.start.as_str()));
        params.push(("_end", range.end.as_str()));

        // Apply ID filter if present
        if let Some(f) = filter {
            match f {
                crate::types::filter::LibraryFilter::ArtistId { id, .. } => {
                    params.push(("id", id));
                }
                // `LibraryFilter::LibraryIds` is folded into
                // `library_id_strings` by the caller via the shared helper.
                crate::types::filter::LibraryFilter::LibraryIds(_) => {}
                // AlbumId / GenreId are not meaningful filters on the
                // /api/artist endpoint — leave the request unfiltered.
                crate::types::filter::LibraryFilter::AlbumId { .. }
                | crate::types::filter::LibraryFilter::GenreId { .. } => {}
            }
        } else if let Some(query) = search_query
            && !query.is_empty()
        {
            params.push(("name", query));
        }

        for s in library_id_strings {
            params.push(("library_id", s.as_str()));
        }
        params
    }

    /// Load albums for a specific artist
    pub async fn load_artist_albums(&self, artist_id: &str) -> Result<Vec<Album>> {
        // Build query parameters - filter by artist_id
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::filter::LibraryFilter;

    fn range(start: &str, end: &str) -> pagination::PagedRange {
        pagination::PagedRange {
            start: start.to_string(),
            end: end.to_string(),
        }
    }

    /// Pin the `/api/artist` browse wire shape, including the `role`
    /// param's position (before pagination) when album-artists-only is on.
    #[test]
    fn browse_params_pin_wire_shape_with_role_gate() {
        let r = range("0", "36");
        let params = ArtistsApiService::build_artist_params(
            "name",
            "ASC",
            &r,
            true,
            Some("massive"),
            None,
            &[],
        );
        assert_eq!(
            params,
            vec![
                ("_sort", "name"),
                ("_order", "ASC"),
                ("role", "albumartist"),
                ("_start", "0"),
                ("_end", "36"),
                ("name", "massive"),
            ]
        );

        // Gate off → no role param at all.
        let params =
            ArtistsApiService::build_artist_params("name", "ASC", &r, false, None, None, &[]);
        assert!(!params.iter().any(|(k, _)| *k == "role"));
    }

    /// `/api/artist` filters by its own primary key: the ArtistId filter
    /// maps to a bare `id` param — never `artist_id`/`artists_id`, which
    /// are the album/song endpoints' join-column spellings.
    #[test]
    fn artist_filter_uses_bare_id_key() {
        let r = range("0", "36");
        let filter = LibraryFilter::ArtistId {
            id: "ar-1".to_string(),
            name: "Massive Attack".to_string(),
        };
        let params = ArtistsApiService::build_artist_params(
            "name",
            "ASC",
            &r,
            false,
            None,
            Some(&filter),
            &[],
        );
        assert!(params.contains(&("id", "ar-1")));
        assert!(
            !params
                .iter()
                .any(|(k, _)| *k == "artist_id" || *k == "artists_id")
        );
    }

    /// AlbumId / GenreId are meaningless on `/api/artist` — the request
    /// stays unfiltered (and the search fallback stays suppressed, since
    /// the filter slot is occupied). Both variants are pinned.
    #[test]
    fn album_and_genre_filters_leave_request_unfiltered() {
        let r = range("0", "36");
        let base_only = vec![
            ("_sort", "name"),
            ("_order", "ASC"),
            ("_start", "0"),
            ("_end", "36"),
        ];

        let genre = LibraryFilter::GenreId {
            id: "g-1".to_string(),
            name: "Trip-Hop".to_string(),
        };
        let params = ArtistsApiService::build_artist_params(
            "name",
            "ASC",
            &r,
            false,
            Some("massive"),
            Some(&genre),
            &[],
        );
        assert_eq!(params, base_only);

        let album = LibraryFilter::AlbumId {
            id: "al-1".to_string(),
            title: "Mezzanine".to_string(),
        };
        let params = ArtistsApiService::build_artist_params(
            "name",
            "ASC",
            &r,
            false,
            Some("massive"),
            Some(&album),
            &[],
        );
        assert_eq!(params, base_only);
    }
}
