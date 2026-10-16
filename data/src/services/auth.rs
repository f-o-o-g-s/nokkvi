use anyhow::{Context, Result};
use serde::Deserialize;
use url::Url;

use crate::services::api::client::ApiClient;

#[derive(Debug, Deserialize)]
struct LoginResponse {
    token: String,
    id: Option<String>,
    /// The Subsonic token pair. Navidrome derives it from the stored
    /// password, so a server that stops keeping passwords can leave it out;
    /// the session's `/rest` credential then has to come from an API key.
    #[serde(rename = "subsonicSalt", default)]
    subsonic_salt: String,
    #[serde(rename = "subsonicToken", default)]
    subsonic_token: String,
}

/// Outcome of a single login attempt against one candidate URL.
enum AttemptError {
    /// The request never reached the server (DNS / connect / TLS / timeout).
    /// Safe to fall through to the next candidate scheme.
    Transport(anyhow::Error),
    /// The server answered but rejected the attempt (bad credentials, non-2xx
    /// status, or an unparseable body). The host is reachable — stop probing.
    Reached(anyhow::Error),
}

pub struct AuthService {
    client: Option<ApiClient>,
    server_url: String,
    username: String,
    token: String,
    user_id: String,
    subsonic_credential: String,
    is_authenticating: bool,
    error_message: String,
}

impl AuthService {
    pub fn new() -> Result<Self> {
        Ok(Self {
            client: None,
            server_url: String::new(),
            username: String::new(),
            token: String::new(),
            user_id: String::new(),
            subsonic_credential: String::new(),
            is_authenticating: false,
            error_message: String::new(),
        })
    }

    pub async fn login(
        &mut self,
        server_url: String,
        username: String,
        password: String,
    ) -> Result<()> {
        self.is_authenticating = true;
        self.error_message.clear();

        // Expand the typed input into ordered candidates: an already-schemed
        // URL is one canonical candidate; a bare host becomes
        // [https://host, http://host] so the user can type `navidrome.local:4533`
        // and we prefer TLS with an HTTP fallback. On success the WINNING URL is
        // committed to `self.server_url` (the resolved URL the caller persists).
        let candidates = crate::utils::server_url::normalize_server_url_candidates(&server_url);
        if candidates.is_empty() {
            self.is_authenticating = false;
            self.error_message = "Server URL is required".to_string();
            return Err(anyhow::anyhow!(self.error_message.clone()));
        }

        let mut last_transport: Option<anyhow::Error> = None;
        for candidate in &candidates {
            match self.try_login_once(candidate, &username, &password).await {
                Ok(()) => {
                    self.is_authenticating = false;
                    return Ok(());
                }
                // The server answered and rejected us (bad credentials, non-2xx,
                // unparseable body): stop. The host is reachable on this scheme,
                // so falling through to http would be pointless and would leak
                // the password in cleartext to a server that already said no.
                Err(AttemptError::Reached(e)) => {
                    self.is_authenticating = false;
                    return Err(e);
                }
                // Never reached this candidate (DNS/connect/TLS/timeout): try the
                // next scheme, remembering the error in case all candidates fail.
                Err(AttemptError::Transport(e)) => last_transport = Some(e),
            }
        }

        self.is_authenticating = false;
        Err(last_transport
            .unwrap_or_else(|| anyhow::anyhow!("Network error. Please check your server URL.")))
    }

