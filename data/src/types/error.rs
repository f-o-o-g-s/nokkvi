use thiserror::Error;

/// Errors emitted by the data layer at the boundary between
/// `crate::services::api` and consumers.
///
/// Currently a single variant — every other API failure path uses
/// `anyhow::Result`. The 401 case is broken out so the UI can downcast
/// and drop to the login screen instead of merely surfacing a toast.
#[derive(Debug, Error)]
pub enum NokkviError {
    #[error("Unauthorized: Session has expired or credentials are invalid")]
    Unauthorized,
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
}
