//! Credential and session persistence
//!
//! Stores server URL and username in config.toml (user-editable).
//! JWT token, Subsonic credential and the session's owner are stored in
//! app.redb (session tokens).
//! No password is stored on disk — expired JWT requires manual re-login.

use std::path::PathBuf;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use tracing::{debug, info};

use crate::services::{queue::slots::QueueOwner, state_storage::StateStorage};

// ================================================================================
// Credential & Session Persistence
// ================================================================================
//
// config.toml  → server_url, username (user-editable)
// app.redb     → jwt_token, subsonic_credential (session tokens, auto-refreshed),
//                session_owner (the server + user the tokens belong to)
//
// On startup, the stored JWT is used to resume the session without a password,
// as long as config.toml still names the session's owner. The Subsonic
// credential is the token pair from /auth/login, or an OpenSubsonic API key
// (`services::api_key`). The JWT auto-refreshes via the X-ND-Authorization
// response header on every API call. If the JWT expires (default 48h
// inactivity), the login screen is shown.
// ================================================================================

/// Redb key for the Navidrome JWT token.
///
/// Re-exported from `services::storage_keys::JWT_TOKEN`; see that module
/// for the on-disk-compat invariant.
const JWT_TOKEN_KEY: &str = crate::services::storage_keys::JWT_TOKEN;

/// Redb key for the Subsonic credential string (`u=X&s=Y&t=Z`, or
/// `apiKey=K`).
///
/// Re-exported from `services::storage_keys::SUBSONIC_CREDENTIAL`; see that
/// module for the on-disk-compat invariant.
const SUBSONIC_CREDENTIAL_KEY: &str = crate::services::storage_keys::SUBSONIC_CREDENTIAL;

/// Redb key for the stored session's [`SessionOwner`].
const SESSION_OWNER_KEY: &str = crate::services::storage_keys::SESSION_OWNER;

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

/// Header written above `server_url` when the login creates config.toml.
const NEW_CONFIG_HEADER: &str = "# Nokkvi Configuration\n\
                                 # You can edit server_url and username.\n\
                                 # Session tokens are managed by the application in app.redb.\n\n";

/// Save server_url and username to config.toml.
///
/// Preserves comments and formatting, and fails loudly: an unreadable or
/// unparseable config.toml returns `Err` and is left untouched rather than
/// rewritten with only these two keys (see `edit_config_doc`). The write
/// routes through `write_atomic`, which also suppresses the config-watcher's
/// reload event. Safe to suppress: auto-login reads credentials directly from
/// `load_credentials()` / redb, never via the file watcher.
pub fn save_credentials(server_url: &str, username: &str) -> Result<()> {
    save_credentials_at(&get_config_path()?, server_url, username)
}

fn save_credentials_at(
    config_path: &std::path::Path,
    server_url: &str,
    username: &str,
) -> Result<()> {
    use toml_edit::value;

    let is_new = !config_path.exists();
    crate::utils::paths::edit_config_doc(config_path, |doc| {
        doc["server_url"] = value(server_url);
        doc["username"] = value(username);
        if is_new && let Some(mut key) = doc.as_table_mut().key_mut("server_url") {
            key.leaf_decor_mut().set_prefix(NEW_CONFIG_HEADER);
        }
        Ok(())
    })?;
    debug!("Saved credentials to {}", config_path.display());
    Ok(())
}

// ── Session (JWT + Subsonic credential + owner) ──────────────────────────────

/// Who a stored session belongs to: the server URL it connected to (the
/// resolved one, as config.toml stores it) and the username it logged in with.
///
/// Saved beside the JWT and the Subsonic credential. A resume checks it
/// against config.toml ([`resume_identity`]), so the session's JWT and its
/// Subsonic credential (the token pair, or an API key that works until it is
/// revoked) never go to a server or user config.toml was since edited to
/// name, and the username never has to be read back out of the credential:
/// an API key credential carries none.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionOwner {
    pub server_url: String,
    pub username: String,
}