    /// A single login attempt against one fully-qualified `base_url`. Splits
    /// failures into [`AttemptError::Transport`] (request never reached the
    /// server — safe to try the next candidate) and [`AttemptError::Reached`]
    /// (server answered but rejected — stop probing). On success the winning
    /// `base_url` and the derived session fields are committed to `self`.
    async fn try_login_once(
        &mut self,
        base_url: &str,
        username: &str,
        password: &str,
    ) -> std::result::Result<(), AttemptError> {
        let login_url = format!("{base_url}/auth/login");

        let client = crate::utils::http::client_base()
            // The body is the password.
            .redirect(crate::utils::redirects::server_only())
            .build()
            .expect("Failed to build HTTP client");
        let response = client
            .post(&login_url)
            .json(&serde_json::json!({
                "username": username,
                "password": password
            }))
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await
            .context("Network error. Please check your server URL.")
            .map_err(AttemptError::Transport)?;

        // Snapshot before the success arm's `.json()` consumes the response.
        let status = response.status();

        if status.is_success() {
            let login_response: LoginResponse = response
                .json()
                .await
                .context("Invalid server response. Please check your server URL.")
                .map_err(AttemptError::Reached)?;

            let base = Url::parse(base_url)
                .context("Invalid server response. Please check your server URL.")
                .map_err(AttemptError::Reached)?;

            // Commit the winning candidate as the resolved session URL.
            self.server_url = base_url.to_string();
            self.username = username.to_string();
            self.token = login_response.token;
            self.user_id = login_response.id.unwrap_or_default();
            // The token pair when the server sent one. Without it the
            // credential stays empty until an API key is set up
            // (`AuthGateway::establish_api_key`, which fails the login when
            // it can't set one up).
            self.subsonic_credential = if login_response.subsonic_salt.is_empty()
                || login_response.subsonic_token.is_empty()
            {
                String::new()
            } else {
                subsonic_token_credential(
                    username,
                    &login_response.subsonic_salt,
                    &login_response.subsonic_token,
                )
            };
            self.client = Some(ApiClient::new(base, self.token.clone()));

            Ok(())
        } else if status == reqwest::StatusCode::UNAUTHORIZED {
            self.error_message = "Invalid username or password. Please try again.".to_string();
            Err(AttemptError::Reached(anyhow::anyhow!(
                self.error_message.clone()
            )))
        } else {
            self.error_message =
                format!("Authentication failed (Status: {status}). Please try again.");
            Err(AttemptError::Reached(anyhow::anyhow!(
                self.error_message.clone()
            )))
        }
    }

    /// Resume a session from stored JWT + subsonic credential (no network call).
    ///
    /// Used for auto-login on startup when a valid JWT is available in redb.
    /// The JWT will be refreshed via the X-ND-Authorization response header
    /// on the first API call.
    pub fn resume_session(
        &mut self,
        server_url: String,
        username: String,
        jwt_token: String,
        subsonic_credential: String,
    ) -> Result<()> {
        // A session saved with a typed `HTTPS://` keeps a lowercase scheme
        // from here on, like a fresh login's.
        let server_url = crate::utils::server_url::lowercase_http_scheme(&server_url);
        self.server_url = server_url.clone();
        self.subsonic_credential = reencode_stored_credential(&username, &subsonic_credential);
        self.username = username;
        self.token = jwt_token.clone();
        // No login response on resume — recover the user id from the JWT's
        // `uid` claim. Best-effort: an unparseable token degrades to an
        // empty id, which the ownership gate reads as "not owned"
        // (conservative — feature-hidden, never a wrong grant).
        self.user_id = crate::services::api::client::decode_jwt_uid(&jwt_token).unwrap_or_default();

        let base_url = Url::parse(&server_url)?;
        self.client = Some(ApiClient::new(base_url, self.token.clone()));

        Ok(())
    }

    pub fn get_client(&self) -> Option<&ApiClient> {
        self.client.as_ref()
    }

    pub fn get_server_url(&self) -> &str {
        &self.server_url
    }

    /// The username this session logged in (or resumed) with.
    pub fn username(&self) -> &str {
        &self.username
    }

    pub fn get_subsonic_credential(&self) -> &str {
        &self.subsonic_credential
    }

    /// Replace the session's `/rest` credential: an API key adopted after the
    /// login (`AuthGateway::establish_api_key`).
    pub(crate) fn set_subsonic_credential(&mut self, subsonic_credential: String) {
        self.subsonic_credential = subsonic_credential;
    }

