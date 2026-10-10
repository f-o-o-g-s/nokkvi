//! Subsonic API client for getSimilarSongs2 and getTopSongs endpoints.
//!
//! Provides algorithmic song recommendations via Navidrome's Subsonic-compatible API.
//! Requires Last.fm or ListenBrainz to be configured on the server.

use anyhow::Result;
use tracing::debug;

use crate::{services::api::client::ApiClient, types::song::Song};

/// Inner payload of the Subsonic `getSimilarSongs2` envelope
/// ([`crate::services::api::subsonic::SubsonicEnvelope`]).
#[derive(Debug, serde::Deserialize)]
struct SimilarResponseInner {
    #[serde(rename = "similarSongs2")]
    similar_songs2: Option<SimilarSongs2>,
}

#[derive(Debug, serde::Deserialize)]
struct SimilarSongs2 {
    song: Option<Vec<Song>>,
}

/// Inner payload of the Subsonic `getTopSongs` envelope
/// ([`crate::services::api::subsonic::SubsonicEnvelope`]).
#[derive(Debug, serde::Deserialize)]
struct TopSongsResponseInner {
    #[serde(rename = "topSongs")]
    top_songs: Option<TopSongs>,
}

#[derive(Debug, serde::Deserialize)]
struct TopSongs {
    song: Option<Vec<Song>>,
}

/// How a `getTopSongs` request names its artist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TopSongsArtist {
    /// `id=<artistId>`, from the OpenSubsonic `topSongsByArtistId` extension
    /// (Navidrome 0.64.0+). Names exactly one artist, so two artists who
    /// share a name don't get each other's songs. The name rides along as
    /// `artist=`: Navidrome's `findArtist` tries the id first and falls back
    /// to the name when the id is unknown (a row from before a rescan).
    Id { id: String, name: String },
    /// `artist=<name>`, the base Subsonic form. Navidrome matches it against
    /// artist names and takes the first hit.
    Name(String),
}

impl TopSongsArtist {
    /// The OpenSubsonic extension that lets `getTopSongs` take an artist id.
    pub const BY_ID_EXTENSION: &'static str = "topSongsByArtistId";

    /// Ask by id when the server advertises [`Self::BY_ID_EXTENSION`] and the
    /// caller has a non-empty id; ask by name otherwise.
    pub fn choose(artist_name: &str, artist_id: Option<&str>, by_id_supported: bool) -> Self {
        match artist_id {
            Some(id) if by_id_supported && !id.is_empty() => Self::Id {
                id: id.to_string(),
                name: artist_name.to_string(),
            },
            _ => Self::Name(artist_name.to_string()),
        }
    }

    /// The query parameters this lookup sends.
    fn query_params(&self) -> Vec<(&'static str, &str)> {
        match self {
            Self::Id { id, name } if name.is_empty() => vec![("id", id)],
            Self::Id { id, name } => vec![("id", id), ("artist", name)],
            Self::Name(name) => vec![("artist", name)],
        }
    }
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
    ///
    /// Reads through the checked envelope, like [`Self::get_top_songs`]: a
    /// failed call is an error, not an empty list that would read as "no
    /// similar songs".
    pub async fn get_similar_songs(&self, id: &str, count: u32) -> Result<Vec<Song>> {
        let count_str = count.to_string();
        let inner: SimilarResponseInner =
            crate::services::api::subsonic::subsonic_get_envelope_checked(
                &self.client.http_client(),
                &self.server_url,
                "getSimilarSongs2",
                &self.subsonic_credential,
                &[("id", id), ("count", &count_str)],
                "getSimilarSongs2",
            )
            .await?;

        let songs = inner
            .similar_songs2
            .and_then(|s| s.song)
            .unwrap_or_default();

        debug!("🎵 getSimilarSongs2: {} results for id={}", songs.len(), id);

        Ok(songs)
    }

