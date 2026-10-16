//! AuthGateway — authentication state and API client management
//!
//! Wraps `AuthService` behind `Arc<Mutex>` for shared access across services.
//! Also sets up the session's OpenSubsonic API key (`establish_api_key`,
//! `revoke_api_key`; the scheme is in `services::api_key`).

use std::{sync::Arc, time::Duration};

use anyhow::{Context, Result, anyhow};
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

use crate::{
    services::{
        api::{client::ApiClient, players::PlayersApiService},
        api_key::{self, ApiKey},
        auth::{self as auth_api, AuthService, same_user},
        queue::slots::QueueOwner,
        state_storage::StateStorage,
    },
    types::error::NokkviError,
};

/// How long setting up an API key may take in all: the extension list, the
/// wait for a logout's revoke, the registration and the check. Past it the
/// session keeps the token pair.
const KEY_SETUP_BUDGET: Duration = Duration::from_secs(30);

/// How long one key request may take.
const KEY_REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// How long a logout's revoke may hold the key-write lock in all: its wait
/// for the session lock, a read of the player and the revoke itself. A login
/// queued behind it waits at most this long.
pub(crate) const KEY_REVOKE_BUDGET: Duration = Duration::from_secs(20);

/// What [`AuthGateway::establish_api_key`] did to the session's `/rest`
/// credential.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApiKeyOutcome {
    /// The session already sends a key (a resumed keyed session). Nothing
    /// was asked of the server.
    AlreadyKeyed,
    /// The server takes no keys; the session keeps the token pair.
    NotOffered,
    /// A fresh key is on nokkvi's player and the session sends it now.
    /// `created` when the player was made for it.
    Adopted { created: bool },
    /// Setting up a key failed; the session keeps the token pair. The reason,
    /// for the log (it never holds the key).
    FellBack(String),
}

/// What a key setup reads from the session, taken under one lock so that no
/// network call holds it.
struct KeySession {
    server_url: String,
    username: String,
    user_id: String,
    /// The username the session's JWT was issued to (its `sub` claim), when
    /// it can be read.
    token_username: Option<String>,
    credential: String,
    client: Option<ApiClient>,
}

impl KeySession {
    fn owner(&self) -> QueueOwner {
        QueueOwner::new(&self.server_url, &self.username)
    }

    /// Fails unless the session's JWT names the session's user. A write to
    /// `/api/player` acts as the JWT's user, so a token that can't be read
    /// fails too (Navidrome's tokens always carry `sub`).
    fn check_token_user(&self) -> Result<()> {
        match &self.token_username {
            Some(token_username) if same_user(token_username, &self.username) => Ok(()),
            Some(_) => Err(anyhow!("the session's token belongs to another user")),
            None => Err(anyhow!("the session's token can't be read")),
        }
    }
}

/// Run one key request under [`KEY_REQUEST_TIMEOUT`].
async fn timed<T>(request: impl Future<Output = Result<T>>) -> Result<T> {
    tokio::time::timeout(KEY_REQUEST_TIMEOUT, request)
        .await
        .map_err(|_| {
            anyhow!(
                "the server did not answer within {} s",
                KEY_REQUEST_TIMEOUT.as_secs()
            )
        })?
}

/// Register a fresh key for `session`: `Ok(None)` when the server takes no
/// keys, else the key the server confirmed and whether its player was
/// created for it. See [`AuthGateway::establish_api_key`].
async fn issue_api_key(
    session: &KeySession,
    storage: &StateStorage,
) -> Result<Option<(ApiKey, bool)>> {
    let client = session.client.clone().context("Not authenticated")?;
    let http = client.http_client();
    let extensions = timed(auth_api::open_subsonic_extensions(
        &http,
        &session.server_url,
        &session.credential,
    ))
    .await?;
    if !extensions
        .iter()
        .any(|name| name == api_key::API_KEY_EXTENSION)
    {
        return Ok(None);
    }

    // Every write below acts as the JWT's user, which must be the session's:
    // a session whose token names someone else (a config.toml edited under an
    // older session) would register a player for that other user.
    session.check_token_user()?;

    // A logout's revoke on this player lands first, or it would kill the
    // key put there next (`api_key::try_claim_key_writes`).
    let _writes = tokio::time::timeout(KEY_REVOKE_BUDGET, api_key::claim_key_writes())
        .await
        .map_err(|_| anyhow!("another API key change is still running"))?;

    let owner = session.owner();
    let key = ApiKey::mint()?;
    let players = PlayersApiService::new(client);
    let mut on_player = false;
    if let Some(player_id) = api_key::recorded_player_id(storage, &owner, &session.user_id)? {
        match timed(players.set_api_key(&player_id, key.as_str())).await {
            Ok(()) => on_player = true,
            // Deleted in the web UI, or a reset server: make a new one. A
            // 403 is not taken as gone (the record is per user id, so it is
            // a proxy refusing, not another user's row); it fails the setup.
            Err(e) if NokkviError::is_not_found(&e) => {
                debug!(
                    target: "nokkvi::auth",
                    "API key player {player_id} is gone; registering a new one"
                );
                if let Err(e) = api_key::forget_player_id(storage, &owner) {
                    warn!(target: "nokkvi::auth", "Couldn't forget the API key player: {e:#}");
                }
            }
            Err(e) => return Err(e),
        }
    }
    let created = !on_player;
    if created {
        // Without the list there is no knowing what the user set on their
        // nokkvi player (scrobbling off, a bit-rate cap): keep the token pair
        // rather than make a player that undoes it.
        let rows = timed(players.list())
            .await
            .context("reading the player list")?;
        let settings = api_key::settings_template(&rows, &session.user_id, &session.username);
        let player_id = timed(players.create(
            &api_key::player_name(),
            api_key::PLAYER_CLIENT,
            key.as_str(),
            &settings,
        ))
        .await?;
        // Unrecorded, the key could never be revoked at a logout: rather keep
        // the token pair (the key on the new player is known to no one).
        api_key::record_player_id(storage, &owner, &player_id, &session.user_id)
            .context("remembering the new API key player")?;
    }

    // The server must take the key, and name this user for it, before the
    // session switches to it.
    let confirmed = timed(auth_api::token_info_username(
        &http,
        &session.server_url,
        &key.credential(),
    ))
    .await
    .context("checking the new API key")?;
    if !same_user(&confirmed, &session.username) {
        anyhow::bail!("the server says the new API key belongs to another user");
    }
    Ok(Some((key, created)))
}

