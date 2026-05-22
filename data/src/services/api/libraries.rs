//! Libraries (music folders) API service.
//!
//! Two-tier fetch:
//!
//! 1. **Native `/api/library`** (admin-only — gated by
//!    `adminOnlyMiddleware` in
//!    `reference-navidrome/server/nativeapi/native_api.go:88-94`). Returns
//!    a JSON array of rich library records including `totalSongs`. When
//!    this succeeds, the popover's right-column counts populate.
//! 2. **Subsonic `getMusicFolders`** — fallback for non-admin sessions
//!    where step 1 returns 403. The Subsonic wire shape is `id` + `name`
//!    only (no counts), so the popover renders the right column blank
//!    for non-admins.
//!
//! Subsonic `getMusicFolders` already filters server-side to the libraries
//! each user has access to via `getUserAccessibleLibraries`
//! (`reference-navidrome/server/subsonic/browsing.go:20-31`), so the
//! returned set matches what the admin endpoint would have returned for
//! the same user — only the counts differ.
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
//! Wire shape (Native `/api/library` JSON):
//! ```json
//! [
//!   { "id": 1, "name": "Music", "totalSongs": 13627, "path": "/music", ... },
//!   { "id": 2, "name": "Audiobooks", "totalSongs": 312, "path": "/audio", ... }
//! ]
//! ```
//!
//! The `id` is a stable `int32` matching the server's `library_id` column,
//! which is the value to pass as the `library_id` query parameter on Native
//! API browse endpoints. Default serde behavior is used — Navidrome may add
//! extra fields, and the parser must not start failing when that happens.

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
            // Subsonic getMusicFolders does not carry a count. The
            // UI renders the right column blank in that case.
            song_count: None,
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
    /// `u=...&p=enc:...`). The `ApiClient` is used for the Native
    /// `/api/library` first-try; the credential string drives the
    /// Subsonic fallback.
    pub fn new(client: ApiClient, server_url: String, subsonic_credential: String) -> Self {
        Self {
            client,
            server_url,
            subsonic_credential,
        }
    }

    /// Fetch the list of libraries accessible to the authenticated user,
    /// sorted alphabetically by name.
    ///
    /// Tries Native `/api/library` first (admin sessions get
    /// per-library `totalSongs`); on any failure (403 for non-admin,
    /// connection drop, parse mismatch) falls back to Subsonic
    /// `getMusicFolders`. The fallback returns the same id+name set
    /// minus the count column, so non-admin users still see the
    /// popover — the right-side count column just renders blank.
    ///
    /// Returns an empty `Vec` when the server reports no folders;
    /// shapes that report an empty list (e.g. brand-new install, user
    /// with no library assignments) yield `Vec::new()`, not an error.
    pub async fn load(&self) -> Result<Vec<Library>> {
        match self.load_native().await {
            Ok(libraries) => {
                debug!(
                    "LibrariesApiService: loaded {} libraries from /api/library (with counts)",
                    libraries.len()
                );
                Ok(libraries)
            }
            Err(native_err) => {
                debug!(
                    "LibrariesApiService: /api/library failed ({native_err:#}); falling back to Subsonic getMusicFolders"
                );
                self.load_via_subsonic().await
            }
        }
    }

    /// Hit the Native admin-only `/api/library` endpoint. Returns the
    /// parsed library list with `song_count` populated. Errors when the
    /// session is non-admin (403), when the endpoint is unreachable, or
    /// when the response doesn't parse — all of which are caller-side
    /// signals to fall back to Subsonic.
    async fn load_native(&self) -> Result<Vec<Library>> {
        let body = self
            .client
            .get("/api/library", &[])
            .await
            .context("GET /api/library failed")?;

        let mut libraries: Vec<Library> =
            parse::parse_json_with_preview(&body, "Native /api/library response")
                .context("parse /api/library JSON array")?;

        libraries.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(libraries)
    }

    /// Fall back to Subsonic `getMusicFolders`. Returns the id+name
    /// list without per-library counts.
    async fn load_via_subsonic(&self) -> Result<Vec<Library>> {
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
            "LibrariesApiService: loaded {} libraries from getMusicFolders (no counts)",
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

    /// Native `/api/library` response shape — JSON array with rich
    /// records including `totalSongs`. Locks in the `song_count`
    /// population path so a future server-side rename of the field
    /// surfaces as a test failure rather than a silent count-loss.
    #[test]
    fn native_api_library_shape_populates_song_count() {
        let body = r#"[
            {
                "id": 1,
                "name": "Music",
                "path": "/music",
                "totalSongs": 13627,
                "totalAlbums": 1502,
                "totalArtists": 437,
                "lastScanAt": "2026-05-21T00:00:00Z"
            },
            {
                "id": 2,
                "name": "Audiobooks",
                "path": "/audio",
                "totalSongs": 312
            }
        ]"#;

        let libraries: Vec<Library> = serde_json::from_str(body).expect("native shape parse");
        assert_eq!(libraries.len(), 2);
        assert_eq!(libraries[0].id, 1);
        assert_eq!(libraries[0].name, "Music");
        assert_eq!(libraries[0].song_count, Some(13_627));
        assert_eq!(libraries[1].id, 2);
        assert_eq!(libraries[1].name, "Audiobooks");
        assert_eq!(libraries[1].song_count, Some(312));
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
