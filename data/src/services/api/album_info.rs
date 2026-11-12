//! Subsonic API client for `getAlbumInfo2`.
//!
//! Navidrome fetches an album's notes and external link from its metadata
//! agents only when a client calls `getAlbumInfo` / `getAlbumInfo2`
//! (`core/external/provider.go`, `UpdateAlbumInfo`): an album nobody asked
//! about has an empty `description` on the native `/api/album` row, and info
//! past its time-to-live (7 days) is refreshed in the background on the next
//! such call. This call is what fills it.

use anyhow::Result;
use tracing::debug;

use crate::{
    services::api::client::ApiClient,
    types::album::{AlbumInfo, plain_notes},
};

/// Inner payload of the Subsonic `getAlbumInfo2` envelope
/// ([`crate::services::api::subsonic::SubsonicEnvelope`]). Navidrome answers
/// `getAlbumInfo` and `getAlbumInfo2` with the same `albumInfo` object.
#[derive(Debug, serde::Deserialize)]
struct AlbumInfoResponseInner {
    #[serde(rename = "albumInfo")]
    album_info: Option<AlbumInfoWire>,
}

/// The `albumInfo` object. Navidrome omits empty fields (`omitempty`); its
/// MusicBrainz id and share-image links are not read (the native album row
/// carries the id, and the cover comes through `getCoverArt`).
#[derive(Debug, Default, serde::Deserialize)]
struct AlbumInfoWire {
    notes: Option<String>,
    #[serde(rename = "lastFmUrl")]
    last_fm_url: Option<String>,
}

impl From<AlbumInfoWire> for AlbumInfo {
    fn from(wire: AlbumInfoWire) -> Self {
        AlbumInfo {
            notes: plain_notes(wire.notes.as_deref()),
            // An empty string means "nothing known", same as an absent key.
            external_url: wire.last_fm_url.filter(|url| !url.trim().is_empty()),
        }
    }
}

/// The query parameters a `getAlbumInfo2` request sends.
fn query_params(album_id: &str) -> [(&'static str, &str); 1] {
    [("id", album_id)]
}

#[derive(Clone)]
pub struct AlbumInfoApiService {
    client: ApiClient,
    server_url: String,
    subsonic_credential: String,
}

impl AlbumInfoApiService {
    /// Create with a pre-authenticated ApiClient.
    pub fn new(client: ApiClient, server_url: String, subsonic_credential: String) -> Self {
        Self {
            client,
            server_url,
            subsonic_credential,
        }
    }

    /// Fetch an album's external info (`getAlbumInfo2`). On an album whose
    /// info was never fetched, the server runs its agents before it answers,
    /// so this can take as long as the agents do.
    ///
    /// Reads through the checked envelope: a failed call (a rejected token,
    /// an id gone since a rescan) is an error, not an empty info that would
    /// read as "this album has no notes".
    pub async fn get_album_info(&self, album_id: &str) -> Result<AlbumInfo> {
        let inner: AlbumInfoResponseInner =
            crate::services::api::subsonic::subsonic_get_envelope_checked(
                &self.client.http_client(),
                &self.server_url,
                "getAlbumInfo2",
                &self.subsonic_credential,
                &query_params(album_id),
                "getAlbumInfo2",
            )
            .await?;
        let info: AlbumInfo = inner.album_info.unwrap_or_default().into();
        debug!(
            "getAlbumInfo2: id={album_id} notes={} link={}",
            info.notes.is_some(),
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

    /// A `getAlbumInfo2` answer recorded from Navidrome 0.64.2 for an album
    /// whose info had never been fetched (so the server ran its agents for
    /// this call). Server address and share-image tokens are redacted.
    const RECORDED: &str = include_str!("../../../testdata/get_album_info2_navidrome_0_64_2.json");

    async fn info_from(base_url: String) -> Result<AlbumInfo> {
        let url = url::Url::parse(&base_url).expect("one-shot server url");
        AlbumInfoApiService::new(ApiClient::new(url, String::new()), base_url, "u=x".into())
            .get_album_info("al1")
            .await
    }

    #[tokio::test]
    async fn reads_the_notes_and_link_from_a_recorded_navidrome_answer() {
        let info = info_from(one_shot_server("200 OK", RECORDED).await)
            .await
            .expect("a recorded ok answer parses");
        let notes = info.notes.expect("the recorded answer has notes");
        assert!(
            notes.starts_with("BORN LIKE THIS is the sixth and final solo studio album"),
            "got: {notes}"
        );
        assert!(
            notes.ends_with("a sampled recording of Bukowski reading the poem."),
            "got: {notes}"
        );
        assert_eq!(
            info.external_url.as_deref(),
            Some("https://www.last.fm/music/MF+DOOM/Born+Like+This")
        );
    }

    /// Most albums come back with a page and no notes: Last.fm lists the
    /// album but nobody wrote its wiki. That is a link without notes, not an
    /// empty info.
    #[tokio::test]
    async fn a_page_without_notes_keeps_the_link() {
        const LINK_ONLY: &str = r#"{"subsonic-response":{"status":"ok","version":"1.16.1","albumInfo":{"musicBrainzId":"b537510f-e4f0-4978-aedf-97732c6fb310","lastFmUrl":"https://www.last.fm/music/AL-90/APXB"}}}"#;
        let info = info_from(one_shot_server("200 OK", LINK_ONLY).await)
            .await
            .expect("an ok answer parses");
        assert_eq!(
            info,
            AlbumInfo {
                notes: None,
                external_url: Some("https://www.last.fm/music/AL-90/APXB".to_string()),
            }
        );
    }

    /// Another agent's notes can carry tags; the views show plain text.
    #[tokio::test]
    async fn notes_are_read_as_plain_text() {
        const TAGGED: &str = r#"{"subsonic-response":{"status":"ok","version":"1.16.1","albumInfo":{"notes":"<p>A <b>loud</b> record.</p> <a href=\"https://example.org/a\">Read more</a>"}}}"#;
        let info = info_from(one_shot_server("200 OK", TAGGED).await)
            .await
            .expect("an ok answer parses");
        assert_eq!(
            info.notes.as_deref(),
            Some("A loud record. Read more (https://example.org/a)")
        );
    }

    #[test]
    fn asks_by_album_id_alone() {
        assert_eq!(query_params("al1"), [("id", "al1")]);
    }

    /// A failed call is an error, never an empty info that would read as
    /// "this album has no notes"; a rejected token routes to login.
    #[tokio::test]
    async fn a_failed_envelope_is_an_error_not_empty_notes() {
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

    /// An album the agents know nothing about answers ok with an empty
    /// object; empty strings read as absent.
    #[tokio::test]
    async fn an_empty_answer_is_an_empty_info() {
        const EMPTY: &str = r#"{"subsonic-response":{"status":"ok","version":"1.16.1","albumInfo":{"notes":"  ","lastFmUrl":""}}}"#;
        let info = info_from(one_shot_server("200 OK", EMPTY).await)
            .await
            .expect("an ok envelope with no info is not an error");
        assert_eq!(info, AlbumInfo::default());
    }
}