/// AuthGateway - Manages authentication state and operations
///
/// This ViewModel:
/// - Owns AuthService directly (no intermediate Model layer)
/// - Exposes authentication state to Views
/// - Handles authentication operations
#[derive(Clone)]
pub struct AuthGateway {
    auth_service: Arc<Mutex<AuthService>>,
}

impl AuthGateway {
    pub fn new() -> Result<Self> {
        Ok(Self {
            auth_service: Arc::new(Mutex::new(AuthService::new()?)),
        })
    }

    /// Perform login operation
    pub async fn login(
        &self,
        server_url: String,
        username: String,
        password: String,
    ) -> Result<()> {
        let mut auth_service = self.auth_service.lock().await;
        auth_service.login(server_url, username, password).await
    }

    /// Resume a session from stored JWT + subsonic credential (no network call)
    pub async fn resume_session(
        &self,
        server_url: String,
        username: String,
        jwt_token: String,
        subsonic_credential: String,
    ) -> Result<()> {
        let mut auth_service = self.auth_service.lock().await;
        auth_service.resume_session(server_url, username, jwt_token, subsonic_credential)
    }

    /// Get server URL
    pub async fn get_server_url(&self) -> String {
        let auth_service = self.auth_service.lock().await;
        auth_service.get_server_url().to_string()
    }

    /// The username this session logged in (or resumed) with.
    pub async fn username(&self) -> String {
        let auth_service = self.auth_service.lock().await;
        auth_service.username().to_string()
    }

    /// Get subsonic credential
    pub async fn get_subsonic_credential(&self) -> String {
        let auth_service = self.auth_service.lock().await;
        auth_service.get_subsonic_credential().to_string()
    }

    /// Get server URL and subsonic credential pair (single lock acquisition).
    ///
    /// Prefer this over calling `get_server_url()` followed by
    /// `get_subsonic_credential()` — it halves auth mutex acquisitions and
    /// removes the micro-window where the pair could be observed across two
    /// different sessions.
    pub async fn server_config(&self) -> (String, String) {
        let auth_service = self.auth_service.lock().await;
        (
            auth_service.get_server_url().to_string(),
            auth_service.get_subsonic_credential().to_string(),
        )
    }

    /// The session's current JWT (see `AuthService::get_token`).
    pub async fn get_token(&self) -> String {
        let auth_service = self.auth_service.lock().await;
        auth_service.get_token()
    }

    /// Set the token refresh callback (for persisting refreshed JWT to redb)
    pub async fn set_token_refresh_callback(
        &self,
        callback: crate::services::api::client::TokenRefreshCallback,
    ) {
        let mut auth_service = self.auth_service.lock().await;
        auth_service.set_token_refresh_callback(callback);
    }

    /// A refreshed JWT from an answer that did not pass through the client
    /// (see `AuthService::apply_refreshed_token`).
    pub async fn apply_refreshed_token(&self, header_value: &str) {
        let auth_service = self.auth_service.lock().await;
        auth_service.apply_refreshed_token(header_value);
    }

    /// Get API client (cloned) - for initializing other ViewModels
    pub async fn get_client(&self) -> Option<ApiClient> {
        let auth_service = self.auth_service.lock().await;
        auth_service.get_client().cloned()
    }

    /// THE single native-API construction path: snapshot a fresh
    /// authenticated client and build a service from it via `factory`
    /// (pass the service's `new` constructor, e.g.
    /// `SongsApiService::new`). Errors with "Not authenticated" when no
    /// client is available yet (pre-login).
    ///
    /// Builds a fresh instance per call — no caching. For the cached
    /// per-service wrapper, see
    /// [`crate::backend::lazy_authed_service::LazyAuthedService::get`].
    pub async fn build_native_api<T>(&self, factory: fn(ApiClient) -> T) -> Result<T> {
        let client = self
            .get_client()
            .await
            .ok_or_else(|| anyhow::anyhow!("Not authenticated"))?;
        Ok(factory(client))
    }

    /// The authenticated user's Navidrome id (empty when unauthenticated).
    /// See `AuthService::user_id` for provenance and the ownership-gate
    /// contract.
    pub async fn user_id(&self) -> String {
        let auth_service = self.auth_service.lock().await;
        auth_service.user_id().to_string()
    }

    /// The server version, from the Subsonic `ping` endpoint. Snapshot under
    /// the lock, ask without it, like `fetch_open_subsonic_extensions`.
    pub async fn fetch_server_version(&self) -> Result<String> {
        let session = self.key_session().await;
        let client = session.client.context("Not authenticated")?;
        auth_api::server_version(
            &client.http_client(),
            &session.server_url,
            &session.credential,
        )
        .await
    }

    /// Fetch the server's advertised OpenSubsonic extension names
    /// (`getOpenSubsonicExtensions`). Used to gate extension-backed features
    /// like the `indexBasedQueue` play-queue sync.
    pub async fn fetch_open_subsonic_extensions(&self) -> Result<Vec<String>> {
        // Snapshot under the lock, ask without it: the session lock must not
        // wait on the network (a logout's revoke reads the session through it).
        let session = self.key_session().await;
        let client = session.client.context("Not authenticated")?;
        auth_api::open_subsonic_extensions(
            &client.http_client(),
            &session.server_url,
            &session.credential,
        )
        .await
    }

