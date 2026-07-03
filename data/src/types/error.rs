use thiserror::Error;

/// Errors emitted by the data layer at the boundary between
/// `crate::services::api` and consumers.
///
/// Two variants — every other API failure path uses `anyhow::Result`. The
/// 401 case is broken out so the UI can downcast and drop to the login
/// screen instead of merely surfacing a toast; the 403 case is broken out so
/// permission failures (artwork upload disabled server-side, playlist not
/// owned) can map to a friendly toast instead of a raw status string.
#[derive(Debug, Error)]
pub enum NokkviError {
    #[error("Unauthorized: Session has expired or credentials are invalid")]
    Unauthorized,
    /// HTTP 403 from the Navidrome native API. Carries the same
    /// `"{ctx} failed with status …: {body}"` detail string the generic arm
    /// would have produced, so logs stay as informative as before the split.
    #[error("Forbidden: {0}")]
    Forbidden(String),
}

impl NokkviError {
    /// Returns true when a stringified error chain carries the
    /// [`NokkviError::Unauthorized`] Display marker. Used by the 5 loader
    /// handlers whose results are flattened to `Result<_, String>` at the
    /// (Clone-able) `Message` boundary, where the typed
    /// `session_expired_message` downcast is unreachable. The marker is THIS
    /// crate's own `#[error("Unauthorized: ...")]` text (set at the
    /// client.rs / subsonic.rs 401->typed-error boundary), not the Navidrome
    /// wire body -- so only rewording that attribute breaks the match.
    pub fn is_unauthorized_str(s: &str) -> bool {
        s.contains("Unauthorized")
    }

    /// Returns true when a stringified error chain carries the
    /// [`NokkviError::Forbidden`] Display marker. Same flatten-to-`String`
    /// rationale as [`Self::is_unauthorized_str`]: upload/reset completion
    /// results cross the (Clone-able) `Message` boundary as strings, where a
    /// typed downcast is unreachable. The marker is this crate's own
    /// `#[error("Forbidden: ...")]` prefix, stamped at the client.rs
    /// 403→typed-error boundary — the only native-API path that can emit it.
    pub fn is_forbidden_str(s: &str) -> bool {
        s.contains("Forbidden")
    }
}

#[cfg(test)]
mod tests {
    use super::NokkviError;

    #[test]
    fn is_unauthorized_str_matches_display_marker() {
        // The bare Display text of the typed variant carries the marker.
        assert!(NokkviError::is_unauthorized_str(
            &NokkviError::Unauthorized.to_string()
        ));
        // A `{:#}`-wrapped / prefixed form (as flattened into the loader
        // `Result<_, String>` boundary, mirroring general.rs:635) still matches.
        assert!(NokkviError::is_unauthorized_str(
            "Failed to fetch albums: Unauthorized: Session expired"
        ));
        // An unrelated error chain does not match.
        assert!(!NokkviError::is_unauthorized_str(
            "API request failed with status 500: boom"
        ));
    }

    #[test]
    fn is_forbidden_str_matches_display_marker() {
        // The bare Display text of the typed variant carries the marker.
        assert!(NokkviError::is_forbidden_str(
            &NokkviError::Forbidden("API POST /api/radio/1/image failed with status 403".into())
                .to_string()
        ));
        // A `{:#}`-wrapped / prefixed chain form still matches.
        assert!(NokkviError::is_forbidden_str(
            "upload failed: Forbidden: API POST /api/playlist/1/image failed with status 403"
        ));
        // Unrelated errors do not match; the Unauthorized marker stays disjoint.
        assert!(!NokkviError::is_forbidden_str(
            "API request failed with status 500: boom"
        ));
        assert!(!NokkviError::is_unauthorized_str(
            &NokkviError::Forbidden("x".into()).to_string()
        ));
        assert!(!NokkviError::is_forbidden_str(
            &NokkviError::Unauthorized.to_string()
        ));
    }
}
