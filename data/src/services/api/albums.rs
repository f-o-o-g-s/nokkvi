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
    /// library_ids: When non-empty, restrict results to the given library
    /// (music folder) IDs by appending repeatable `library_id` params. An
    /// empty slice omits the param entirely — Navidrome's auto-scoping
    /// already limits to libraries the user has access to.
    #[allow(clippy::too_many_arguments)]
    pub async fn load_albums(
        &self,
        sort_mode: &str,
        sort_order: &str,
        search_query: Option<&str>,
        filter: Option<&crate::types::filter::LibraryFilter>,
        library_ids: &[i32],
        offset: Option<usize>,
        limit: Option<usize>,
    ) -> Result<(Vec<Album>, u32)> {
        // Map viewType to API sort parameter
        let sort_param = sort::map_sort_mode(SortDomain::Albums, sort_mode);
        let order_param = sort::resolve_order(SortDomain::Albums, sort_mode, sort_order);

        // Pagination range (Navidrome uses _start and _end, not _offset and
        // _limit) and owned `library_id` strings — both owned alongside
        // `params` so the `&str` borrows built below outlive the call to
        // `get_with_headers`. The fold combines the orthogonal `library_ids`
        // argument with any `LibraryFilter::LibraryIds` routed through the
        // filter slot (both express "scope by music folder", just from
        // different navigation surfaces).
        let range = pagination::paged_range(offset.unwrap_or(0) as u32, limit.map(|x| x as u32));
        let library_id_strings =
            crate::services::api::songs::collect_library_id_strings(library_ids, filter);

        let params = Self::build_album_params(
            sort_param,
            order_param,
            &range,
            search_query,
            filter,
            &library_id_strings,
        );

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

    /// Build the `_sort` / `_order` / filter / search / pagination /
    /// `library_id` params for an `/api/album` browse request. Extracted
    /// (mirroring [`SongsApiService::build_song_params`]) so the wire shape
    /// is pinned by tests — including the `artist_id` key spelling, which
    /// deliberately differs from `/api/song`'s `artists_id` (each matches
    /// its Navidrome repository's filter registry).
    ///
    /// [`SongsApiService::build_song_params`]: crate::services::api::songs::SongsApiService
    fn build_album_params<'a>(
        sort_param: &'a str,
        order_param: &'a str,
        range: &'a pagination::PagedRange,
        search_query: Option<&'a str>,
        filter: Option<&'a crate::types::filter::LibraryFilter>,
        library_id_strings: &'a [String],
    ) -> Vec<(&'a str, &'a str)> {
        let mut params = vec![
            ("_sort", sort_param),
            ("_order", order_param),
            ("filter", "{}"),
            ("_start", range.start.as_str()),
            ("_end", range.end.as_str()),
        ];

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
                // `LibraryFilter::LibraryIds` is folded into
                // `library_id_strings` by the caller via the shared helper.
                crate::types::filter::LibraryFilter::LibraryIds(_) => {}
            }
        } else if let Some(query) = search_query
            && !query.is_empty()
        {
            // Only fall back to text search if no ID filter is active
            params.push(("name", query));
        }

        for s in library_id_strings {
            params.push(("library_id", s.as_str()));
        }
        params
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

    /// Pin the full `/api/album` browse wire shape: base params in order,
    /// text search on `name`, `library_id` repeats last.
    #[test]
    fn browse_params_pin_wire_shape() {
        let r = range("0", "36");
        let ids = vec!["1".to_string(), "2".to_string()];
        let params = AlbumsApiService::build_album_params(
            "recently_added",
            "DESC",
            &r,
            Some("mezzanine"),
            None,
            &ids,
        );
        assert_eq!(
            params,
            vec![
                ("_sort", "recently_added"),
                ("_order", "DESC"),
                ("filter", "{}"),
                ("_start", "0"),
                ("_end", "36"),
                ("name", "mezzanine"),
                ("library_id", "1"),
                ("library_id", "2"),
            ]
        );
    }

    /// `/api/album` filters by artist via `artist_id` — NOT `artists_id`,
    /// which is `/api/song`'s spelling. Each key matches its Navidrome
    /// repository's filter registry (`album_repository.go` registers
    /// `artist_id`; `mediafile_repository.go` registers `artists_id`), so a
    /// future "unification" of the two spellings must turn this test or its
    /// songs-side companion red, whichever endpoint's spelling changed.
    /// Companion pin on the songs side:
    /// `songs::tests::library_filter_artist_id_does_not_contribute_library_ids`.
    #[test]
    fn artist_filter_uses_album_endpoint_spelling() {
        let r = range("0", "36");
        let filter = LibraryFilter::ArtistId {
            id: "ar-1".to_string(),
            name: "Massive Attack".to_string(),
        };
        let params =
            AlbumsApiService::build_album_params("name", "ASC", &r, None, Some(&filter), &[]);
        assert!(params.contains(&("artist_id", "ar-1")));
        assert!(!params.iter().any(|(k, _)| *k == "artists_id"));
    }

    /// An active ID filter suppresses the text-search fallback — search only
    /// applies when no filter occupies the slot.
    #[test]
    fn search_is_ignored_when_id_filter_active() {
        let r = range("0", "36");
        let filter = LibraryFilter::GenreId {
            id: "g-1".to_string(),
            name: "Trip-Hop".to_string(),
        };
        let params = AlbumsApiService::build_album_params(
            "name",
            "ASC",
            &r,
            Some("mezzanine"),
            Some(&filter),
            &[],
        );
        assert!(params.contains(&("genre_id", "Trip-Hop")));
        assert!(!params.iter().any(|(k, _)| *k == "name"));
    }
}
