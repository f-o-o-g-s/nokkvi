//! Subsonic API client for getSimilarSongs2 and getTopSongs endpoints.
//!
//! Provides algorithmic song recommendations via Navidrome's Subsonic-compatible API.
//! Requires Last.fm or ListenBrainz to be configured on the server.

use anyhow::{Context, Result};
use tracing::debug;

use crate::{
    services::api::{client::ApiClient, parse},
    types::song::Song,
};

/// Subsonic response wrapper for `getSimilarSongs2`
#[derive(Debug, serde::Deserialize)]
struct SubsonicSimilarResponse {
    #[serde(rename = "subsonic-response")]
    subsonic_response: SimilarResponseInner,
}

#[derive(Debug, serde::Deserialize)]
struct SimilarResponseInner {
    #[serde(rename = "similarSongs2")]
    similar_songs2: Option<SimilarSongs2>,
}

#[derive(Debug, serde::Deserialize)]
struct SimilarSongs2 {
    song: Option<Vec<Song>>,
}

/// Subsonic response wrapper for `getTopSongs`
#[derive(Debug, serde::Deserialize)]
struct SubsonicTopSongsResponse {
    #[serde(rename = "subsonic-response")]
    subsonic_response: TopSongsResponseInner,
}

#[derive(Debug, serde::Deserialize)]
struct TopSongsResponseInner {
    #[serde(rename = "topSongs")]
    top_songs: Option<TopSongs>,
}

#[derive(Debug, serde::Deserialize)]
struct TopSongs {
    song: Option<Vec<Song>>,
}

#[derive(Clone)]
pub struct SimilarApiService {
    client: ApiClient,
    server_url: String,
    subsonic_credential: String,
}

impl SimilarApiService {
    /// Create with a pre-authenticated ApiClient.
    pub fn new(client: ApiClient, server_url: String, subsonic_credential: String) -> Self {
        Self {
            client,
            server_url,
            subsonic_credential,
        }
    }

    /// Fetch similar songs for an entity (song, album, or artist ID).
    ///
    /// Uses Navidrome's `getSimilarSongs2` endpoint which leverages Last.fm
    /// or ListenBrainz for recommendations.
    pub async fn get_similar_songs(&self, id: &str, count: u32) -> Result<Vec<Song>> {
        let count_str = count.to_string();
        let response = crate::services::api::subsonic::subsonic_post(
            &self.client.http_client(),
            &self.server_url,
            "getSimilarSongs2",
            &self.subsonic_credential,
            &[("id", id), ("count", &count_str)],
        )
        .await
        .context("Failed to fetch similar songs from Subsonic API")?;

        let body = response
            .text()
            .await
            .context("Failed to read getSimilarSongs2 response")?;

        let parsed: SubsonicSimilarResponse =
            parse::parse_json_with_preview(&body, "getSimilarSongs2 response")?;

        let songs = parsed
            .subsonic_response
            .similar_songs2
            .and_then(|s| s.song)
            .unwrap_or_default();

        debug!("🎵 getSimilarSongs2: {} results for id={}", songs.len(), id);

        Ok(songs)
    }

    /// Fetch top songs for an artist by name.
    ///
    /// Uses Navidrome's `getTopSongs` endpoint.
    pub async fn get_top_songs(&self, artist_name: &str, count: u32) -> Result<Vec<Song>> {
        let count_str = count.to_string();
        let response = crate::services::api::subsonic::subsonic_post(
            &self.client.http_client(),
            &self.server_url,
            "getTopSongs",
            &self.subsonic_credential,
            &[("artist", artist_name), ("count", &count_str)],
        )
        .await
        .context("Failed to fetch top songs from Subsonic API")?;

        let body = response
            .text()
            .await
            .context("Failed to read getTopSongs response")?;

        let parsed: SubsonicTopSongsResponse =
            parse::parse_json_with_preview(&body, "getTopSongs response")?;

        let songs = parsed
            .subsonic_response
            .top_songs
            .and_then(|s| s.song)
            .unwrap_or_default();

        debug!(
            "🎵 getTopSongs: {} results for artist='{}'",
            songs.len(),
            artist_name
        );

        Ok(songs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verify `getSimilarSongs2` JSON parses correctly.
    #[test]
    fn test_parse_similar_songs_response() {
        let json = r#"{
            "subsonic-response": {
                "status": "ok",
                "version": "1.16.1",
                "similarSongs2": {
                    "song": [
                        {
                            "id": "s1",
                            "title": "Creep",
                            "artist": "Radiohead",
                            "album": "Pablo Honey",
                            "duration": 239
                        },
                        {
                            "id": "s2",
                            "title": "Karma Police",
                            "artist": "Radiohead",
                            "album": "OK Computer",
                            "duration": 264,
                            "path": "/music/radiohead/karma.flac",
                            "size": 35000000
                        }
                    ]
                }
            }
        }"#;

        let parsed: SubsonicSimilarResponse =
            serde_json::from_str(json).expect("should parse similar songs response");
        let songs = parsed
            .subsonic_response
            .similar_songs2
            .unwrap()
            .song
            .unwrap();
        assert_eq!(songs.len(), 2);
        assert_eq!(songs[0].id, "s1");
        assert_eq!(songs[0].path, ""); // omitted → default
        assert_eq!(songs[1].path, "/music/radiohead/karma.flac");
    }

    /// Verify empty `similarSongs2` response (no matches found).
    #[test]
    fn test_parse_similar_songs_empty() {
        let json = r#"{
            "subsonic-response": {
                "status": "ok",
                "version": "1.16.1",
                "similarSongs2": {}
            }
        }"#;

        let parsed: SubsonicSimilarResponse =
            serde_json::from_str(json).expect("should parse empty response");
        let songs = parsed
            .subsonic_response
            .similar_songs2
            .and_then(|s| s.song)
            .unwrap_or_default();
        assert!(songs.is_empty());
    }

    /// Verify `getTopSongs` JSON parses correctly.
    #[test]
    fn test_parse_top_songs_response() {
        let json = r#"{
            "subsonic-response": {
                "status": "ok",
                "version": "1.16.1",
                "topSongs": {
                    "song": [
                        {
                            "id": "t1",
                            "title": "Everything In Its Right Place",
                            "artist": "Radiohead",
                            "album": "Kid A",
                            "duration": 252
                        }
                    ]
                }
            }
        }"#;

        let parsed: SubsonicTopSongsResponse =
            serde_json::from_str(json).expect("should parse top songs response");
        let songs = parsed.subsonic_response.top_songs.unwrap().song.unwrap();
        assert_eq!(songs.len(), 1);
        assert_eq!(songs[0].title, "Everything In Its Right Place");
    }

    /// Verify missing `topSongs` key returns empty vec.
    #[test]
    fn test_parse_top_songs_missing() {
        let json = r#"{
            "subsonic-response": {
                "status": "ok",
                "version": "1.16.1"
            }
        }"#;

        let parsed: SubsonicTopSongsResponse =
            serde_json::from_str(json).expect("should parse response without topSongs");
        let songs = parsed
            .subsonic_response
            .top_songs
            .and_then(|s| s.song)
            .unwrap_or_default();
        assert!(songs.is_empty());
    }
}
