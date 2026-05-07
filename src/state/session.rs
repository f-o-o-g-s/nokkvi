//! Stored session and active-playlist context.

/// Stored session for JWT-based auto-login.
///
/// Replaces the anonymous `Option<(String, String, String, String)>` tuple
/// that made field order ambiguous at every destructure site.
#[derive(Debug, Clone)]
pub struct StoredSession {
    pub server_url: String,
    pub username: String,
    pub jwt_token: String,
    pub subsonic_credential: String,
}

/// Identity of the playlist currently loaded in the queue.
///
/// Replaces the anonymous `Option<(String, String, String)>` tuple.
/// Set on PlayPlaylist, cleared on non-playlist play actions.
#[derive(Debug, Clone)]
pub struct ActivePlaylistContext {
    pub id: String,
    pub name: String,
    pub comment: String,
}
