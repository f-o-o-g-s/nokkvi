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

#[derive(Clone)]
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
    library_ids: &'a [i32],
    sort_mode: &'a str,
}

impl SongsApiService {
    pub fn new(client: ApiClient) -> Self {
        Self { client }
    }

    /// Load songs with sorting, filtering, and pagination.
    ///
    /// # Arguments
    /// * `sort_mode` — Sort/filter type: `"recentlyAdded"`, `"random"`, `"title"`, etc.
    /// * `sort_order` — `"ASC"` or `"DESC"`. Empty falls back to the per-mode default.
    /// * `search_query` — Optional title-substring search.
    /// * `filter` — Optional `LibraryFilter` (artist / album / genre scope).
    /// * `library_ids` — When non-empty, restrict results to the given library
    ///   (music folder) IDs by appending repeatable `library_id` params. An
    ///   empty slice omits the param entirely — Navidrome's auto-scoping
    ///   already limits to libraries the user has access to.
    /// * `offset` — Optional starting index (defaults to 0).
    /// * `limit` — `Some(n)` issues a single page of `n` rows; `None` paginates
    ///   internally in `FULL_LOAD_PAGE_SIZE` chunks until the server reports a
    ///   short page or the cumulative count meets `X-Total-Count`. The latter
    ///   replaced the legacy `_end=50000` ceiling that silently truncated
    ///   libraries with more than 50_000 songs.
    #[allow(clippy::too_many_arguments)]
    pub async fn load_songs(
        &self,
        sort_mode: &str,
        sort_order: &str,
        search_query: Option<&str>,
        filter: Option<&crate::types::filter::LibraryFilter>,
        library_ids: &[i32],
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
            library_ids,
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
    /// / library_id params for a single Songs request. Shared between the
    /// single-page and paginated paths so the per-request shape stays in
    /// lockstep.
    ///
    /// `library_id_strings` must be a slice of `String` values that
    /// outlives the returned params vec. The caller owns the Vec — pushing
    /// `as_str()` borrows directly from the slice keeps the lifetime
    /// gymnastics off the helper signature.
    fn build_song_params<'a>(
        shape: &SongQueryShape<'a>,
        start_str: &'a str,
        end_str: &'a str,
        library_id_strings: &'a [String],
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
                // LibraryFilter::LibraryIds threaded through the orthogonal
                // `library_id_strings` slot — see callers, which fold both
                // the orthogonal `library_ids` argument and any
                // `LibraryFilter::LibraryIds` payload into a single owned
                // Vec<String> before invoking this helper.
                crate::types::filter::LibraryFilter::LibraryIds(_) => {}
            }
        } else if let Some(query) = shape.search_query
            && !query.is_empty()
        {
            params.push(("title", query));
        }
        if shape.sort_mode == "favorited" {
            params.push(("starred", "true"));
        }
        for s in library_id_strings {
            params.push(("library_id", s.as_str()));
        }
        params
    }

    /// Fold the orthogonal `library_ids` slice and any
    /// `LibraryFilter::LibraryIds` payload into a single owned
    /// `Vec<String>` so the borrows pushed into `build_song_params` outlive
    /// the params Vec. Shared by both the single-page and paginated paths —
    /// a thin delegate over the module-level [`collect_library_id_strings`]
    /// so the per-endpoint browse loaders (albums / artists / genres) reuse
    /// the exact same fold logic.
    fn collect_library_id_strings(shape: &SongQueryShape<'_>) -> Vec<String> {
        collect_library_id_strings(shape.library_ids, shape.filter)
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
        let library_id_strings = Self::collect_library_id_strings(shape);
        let params = Self::build_song_params(shape, &range.start, &range.end, &library_id_strings);
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
        let library_ids: Vec<i32> = shape.library_ids.to_vec();
        let sort_mode = shape.sort_mode.to_string();

        pagination::fetch_all_pages(FULL_LOAD_PAGE_SIZE, |start, end| {
            let client = client.clone();
            let sort_param = sort_param.clone();
            let order = order.clone();
            let search_query = search_query.clone();
            let filter = filter.clone();
            let library_ids = library_ids.clone();
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
                    library_ids: &library_ids,
                    sort_mode: &sort_mode,
                };
                let library_id_strings = Self::collect_library_id_strings(&shape);
                let params =
                    Self::build_song_params(&shape, &start_str, &end_str, &library_id_strings);
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

/// Fold the orthogonal `library_ids` slice and any
/// `LibraryFilter::LibraryIds` filter payload into a single owned
/// `Vec<String>` of `library_id` param values.
///
/// Shared across every browse loader (songs / albums / artists / genres):
/// the multi-library scope arrives two ways — as the orthogonal
/// `library_ids: &[i32]` argument and, on the future "show everything in
/// libraries X, Y" navigation surface, as a [`LibraryFilter::LibraryIds`]
/// payload. Both express the same `library_id IN (...)` server filter
/// (`reference-navidrome/persistence/sql_tags.go`), so they fold into one
/// repeat-per-id list. The owned `Vec<String>` outlives the borrows pushed
/// into each endpoint's params Vec. Endpoints without a filter slot (genres)
/// pass `None`.
pub(crate) fn collect_library_id_strings(
    library_ids: &[i32],
    filter: Option<&crate::types::filter::LibraryFilter>,
) -> Vec<String> {
    let mut strings: Vec<String> = library_ids.iter().map(|id| id.to_string()).collect();
    if let Some(crate::types::filter::LibraryFilter::LibraryIds(ids)) = filter {
        strings.extend(ids.iter().map(|id| id.to_string()));
    }
    strings
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::filter::LibraryFilter;

    /// Empty `library_ids` slice + no `LibraryFilter::LibraryIds` means
    /// `collect_library_id_strings` produces an empty Vec, and
    /// `build_song_params` emits zero `library_id` params — Navidrome
    /// auto-scopes to the user's accessible libraries when the param
    /// is absent.
    #[test]
    fn empty_library_ids_emits_zero_library_id_params() {
        let shape = SongQueryShape {
            sort_param: "album",
            order: "ASC",
            search_query: None,
            filter: None,
            library_ids: &[],
            sort_mode: "title",
        };
        let library_id_strings = SongsApiService::collect_library_id_strings(&shape);
        assert!(library_id_strings.is_empty());

        let params = SongsApiService::build_song_params(&shape, "0", "100", &library_id_strings);
        let library_id_count = params.iter().filter(|(k, _)| *k == "library_id").count();
        assert_eq!(library_id_count, 0);
    }

    /// Non-empty `library_ids` emits one `library_id` repeat per ID —
    /// matches Navidrome's react-admin `arrayFormat: 'none'` wire shape.
    #[test]
    fn nonempty_library_ids_emit_one_repeat_per_id() {
        let shape = SongQueryShape {
            sort_param: "album",
            order: "ASC",
            search_query: None,
            filter: None,
            library_ids: &[1, 2, 3],
            sort_mode: "title",
        };
        let library_id_strings = SongsApiService::collect_library_id_strings(&shape);
        assert_eq!(library_id_strings, vec!["1", "2", "3"]);

        let params = SongsApiService::build_song_params(&shape, "0", "100", &library_id_strings);
        let library_id_values: Vec<&str> = params
            .iter()
            .filter(|(k, _)| *k == "library_id")
            .map(|(_, v)| *v)
            .collect();
        assert_eq!(library_id_values, vec!["1", "2", "3"]);
    }

    /// `LibraryFilter::LibraryIds` and the orthogonal `library_ids`
    /// argument are merged into a single set of repeats — both
    /// navigation surfaces are treated identically.
    #[test]
    fn library_filter_ids_and_orthogonal_arg_are_merged() {
        let filter = LibraryFilter::LibraryIds(vec![10, 20]);
        let shape = SongQueryShape {
            sort_param: "album",
            order: "ASC",
            search_query: None,
            filter: Some(&filter),
            library_ids: &[1, 2],
            sort_mode: "title",
        };
        let library_id_strings = SongsApiService::collect_library_id_strings(&shape);
        // Orthogonal arg first, then filter payload — order doesn't
        // matter to Navidrome (Squirrel's `Eq{}` is a SQL `IN (...)`)
        // but stable iteration order is nice for snapshot tests.
        assert_eq!(library_id_strings, vec!["1", "2", "10", "20"]);
    }

    /// `LibraryFilter::ArtistId` does NOT contribute to library_ids —
    /// only the dedicated `LibraryIds` variant does.
    #[test]
    fn library_filter_artist_id_does_not_contribute_library_ids() {
        let filter = LibraryFilter::ArtistId {
            id: "abc".to_string(),
            name: "Some Artist".to_string(),
        };
        let shape = SongQueryShape {
            sort_param: "album",
            order: "ASC",
            search_query: None,
            filter: Some(&filter),
            library_ids: &[],
            sort_mode: "title",
        };
        let library_id_strings = SongsApiService::collect_library_id_strings(&shape);
        assert!(library_id_strings.is_empty());

        let params = SongsApiService::build_song_params(&shape, "0", "100", &library_id_strings);
        // ArtistId is still honored on its own param key.
        assert!(
            params
                .iter()
                .any(|(k, v)| *k == "artists_id" && *v == "abc")
        );
        assert_eq!(params.iter().filter(|(k, _)| *k == "library_id").count(), 0);
    }

    /// The module-level free fn (shared by albums / artists / genres) folds
    /// the orthogonal `library_ids` arg and any `LibraryFilter::LibraryIds`
    /// payload, ignores non-library filter variants, and handles the empty
    /// case — mirroring the `SongQueryShape`-bound delegate above so both
    /// paths stay in lockstep.
    #[test]
    fn collect_library_id_strings_merges_orthogonal_and_filter() {
        // Orthogonal arg + LibraryIds payload merge (arg first, then payload).
        assert_eq!(
            collect_library_id_strings(&[1, 2], Some(&LibraryFilter::LibraryIds(vec![10, 20]))),
            vec!["1", "2", "10", "20"]
        );

        // Empty arg + no filter → empty Vec (no library_id param emitted).
        assert!(collect_library_id_strings(&[], None).is_empty());

        // A non-library filter variant does NOT contribute library ids.
        assert_eq!(
            collect_library_id_strings(
                &[5],
                Some(&LibraryFilter::ArtistId {
                    id: "abc".to_string(),
                    name: "Some Artist".to_string(),
                })
            ),
            vec!["5"]
        );
    }

    /// Negative library IDs (defensive — server uses signed int32) and
    /// large IDs round-trip through `to_string` correctly.
    #[test]
    fn large_and_negative_ids_format_correctly() {
        let shape = SongQueryShape {
            sort_param: "album",
            order: "ASC",
            search_query: None,
            filter: None,
            library_ids: &[i32::MIN, -1, 0, 1, i32::MAX],
            sort_mode: "title",
        };
        let library_id_strings = SongsApiService::collect_library_id_strings(&shape);
        assert_eq!(
            library_id_strings,
            vec![
                i32::MIN.to_string(),
                (-1).to_string(),
                "0".to_string(),
                "1".to_string(),
                i32::MAX.to_string(),
            ]
        );
    }
}
