//! Music library (music folder) reported by the Navidrome server.
//!
//! v1 fetches the list via the Subsonic `getMusicFolders` endpoint because
//! the Native API `/api/library` is gated by `adminOnlyMiddleware`
//! (`reference-navidrome/server/nativeapi/native_api.go:88-94`). The `id` is
//! a stable integer matching the server's `library_id` column, used as the
//! `library_id` filter param on Native API browse requests.
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
        };
        let bytes =
            bincode_next::encode_to_vec(&lib, bincode_next::config::standard()).expect("encode");
        let (decoded, _): (Library, usize) =
            bincode_next::decode_from_slice(&bytes, bincode_next::config::standard())
                .expect("decode");
        assert_eq!(decoded, lib);
    }

    /// JSON deserialize via Subsonic shape (`id` + `name`) — the
    /// minimal-fields contract.
    #[test]
    fn library_json_deserialize_minimal_shape() {
        let body = r#"{"id": 7, "name": "Audiobooks"}"#;
        let lib: Library = serde_json::from_str(body).expect("deserialize");
        assert_eq!(lib.id, 7);
        assert_eq!(lib.name, "Audiobooks");
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
