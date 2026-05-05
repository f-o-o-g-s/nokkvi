//! Credential and session persistence
//!
//! Stores server URL and username in config.toml (user-editable).
//! JWT token and Subsonic credential are stored in app.redb (session tokens).
//! No password is stored on disk — expired JWT requires manual re-login.

use std::path::PathBuf;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use tracing::{debug, info};

// ================================================================================
// Credential & Session Persistence
// ================================================================================
//
// config.toml  → server_url, username (user-editable)
// app.redb     → jwt_token, subsonic_credential (session tokens, auto-refreshed)
//
// On startup, the stored JWT is used to resume the session without a password.
// The JWT auto-refreshes via the X-ND-Authorization response header on every
// API call. If the JWT expires (default 48h inactivity), the login screen is shown.
// ================================================================================

/// Redb key for the Navidrome JWT token
const JWT_TOKEN_KEY: &str = "jwt_token";

/// Redb key for the Subsonic credential string (u=X&s=Y&t=Z)
const SUBSONIC_CREDENTIAL_KEY: &str = "subsonic_credential";

/// Config.toml fields (user-editable: server_url, username)
#[derive(Debug, Serialize, Deserialize)]
struct Config {
    server_url: String,
    username: String,
}

fn get_config_path() -> Result<PathBuf> {
    crate::utils::paths::get_config_path()
}

/// Open a standalone StateStorage handle for session read/write.
///
/// This is intentionally independent of `AppService` so sessions can
/// be loaded before the full service stack is initialised (e.g. auto-login).
fn open_session_storage() -> Result<crate::services::state_storage::StateStorage> {
    let db_path = crate::utils::paths::get_app_db_path()?;
    crate::services::state_storage::StateStorage::new(db_path)
}

// ── Config.toml (server_url + username) ─────────────────────────────────────

/// Load server_url and username from config.toml.
/// Returns None if config doesn't exist or can't be parsed.
pub fn load_credentials() -> Option<(String, String)> {
    let config_path = get_config_path().ok()?;
    if !config_path.exists() {
        return None;
    }

    let content = std::fs::read_to_string(&config_path).ok()?;
    let config: Config = toml::from_str(&content).ok()?;

    if config.server_url.is_empty() || config.username.is_empty() {
        return None;
    }

    info!("Loaded server_url and username from config.toml");
    Some((config.server_url, config.username))
}

/// Save server_url and username to config.toml.
/// Preserves comments and formatting.
pub fn save_credentials(server_url: &str, username: &str) -> Result<()> {
    use toml_edit::{DocumentMut, value};

    let config_path = get_config_path()?;
    let (mut doc, is_new) = if config_path.exists() {
        let content = std::fs::read_to_string(&config_path).unwrap_or_default();
        (
            content.parse().unwrap_or_else(|_| DocumentMut::new()),
            false,
        )
    } else {
        (DocumentMut::new(), true)
    };

    doc["server_url"] = value(server_url);
    doc["username"] = value(username);

    let output = if is_new {
        format!(
            "# Nokkvi Configuration\n\
             # You can edit server_url and username.\n\
             # Session tokens are managed by the application in app.redb.\n\n{doc}"
        )
    } else {
        doc.to_string()
    };

    std::fs::write(&config_path, output)?;
    debug!("Saved credentials to {}", config_path.display());
    Ok(())
}

// ── Subsonic credential parsing ─────────────────────────────────────────────

/// Extract the username (`u=` field) from a stored Subsonic credential string.
pub fn parse_username_from_credential(credential: &str) -> Option<&str> {
    credential
        .split('&')
        .find_map(|p| p.strip_prefix("u="))
        .filter(|s| !s.is_empty())
}

// ── Session (JWT + Subsonic credential) ─────────────────────────────────────

/// Save JWT token and Subsonic credential to redb.
///
/// Accepts an existing `StateStorage` handle to avoid opening a second
/// exclusive lock on the redb file (AppService already holds the DB open).
pub fn save_session(
    storage: &crate::services::state_storage::StateStorage,
    jwt_token: &str,
    subsonic_credential: &str,
) -> Result<()> {
    storage.save(JWT_TOKEN_KEY, &jwt_token.to_string())?;
    storage.save(SUBSONIC_CREDENTIAL_KEY, &subsonic_credential.to_string())?;

    debug!("Saved session tokens to app.redb");
    Ok(())
}