    /// The authenticated user's Navidrome id — from the login response on a
    /// fresh login, or the JWT `uid` claim on session resume. Empty when
    /// unauthenticated (or when a resumed token carried no `uid`). The
    /// playlist ownership gate compares this against `Playlist.owner_id`;
    /// NEVER compare owner names (Navidrome logins are case-insensitive, so
    /// name equality breaks for a "Foogs"/"foogs" login).
    pub fn user_id(&self) -> &str {
        &self.user_id
    }

    /// The session's current JWT: the client's, which every response's
    /// `X-ND-Authorization` rotates, or the login/resume token until a client
    /// exists. The event stream reconnects with it and the session save
    /// stores it; both used to read the login-time token, which a reconnect
    /// past the token's lifetime (48 h by default) could no longer use.
    pub fn get_token(&self) -> String {
        self.client
            .as_ref()
            .map_or_else(|| self.token.clone(), ApiClient::current_token)
    }

    pub fn set_token_refresh_callback(
        &mut self,
        callback: crate::services::api::client::TokenRefreshCallback,
    ) {
        if let Some(client) = &mut self.client {
            client.set_on_token_refresh(callback);
        }
    }

    /// A refreshed JWT from an `X-ND-Authorization` answer header that did
    /// not pass through the client (the event stream's connect: Navidrome
    /// refreshes the token on it like on every native answer). Applied like
    /// the client's own interception, persistence callback included, so a
    /// session with no native traffic still rotates its token on each
    /// reconnect instead of running into the 48 h expiry.
    pub fn apply_refreshed_token(&self, header_value: &str) {
        if let Some(client) = &self.client {
            let token = header_value.strip_prefix("Bearer ").unwrap_or(header_value);
            client.apply_refreshed_token(token);
        }
    }
}

/// How long the version ping may take: it runs beside the login's other
/// requests and gates nothing but the About box and a caps hint.
const PING_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// The server's version, from `ping`'s envelope. A free function so the
/// gateway asks with its session lock released (see
/// [`open_subsonic_extensions`]). It goes out as the shared POST with the
/// standard params: a hand-built GET once lacked `v`, which Navidrome warns
/// about on every login (`checkRequiredParameters`) and answers with error
/// 10, and it carried the credential in the URL. Lenient on purpose: a failed
/// envelope (a dead credential) still carries `serverVersion`, which is all
/// the caller is after.
pub(crate) async fn server_version(
    http_client: &std::sync::Arc<reqwest::Client>,
    server_url: &str,
    subsonic_credential: &str,
) -> Result<String> {
    let (_status, body) = tokio::time::timeout(
        PING_TIMEOUT,
        crate::services::api::subsonic::fetch_subsonic_body(
            http_client,
            server_url,
            "ping",
            subsonic_credential,
            &[],
            "ping",
        ),
    )
    .await
    .context("Timed out pinging the server")??;
    extract_server_version(&body)
        .context("Could not extract Navidrome server version from API response")
}

/// The server's advertised OpenSubsonic extension names, via
/// `getOpenSubsonicExtensions`. A free function so a caller can ask without
/// holding the auth lock.
///
/// The endpoint is registered outside Navidrome's auth group
/// (unauthenticated per the OpenSubsonic spec); POSTing the credential in
/// the form body like every other Subsonic call is harmless. An absent
/// `openSubsonicExtensions` field (pre-OpenSubsonic server) deserializes
/// to an empty list, never an error.
///
/// Deliberately the LENIENT reader, unlike the other Subsonic reads: the
/// endpoint is public, so a rejected token cannot fail it, and an empty
/// list is fail-safe (every extension-gated feature stays hidden or
/// falls back). A proxy's error page still fails to parse and errs.
pub(crate) async fn open_subsonic_extensions(
    http_client: &std::sync::Arc<reqwest::Client>,
    server_url: &str,
    subsonic_credential: &str,
) -> Result<Vec<String>> {
    let inner: OpenSubsonicExtensionsInner = crate::services::api::subsonic::subsonic_get_envelope(
        http_client,
        server_url,
        "getOpenSubsonicExtensions",
        subsonic_credential,
        &[],
        "open subsonic extensions",
    )
    .await?;
    Ok(inner.extensions.into_iter().map(|e| e.name).collect())
}

