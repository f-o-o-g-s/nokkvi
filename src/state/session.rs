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

/// Identity and metadata of the playlist currently loaded in the queue.
///
/// Replaces the anonymous `Option<(String, String, String)>` tuple.
/// Set on PlayPlaylist, cleared on non-playlist play actions.
///
/// `song_count` / `duration_secs` / `public` / `updated` are captured from the
/// full playlist model on the PlayPlaylist paths via [`Self::from_playlist`].
/// Sites that only have id/name/comment in scope (split-view save, fresh
/// creation, session restore) build a [`Self::minimal`] context; the playlist
/// strip then falls back to the live queue length for the count and hides the
/// duration / updated-date when these stay unset.
#[derive(Debug, Clone, PartialEq)]
pub struct ActivePlaylistContext {
    pub id: String,
    pub name: String,
    pub comment: String,
    /// Song count, or 0 when unknown (strip falls back to the queue length).
    pub song_count: u32,
    /// Total duration in seconds, or 0.0 when unknown (strip hides the segment).
    pub duration_secs: f32,
    /// Public/private visibility — drives the lock chip in the expanded strip.
    pub public: bool,
    /// Last-updated timestamp (raw ISO-8601 string, formatted at render).
    /// Empty when unknown.
    pub updated: String,
}

impl ActivePlaylistContext {
    /// Build from the full playlist view-model (PlayPlaylist paths), capturing
    /// count / duration / visibility / updated so the strip renders complete
    /// metadata without a re-lookup.
    pub fn from_playlist(p: &nokkvi_data::backend::playlists::PlaylistUIViewData) -> Self {
        Self {
            id: p.id.clone(),
            name: p.name.clone(),
            comment: p.comment.clone(),
            song_count: p.song_count,
            duration_secs: p.duration,
            public: p.public,
            updated: p.updated_at.clone(),
        }
    }

    /// Rebuild from persisted settings on session restore. Carries the full
    /// metadata captured when the playlist was last played or re-synced, so
    /// the restored banner is complete and correct (visibility included) before
    /// the playlists list reloads. `handle_playlists_loaded` later re-syncs it
    /// against fresh server metadata.
    #[allow(clippy::too_many_arguments)]
    pub fn from_persisted(
        id: String,
        name: String,
        comment: String,
        duration_secs: f32,
        updated: String,
        public: bool,
        song_count: u32,
    ) -> Self {
        Self {
            id,
            name,
            comment,
            song_count,
            duration_secs,
            public,
            updated,
        }
    }

    /// Build with only the always-available fields. Count / duration / public
    /// / updated degrade to neutral defaults; the strip falls back to the live
    /// queue length for the count and hides the rest.
    pub fn minimal(id: String, name: String, comment: String) -> Self {
        Self {
            id,
            name,
            comment,
            song_count: 0,
            duration_secs: 0.0,
            public: false,
            updated: String::new(),
        }
    }
}