impl SessionOwner {
    /// Whether `server_url` and `username` name this owner, compared like the
    /// queue's owner stamp: URL normalized, username case-insensitive.
    pub fn is(&self, server_url: &str, username: &str) -> bool {
        QueueOwner::new(&self.server_url, &self.username) == QueueOwner::new(server_url, username)
    }
}

/// A session loaded from redb for auto-login. `Debug` leaves out the JWT and
/// the credential.
#[derive(Clone)]
pub struct LoadedSession {
    pub jwt_token: String,
    pub subsonic_credential: String,
    /// `None` for a session saved before owners were stored.
    pub owner: Option<SessionOwner>,
}

impl std::fmt::Debug for LoadedSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoadedSession")
            .field("owner", &self.owner)
            .finish_non_exhaustive()
    }
}

/// The server and user a stored session resumes as, given config.toml's pair
/// (`None` when config.toml has none) and the session's owner.
///
/// - config.toml names the owner: config.toml's pair.
/// - config.toml names another server or user: `None`, so the login screen
///   shows config.toml's pair and the session's tokens stay home.
/// - config.toml has no pair: the owner's.
/// - A session saved before owners were stored follows config.toml, as
///   before, and has nowhere to resume without it.
pub fn resume_identity(
    config: Option<&(String, String)>,
    owner: Option<&SessionOwner>,
) -> Option<(String, String)> {
    match (config, owner) {
        (Some((server_url, username)), Some(owner)) => owner
            .is(server_url, username)
            .then(|| (server_url.clone(), username.clone())),
        (Some(pair), None) => Some(pair.clone()),
        (None, Some(owner)) => Some((owner.server_url.clone(), owner.username.clone())),
        (None, None) => None,
    }
}

/// JSON-encode `value` the way [`StateStorage::save`] does, for a
/// [`StateStorage::write_batch`] put.
fn json<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    Ok(serde_json::to_vec(value)?)
}

/// Save the JWT, the Subsonic credential and their owner to redb, in one
/// transaction.
///
/// Accepts an existing `StateStorage` handle to avoid opening a second
/// exclusive lock on the redb file (AppService already holds the DB open).
pub fn save_session(
    storage: &StateStorage,
    owner: &SessionOwner,
    jwt_token: &str,
    subsonic_credential: &str,
) -> Result<()> {
    storage.write_batch(
        &[
            (JWT_TOKEN_KEY, &json(&jwt_token)?),
            (SUBSONIC_CREDENTIAL_KEY, &json(&subsonic_credential)?),
            (SESSION_OWNER_KEY, &json(owner)?),
        ],
        &[],
    )?;

    debug!("Saved session tokens to app.redb");
    Ok(())
}