/// The username a `/rest` credential authenticates as: the OpenSubsonic
/// `tokenInfo` endpoint, which comes with the `apiKeyAuthentication`
/// extension. Read checked, so a rejected credential is an error, and an
/// unknown key (code 44) is [`crate::types::error::NokkviError::Unauthorized`].
pub(crate) async fn token_info_username(
    http_client: &std::sync::Arc<reqwest::Client>,
    server_url: &str,
    subsonic_credential: &str,
) -> Result<String> {
    let inner: TokenInfoInner = crate::services::api::subsonic::subsonic_get_envelope_checked(
        http_client,
        server_url,
        "tokenInfo",
        subsonic_credential,
        &[],
        "token info",
    )
    .await?;
    Ok(inner.token_info.username)
}

/// The Subsonic token credential (`u=…&s=…&t=…`) every `/rest` request
/// carries in its form body or query string. The username is form-encoded:
/// raw, Navidrome's form parser reads `+` as a space and `&` as the end of the
/// field, so the user lookup fails with code 40, which drops the session.
/// The salt and token are hex.
pub(crate) fn subsonic_token_credential(username: &str, salt: &str, token: &str) -> String {
    let username: String = url::form_urlencoded::byte_serialize(username.as_bytes()).collect();
    format!("u={username}&s={salt}&t={token}")
}

/// The fields of a `/rest` credential string, form-decoded: the salt and
/// token of the pair [`subsonic_token_credential`] builds, or the key
/// `ApiKey::credential` builds (`apiKey`). A field given twice keeps its last
/// value. The username is left out on purpose: an API key credential holds
/// none, so the session's username never comes from its credential.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct CredentialParts {
    pub(crate) salt: Option<String>,
    pub(crate) token: Option<String>,
    pub(crate) api_key: Option<String>,
}

impl CredentialParts {
    pub(crate) fn parse(credential: &str) -> Self {
        let mut parts = Self::default();
        for (field, value) in url::form_urlencoded::parse(credential.as_bytes()) {
            let slot = match field.as_ref() {
                "s" => &mut parts.salt,
                "t" => &mut parts.token,
                "apiKey" => &mut parts.api_key,
                _ => continue,
            };
            *slot = Some(value.into_owned());
        }
        parts
    }
}

/// The form of a username that names one Navidrome user: Navidrome looks
/// users up case-insensitively (`COLLATE NOCASE`), so it is lowercased.
pub(crate) fn user_key(username: &str) -> String {
    username.to_lowercase()
}

/// Whether two usernames name the same Navidrome user (see [`user_key`]).
pub(crate) fn same_user(a: &str, b: &str) -> bool {
    user_key(a) == user_key(b)
}

/// Rebuild a stored token credential with `username` encoded: sessions saved
/// before the encoding fix stored the username raw. A credential without a
/// salt/token pair (or with no username to rebuild from) is kept as stored.
fn reencode_stored_credential(username: &str, stored: &str) -> String {
    if username.is_empty() {
        return stored.to_string();
    }
    match CredentialParts::parse(stored) {
        CredentialParts {
            salt: Some(salt),
            token: Some(token),
            ..
        } => subsonic_token_credential(username, &salt, &token),
        _ => stored.to_string(),
    }
}

/// Inner payload of the Subsonic ping envelope — only the `serverVersion`
/// leaf is consulted; everything else (status, error, ...) is ignored by
/// default serde behavior.
#[derive(Debug, Deserialize)]
struct PingInner {
    #[serde(rename = "serverVersion")]
    server_version: Option<String>,
}

/// Inner payload of the `getOpenSubsonicExtensions` envelope. Only the
/// extension names are consulted; `versions` and any additive fields are
/// ignored by default serde behavior.
#[derive(Debug, Deserialize)]
struct OpenSubsonicExtensionsInner {
    #[serde(rename = "openSubsonicExtensions", default)]
    extensions: Vec<OpenSubsonicExtension>,
}

