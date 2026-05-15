use std::{collections::HashMap, sync::Arc};

use anyhow::{Context, Result};
use tracing::{debug, warn};

use crate::{
    services::api::{
        client::ApiClient,
        sort::{self, SortDomain},
    },
    types::genre::Genre,
};

/// Subsonic API response for getGenres
#[derive(Debug, serde::Deserialize)]
struct SubsonicGenresResponse {
    #[serde(rename = "subsonic-response")]
    subsonic_response: SubsonicResponseInner,
}

#[derive(Debug, serde::Deserialize)]
struct SubsonicResponseInner {
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

pub struct GenresApiService {
    client: Arc<ApiClient>,
    server_url: String,
    subsonic_credential: String,
}

impl GenresApiService {
    /// Create with a pre-authenticated ApiClient
    pub fn new_with_client(
        client: ApiClient,
        server_url: String,
        subsonic_credential: String,
    ) -> Self {
        Self {
            client: Arc::new(client),
            server_url,
            subsonic_credential,
        }
    }

    /// Load genres from the API using hybrid approach:
    /// - Native API (/api/genre) for genre IDs
    /// - Subsonic API (getGenres) for album/song counts
    ///
    /// sort_mode: Sort mode (name, albumCount, songCount, random)
    /// sort_order: Sort order (ASC or DESC)
    /// search_query: Optional search query
    pub async fn load_genres(
        &self,
        sort_mode: &str,
        sort_order: &str,
        search_query: Option<&str>,
    ) -> Result<(Vec<Genre>, u32)> {
        // For random view, we load by name and shuffle client-side
        let is_random = sort_mode == "random";
        let actual_sort_mode = if is_random { "name" } else { sort_mode };

        // Map viewType to API sort parameter
        let sort_param = sort::map_sort_mode(SortDomain::Genres, actual_sort_mode);
        let default_order = sort::default_order(SortDomain::Genres, actual_sort_mode);
        let order_param = if sort_order.is_empty() {
            default_order
        } else {
            sort_order
        };

        // Build query parameters for native API
        let mut params = vec![
            ("_sort", sort_param),
            ("_order", order_param),
            ("_start", "0"),
            ("_end", "999999"),
        ];

        // Add search query if provided
        let search_query_string: String;
        if let Some(query) = search_query
            && !query.is_empty()
        {
            search_query_string = query.to_string();
            params.push(("name", &search_query_string));
        }

        // Fetch from both APIs in parallel
        let native_result = self.client.get_with_headers("/api/genre", &params).await;
        let subsonic_result = self.fetch_subsonic_genres().await;

        // Parse native API response (for IDs)
        let native_genres: Vec<Genre> = match native_result {
            Ok((response_text, _)) => serde_json::from_str(&response_text).unwrap_or_default(),
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

    /// Fetch genres from Subsonic API (for counts)
    async fn fetch_subsonic_genres(&self) -> Result<Vec<(String, u32, u32)>> {
        let response = crate::services::api::subsonic::subsonic_post(
            &self.client.http_client(),
            &self.server_url,
            "getGenres",
            &self.subsonic_credential,
            &[],
        )
        .await
        .context("Failed to fetch genres from Subsonic API")?;

        let body = response
            .text()
            .await
            .context("Failed to read Subsonic response")?;

        let parsed: SubsonicGenresResponse = serde_json::from_str(&body).with_context(|| {
            format!(
                "Failed to parse Subsonic genres response: {}",
                &body[..body.len().min(200)]
            )
        })?;

        let mut genres = Vec::new();

        if let Some(genres_obj) = parsed.subsonic_response.genres
            && let Some(genre_value) = genres_obj.genre
        {
            // Handle both array and single object cases
            let genre_array: Vec<SubsonicGenre> = if genre_value.is_array() {
                serde_json::from_value(genre_value)?
            } else {
                vec![serde_json::from_value(genre_value)?]
            };

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
                // Parse as array of album objects, extract IDs
                let albums: Vec<serde_json::Value> =
                    serde_json::from_str(&response_text).unwrap_or_default();

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
            ("_end", "999999"),
            ("genre_id", genre_name),
        ];

        let (response_text, _) = self
            .client
            .get_with_headers("/api/album", &params)
            .await
            .with_context(|| format!("Failed to fetch albums for genre '{genre_name}'"))?;

        let albums: Vec<crate::types::album::Album> = serde_json::from_str(&response_text)
            .with_context(|| {
                let preview = response_text.chars().take(500).collect::<String>();
                format!("Failed to parse genre albums JSON. Preview: {preview}")
            })?;

        debug!(
            " GenresService: Loaded {} albums for genre '{}'",
            albums.len(),
            genre_name
        );

        Ok(albums)
    }
}

impl Clone for GenresApiService {
    fn clone(&self) -> Self {
        Self {
            client: self.client.clone(),
            server_url: self.server_url.clone(),
            subsonic_credential: self.subsonic_credential.clone(),
        }
    }
}