    /// Whether the session's `/rest` credential is an API key.
    pub async fn sends_api_key(&self) -> bool {
        let auth_service = self.auth_service.lock().await;
        api_key::is_api_key_credential(auth_service.get_subsonic_credential())
    }

    async fn key_session(&self) -> KeySession {
        let auth = self.auth_service.lock().await;
        KeySession {
            server_url: auth.get_server_url().to_owned(),
            username: auth.username().to_owned(),
            user_id: auth.user_id().to_owned(),
            token_username: crate::services::api::client::decode_jwt_sub(&auth.get_token()).ok(),
            credential: auth.get_subsonic_credential().to_owned(),
            client: auth.get_client().cloned(),
        }
    }

    /// Switch the session's `/rest` credential from `expected` to `next`,
    /// unless something replaced it meanwhile.
    async fn swap_credential(&self, expected: &str, next: String) -> bool {
        let mut auth = self.auth_service.lock().await;
        if auth.get_subsonic_credential() != expected {
            return false;
        }
        auth.set_subsonic_credential(next);
        true
    }

    /// Make this session's `/rest` requests send an OpenSubsonic API key when
    /// the server offers keys (the scheme is in `services::api_key`): put a
    /// fresh key on nokkvi's player (creating the player the first time, with
    /// the settings of the player Navidrome registered for nokkvi's token
    /// requests), check that the server takes it for this user, then switch
    /// the session to it.
    ///
    /// A session that already sends a key is left alone. A server without
    /// the extension, or a key that can't be set up in time, leaves the token
    /// pair in place. Errs only when the session is left with no `/rest`
    /// credential at all (a login whose server sent no token pair), which
    /// must fail the login.
    pub async fn establish_api_key(&self, storage: &StateStorage) -> Result<ApiKeyOutcome> {
        let session = self.key_session().await;
        if api_key::is_api_key_credential(&session.credential) {
            debug!(target: "nokkvi::auth", "Subsonic requests already send an API key");
            return Ok(ApiKeyOutcome::AlreadyKeyed);
        }
        let outcome =
            match tokio::time::timeout(KEY_SETUP_BUDGET, issue_api_key(&session, storage)).await {
                Ok(Ok(None)) => ApiKeyOutcome::NotOffered,
                Ok(Ok(Some((key, created)))) => {
                    if self
                        .swap_credential(&session.credential, key.credential())
                        .await
                    {
                        ApiKeyOutcome::Adopted { created }
                    } else {
                        ApiKeyOutcome::FellBack(
                            "the session changed while the key was set up".to_owned(),
                        )
                    }
                }
                Ok(Err(e)) => ApiKeyOutcome::FellBack(format!("{e:#}")),
                Err(_) => ApiKeyOutcome::FellBack(format!(
                    "it took longer than {} s",
                    KEY_SETUP_BUDGET.as_secs()
                )),
            };
        match &outcome {
            ApiKeyOutcome::Adopted { created } => info!(
                target: "nokkvi::auth",
                "Subsonic requests now send an API key ({} player)",
                if *created { "new" } else { "existing" }
            ),
            ApiKeyOutcome::NotOffered => debug!(
                target: "nokkvi::auth",
                "The server takes no API keys; Subsonic requests keep the token"
            ),
            ApiKeyOutcome::FellBack(reason) => warn!(
                target: "nokkvi::auth",
                "Couldn't set up an API key; Subsonic requests keep the token: {reason}"
            ),
            ApiKeyOutcome::AlreadyKeyed => {}
        }
        if session.credential.is_empty() {
            match &outcome {
                ApiKeyOutcome::Adopted { .. } | ApiKeyOutcome::AlreadyKeyed => {}
                ApiKeyOutcome::NotOffered => anyhow::bail!(
                    "This Navidrome sent no Subsonic login and takes no API keys, so nokkvi \
                     can't play from it."
                ),
                ApiKeyOutcome::FellBack(reason) => anyhow::bail!(
                    "This Navidrome sent no Subsonic login, and nokkvi couldn't set up an API \
                     key: {reason}"
                ),
            }
        }
        Ok(outcome)
    }