#[derive(Debug, Deserialize)]
struct OpenSubsonicExtension {
    name: String,
}

/// Inner payload of the `tokenInfo` envelope.
#[derive(Debug, Deserialize)]
struct TokenInfoInner {
    #[serde(rename = "tokenInfo")]
    token_info: TokenInfo,
}

#[derive(Debug, Deserialize)]
struct TokenInfo {
    username: String,
}

/// Total-tolerant version probe: any parse failure or missing key returns
/// `None`, never an error. A `status == "failed"` ping that still carries
/// `serverVersion` extracts it (unknown fields are ignored, the leaf is
/// `Option`).
pub(crate) fn extract_server_version(json: &str) -> Option<String> {
    serde_json::from_str::<crate::services::api::subsonic::SubsonicEnvelope<PingInner>>(json)
        .ok()
        .and_then(|envelope| envelope.response.server_version)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The credential is a query-string / form fragment, so the username is
    /// form-encoded: raw, Navidrome's form parser reads `+` as a space and
    /// `&` as the end of the field, rejects the user with code 40, and that
    /// now drops the session to the login screen on every request.
    #[test]
    fn token_credential_form_encodes_the_username() {
        assert_eq!(
            subsonic_token_credential("a+b@x.com", "salt", "tok"),
            "u=a%2Bb%40x.com&s=salt&t=tok"
        );
        assert_eq!(
            subsonic_token_credential("foogs", "salt", "tok"),
            "u=foogs&s=salt&t=tok"
        );
    }

    /// What the two credential builders write, `CredentialParts` reads back.
    #[test]
    fn credential_parts_read_back_what_the_builders_write() {
        assert_eq!(
            CredentialParts::parse(&subsonic_token_credential("a+b&s=x@y", "5a17", "70ce")),
            CredentialParts {
                salt: Some("5a17".into()),
                token: Some("70ce".into()),
                api_key: None,
            }
        );
        let key = crate::services::api_key::ApiKey::mint().expect("mint");
        assert_eq!(
            CredentialParts::parse(&key.credential()),
            CredentialParts {
                api_key: Some(key.as_str().to_owned()),
                ..CredentialParts::default()
            }
        );
    }

    /// The event stream and the session save read the session's JWT through
    /// `get_token`, so it follows the rotation the client applies on every
    /// response instead of staying at the login-time token.
    #[test]
    fn get_token_follows_the_clients_rotated_jwt() {
        let mut auth = AuthService::new().expect("new");
        auth.resume_session(
            "http://127.0.0.1:1".into(),
            "foogs".into(),
            "jwt-at-resume".into(),
            "u=foogs&s=salt&t=tok".into(),
        )
        .expect("resume");
        assert_eq!(auth.get_token(), "jwt-at-resume");

        auth.get_client()
            .expect("a resumed session has a client")
            .apply_refreshed_token("jwt-rotated");
        assert_eq!(auth.get_token(), "jwt-rotated");

        // The event stream's connect answer carries the header's value.
        auth.apply_refreshed_token("Bearer jwt-from-sse");
        assert_eq!(auth.get_token(), "jwt-from-sse");
    }

    /// The version ping is the shared Subsonic POST: a hand-built GET once
    /// lacked `v` (Navidrome warns on every login and answers error 10 before
    /// it authenticates) and carried the credential in its URL.
    #[tokio::test]
    async fn the_version_ping_is_the_standard_post() {
        let server = crate::services::api::test_server::ScriptedServer::start(|r| {
            if r.is("POST", "/rest/ping") {
                (
                    200,
                    crate::services::api::test_server::subsonic_ok(r#""serverVersion":"0.64.2""#),
                )
            } else {
                (404, String::new())
            }
        })
        .await;
        let client = std::sync::Arc::new(reqwest::Client::new());

        let version = server_version(&client, &server.base_url, "u=foogs&s=salt&t=tok")
            .await
            .expect("version");

        assert_eq!(version, "0.64.2");
        let requests = server.requests();
        let ping = requests.first().expect("one request");
        assert!(ping.is("POST", "/rest/ping"), "{ping:?}");
        assert_eq!(ping.form("v").as_deref(), Some("1.8.0"));
        assert_eq!(ping.form("c").as_deref(), Some("nokkvi"));
        assert_eq!(ping.form("t").as_deref(), Some("tok"));
        assert!(
            ping.query.is_empty(),
            "the credential stays out of the URL: {}",
            ping.query
        );
    }

    #[test]
    fn same_user_ignores_case_and_user_key_lowercases() {
        assert!(same_user("Foogs", "fOOGS"));
        assert!(!same_user("foogs", "foogs2"));
        assert_eq!(user_key("Foogs"), "foogs");
    }

    /// A session saved before the encoding fix stored the username raw;
    /// resuming it rebuilds the credential with the encoded username.
    #[test]
    fn resume_re_encodes_a_stored_raw_username() {
        let mut auth = AuthService::new().expect("auth service");
        auth.resume_session(
            "http://localhost:4533".into(),
            "a+b&c".into(),
            "jwt".into(),
            "u=a+b&c&s=salt&t=tok".into(),
        )
        .expect("resume");
        assert_eq!(auth.get_subsonic_credential(), "u=a%2Bb%26c&s=salt&t=tok");

        // A credential without a token pair is kept as stored.
        auth.resume_session(
            "http://localhost:4533".into(),
            "foogs".into(),
            "jwt".into(),
            "u=foogs&p=enc:abcd".into(),
        )
        .expect("resume");
        assert_eq!(auth.get_subsonic_credential(), "u=foogs&p=enc:abcd");
    }

    use crate::services::api::test_server::{ScriptedServer, subsonic_failed, subsonic_ok};

    fn login_answer(pair: bool) -> String {
        let pair = if pair {
            r#","subsonicSalt":"salt","subsonicToken":"tok""#
        } else {
            ""
        };
        format!(r#"{{"token":"jwt","id":"u1","name":"Alice","username":"alice"{pair}}}"#)
    }

    #[tokio::test]
    async fn login_with_a_token_pair_builds_the_token_credential() {
        let server = ScriptedServer::start(|r| {
            if r.is("POST", "/auth/login") {
                (200, login_answer(true))
            } else {
                (404, String::new())
            }
        })
        .await;
        let mut auth = AuthService::new().expect("auth service");
        auth.login(server.base_url.clone(), "alice".into(), "pw".into())
            .await
            .expect("login");
        assert_eq!(auth.get_subsonic_credential(), "u=alice&s=salt&t=tok");
        assert_eq!(auth.user_id(), "u1");
    }

    /// Navidrome derives the token pair from the stored password; a server
    /// that stops keeping passwords can leave it out. The login still
    /// succeeds, with no `/rest` credential, which then has to come from an
    /// API key (`AuthGateway::establish_api_key` fails the login otherwise).
    #[tokio::test]
    async fn login_without_a_token_pair_leaves_the_credential_empty() {
        let server = ScriptedServer::start(|r| {
            if r.is("POST", "/auth/login") {
                (200, login_answer(false))
            } else {
                (404, String::new())
            }
        })
        .await;
        let mut auth = AuthService::new().expect("auth service");
        auth.login(server.base_url.clone(), "alice".into(), "pw".into())
            .await
            .expect("login");
        assert_eq!(auth.get_subsonic_credential(), "");
        assert_eq!(auth.username(), "alice");
    }

    /// The login's password never follows a redirect to another host.
    #[tokio::test]
    async fn a_login_never_follows_a_redirect_off_the_server() {
        let (redirecting, elsewhere) =
            crate::services::api::test_server::redirect_to_another_host().await;
        let mut auth = AuthService::new().expect("auth service");

        let result = auth
            .login(
                redirecting.base_url.clone(),
                "alice".into(),
                "SECRET_PW".into(),
            )
            .await;

        assert!(result.is_err());
        assert!(
            elsewhere.requests().is_empty(),
            "{:?}",
            elsewhere.requests()
        );
    }

    #[tokio::test]
    async fn token_info_names_the_credential_owner() {
        let server = ScriptedServer::start(|r| {
            assert!(r.is("POST", "/rest/tokenInfo"));
            assert_eq!(r.param("u"), None, "a key goes alone");
            match r.param("apiKey").as_deref() {
                Some("nds_good") => (200, subsonic_ok(r#""tokenInfo":{"username":"alice"}"#)),
                _ => (200, subsonic_failed(44, "Invalid API key")),
            }
        })
        .await;
        let http = std::sync::Arc::new(reqwest::Client::new());

        let owner = token_info_username(&http, &server.base_url, "apiKey=nds_good")
            .await
            .expect("token info");
        assert_eq!(owner, "alice");

        let err = token_info_username(&http, &server.base_url, "apiKey=nds_bad")
            .await
            .expect_err("an unknown key is refused");
        assert!(crate::types::error::NokkviError::is_unauthorized(&err));
    }

    /// A session saved with a typed `HTTPS://` resumes with the scheme in
    /// lowercase (see `utils::server_url::lowercase_http_scheme`).
    #[test]
    fn resume_lowercases_the_server_scheme() {
        let mut auth = AuthService::new().expect("auth service");
        auth.resume_session(
            "HTTPS://Music.Example.com".into(),
            "alice".into(),
            "jwt".into(),
            "u=alice&s=salt&t=tok".into(),
        )
        .expect("resume");
        assert_eq!(auth.get_server_url(), "https://Music.Example.com");
    }

    #[test]
    fn test_extract_server_version_success() {
        let json = r#"{"subsonic-response":{"status":"ok","version":"1.16.1","type":"navidrome","serverVersion":"0.61.1 (e7c7cba87374ebe1bace57271bc5e8cf731b7a6e)","openSubsonic":true}}"#;
        assert_eq!(
            extract_server_version(json).unwrap(),
            "0.61.1 (e7c7cba87374ebe1bace57271bc5e8cf731b7a6e)"
        );
    }

    #[test]
    fn test_extract_server_version_auth_failure() {
        let json = r#"{"subsonic-response":{"status":"failed","version":"1.16.1","type":"navidrome","serverVersion":"0.61.0","openSubsonic":true,"error":{"code":40,"message":"Wrong username or password"}}}"#;
        assert_eq!(extract_server_version(json).unwrap(), "0.61.0");
    }

    #[test]
    fn test_extract_server_version_missing() {
        let json = r#"{"subsonic-response":{"status":"ok"}}"#;
        assert_eq!(extract_server_version(json), None);
    }

    #[test]
    fn open_subsonic_extensions_parse_present() {
        let json = r#"{"subsonic-response":{"status":"ok","version":"1.16.1","openSubsonicExtensions":[{"name":"indexBasedQueue","versions":[1]},{"name":"formPost","versions":[1]}]}}"#;
        let envelope: crate::services::api::subsonic::SubsonicEnvelope<
            OpenSubsonicExtensionsInner,
        > = serde_json::from_str(json).unwrap();
        let names: Vec<String> = envelope
            .response
            .extensions
            .into_iter()
            .map(|e| e.name)
            .collect();
        assert_eq!(names, ["indexBasedQueue", "formPost"]);
    }

    #[test]
    fn open_subsonic_extensions_parse_absent_field() {
        // Pre-OpenSubsonic server: field absent → empty list, never an error.
        let json = r#"{"subsonic-response":{"status":"ok","version":"1.16.1"}}"#;
        let envelope: crate::services::api::subsonic::SubsonicEnvelope<
            OpenSubsonicExtensionsInner,
        > = serde_json::from_str(json).unwrap();
        assert!(envelope.response.extensions.is_empty());
    }
}
