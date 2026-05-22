//! Music library (music folder) reported by the Navidrome server.
//!
//! `LibrariesApiService::load()` tries the Native `/api/library` endpoint
//! first (admin-only — gated by `adminOnlyMiddleware` in
//! `reference-navidrome/server/nativeapi/native_api.go:88-94`); it returns
//! a rich response that includes `totalSongs`. On 403 (non-admin session)
//! it falls back to Subsonic `getMusicFolders`, which carries only id +
//! name. The `song_count` field therefore is `Some(n)` when the admin
//! path succeeded and `None` when the fallback was used.
//!
//! `id` is a stable integer matching the server's `library_id` column,
//! used as the `library_id` filter param on Native API browse requests.
//!
//! Persistence: only `active_library_ids: HashSet<i32>` is persisted to
//! redb. `Library` derives `bincode_next::Encode + Decode` so a future
//! commit can persist the full list without revisiting this file. Default
//! serde behavior accepts unknown fields — Navidrome may extend the
//! `getMusicFolders` shape with `path` / `lastScanAt` / `totalSize` and
//! callers must not start failing to deserialize when that happens.

#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Deserialize,
    bincode_next::Encode,
    bincode_next::Decode,
)]
pub struct Library {
    pub id: i32,
    pub name: String,
    /// Total song count. `Some(n)` when fetched via the admin-only
    /// Native `/api/library` endpoint; `None` when falling back to
    /// Subsonic `getMusicFolders` (the Subsonic response carries no
    /// counts). The UI renders the column blank in the `None` case.
    #[serde(default, rename = "totalSongs")]
    pub song_count: Option<u32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `Library` round-trips through bincode. Locks in the persistence
    /// shape so a future commit can persist `Vec<Library>` without
    /// silently changing the wire format and tripping a downgrade.
    #[test]
    fn library_bincode_round_trip() {
        let lib = Library {
            id: 42,
            name: "Music".to_string(),
            song_count: Some(13_627),
        };
        let bytes =
            bincode_next::encode_to_vec(&lib, bincode_next::config::standard()).expect("encode");
        let (decoded, _): (Library, usize) =
            bincode_next::decode_from_slice(&bytes, bincode_next::config::standard())
                .expect("decode");
        assert_eq!(decoded, lib);
    }

    /// Subsonic shape — `id` + `name` only, no `totalSongs`. `song_count`
    /// must default to `None` (the `#[serde(default)]` contract).
    #[test]
    fn library_json_deserialize_subsonic_shape() {
        let body = r#"{"id": 7, "name": "Audiobooks"}"#;
        let lib: Library = serde_json::from_str(body).expect("deserialize");
        assert_eq!(lib.id, 7);
        assert_eq!(lib.name, "Audiobooks");
        assert_eq!(lib.song_count, None);
    }

    /// Native `/api/library` shape — `totalSongs` is present and maps to
    /// `song_count`. Locks in the camelCase rename so a server-side rename
    /// (or a transcribe to snake_case) shows up as a test failure rather
    /// than a silent count-loss.
    #[test]
    fn library_json_deserialize_native_shape_with_total_songs() {
        let body = r#"{"id": 7, "name": "Audiobooks", "totalSongs": 312}"#;
        let lib: Library = serde_json::from_str(body).expect("deserialize");
        assert_eq!(lib.id, 7);
        assert_eq!(lib.name, "Audiobooks");
        assert_eq!(lib.song_count, Some(312));
    }

    /// Unknown extra fields on the wire must be ignored — default serde
    /// behavior. Documents the "no `deny_unknown_fields`" contract.
    #[test]
    fn library_json_deserialize_tolerates_extra_fields() {
        let body = r#"{"id": 7, "name": "Audiobooks", "path": "/data/audio", "totalSize": 12345}"#;
        let lib: Library = serde_json::from_str(body).expect("deserialize");
        assert_eq!(lib.id, 7);
        assert_eq!(lib.name, "Audiobooks");
    }
}