    /// Revoke the API key on nokkvi's player for this session's server and
    /// user (an explicit logout). The player stays, with its settings, and
    /// the next login puts a new key on it. Nothing to do when nokkvi has no
    /// player there, or its key is already gone.
    ///
    /// The player is read first: a revoke is owner-or-admin on the server, so
    /// a record that somehow names another user's player must never revoke
    /// that user's key. The requests run on a client that never persists a
    /// refreshed JWT, since the session's tokens are being cleared. Call it
    /// holding the key-write lock (`api_key::try_claim_key_writes`).
    pub async fn revoke_api_key(&self, storage: &StateStorage) -> Result<()> {
        let session = self.key_session().await;
        let owner = session.owner();
        let Some(player_id) = api_key::recorded_player_id(storage, &owner, &session.user_id)?
        else {
            // A session that sends a key always has its player recorded; one
            // without a record has a key nokkvi can't revoke.
            if api_key::is_api_key_credential(&session.credential) {
                anyhow::bail!("nokkvi has no record of the player its API key is on");
            }
            return Ok(());
        };
        // The revoke acts as the JWT's user (see `issue_api_key`).
        session.check_token_user()?;
        let client = session
            .client
            .as_ref()
            .context("Not authenticated")?
            .without_token_refresh();
        let players = PlayersApiService::new(client);
        let row = match timed(players.get(&player_id)).await {
            Ok(row) => row,
            Err(e) if NokkviError::is_not_found(&e) => {
                return api_key::forget_player_id(storage, &owner);
            }
            Err(e) => return Err(e),
        };
        let someone_elses = if !session.user_id.is_empty() && !row.user_id.is_empty() {
            row.user_id != session.user_id
        } else {
            !same_user(&row.user_name, &session.username)
        };
        if someone_elses {
            warn!(
                target: "nokkvi::auth",
                "The recorded API key player belongs to another user; forgetting it"
            );
            return api_key::forget_player_id(storage, &owner);
        }
        if !row.has_api_key {
            return Ok(());
        }
        match timed(players.set_api_key(&player_id, "")).await {
            Ok(()) => {
                info!(target: "nokkvi::auth", "Revoked the API key at logout");
                Ok(())
            }
            Err(e) if NokkviError::is_not_found(&e) => api_key::forget_player_id(storage, &owner),
            Err(e) => Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `server_config()` returns the same `(server_url, subsonic_credential)`
    /// pair that solo `get_server_url()` / `get_subsonic_credential()` calls
    /// would produce — but under a single mutex acquisition. This pins the
    /// pair-getter invariant so the six backend pair-lock sites can safely
    /// migrate to it.
    #[tokio::test]
    async fn server_config_returns_same_pair_as_solo_getters() {
        let gateway = AuthGateway::new().expect("auth gateway");
        gateway
            .resume_session(
                "http://localhost:4533".to_string(),
                "alice".to_string(),
                "jwt-token-abc".to_string(),
                "u=alice&s=salt&t=token".to_string(),
            )
            .await
            .expect("resume session");

        let solo_url = gateway.get_server_url().await;
        let solo_cred = gateway.get_subsonic_credential().await;
        let (pair_url, pair_cred) = gateway.server_config().await;

        assert_eq!(pair_url, solo_url);
        assert_eq!(pair_cred, solo_cred);
        assert_eq!(pair_url, "http://localhost:4533");
        assert_eq!(pair_cred, "u=alice&s=salt&t=token");
    }

    /// Fresh gateway with no session resumed yields empty strings — matches
    /// the empty-state arm in the `get_server_config()` callsites.
    #[tokio::test]
    async fn server_config_fresh_gateway_returns_empty_pair() {
        let gateway = AuthGateway::new().expect("auth gateway");
        let (url, cred) = gateway.server_config().await;
        assert!(url.is_empty());
        assert!(cred.is_empty());
    }

    // ── API key setup against a scripted server ────────────────────────────

    use crate::services::api::test_server::{
        Recorded, ScriptedServer, extensions, subsonic_failed, subsonic_ok,
    };

    const TOKEN_PAIR: &str = "u=alice&s=salt&t=tok";
    const KEYED: &str = "apiKey=nds_aaaaaaaaaaaaaaaaaaaaaa";

    /// A scripted Navidrome: it advertises keys when `offers_keys`; its
    /// players are `players` (owned by `player_owner`, keyed when
    /// `players_keyed`), answered on GET and on PUT (`put_status` overrides
    /// the PUT), 404 for any other id; the list answers `list_status` with
    /// `player_list`; a POST creates `p-new`; `tokenInfo` confirms, as
    /// `confirms_as`, only the last key it was given, sent alone in the body.
    struct FakeNavidrome {
        offers_keys: bool,
        players: Vec<&'static str>,
        player_owner: &'static str,
        /// The players' `userId` (empty: the row carries none).
        player_owner_id: &'static str,
        players_keyed: bool,
        put_status: u16,
        list_status: u16,
        player_list: &'static str,
        create_status: u16,
        confirms_as: &'static str,
        /// A refreshed JWT every answer carries in `X-ND-Authorization`, as
        /// Navidrome's JWTRefresher sends one.
        refreshed_jwt: Option<String>,
    }

    impl Default for FakeNavidrome {
        fn default() -> Self {
            Self {
                offers_keys: true,
                players: Vec::new(),
                player_owner: "alice",
                player_owner_id: "",
                players_keyed: true,
                put_status: 200,
                list_status: 200,
                player_list: "[]",
                create_status: 200,
                confirms_as: "alice",
                refreshed_jwt: None,
            }
        }
    }

    impl FakeNavidrome {
        async fn start(self) -> ScriptedServer {
            let registered = Arc::new(parking_lot::Mutex::new(None::<String>));
            let headers = self
                .refreshed_jwt
                .iter()
                .map(|jwt| ("X-ND-Authorization".to_owned(), format!("Bearer {jwt}")))
                .collect();
            ScriptedServer::start_with_headers(headers, move |r: &Recorded| {
                if r.is("POST", "/rest/getOpenSubsonicExtensions") {
                    let names: &[&str] = if self.offers_keys {
                        &["formPost", "apiKeyAuthentication"]
                    } else {
                        &["formPost"]
                    };
                    return (200, extensions(names));
                }
                if r.is("GET", "/api/player") {
                    return (self.list_status, self.player_list.to_owned());
                }
                if r.is("POST", "/api/player") {
                    if self.create_status != 200 {
                        return (self.create_status, "boom".to_owned());
                    }
                    *registered.lock() = r.json()["apiKey"].as_str().map(str::to_owned);
                    return (200, r#"{"id":"p-new"}"#.to_owned());
                }
                if let Some(id) = r.path.strip_prefix("/api/player/") {
                    if !self.players.contains(&id) {
                        return (404, r#""player not found""#.to_owned());
                    }
                    let row = format!(
                        r#"{{"id":"{id}","name":"nokkvi (host)","client":"nokkvi","userName":"{}","userId":"{}","hasApiKey":{}}}"#,
                        self.player_owner, self.player_owner_id, self.players_keyed
                    );
                    if r.method == "GET" {
                        return (200, row);
                    }
                    if r.method == "PUT" {
                        if self.put_status != 200 {
                            return (self.put_status, "refused".to_owned());
                        }
                        let key = r.json()["apiKey"].as_str().map(str::to_owned);
                        *registered.lock() = key.filter(|k| !k.is_empty());
                        return (200, row);
                    }
                }
                if r.is("POST", "/rest/tokenInfo") {
                    // Navidrome's `apiKeyConflicts`: any other credential
                    // beside the key, in the body or the query, is code 43.
                    let mixed = ["u", "p", "t", "s", "jwt"]
                        .iter()
                        .any(|name| r.param(name).is_some());
                    if mixed {
                        return (200, subsonic_failed(43, "Multiple conflicting authentication mechanisms provided"));
                    }
                    let sent = r.form("apiKey");
                    let in_body_only = !r.query.contains("apiKey");
                    if in_body_only && sent.is_some() && sent == *registered.lock() {
                        return (
                            200,
                            subsonic_ok(&format!(
                                r#""tokenInfo":{{"username":"{}"}}"#,
                                self.confirms_as
                            )),
                        );
                    }
                    return (200, subsonic_failed(44, "Invalid API key"));
                }
                (404, String::new())
            })
            .await
        }
    }

    async fn gateway_with(server: &ScriptedServer, credential: &str, jwt: &str) -> AuthGateway {
        let gateway = AuthGateway::new().expect("auth gateway");
        gateway
            .resume_session(
                server.base_url.clone(),
                "alice".to_string(),
                jwt.to_string(),
                credential.to_string(),
            )
            .await
            .expect("resume session");
        gateway
    }

    /// A Navidrome-shaped JWT for alice (`sub` and `uid` claims).
    fn alice_jwt() -> String {
        crate::services::api::client::jwt_for_test(
            &serde_json::json!({ "sub": "alice", "uid": "u-alice" }),
        )
    }

    async fn gateway_on(server: &ScriptedServer, credential: &str) -> AuthGateway {
        gateway_with(server, credential, &alice_jwt()).await
    }

    fn storage() -> (StateStorage, tempfile::TempDir) {
        let dir = tempfile::TempDir::new().expect("temp dir");
        let storage = StateStorage::new(dir.path().join("app.redb")).expect("storage");
        (storage, dir)
    }

    fn owner_of(server: &ScriptedServer) -> QueueOwner {
        QueueOwner::new(&server.base_url, "alice")
    }

    fn recorded(storage: &StateStorage, server: &ScriptedServer) -> Option<String> {
        api_key::recorded_player_id(storage, &owner_of(server), "").expect("read")
    }

    fn record(storage: &StateStorage, server: &ScriptedServer, player_id: &str) {
        api_key::record_player_id(storage, &owner_of(server), player_id, "").expect("record");
    }

    /// The key the session sends, out of its credential.
    async fn sent_key(gateway: &AuthGateway) -> Option<String> {
        gateway
            .get_subsonic_credential()
            .await
            .strip_prefix("apiKey=")
            .map(str::to_owned)
    }

    /// The requests to `/api/player…` with `method`.
    fn player_requests(server: &ScriptedServer, method: &str) -> Vec<Recorded> {
        server
            .requests()
            .into_iter()
            .filter(|r| r.method == method && r.path.starts_with("/api/player"))
            .collect()
    }

    fn any_player_request(server: &ScriptedServer) -> bool {
        server
            .requests()
            .iter()
            .any(|r| r.path.starts_with("/api/player"))
    }

    /// The extension gate: a server that doesn't advertise
    /// `apiKeyAuthentication` keeps the token pair, and nokkvi never touches
    /// its players.
    #[tokio::test]
    async fn a_server_without_keys_keeps_the_token_pair() {
        let _serial = api_key::KEY_WRITES_TEST_LOCK.lock().await;
        let server = FakeNavidrome {
            offers_keys: false,
            ..FakeNavidrome::default()
        }
        .start()
        .await;
        let gateway = gateway_on(&server, TOKEN_PAIR).await;
        let (storage, _dir) = storage();

        let outcome = gateway.establish_api_key(&storage).await.expect("setup");

        assert_eq!(outcome, ApiKeyOutcome::NotOffered);
        assert_eq!(gateway.get_subsonic_credential().await, TOKEN_PAIR);
        assert!(!any_player_request(&server));
        assert_eq!(recorded(&storage, &server), None);
    }

    /// The first login on a server with keys makes nokkvi's player, copying
    /// the settings of the player Navidrome registered for its token
    /// requests, and switches the session to the key once the server
    /// confirms it. Every player request carries the session's JWT.
    #[tokio::test]
    async fn the_first_login_creates_a_keyed_player() {
        let _serial = api_key::KEY_WRITES_TEST_LOCK.lock().await;
        let server = FakeNavidrome {
            player_list: r#"[{"id":"auto","name":"nokkvi [nokkvi]","client":"nokkvi",
                "userAgent":"nokkvi","userName":"alice","lastSeen":"2026-10-01T00:00:00Z",
                "maxBitRate":192,"transcodingId":"opus","scrobbleEnabled":false,
                "hasApiKey":false}]"#,
            ..FakeNavidrome::default()
        }
        .start()
        .await;
        let gateway = gateway_on(&server, TOKEN_PAIR).await;
        let (storage, _dir) = storage();

        let outcome = gateway.establish_api_key(&storage).await.expect("setup");

        assert_eq!(outcome, ApiKeyOutcome::Adopted { created: true });
        let creates = player_requests(&server, "POST");
        assert_eq!(creates.len(), 1);
        let body = creates[0].json();
        assert!(
            body["name"]
                .as_str()
                .is_some_and(|n| n.starts_with("nokkvi"))
        );
        assert_eq!(body["client"], "nokkvi");
        assert_eq!(body["maxBitRate"], 192);
        assert_eq!(body["transcodingId"], "opus");
        assert_eq!(body["scrobbleEnabled"], false);
        let posted = body["apiKey"].as_str().expect("a key").to_owned();
        assert_eq!(sent_key(&gateway).await.as_deref(), Some(posted.as_str()));
        assert_eq!(recorded(&storage, &server).as_deref(), Some("p-new"));
        for request in player_requests(&server, "GET").iter().chain(creates.iter()) {
            assert_eq!(
                request.header("x-nd-authorization"),
                Some(format!("Bearer {}", alice_jwt()).as_str()),
                "{request:?}"
            );
        }
    }

    /// A later login puts a fresh key on the same player and creates nothing.
    #[tokio::test]
    async fn the_next_login_rotates_the_key_on_the_same_player() {
        let _serial = api_key::KEY_WRITES_TEST_LOCK.lock().await;
        let server = FakeNavidrome {
            players: vec!["p1"],
            ..FakeNavidrome::default()
        }
        .start()
        .await;
        let gateway = gateway_on(&server, TOKEN_PAIR).await;
        let (storage, _dir) = storage();
        record(&storage, &server, "p1");

        let outcome = gateway.establish_api_key(&storage).await.expect("setup");

        assert_eq!(outcome, ApiKeyOutcome::Adopted { created: false });
        let puts = player_requests(&server, "PUT");
        assert_eq!(puts.len(), 1);
        assert!(puts[0].is("PUT", "/api/player/p1"));
        assert!(player_requests(&server, "POST").is_empty());
        assert!(player_requests(&server, "GET").is_empty());
        let body = puts[0].json();
        assert_eq!(
            body.as_object()
                .map(|o| o.keys().cloned().collect::<Vec<_>>()),
            Some(vec!["apiKey".to_owned()]),
            "only the key changes on the player"
        );
        assert_eq!(sent_key(&gateway).await.as_deref(), body["apiKey"].as_str());
    }

    /// A player deleted in the web UI is made again, from the defaults, and
    /// the record moves to it.
    #[tokio::test]
    async fn a_deleted_player_is_made_again() {
        let _serial = api_key::KEY_WRITES_TEST_LOCK.lock().await;
        let server = FakeNavidrome::default().start().await;
        let gateway = gateway_on(&server, TOKEN_PAIR).await;
        let (storage, _dir) = storage();
        record(&storage, &server, "p-gone");

        let outcome = gateway.establish_api_key(&storage).await.expect("setup");

        assert_eq!(outcome, ApiKeyOutcome::Adopted { created: true });
        let create = player_requests(&server, "POST")
            .into_iter()
            .next()
            .expect("a create");
        assert_eq!(
            create.json()["scrobbleEnabled"],
            true,
            "Navidrome's default"
        );
        assert_eq!(recorded(&storage, &server).as_deref(), Some("p-new"));
    }

    /// A 403 on the key write is a refusal (a proxy), not a player that is
    /// gone: the record stays, nothing is created, the token pair stays.
    #[tokio::test]
    async fn a_refused_key_write_keeps_the_record() {
        let _serial = api_key::KEY_WRITES_TEST_LOCK.lock().await;
        let server = FakeNavidrome {
            players: vec!["p1"],
            put_status: 403,
            ..FakeNavidrome::default()
        }
        .start()
        .await;
        let gateway = gateway_on(&server, TOKEN_PAIR).await;
        let (storage, _dir) = storage();
        record(&storage, &server, "p1");

        let outcome = gateway.establish_api_key(&storage).await.expect("setup");

        assert!(matches!(outcome, ApiKeyOutcome::FellBack(_)), "{outcome:?}");
        assert!(player_requests(&server, "POST").is_empty());
        assert_eq!(recorded(&storage, &server).as_deref(), Some("p1"));
        assert_eq!(gateway.get_subsonic_credential().await, TOKEN_PAIR);
    }

    /// Without the player list there is no knowing what the user set on their
    /// nokkvi player, so no player is made from the defaults.
    #[tokio::test]
    async fn an_unreadable_player_list_keeps_the_token_pair() {
        let _serial = api_key::KEY_WRITES_TEST_LOCK.lock().await;
        let server = FakeNavidrome {
            list_status: 500,
            ..FakeNavidrome::default()
        }
        .start()
        .await;
        let gateway = gateway_on(&server, TOKEN_PAIR).await;
        let (storage, _dir) = storage();

        let outcome = gateway.establish_api_key(&storage).await.expect("setup");

        assert!(matches!(outcome, ApiKeyOutcome::FellBack(_)), "{outcome:?}");
        assert!(player_requests(&server, "POST").is_empty());
        assert_eq!(gateway.get_subsonic_credential().await, TOKEN_PAIR);
    }

    /// A session whose JWT names another user writes nothing: every write
    /// would act as that other user.
    #[tokio::test]
    async fn a_token_for_another_user_writes_nothing() {
        let _serial = api_key::KEY_WRITES_TEST_LOCK.lock().await;
        let server = FakeNavidrome::default().start().await;
        let jwt = crate::services::api::client::jwt_for_test(
            &serde_json::json!({ "sub": "bob", "uid": "u-bob" }),
        );
        let gateway = gateway_with(&server, TOKEN_PAIR, &jwt).await;
        let (storage, _dir) = storage();

        let outcome = gateway.establish_api_key(&storage).await.expect("setup");

        assert!(matches!(outcome, ApiKeyOutcome::FellBack(_)), "{outcome:?}");
        assert!(!any_player_request(&server));
        assert_eq!(gateway.get_subsonic_credential().await, TOKEN_PAIR);
    }

    /// A token whose user can't be read gives no assurance whose player a
    /// write would touch: nothing is written.
    #[tokio::test]
    async fn an_unreadable_token_writes_nothing() {
        let _serial = api_key::KEY_WRITES_TEST_LOCK.lock().await;
        let server = FakeNavidrome::default().start().await;
        let gateway = gateway_with(&server, TOKEN_PAIR, "not-a-jwt").await;
        let (storage, _dir) = storage();

        let outcome = gateway.establish_api_key(&storage).await.expect("setup");

        assert!(matches!(outcome, ApiKeyOutcome::FellBack(_)), "{outcome:?}");
        assert!(!any_player_request(&server));
    }

    /// A player nokkvi can't record would carry a key no logout could
    /// revoke: the session keeps the token pair instead.
    #[tokio::test]
    async fn a_player_that_cant_be_recorded_keeps_the_token_pair() {
        let _serial = api_key::KEY_WRITES_TEST_LOCK.lock().await;
        let server = FakeNavidrome::default().start().await;
        let gateway = gateway_on(&server, TOKEN_PAIR).await;
        let (storage, _dir) = storage();
        storage.fail_writes_for_test(true);

        let outcome = gateway.establish_api_key(&storage).await.expect("setup");

        assert!(matches!(outcome, ApiKeyOutcome::FellBack(_)), "{outcome:?}");
        assert_eq!(gateway.get_subsonic_credential().await, TOKEN_PAIR);
    }

    /// The session only switches to a key the server confirms for this user.
    #[tokio::test]
    async fn a_key_the_server_names_for_someone_else_is_not_used() {
        let _serial = api_key::KEY_WRITES_TEST_LOCK.lock().await;
        let server = FakeNavidrome {
            confirms_as: "bob",
            ..FakeNavidrome::default()
        }
        .start()
        .await;
        let gateway = gateway_on(&server, TOKEN_PAIR).await;
        let (storage, _dir) = storage();

        let outcome = gateway.establish_api_key(&storage).await.expect("setup");

        assert!(matches!(outcome, ApiKeyOutcome::FellBack(_)), "{outcome:?}");
        assert_eq!(gateway.get_subsonic_credential().await, TOKEN_PAIR);
    }

    #[tokio::test]
    async fn a_failed_registration_keeps_the_token_pair() {
        let _serial = api_key::KEY_WRITES_TEST_LOCK.lock().await;
        let server = FakeNavidrome {
            create_status: 500,
            ..FakeNavidrome::default()
        }
        .start()
        .await;
        let gateway = gateway_on(&server, TOKEN_PAIR).await;
        let (storage, _dir) = storage();

        let outcome = gateway.establish_api_key(&storage).await.expect("setup");

        let ApiKeyOutcome::FellBack(reason) = outcome else {
            panic!("expected a fallback, got {outcome:?}");
        };
        assert!(!reason.contains("nds_"), "the reason never holds the key");
        assert_eq!(gateway.get_subsonic_credential().await, TOKEN_PAIR);
        assert_eq!(recorded(&storage, &server), None);
    }

    /// A resumed keyed session asks nothing of the server.
    #[tokio::test]
    async fn a_keyed_session_is_left_alone() {
        let server = FakeNavidrome::default().start().await;
        let gateway = gateway_on(&server, KEYED).await;
        let (storage, _dir) = storage();

        let outcome = gateway.establish_api_key(&storage).await.expect("setup");

        assert_eq!(outcome, ApiKeyOutcome::AlreadyKeyed);
        assert!(server.requests().is_empty());
    }

    /// A login whose server sent no token pair needs a key: without one the
    /// login fails with a reason; with one it goes through.
    #[tokio::test]
    async fn no_token_pair_and_no_key_fails_the_login() {
        let _serial = api_key::KEY_WRITES_TEST_LOCK.lock().await;
        let server = FakeNavidrome {
            offers_keys: false,
            ..FakeNavidrome::default()
        }
        .start()
        .await;
        let gateway = gateway_on(&server, "").await;
        let (storage, _dir) = storage();
        let err = gateway
            .establish_api_key(&storage)
            .await
            .expect_err("no credential at all");
        assert!(format!("{err:#}").contains("takes no API keys"), "{err:#}");

        let server = FakeNavidrome::default().start().await;
        let gateway = gateway_on(&server, "").await;
        let outcome = gateway.establish_api_key(&storage).await.expect("setup");
        assert_eq!(outcome, ApiKeyOutcome::Adopted { created: true });
        assert!(sent_key(&gateway).await.is_some());
    }

    /// An explicit logout reads its player, then revokes the key with a
    /// key-only PUT and keeps the player for the next login.
    #[tokio::test]
    async fn logout_revokes_the_key_and_keeps_the_player() {
        let server = FakeNavidrome {
            players: vec!["p1"],
            ..FakeNavidrome::default()
        }
        .start()
        .await;
        let gateway = gateway_on(&server, KEYED).await;
        let (storage, _dir) = storage();
        record(&storage, &server, "p1");

        gateway.revoke_api_key(&storage).await.expect("revoke");

        let puts = player_requests(&server, "PUT");
        assert_eq!(puts.len(), 1);
        assert!(puts[0].is("PUT", "/api/player/p1"));
        assert_eq!(puts[0].json(), serde_json::json!({ "apiKey": "" }));
        assert_eq!(recorded(&storage, &server).as_deref(), Some("p1"));
    }

    /// A revoke is owner-or-admin on the server: an admin's record that names
    /// another user's player must never revoke that user's key.
    #[tokio::test]
    async fn logout_never_revokes_another_users_key() {
        let server = FakeNavidrome {
            players: vec!["p1"],
            player_owner: "bob",
            ..FakeNavidrome::default()
        }
        .start()
        .await;
        let gateway = gateway_on(&server, KEYED).await;
        let (storage, _dir) = storage();
        record(&storage, &server, "p1");

        gateway.revoke_api_key(&storage).await.expect("revoke");

        assert!(player_requests(&server, "PUT").is_empty());
        assert_eq!(recorded(&storage, &server), None);
    }

    #[tokio::test]
    async fn logout_skips_a_player_without_a_key() {
        let server = FakeNavidrome {
            players: vec!["p1"],
            players_keyed: false,
            ..FakeNavidrome::default()
        }
        .start()
        .await;
        let gateway = gateway_on(&server, TOKEN_PAIR).await;
        let (storage, _dir) = storage();
        record(&storage, &server, "p1");

        gateway.revoke_api_key(&storage).await.expect("revoke");

        assert!(player_requests(&server, "PUT").is_empty());
        assert_eq!(recorded(&storage, &server).as_deref(), Some("p1"));
    }

    #[tokio::test]
    async fn revoking_a_deleted_player_forgets_it() {
        let server = FakeNavidrome::default().start().await;
        let gateway = gateway_on(&server, KEYED).await;
        let (storage, _dir) = storage();
        record(&storage, &server, "p-gone");

        gateway.revoke_api_key(&storage).await.expect("revoke");

        assert_eq!(recorded(&storage, &server), None);
    }

    /// A keyed session whose player nokkvi has no record of can't revoke its
    /// key: the logout says so instead of passing over it.
    #[tokio::test]
    async fn logout_reports_a_key_it_cannot_revoke() {
        let server = FakeNavidrome::default().start().await;
        let gateway = gateway_on(&server, KEYED).await;
        let (storage, _dir) = storage();

        let err = gateway
            .revoke_api_key(&storage)
            .await
            .expect_err("a key with no record");
        assert!(format!("{err:#}").contains("no record"), "{err:#}");
        assert!(server.requests().is_empty());
    }

    /// A JWT Navidrome refreshed on an answer, with a full lifetime ahead.
    fn refreshed_jwt() -> String {
        let exp = chrono::Utc::now().timestamp() + 48 * 3600;
        crate::services::api::client::jwt_for_test(
            &serde_json::json!({ "sub": "alice", "uid": "u-alice", "exp": exp }),
        )
    }

    async fn count_persisted_tokens(gateway: &AuthGateway) -> Arc<std::sync::atomic::AtomicUsize> {
        let persisted = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counter = persisted.clone();
        gateway
            .set_token_refresh_callback(Arc::new(move |_token: &str| {
                counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            }))
            .await;
        persisted
    }

    /// The control for the revoke below: on a live session a refreshed JWT
    /// from these answers does reach the persistence callback.
    #[tokio::test]
    async fn a_live_session_persists_a_refreshed_token() {
        let _serial = api_key::KEY_WRITES_TEST_LOCK.lock().await;
        let server = FakeNavidrome {
            refreshed_jwt: Some(refreshed_jwt()),
            ..FakeNavidrome::default()
        }
        .start()
        .await;
        let gateway = gateway_on(&server, TOKEN_PAIR).await;
        let persisted = count_persisted_tokens(&gateway).await;
        let (storage, _dir) = storage();

        let outcome = gateway.establish_api_key(&storage).await.expect("setup");

        assert_eq!(outcome, ApiKeyOutcome::Adopted { created: true });
        assert!(persisted.load(std::sync::atomic::Ordering::SeqCst) >= 1);
    }

    /// A logout's revoke runs after the session's tokens are cleared from
    /// redb, so a refreshed JWT on its answers must never be written back.
    #[tokio::test]
    async fn the_logout_revoke_never_persists_a_refreshed_token() {
        let server = FakeNavidrome {
            players: vec!["p1"],
            refreshed_jwt: Some(refreshed_jwt()),
            ..FakeNavidrome::default()
        }
        .start()
        .await;
        let gateway = gateway_on(&server, KEYED).await;
        let persisted = count_persisted_tokens(&gateway).await;
        let (storage, _dir) = storage();
        record(&storage, &server, "p1");

        gateway.revoke_api_key(&storage).await.expect("revoke");

        assert_eq!(player_requests(&server, "PUT").len(), 1, "it did revoke");
        assert_eq!(persisted.load(std::sync::atomic::Ordering::SeqCst), 0);
    }

    /// A record left by another user who had the same name (the record keeps
    /// the user id) is not this user's player: a new one is made, and the
    /// record moves to this user's id.
    #[tokio::test]
    async fn a_record_for_another_user_id_is_not_used() {
        let _serial = api_key::KEY_WRITES_TEST_LOCK.lock().await;
        let server = FakeNavidrome {
            players: vec!["p-old"],
            ..FakeNavidrome::default()
        }
        .start()
        .await;
        let gateway = gateway_on(&server, TOKEN_PAIR).await;
        let (storage, _dir) = storage();
        let owner = owner_of(&server);
        api_key::record_player_id(&storage, &owner, "p-old", "u-other").expect("record");

        let outcome = gateway.establish_api_key(&storage).await.expect("setup");

        assert_eq!(outcome, ApiKeyOutcome::Adopted { created: true });
        assert!(player_requests(&server, "PUT").is_empty());
        let lookup =
            |user_id: &str| api_key::recorded_player_id(&storage, &owner, user_id).expect("read");
        assert_eq!(lookup("u-other"), None);
        assert_eq!(lookup("u-alice").as_deref(), Some("p-new"));
    }

    /// The revoke looks its player up by the session's user id too.
    #[tokio::test]
    async fn logout_ignores_a_record_for_another_user_id() {
        let server = FakeNavidrome {
            players: vec!["p1"],
            ..FakeNavidrome::default()
        }
        .start()
        .await;
        let gateway = gateway_on(&server, TOKEN_PAIR).await;
        let (storage, _dir) = storage();
        api_key::record_player_id(&storage, &owner_of(&server), "p1", "u-other").expect("record");

        gateway.revoke_api_key(&storage).await.expect("revoke");

        assert!(server.requests().is_empty());
    }

    /// The ownership check compares user ids when both sides have one, even
    /// when the names match.
    #[tokio::test]
    async fn logout_never_revokes_another_user_id_s_key() {
        let server = FakeNavidrome {
            players: vec!["p1"],
            player_owner_id: "u-bob",
            ..FakeNavidrome::default()
        }
        .start()
        .await;
        let gateway = gateway_on(&server, KEYED).await;
        let (storage, _dir) = storage();
        record(&storage, &server, "p1");

        gateway.revoke_api_key(&storage).await.expect("revoke");

        assert!(player_requests(&server, "PUT").is_empty());
        assert_eq!(recorded(&storage, &server), None);
    }

    #[tokio::test]
    async fn revoking_with_no_player_asks_nothing() {
        let server = FakeNavidrome::default().start().await;
        let gateway = gateway_on(&server, TOKEN_PAIR).await;
        let (storage, _dir) = storage();

        gateway.revoke_api_key(&storage).await.expect("revoke");

        assert!(server.requests().is_empty());
    }
}