/// Save the Subsonic credential and its owner, leaving the JWT alone: a
/// session that moved to an API key mid-run (the JWT on disk is whatever the
/// refresh policy last persisted, which must not be rolled back).
pub fn save_subsonic_credential(
    storage: &StateStorage,
    owner: &SessionOwner,
    subsonic_credential: &str,
) -> Result<()> {
    storage.write_batch(
        &[
            (SUBSONIC_CREDENTIAL_KEY, &json(&subsonic_credential)?),
            (SESSION_OWNER_KEY, &json(owner)?),
        ],
        &[],
    )?;
    debug!("Saved the session's Subsonic credential to app.redb");
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

/// Load the stored session from redb.
/// Opens its own storage handle (safe before AppService is initialised).
/// Returns None if either token is missing.
pub fn load_session() -> Option<LoadedSession> {
    let storage = open_session_storage().ok()?;
    let session = load_session_from(&storage)?;
    info!("Loaded session tokens from app.redb");
    Some(session)
}

fn load_session_from(storage: &StateStorage) -> Option<LoadedSession> {
    let jwt_token: String = storage.load(JWT_TOKEN_KEY).ok()??;
    let subsonic_credential: String = storage.load(SUBSONIC_CREDENTIAL_KEY).ok()??;

    if jwt_token.is_empty() || subsonic_credential.is_empty() {
        return None;
    }

    // An absent owner is a session saved before owners were stored (it
    // follows config.toml, as it always did). One that is stored but can't
    // be read gives no way to tell where the tokens belong: no resume.
    let owner = match storage.load::<SessionOwner>(SESSION_OWNER_KEY) {
        Ok(owner) => owner,
        Err(e) => {
            tracing::warn!("Stored session not resumed: its owner can't be read: {e:#}");
            return None;
        }
    };
    Some(LoadedSession {
        jwt_token,
        subsonic_credential,
        owner,
    })
}

/// Clear stored session tokens (and their owner) from redb.
/// Used when JWT expires (401) to force re-login.
pub fn clear_session(storage: &StateStorage) -> Result<()> {
    let empty = json(&"")?;
    storage.write_batch(
        &[(JWT_TOKEN_KEY, &empty), (SUBSONIC_CREDENTIAL_KEY, &empty)],
        &[SESSION_OWNER_KEY],
    )?;
    info!("Cleared session tokens from app.redb");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_round_trip() {
        let temp = tempfile::TempDir::new().unwrap();
        let db_path = temp.path().join("session.redb");

        let storage = crate::services::state_storage::StateStorage::new(db_path.clone()).unwrap();

        let jwt = "eyJhbGciOiJIUzI1NiJ9.test.signature";
        let subsonic = "u=testuser&s=abc123&t=md5hash";

        save_session(
            &storage,
            &owner("https://music.example.com", "testuser"),
            jwt,
            subsonic,
        )
        .unwrap();

        // Load from a fresh handle (simulates app restart)
        drop(storage);
        let storage2 = crate::services::state_storage::StateStorage::new(db_path).unwrap();

        let loaded = load_session_from(&storage2).expect("a stored session");
        assert_eq!(loaded.jwt_token, jwt);
        assert_eq!(loaded.subsonic_credential, subsonic);
        assert_eq!(
            loaded.owner,
            Some(owner("https://music.example.com", "testuser"))
        );
    }

    fn owner(server_url: &str, username: &str) -> SessionOwner {
        SessionOwner {
            server_url: server_url.into(),
            username: username.into(),
        }
    }

    fn pair(server_url: &str, username: &str) -> (String, String) {
        (server_url.into(), username.into())
    }

    /// A session saved before owners were stored loads with no owner and
    /// resumes as config.toml says, like before.
    #[test]
    fn a_session_without_an_owner_still_loads() {
        let temp = tempfile::TempDir::new().unwrap();
        let storage = StateStorage::new(temp.path().join("session.redb")).unwrap();
        storage.save(JWT_TOKEN_KEY, &"jwt".to_string()).unwrap();
        storage
            .save(SUBSONIC_CREDENTIAL_KEY, &"u=a&s=b&t=c".to_string())
            .unwrap();

        let loaded = load_session_from(&storage).expect("a stored session");
        assert_eq!(loaded.owner, None);
        let config = pair("https://music.example.com", "alice");
        assert_eq!(resume_identity(Some(&config), None), Some(config.clone()));
        assert_eq!(resume_identity(None, None), None);
    }

    /// An owner that is stored but can't be read is not a session from
    /// before owners existed: the session does not resume at all.
    #[test]
    fn an_unreadable_owner_refuses_the_resume() {
        let temp = tempfile::TempDir::new().unwrap();
        let storage = StateStorage::new(temp.path().join("session.redb")).unwrap();
        save_session(&storage, &owner("https://m", "a"), "jwt", "u=a&s=b&t=c").unwrap();
        storage.save(SESSION_OWNER_KEY, &"not an owner").unwrap();

        assert!(load_session_from(&storage).is_none());
    }

    /// The session's tokens go only to the server and user they came from.
    #[test]
    fn resume_follows_config_only_while_it_names_the_owner() {
        let stored = owner("https://music.example.com", "alice");

        // Same server and user, written differently: resume as config says.
        let config = pair("https://Music.Example.com/", "Alice");
        assert_eq!(resume_identity(Some(&config), Some(&stored)), Some(config));

        // config.toml now names another server, or another user: no resume.
        let elsewhere = pair("https://other.example.com", "alice");
        assert_eq!(resume_identity(Some(&elsewhere), Some(&stored)), None);
        let someone_else = pair("https://music.example.com", "bob");
        assert_eq!(resume_identity(Some(&someone_else), Some(&stored)), None);

        // No pair in config.toml: the owner's.
        assert_eq!(
            resume_identity(None, Some(&stored)),
            Some(pair("https://music.example.com", "alice"))
        );
    }

    #[test]
    fn loaded_session_debug_hides_the_tokens() {
        let hidden = "apiKey=nds_HIDE";
        let session = LoadedSession {
            jwt_token: "JWT_SECRET".into(),
            subsonic_credential: hidden.into(),
            owner: Some(owner("https://music.example.com", "alice")),
        };
        let debug = format!("{session:?}");
        assert!(!debug.contains("JWT_SECRET"));
        assert!(!debug.contains("HIDE"));
        assert!(debug.contains("alice"));
    }

    /// A credential change mid-session keeps the JWT the refresh policy
    /// persisted.
    #[test]
    fn saving_the_credential_alone_keeps_the_jwt() {
        let temp = tempfile::TempDir::new().unwrap();
        let storage = StateStorage::new(temp.path().join("session.redb")).unwrap();
        let alice = owner("https://music.example.com", "alice");
        save_session(&storage, &alice, "jwt-refreshed", "u=alice&s=b&t=c").unwrap();

        let fresh = "apiKey=nds_new";
        save_subsonic_credential(&storage, &alice, fresh).unwrap();

        let loaded = load_session_from(&storage).expect("a stored session");
        assert_eq!(loaded.jwt_token, "jwt-refreshed");
        assert_eq!(loaded.subsonic_credential, fresh);
        assert_eq!(loaded.owner, Some(alice));
    }

    #[test]
    fn clear_session_removes_tokens() {
        let temp = tempfile::TempDir::new().unwrap();
        let db_path = temp.path().join("session.redb");

        let storage = crate::services::state_storage::StateStorage::new(db_path).unwrap();

        save_session(&storage, &owner("https://m", "a"), "jwt", "subsonic").unwrap();
        clear_session(&storage).unwrap();

        // Verify tokens are emptied via the same storage handle
        // (load_session() opens a new handle which may conflict)
        let jwt: Option<String> = storage.load(JWT_TOKEN_KEY).unwrap();
        assert!(jwt.unwrap().is_empty());
        let stored_owner: Option<SessionOwner> = storage.load(SESSION_OWNER_KEY).unwrap();
        assert_eq!(stored_owner, None, "the owner goes with the tokens");
        assert!(load_session_from(&storage).is_none());
    }

    /// Pins the N3/N14 ordering invariant: when an in-flight credential writer
    /// is drained to completion BEFORE `clear_session` runs, the clear is the
    /// last write to win, so no non-empty session blob survives logout.
    #[tokio::test]
    async fn clear_session_after_drain_wins_over_in_flight_writer() {
        use std::time::Duration;

        use crate::services::task_manager::TaskManager;

        let temp = tempfile::TempDir::new().unwrap();
        let db_path = temp.path().join("session.redb");
        let storage = crate::services::state_storage::StateStorage::new(db_path).unwrap();

        // Initial logged-in session.
        let stale_owner = owner("https://m", "a");
        save_session(&storage, &stale_owner, "jwt", "subsonic").unwrap();

        // Straggler credential writer: mid-flight, re-materializes a non-empty
        // session blob after a delay (simulates a save_session / save_jwt_token
        // task that is past its last await when logout fires).
        let tm = TaskManager::new();
        let storage_clone = storage.clone();
        tm.spawn_result("straggler_save", move || async move {
            tokio::time::sleep(Duration::from_millis(20)).await;
            save_session(&storage_clone, &stale_owner, "STALE_JWT", "STALE_SUB")?;
            Ok::<(), anyhow::Error>(())
        });

        // Drain first (writer finishes), THEN clear (clear wins last).
        let _clean = tm.shutdown_all(Duration::from_millis(500)).await;
        clear_session(&storage).unwrap();

        let jwt: Option<String> = storage.load(JWT_TOKEN_KEY).unwrap();
        let sub: Option<String> = storage.load(SUBSONIC_CREDENTIAL_KEY).unwrap();
        assert_eq!(
            jwt.unwrap(),
            String::new(),
            "draining before clear means the straggler's write was completed \
             during the drain and then cleared — no non-empty JWT survives"
        );
        assert_eq!(sub.unwrap(), String::new());
    }

    #[test]
    fn load_session_returns_none_when_empty() {
        let temp = tempfile::TempDir::new().unwrap();
        let db_path = temp.path().join("session.redb");

        let storage = crate::services::state_storage::StateStorage::new(db_path).unwrap();

        // No session saved yet
        let jwt: Option<String> = storage.load(JWT_TOKEN_KEY).unwrap();
        assert!(jwt.is_none());
    }

    /// Pins the HIGH-RISK suppress contract on the credentials save path:
    /// the write must land in the internal-write registry so the config
    /// watcher identity-matches it instead of echoing a reload.
    #[test]
    fn save_credentials_records_internal_write_and_writes_header_on_new_file() {
        let _guard = crate::utils::paths::INTERNAL_WRITE_TEST_LOCK.lock();

        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("config.toml");

        save_credentials_at(&path, "https://example.com", "alice").unwrap();

        let on_disk = std::fs::read_to_string(&path).unwrap();
        assert!(
            crate::utils::paths::was_internal_write(
                &path,
                crate::utils::paths::hash_config_bytes(on_disk.as_bytes())
            ),
            "save_credentials must route through write_atomic so the watcher \
             can identity-match its own write"
        );
        assert!(on_disk.starts_with("# Nokkvi Configuration\n"));
        assert!(on_disk.contains("server_url = \"https://example.com\""));
        assert!(on_disk.contains("username = \"alice\""));
    }

    #[test]
    fn save_credentials_preserves_the_rest_of_an_existing_config() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            "# my notes\nserver_url = \"https://old\"\ntheme = \"svalbard\"\n\n[settings]\nseek_step_secs = 9\n",
        )
        .unwrap();

        save_credentials_at(&path, "https://new", "bob").unwrap();

        let on_disk = std::fs::read_to_string(&path).unwrap();
        assert!(on_disk.contains("# my notes"), "comments survive");
        assert!(on_disk.contains("server_url = \"https://new\""));
        assert!(on_disk.contains("username = \"bob\""));
        assert!(on_disk.contains("theme = \"svalbard\""));
        assert!(on_disk.contains("seek_step_secs = 9"));
        assert!(
            !on_disk.contains("# Nokkvi Configuration"),
            "the new-file header is only for a file nokkvi creates"
        );
    }

    /// A single typo in config.toml must not let the next login rewrite the
    /// file with only `server_url` + `username`.
    #[test]
    fn save_credentials_refuses_to_rewrite_an_unparseable_config() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("config.toml");
        let original = "theme = \"svalbard\"\n[settings\nseek_step_secs = 9\n";
        std::fs::write(&path, original).unwrap();

        let result = save_credentials_at(&path, "https://new", "bob");

        assert!(
            result.is_err(),
            "a parse error must surface, not be papered over"
        );
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            original,
            "config.toml must be left exactly as it was"
        );
    }

    /// A read error (here: a stray non-UTF-8 byte) must not be treated as an
    /// empty file.
    #[test]
    fn save_credentials_refuses_to_rewrite_an_unreadable_config() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("config.toml");
        let original: &[u8] = b"theme = \"svalbard\"\nusername = \"caf\xe9\"\n";
        std::fs::write(&path, original).unwrap();

        let result = save_credentials_at(&path, "https://new", "bob");

        assert!(
            result.is_err(),
            "a read error must surface, not be papered over"
        );
        assert_eq!(
            std::fs::read(&path).unwrap(),
            original,
            "config.toml must be left exactly as it was"
        );
    }
}