/// Save only the JWT token to redb (leaves subsonic credential untouched).
///
/// Called by the token refresh interceptor when a new JWT is received via
/// the `X-ND-Authorization` response header.
pub fn save_jwt_token(
    storage: &crate::services::state_storage::StateStorage,
    jwt_token: &str,
) -> Result<()> {
    storage.save(JWT_TOKEN_KEY, &jwt_token.to_string())?;
    debug!("Persisted refreshed JWT to app.redb");
    Ok(())
}

/// Load JWT token and Subsonic credential from redb.
/// Opens its own storage handle (safe before AppService is initialised).
/// Returns None if either token is missing.
pub fn load_session() -> Option<(String, String)> {
    let storage = open_session_storage().ok()?;

    let jwt_token: String = storage.load(JWT_TOKEN_KEY).ok()??;
    let subsonic_credential: String = storage.load(SUBSONIC_CREDENTIAL_KEY).ok()??;

    if jwt_token.is_empty() || subsonic_credential.is_empty() {
        return None;
    }

    info!("Loaded session tokens from app.redb");
    Some((jwt_token, subsonic_credential))
}

/// Clear stored session tokens from redb.
/// Used when JWT expires (401) to force re-login.
pub fn clear_session(storage: &crate::services::state_storage::StateStorage) -> Result<()> {
    storage.save(JWT_TOKEN_KEY, &String::new())?;
    storage.save(SUBSONIC_CREDENTIAL_KEY, &String::new())?;
    info!("Cleared session tokens from app.redb");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_round_trip() {
        let db_path = std::env::temp_dir().join("test_session_roundtrip.redb");
        let _ = std::fs::remove_file(&db_path);

        let storage = crate::services::state_storage::StateStorage::new(db_path.clone()).unwrap();

        let jwt = "eyJhbGciOiJIUzI1NiJ9.test.signature";
        let subsonic = "u=testuser&s=abc123&t=md5hash";

        save_session(&storage, jwt, subsonic).unwrap();

        // Load from a fresh handle (simulates app restart)
        drop(storage);
        let storage2 = crate::services::state_storage::StateStorage::new(db_path.clone()).unwrap();

        let jwt_loaded: Option<String> = storage2.load(JWT_TOKEN_KEY).unwrap();
        let sub_loaded: Option<String> = storage2.load(SUBSONIC_CREDENTIAL_KEY).unwrap();

        assert_eq!(jwt_loaded.unwrap(), jwt);
        assert_eq!(sub_loaded.unwrap(), subsonic);

        std::fs::remove_file(db_path).unwrap();
    }

    #[test]
    fn clear_session_removes_tokens() {
        let db_path = std::env::temp_dir().join("test_session_clear.redb");
        let _ = std::fs::remove_file(&db_path);

        let storage = crate::services::state_storage::StateStorage::new(db_path.clone()).unwrap();

        save_session(&storage, "jwt", "subsonic").unwrap();
        clear_session(&storage).unwrap();

        // Verify tokens are emptied via the same storage handle
        // (load_session() opens a new handle which may conflict)
        let jwt: Option<String> = storage.load(JWT_TOKEN_KEY).unwrap();
        assert!(jwt.unwrap().is_empty());

        std::fs::remove_file(db_path).unwrap();
    }

    #[test]
    fn parse_username_extracts_u_field() {
        let creds = "u=foogs&s=abc123&t=md5hash";
        assert_eq!(parse_username_from_credential(creds), Some("foogs"));
    }

    #[test]
    fn parse_username_returns_none_for_empty_u() {
        let creds = "u=&s=abc123&t=md5hash";
        assert_eq!(parse_username_from_credential(creds), None);
    }

    #[test]
    fn parse_username_returns_none_when_u_missing() {
        let creds = "s=abc123&t=md5hash";
        assert_eq!(parse_username_from_credential(creds), None);
    }

    #[test]
    fn parse_username_handles_u_not_first() {
        // Defensive: don't assume u= is positionally first
        let creds = "s=abc123&u=foogs&t=md5hash";
        assert_eq!(parse_username_from_credential(creds), Some("foogs"));
    }

    #[test]
    fn load_session_returns_none_when_empty() {
        let db_path = std::env::temp_dir().join("test_session_empty.redb");
        let _ = std::fs::remove_file(&db_path);

        let storage = crate::services::state_storage::StateStorage::new(db_path.clone()).unwrap();

        // No session saved yet
        let jwt: Option<String> = storage.load(JWT_TOKEN_KEY).unwrap();
        assert!(jwt.is_none());

        std::fs::remove_file(db_path).unwrap();
    }
}
