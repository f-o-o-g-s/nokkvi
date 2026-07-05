use std::collections::HashMap;

use anyhow::{Context, Result};
use tracing::{debug, warn};

use crate::{
    services::api::{
        client::ApiClient,
        pagination, parse,
        sort::{self, SortDomain},
    },
    types::genre::Genre,
};

/// Inner payload of the Subsonic `getGenres` envelope
/// ([`crate::services::api::subsonic::SubsonicEnvelope`]).
#[derive(Debug, serde::Deserialize)]
struct GenresInner {
    genres: Option<SubsonicGenres>,
}

#[derive(Debug, serde::Deserialize)]
struct SubsonicGenres {
    genre: Option<serde_json::Value>, // Can be array or single object
}

#[derive(Debug, serde::Deserialize)]
struct SubsonicGenre {
    value: Option<String>,
    #[serde(rename = "songCount")]
    song_count: Option<u32>,
    #[serde(rename = "albumCount")]
    album_count: Option<u32>,
}

#[derive(Clone)]
pub struct GenresApiService {
    client: ApiClient,
    server_url: String,
    subsonic_credential: String,
}

impl GenresApiService {
    /// Create with a pre-authenticated ApiClient.
    pub fn new(client: ApiClient, server_url: String, subsonic_credential: String) -> Self {
        Self {
            client,
            server_url,
            subsonic_credential,
        }
    }

    /// Library-aware genre loader: scopes the Native
    /// `/api/genre` result to the given library (music folder) IDs via
    /// the `library_tag.library_id` join
    /// (`reference-navidrome/persistence/sql_tags.go:60-86`). The Subsonic
    /// `getGenres` enrichment call is left unfiltered — its counts cover
    /// the user's full accessible set and a future commit can teach it to
    /// honor `musicFolderId` when Navidrome ships that filter.
    ///
    /// An empty `library_ids` slice omits the param entirely (Navidrome
    /// auto-scopes to libraries the user can access).
    pub async fn load_genres_with_libraries(
        &self,
        sort_mode: &str,
        sort_order: &str,
        search_query: Option<&str>,
        library_ids: &[i32],
    ) -> Result<(Vec<Genre>, u32)> {
        // For random view, we load by name and shuffle client-side
        let (is_random, actual_sort_mode) = sort::resolve_random_sort_mode(sort_mode);

        // Map viewType to API sort parameter
        let sort_param = sort::map_sort_mode(SortDomain::Genres, actual_sort_mode);
        let order_param = sort::resolve_order(SortDomain::Genres, actual_sort_mode, sort_order);

        // Owned `String`s for any `library_id` filter param values. Owned
        // alongside `params` so the `&str` borrows built below outlive
        // the call to `get_with_headers`. `/api/genre` has no `LibraryFilter`
        // slot, so the shared helper is called with `None`.
        let library_id_strings =
            crate::services::api::songs::collect_library_id_strings(library_ids, None);

        let params =
            Self::build_genre_params(sort_param, order_param, search_query, &library_id_strings);

        // Fetch from both APIs in parallel
        let native_result = self.client.get_with_headers("/api/genre", &params).await;
        let subsonic_result = self.fetch_subsonic_genres().await;

        // Parse native API response (for IDs). Parse failures degrade to an
        // empty list like the network-error arm below, but are logged — a
        // malformed body must never yield a silently empty Genres view.
        let native_genres: Vec<Genre> = match native_result {
            Ok((response_text, _)) => {
                parse::parse_json_or_default(&response_text, "genres JSON response")
            }
            Err(e) => {
                warn!(" GenresApiService: Native API failed: {}", e);
                Vec::new()
            }
        };

        // Build counts map from Subsonic API
        let counts_map: HashMap<String, (u32, u32)> = match subsonic_result {
            Ok(subsonic_genres) => {
                let mut map = HashMap::new();
                for g in subsonic_genres {
                    map.insert(g.0, (g.1, g.2)); // name -> (album_count, song_count)
                }
                map
            }
            Err(e) => {
                warn!(" GenresApiService: Subsonic API failed: {}", e);
                HashMap::new()
            }
        };

        // Merge: use native API genres (with IDs) and enrich with counts from Subsonic
        let mut merged_genres: Vec<Genre> = native_genres
            .into_iter()
            .map(|mut g| {
                if let Some(&(album_count, song_count)) = counts_map.get(&g.name) {
                    g.album_count = album_count;
                    g.song_count = song_count;
                }
                g
            })
            .collect();

        // Sort by album/song count if needed (native API may not support these sorts)
        match actual_sort_mode {
            "albumCount" => {
                merged_genres.sort_by(|a, b| {
                    if order_param == "DESC" {
                        b.album_count.cmp(&a.album_count)
                    } else {
                        a.album_count.cmp(&b.album_count)
                    }
                });
            }
            "songCount" => {
                merged_genres.sort_by(|a, b| {
                    if order_param == "DESC" {
                        b.song_count.cmp(&a.song_count)
                    } else {
                        a.song_count.cmp(&b.song_count)
                    }
                });
            }
            _ => {}
        }

        // Client-side shuffle for random view
        if is_random {
            sort::apply_random_shuffle(&mut merged_genres);
            if let Some(first) = merged_genres.first() {
                debug!(" Random sort - First genre: {}", first.name);
            }
        }

        let total_count = merged_genres.len() as u32;

        debug!(" GenresService: Loaded {} genres", total_count);

        Ok((merged_genres, total_count))
    }

