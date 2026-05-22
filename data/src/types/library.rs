//! Music library (music folder) reported by the Navidrome server.
//!
//! v1 fetches the list via the Subsonic `getMusicFolders` endpoint because
//! the Native API `/api/library` is gated by `adminOnlyMiddleware`
//! (`reference-navidrome/server/nativeapi/native_api.go:88-94`). The `id` is
//! a stable integer matching the server's `library_id` column, used as the
//! `library_id` filter param on Native API browse requests.

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Deserialize)]
pub struct Library {
    pub id: i32,
    pub name: String,
}
