use anyhow::{Context, Result};
use serde::Deserialize;
use url::Url;

use crate::services::api::client::ApiClient;

#[derive(Debug, Deserialize)]
struct LoginResponse {
    token: String,
    id: Option<String>,
    #[serde(rename = "subsonicSalt")]
    subsonic_salt: String,
    #[serde(rename = "subsonicToken")]
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

        let client = reqwest::Client::builder()
            .user_agent(crate::USER_AGENT)
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

            // Validate required fields
            if login_response.subsonic_salt.is_empty() || login_response.subsonic_token.is_empty() {
                self.error_message =
                    "Server response missing required authentication fields".to_string();
                return Err(AttemptError::Reached(anyhow::anyhow!(
                    self.error_message.clone()
                )));
            }

            let base = Url::parse(base_url)
                .context("Invalid server response. Please check your server URL.")
                .map_err(AttemptError::Reached)?;

            // Commit the winning candidate as the resolved session URL.
            self.server_url = base_url.to_string();
            self.username = username.to_string();
            self.token = login_response.token;
            self.user_id = login_response.id.unwrap_or_default();
            self.subsonic_credential = format!(
                "u={}&s={}&t={}",
                username, login_response.subsonic_salt, login_response.subsonic_token
            );
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
        self.server_url = server_url.clone();
        self.username = username;
        self.token = jwt_token.clone();
        self.subsonic_credential = subsonic_credential;

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

    pub fn get_subsonic_credential(&self) -> &str {
        &self.subsonic_credential
    }

    /// Get the initial JWT token (as received from login or resume).
    /// Note: The live token may differ if refreshed by the ApiClient interceptor.
    pub fn get_token(&self) -> &str {
        &self.token
    }

    pub fn set_token_refresh_callback(
        &mut self,
        callback: crate::services::api::client::TokenRefreshCallback,
    ) {
        if let Some(client) = &mut self.client {
            client.set_on_token_refresh(callback);
        }
    }

    /// Fetches the server version dynamically using the Subsonic /rest/ping endpoint.
    pub async fn fetch_server_version(&self) -> Result<String> {
        let ping_url = format!(
            "{}/rest/ping?{}&f=json&c=nokkvi",
            self.server_url, self.subsonic_credential
        );
        let client = self
            .client
            .as_ref()
            .context("Not authenticated")?
            .http_client();
        let response = client
            .get(&ping_url)
            .timeout(std::time::Duration::from_secs(5))
            .send()
            .await
            // `ping_url` is the one Subsonic request that carries `s=`/`t=` in the
            // query string (every other call POSTs them in the body), so strip the
            // URL from any transport error before it can reach a log sink.
            .map_err(reqwest::Error::without_url)
            .context("Failed to connect to server for ping")?;

        let body = response.text().await.unwrap_or_default();
        extract_server_version(&body)
            .context("Could not extract Navidrome server version from API response")
    }

    /// Fetch the server's advertised OpenSubsonic extension names via
    /// `getOpenSubsonicExtensions`.
    ///
    /// The endpoint is registered outside Navidrome's auth group
    /// (unauthenticated per the OpenSubsonic spec); POSTing the credential in
    /// the form body like every other Subsonic call is harmless. An absent
    /// `openSubsonicExtensions` field (pre-OpenSubsonic server) deserializes
    /// to an empty list, never an error.
    pub async fn fetch_open_subsonic_extensions(&self) -> Result<Vec<String>> {
        let client = self
            .client
            .as_ref()
            .context("Not authenticated")?
            .http_client();
        let inner: OpenSubsonicExtensionsInner =
            crate::services::api::subsonic::subsonic_get_envelope(
                &client,
                &self.server_url,
                "getOpenSubsonicExtensions",
                &self.subsonic_credential,
                &[],
                "open subsonic extensions",
            )
            .await?;
        Ok(inner.extensions.into_iter().map(|e| e.name).collect())
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