    /// Build the `_sort` / `_order` / search / `library_id` params for an
    /// `/api/genre` browse request. Extracted (mirroring
    /// `SongsApiService::build_song_params`) so the wire shape is pinned by
    /// tests. Genres always load the full set (`_end` = no-limit) — counts
    /// come from a parallel Subsonic call, and sorting by count happens
    /// client-side after the merge.
    fn build_genre_params<'a>(
        sort_param: &'a str,
        order_param: &'a str,
        search_query: Option<&'a str>,
        library_id_strings: &'a [String],
    ) -> Vec<(&'a str, &'a str)> {
        let mut params = vec![
            ("_sort", sort_param),
            ("_order", order_param),
            ("_start", "0"),
            ("_end", pagination::NO_LIMIT_END_STR),
        ];

        // Add search query if provided
        if let Some(query) = search_query
            && !query.is_empty()
        {
            params.push(("name", query));
        }

        for s in library_id_strings {
            params.push(("library_id", s.as_str()));
        }
        params
    }

    /// Fetch genres from Subsonic API (for counts)
    async fn fetch_subsonic_genres(&self) -> Result<Vec<(String, u32, u32)>> {
        let inner: GenresInner = crate::services::api::subsonic::subsonic_get_envelope(
            &self.client.http_client(),
            &self.server_url,
            "getGenres",
            &self.subsonic_credential,
            &[],
            "Subsonic genres",
        )
        .await?;

        let mut genres = Vec::new();

        if let Some(genres_obj) = inner.genres
            && let Some(genre_value) = genres_obj.genre
        {
            // Subsonic returns a single object instead of a one-element array;
            // `deserialize_one_or_many` absorbs that quirk.
            let genre_array: Vec<SubsonicGenre> =
                crate::services::api::subsonic::deserialize_one_or_many(genre_value)?;

            for g in genre_array {
                if let Some(name) = g.value {
                    genres.push((name, g.album_count.unwrap_or(0), g.song_count.unwrap_or(0)));
                }
            }
        }

        Ok(genres)
    }

    /// Load albums for a specific genre (for artwork display)
    /// Returns up to 9 album IDs for the 3x3 collage
    pub async fn load_genre_albums(&self, genre_name: &str) -> Result<Vec<String>> {
        // Use Native API to load albums filtered by genre
        // The API endpoint is /api/album with genre_id filter
        let params = vec![
            ("_sort", "name"),
            ("_order", "ASC"),
            ("_start", "0"),
            ("_end", "9"), // Only need 9 for collage
            ("genre_id", genre_name),
        ];

        let result = self.client.get_with_headers("/api/album", &params).await;

        match result {
            Ok((response_text, _)) => {
                // Parse as array of album objects, extract IDs. Collage
                // artwork is optional, so a malformed body degrades to an
                // empty collage — logged, mirroring the Err arm below.
                let albums: Vec<serde_json::Value> =
                    parse::parse_json_or_default(&response_text, "genre collage albums JSON");

                let album_ids: Vec<String> = albums
                    .iter()
                    .filter_map(|a| a.get("id").and_then(|v| v.as_str()).map(|s| s.to_string()))
                    .take(9)
                    .collect();

                Ok(album_ids)
            }
            Err(e) => {
                warn!(
                    " GenresApiService: Failed to load albums for genre '{}': {}",
                    genre_name, e
                );
                Ok(Vec::new())
            }
        }
    }

    /// Load full album objects for a specific genre (for expansion display).
    /// Returns all albums in the genre as full Album structs.
    pub async fn load_genre_albums_full(
        &self,
        genre_name: &str,
    ) -> Result<Vec<crate::types::album::Album>> {
        let params = vec![
            ("_sort", "name"),
            ("_order", "ASC"),
            ("_start", "0"),
            ("_end", pagination::NO_LIMIT_END_STR),
            ("genre_id", genre_name),
        ];

        let (response_text, _) = self
            .client
            .get_with_headers("/api/album", &params)
            .await
            .with_context(|| format!("Failed to fetch albums for genre '{genre_name}'"))?;

        let albums: Vec<crate::types::album::Album> =
            parse::parse_json_with_preview(&response_text, "genre albums JSON")?;

        debug!(
            " GenresService: Loaded {} albums for genre '{}'",
            albums.len(),
            genre_name
        );

        Ok(albums)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pin the `/api/genre` browse wire shape: full-set load (no-limit
    /// `_end`), text search on `name`, `library_id` repeats last.
    #[test]
    fn browse_params_pin_wire_shape() {
        let ids = vec!["1".to_string(), "2".to_string()];
        let params = GenresApiService::build_genre_params("name", "ASC", Some("trip"), &ids);
        assert_eq!(
            params,
            vec![
                ("_sort", "name"),
                ("_order", "ASC"),
                ("_start", "0"),
                ("_end", pagination::NO_LIMIT_END_STR),
                ("name", "trip"),
                ("library_id", "1"),
                ("library_id", "2"),
            ]
        );
    }

    /// Empty search and no library scope → base params only.
    #[test]
    fn browse_params_omit_empty_search_and_library_scope() {
        let params = GenresApiService::build_genre_params("name", "ASC", Some(""), &[]);
        assert_eq!(
            params,
            vec![
                ("_sort", "name"),
                ("_order", "ASC"),
                ("_start", "0"),
                ("_end", pagination::NO_LIMIT_END_STR),
            ]
        );
    }
}
