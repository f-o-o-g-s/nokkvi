//! Libraries (music folders) API service.
//!
//! v1 fetches the list via the Subsonic `getMusicFolders` endpoint because
//! the Native API `/api/library` is gated by `adminOnlyMiddleware` and
//! returns 403 for non-admin users
//! (`reference-navidrome/server/nativeapi/native_api.go:88-94`).
//!
//! Subsonic `getMusicFolders` already filters server-side to the libraries
//! each user has access to via `getUserAccessibleLibraries`
//! (`reference-navidrome/server/subsonic/browsing.go:20-31`), so the
//! returned set is what nokkvi should show in the selector regardless of
//! whether the active session is admin or regular.
//!
//! Wire shape (Subsonic JSON):
//! ```json
//! {
//!   "subsonic-response": {
//!     "musicFolders": {
//!       "musicFolder": [
//!         { "id": 1, "name": "Music" },
//!         { "id": 2, "name": "Audiobooks" }
//!       ]
//!     }
//!   }
//! }
//! ```
//!
//! The `id` is a stable `int32` matching the server's `library_id` column,
//! which is the value to pass as the `library_id` query parameter on Native
//! API browse endpoints. Default serde behavior is used — Navidrome may add
//! extra fields (e.g. `path`, `lastScanAt`), and the parser must not start
//! failing when that happens.

use anyhow::{Context, Result};
use tracing::debug;

use crate::{
    services::api::{client::ApiClient, parse, subsonic},
    types::library::Library,
};

/// Subsonic envelope for `getMusicFolders`.
#[derive(Debug, serde::Deserialize)]
struct SubsonicMusicFoldersResponse {
    #[serde(rename = "subsonic-response")]
    subsonic_response: MusicFoldersResponseInner,
}

#[derive(Debug, serde::Deserialize)]
struct MusicFoldersResponseInner {
    #[serde(rename = "musicFolders")]
    music_folders: Option<MusicFoldersList>,
}

#[derive(Debug, serde::Deserialize)]
struct MusicFoldersList {
    #[serde(rename = "musicFolder")]
    music_folder: Option<Vec<MusicFolderEntry>>,
}

/// Wire shape of a single entry under `musicFolder`. Mirrors
/// `responses.MusicFolder` server-side (id: int32, name: string). Extra
/// fields are tolerated by default serde behavior.
#[derive(Debug, serde::Deserialize)]
struct MusicFolderEntry {
    id: i32,
    name: String,
}

impl From<MusicFolderEntry> for Library {
    fn from(value: MusicFolderEntry) -> Self {
        Library {
            id: value.id,
            name: value.name,
        }
    }
}

#[derive(Clone)]
pub struct LibrariesApiService {
    client: ApiClient,
    server_url: String,
    subsonic_credential: String,
}

impl LibrariesApiService {
    /// Create with a pre-authenticated `ApiClient` and the cached
    /// Subsonic credential string (`u=...&t=...&s=...` or
    /// `u=...&p=enc:...`). The `ApiClient` is unused today — the call goes
    /// through `subsonic_post` against the cached HTTP client — but is
    /// retained so a future migration to the Native `/api/library`
    /// endpoint (if admin-gating is relaxed upstream) doesn't change the
    /// constructor shape that the `subsonic_api_factory!` macro on
    /// `AppService` emits.
    pub fn new(client: ApiClient, server_url: String, subsonic_credential: String) -> Self {
        Self {
            client,
            server_url,
            subsonic_credential,
        }
    }

