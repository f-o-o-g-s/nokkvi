//! Subsonic API client for `getArtistInfo2`.
//!
//! Navidrome fetches an artist's biography, external link and images from its
//! metadata agents only when a client calls `getArtistInfo` /
//! `getArtistInfo2` (`core/external/provider.go`, `refreshArtistInfo`): an
//! artist nobody asked about has an empty biography on the native
//! `/api/artist` row, and info past its time-to-live is refreshed in the
//! background on the next such call. This call is what fills it.

use anyhow::Result;
use tracing::debug;

use crate::{services::api::client::ApiClient, types::artist::ArtistInfo};

/// Inner payload of the Subsonic `getArtistInfo2` envelope
/// ([`crate::services::api::subsonic::SubsonicEnvelope`]).
#[derive(Debug, serde::Deserialize)]
struct ArtistInfoResponseInner {
    #[serde(rename = "artistInfo2")]
    artist_info2: Option<ArtistInfo2Wire>,
}

/// The `artistInfo2` object. Navidrome omits empty fields (`omitempty`), and
/// the similar-artist list is not read: this call asks for none.
#[derive(Debug, Default, serde::Deserialize)]
struct ArtistInfo2Wire {
    biography: Option<String>,
    #[serde(rename = "musicBrainzId")]
    musicbrainz_id: Option<String>,
    #[serde(rename = "lastFmUrl")]
    last_fm_url: Option<String>,
    #[serde(rename = "smallImageUrl")]
    small_image_url: Option<String>,
    #[serde(rename = "mediumImageUrl")]
    medium_image_url: Option<String>,
    #[serde(rename = "largeImageUrl")]
    large_image_url: Option<String>,
}

impl From<ArtistInfo2Wire> for ArtistInfo {
    fn from(wire: ArtistInfo2Wire) -> Self {
        // An empty string means "nothing known", same as an absent key.
        let present = |field: Option<String>| field.filter(|value| !value.trim().is_empty());
        ArtistInfo {
            biography: present(wire.biography),
            external_url: present(wire.last_fm_url),
            musicbrainz_id: present(wire.musicbrainz_id),
            small_image_url: present(wire.small_image_url),
            medium_image_url: present(wire.medium_image_url),
            large_image_url: present(wire.large_image_url),
        }
    }
}

/// The query parameters a `getArtistInfo2` request sends: no similar
/// artists (`count=0`), since only the biography and link are read.
fn query_params(artist_id: &str) -> [(&'static str, &str); 3] {
    [
        ("id", artist_id),
        ("count", "0"),
        ("includeNotPresent", "false"),
    ]
}

#[derive(Clone)]
pub struct ArtistInfoApiService {
    client: ApiClient,
    server_url: String,
    subsonic_credential: String,
}

impl ArtistInfoApiService {
    /// Create with a pre-authenticated ApiClient.
    pub fn new(client: ApiClient, server_url: String, subsonic_credential: String) -> Self {
        Self {
            client,
            server_url,
            subsonic_credential,
        }
    }

    /// Fetch an artist's external info (`getArtistInfo2`). On an artist
    /// whose info was never fetched, the server runs its agents before it
    /// answers, so this can take as long as the agents do.
    ///
    /// Reads through the checked envelope: a failed call (a rejected token,
    /// an id gone since a rescan) is an error, not an empty info that would
    /// read as "this artist has no biography".
    pub async fn get_artist_info(&self, artist_id: &str) -> Result<ArtistInfo> {
        let inner: ArtistInfoResponseInner =
            crate::services::api::subsonic::subsonic_get_envelope_checked(
                &self.client.http_client(),
                &self.server_url,
                "getArtistInfo2",
                &self.subsonic_credential,
                &query_params(artist_id),
                "getArtistInfo2",
            )
            .await?;
        let info: ArtistInfo = inner.artist_info2.unwrap_or_default().into();
        debug!(
            "getArtistInfo2: id={artist_id} biography={} link={}",
            info.biography.is_some(),
            info.external_url.is_some()
        );
        Ok(info)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        services::api::subsonic::tests::{FAILED_ENVELOPE, NOT_FOUND_ENVELOPE, one_shot_server},
        types::error::NokkviError,
    };

    /// A `getArtistInfo2` answer recorded from Navidrome 0.64.2 for an artist
    /// whose info had never been fetched (so the server ran its agents for
    /// this call). Server address and share-image tokens are redacted.
    const RECORDED: &str = include_str!("../../../testdata/get_artist_info2_navidrome_0_64_2.json");

    async fn info_from(base_url: String) -> Result<ArtistInfo> {
        let url = url::Url::parse(&base_url).expect("one-shot server url");
        ArtistInfoApiService::new(ApiClient::new(url, String::new()), base_url, "u=x".into())
            .get_artist_info("ar1")
            .await
    }

    #[tokio::test]
    async fn reads_the_biography_and_link_from_a_recorded_navidrome_answer() {
        let info = info_from(one_shot_server("200 OK", RECORDED).await)
            .await
            .expect("a recorded ok answer parses");
        let bio = info.biography.expect("the recorded answer has a biography");
        assert!(
            bio.starts_with("Sia Kate Isobelle Furler (born 18 December 1975)"),
            "got: {bio}"
        );
        assert_eq!(
            info.external_url.as_deref(),
            Some("https://www.last.fm/music/Sia")
        );
        assert_eq!(
            info.musicbrainz_id.as_deref(),
            Some("2f548675-008d-4332-876c-108b0c7ab9c5")
        );
        for (url, size) in [
            (&info.small_image_url, "300"),
            (&info.medium_image_url, "600"),
            (&info.large_image_url, "1200"),
        ] {
            let url = url.as_deref().expect("the recorded answer has image urls");
            assert!(url.ends_with(&format!("size={size}")), "got: {url}");
        }
    }

    /// Only the biography and link are read, so the request asks for no
    /// similar artists.
    #[test]
    fn asks_for_no_similar_artists() {
        assert_eq!(
            query_params("ar1"),
            [
                ("id", "ar1"),
                ("count", "0"),
                ("includeNotPresent", "false")
            ]
        );
    }

    /// A failed call is an error, never an empty info that would read as
    /// "this artist has no biography"; a rejected token routes to login.
    #[tokio::test]
    async fn a_failed_envelope_is_an_error_not_an_empty_biography() {
        let err = info_from(one_shot_server("200 OK", NOT_FOUND_ENVELOPE).await)
            .await
            .expect_err("a failed envelope must be an error");
        assert!(
            format!("{err:#}").contains("data not found"),
            "got: {err:#}"
        );

        let err = info_from(one_shot_server("200 OK", FAILED_ENVELOPE).await)
            .await
            .expect_err("a rejected token must be an error");
        assert!(
            matches!(
                err.downcast_ref::<NokkviError>(),
                Some(NokkviError::Unauthorized)
            ),
            "got: {err:#}"
        );
    }

    /// An artist the agents know nothing about answers ok with an empty
    /// object; empty strings read as absent.
    #[tokio::test]
    async fn an_empty_answer_is_an_empty_info() {
        const EMPTY: &str = r#"{"subsonic-response":{"status":"ok","version":"1.16.1","artistInfo2":{"biography":"","lastFmUrl":""}}}"#;
        let info = info_from(one_shot_server("200 OK", EMPTY).await)
            .await
            .expect("an ok envelope with no info is not an error");
        assert_eq!(info, ArtistInfo::default());
    }
}