    /// Fetch an artist's top songs (`getTopSongs`), by id or by name as
    /// `artist` says.
    ///
    /// Reads through the checked envelope: a failed call is an error, not an
    /// empty list that would read as "this artist has no top songs".
    pub async fn get_top_songs(&self, artist: &TopSongsArtist, count: u32) -> Result<Vec<Song>> {
        let count_str = count.to_string();
        let mut params = artist.query_params();
        params.push(("count", &count_str));
        let inner: TopSongsResponseInner =
            crate::services::api::subsonic::subsonic_get_envelope_checked(
                &self.client.http_client(),
                &self.server_url,
                "getTopSongs",
                &self.subsonic_credential,
                &params,
                "getTopSongs",
            )
            .await?;

        let songs = inner.top_songs.and_then(|s| s.song).unwrap_or_default();

        debug!("🎵 getTopSongs: {} results for {:?}", songs.len(), artist);

        Ok(songs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        services::api::subsonic::{
            SubsonicEnvelope,
            tests::{FAILED_ENVELOPE, NOT_FOUND_ENVELOPE, one_shot_server},
        },
        types::error::NokkviError,
    };

    async fn similar_from(base_url: String) -> Result<Vec<Song>> {
        let url = url::Url::parse(&base_url).expect("one-shot server url");
        SimilarApiService::new(ApiClient::new(url, String::new()), base_url, "u=x".into())
            .get_similar_songs("s1", 50)
            .await
    }

    /// A failed `getSimilarSongs2` (an id gone since a rescan, a rejected
    /// token) must be an error, not "No similar songs found". A server with
    /// no matches still answers ok with an empty list.
    #[tokio::test]
    async fn similar_songs_turn_a_failed_envelope_into_an_error() {
        let err = similar_from(one_shot_server("200 OK", NOT_FOUND_ENVELOPE).await)
            .await
            .expect_err("a failed envelope must be an error");
        assert!(
            format!("{err:#}").contains("data not found"),
            "got: {err:#}"
        );

        let err = similar_from(one_shot_server("200 OK", FAILED_ENVELOPE).await)
            .await
            .expect_err("a rejected token must be an error");
        assert!(
            matches!(
                err.downcast_ref::<NokkviError>(),
                Some(NokkviError::Unauthorized)
            ),
            "got: {err:#}"
        );

        const NO_MATCHES: &str =
            r#"{"subsonic-response":{"status":"ok","version":"1.16.1","similarSongs2":{}}}"#;
        let songs = similar_from(one_shot_server("200 OK", NO_MATCHES).await)
            .await
            .expect("an ok envelope with no songs is not an error");
        assert!(songs.is_empty());
    }

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

        let parsed: SubsonicEnvelope<SimilarResponseInner> =
            serde_json::from_str(json).expect("should parse similar songs response");
        let songs = parsed.response.similar_songs2.unwrap().song.unwrap();
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

        let parsed: SubsonicEnvelope<SimilarResponseInner> =
            serde_json::from_str(json).expect("should parse empty response");
        let songs = parsed
            .response
            .similar_songs2
            .and_then(|s| s.song)
            .unwrap_or_default();
        assert!(songs.is_empty());
    }

    /// With `topSongsByArtistId` advertised, a known id is sent as `id`, with
    /// the name as `artist` for Navidrome's unknown-id fallback; without the
    /// extension, or without an id, only the name is sent.
    #[test]
    fn top_songs_asks_by_id_with_the_extension_and_an_id() {
        let by_id = TopSongsArtist::choose("Mono", Some("ar-1"), true);
        assert_eq!(
            by_id,
            TopSongsArtist::Id {
                id: "ar-1".into(),
                name: "Mono".into()
            }
        );
        assert_eq!(by_id.query_params(), [("id", "ar-1"), ("artist", "Mono")]);

        let no_ext = TopSongsArtist::choose("Mono", Some("ar-1"), false);
        assert_eq!(no_ext, TopSongsArtist::Name("Mono".into()));
        assert_eq!(no_ext.query_params(), [("artist", "Mono")]);

        assert_eq!(
            TopSongsArtist::choose("Mono", None, true),
            TopSongsArtist::Name("Mono".into())
        );
        // The queue row carries the id as a plain String, empty when unknown.
        assert_eq!(
            TopSongsArtist::choose("Mono", Some(""), true),
            TopSongsArtist::Name("Mono".into())
        );
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

        let parsed: SubsonicEnvelope<TopSongsResponseInner> =
            serde_json::from_str(json).expect("should parse top songs response");
        let songs = parsed.response.top_songs.unwrap().song.unwrap();
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

        let parsed: SubsonicEnvelope<TopSongsResponseInner> =
            serde_json::from_str(json).expect("should parse response without topSongs");
        let songs = parsed
            .response
            .top_songs
            .and_then(|s| s.song)
            .unwrap_or_default();
        assert!(songs.is_empty());
    }
}