    /// Fetch the list of libraries (music folders) accessible to the
    /// authenticated user, sorted alphabetically by name.
    ///
    /// Returns an empty `Vec` when the server reports no folders or the
    /// response omits the `musicFolders` envelope entirely; both shapes
    /// are observed in practice on freshly-provisioned servers and on
    /// users with no library assignments.
    pub async fn load(&self) -> Result<Vec<Library>> {
        let response = subsonic::subsonic_post(
            &self.client.http_client(),
            &self.server_url,
            "getMusicFolders",
            &self.subsonic_credential,
            &[],
        )
        .await
        .context("Failed to fetch music folders from Subsonic API")?;

        let body = response
            .text()
            .await
            .context("Failed to read Subsonic getMusicFolders response")?;

        let parsed: SubsonicMusicFoldersResponse =
            parse::parse_json_with_preview(&body, "Subsonic getMusicFolders response")?;

        let entries = parsed
            .subsonic_response
            .music_folders
            .and_then(|list| list.music_folder)
            .unwrap_or_default();

        let mut libraries: Vec<Library> = entries.into_iter().map(Library::from).collect();
        libraries.sort_by(|a, b| a.name.cmp(&b.name));

        debug!(
            " LibrariesApiService: Loaded {} libraries from getMusicFolders",
            libraries.len()
        );

        Ok(libraries)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Round-trip the canonical Subsonic `getMusicFolders` shape into
    /// `Vec<Library>`. Covers the two-library happy path the popover
    /// will be tested against (Music Library + Longmont Potion Castle).
    #[test]
    fn parses_canonical_two_folder_response() {
        let body = r#"{
            "subsonic-response": {
                "status": "ok",
                "version": "1.8.0",
                "musicFolders": {
                    "musicFolder": [
                        { "id": 1, "name": "Music Library" },
                        { "id": 2, "name": "Longmont Potion Castle" }
                    ]
                }
            }
        }"#;

        let parsed: SubsonicMusicFoldersResponse = serde_json::from_str(body).unwrap();
        let libraries: Vec<Library> = parsed
            .subsonic_response
            .music_folders
            .and_then(|list| list.music_folder)
            .unwrap_or_default()
            .into_iter()
            .map(Library::from)
            .collect();

        assert_eq!(libraries.len(), 2);
        assert_eq!(libraries[0].id, 1);
        assert_eq!(libraries[0].name, "Music Library");
        assert_eq!(libraries[1].id, 2);
        assert_eq!(libraries[1].name, "Longmont Potion Castle");
    }

    /// Servers may omit `musicFolders` entirely on a brand-new install or
    /// for users with no library assignments. Parser must yield an empty
    /// vec, not error.
    #[test]
    fn missing_music_folders_envelope_yields_empty() {
        let body = r#"{
            "subsonic-response": {
                "status": "ok",
                "version": "1.8.0"
            }
        }"#;

        let parsed: SubsonicMusicFoldersResponse = serde_json::from_str(body).unwrap();
        let entries = parsed
            .subsonic_response
            .music_folders
            .and_then(|list| list.music_folder)
            .unwrap_or_default();

        assert!(entries.is_empty());
    }

    /// Unknown extra fields on the wire (Navidrome may add `path`,
    /// `lastScanAt`, `totalSize`, etc. in future versions) must be
    /// ignored, not rejected. Default serde behavior — i.e., no
    /// `#[serde(deny_unknown_fields)]` — is the contract.
    #[test]
    fn extra_unknown_fields_are_tolerated() {
        let body = r#"{
            "subsonic-response": {
                "status": "ok",
                "version": "1.8.0",
                "musicFolders": {
                    "musicFolder": [
                        {
                            "id": 1,
                            "name": "Music Library",
                            "path": "/music",
                            "lastScanAt": "2026-05-21T00:00:00Z",
                            "totalSize": 12345678
                        }
                    ]
                }
            }
        }"#;

        let parsed: SubsonicMusicFoldersResponse =
            serde_json::from_str(body).expect("extra fields must not cause parse failure");
        let entries = parsed
            .subsonic_response
            .music_folders
            .and_then(|list| list.music_folder)
            .unwrap_or_default();

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id, 1);
        assert_eq!(entries[0].name, "Music Library");
    }

    /// Empty `musicFolder` array is the explicit "no folders" shape some
    /// servers emit instead of omitting `musicFolders` entirely. Must
    /// also yield an empty vec.
    #[test]
    fn empty_music_folder_array_yields_empty() {
        let body = r#"{
            "subsonic-response": {
                "status": "ok",
                "version": "1.8.0",
                "musicFolders": {
                    "musicFolder": []
                }
            }
        }"#;

        let parsed: SubsonicMusicFoldersResponse = serde_json::from_str(body).unwrap();
        let entries = parsed
            .subsonic_response
            .music_folders
            .and_then(|list| list.music_folder)
            .unwrap_or_default();

        assert!(entries.is_empty());
    }
}
