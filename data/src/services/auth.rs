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
        self.server_url = server_url.clone();
        self.username = username.clone();

        let login_url = format!("{server_url}/auth/login");

        let client = reqwest::Client::new();
        let response = client
            .post(&login_url)
            .json(&serde_json::json!({
                "username": username,
                "password": password
            }))
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await
            .context("Network error. Please check your server URL.")?;

        self.is_authenticating = false;

        if response.status() == 200 {
            let login_response: LoginResponse = response
                .json()
                .await
                .context("Invalid server response. Please check your server URL.")?;

            // Validate required fields
            if login_response.subsonic_salt.is_empty() || login_response.subsonic_token.is_empty() {
                self.error_message =
                    "Server response missing required authentication fields".to_string();
                return Err(anyhow::anyhow!(self.error_message.clone()));
            }

            self.token = login_response.token;
            self.user_id = login_response.id.unwrap_or_default();
            self.subsonic_credential = format!(
                "u={}&s={}&t={}",
                username, login_response.subsonic_salt, login_response.subsonic_token
            );

            // Create API client with token
            let base_url = Url::parse(&self.server_url)?;
            self.client = Some(ApiClient::new(base_url, self.token.clone()));

            Ok(())
        } else if response.status() == 401 {
            self.error_message = "Invalid username or password. Please try again.".to_string();
            Err(anyhow::anyhow!(self.error_message.clone()))
        } else if response.status() == 0 {
            self.error_message =
                "Cannot connect to server. Please check your server URL.".to_string();
            Err(anyhow::anyhow!(self.error_message.clone()))
        } else {
            self.error_message = format!(
                "Authentication failed (Status: {}). Please try again.",
                response.status()
            );
            Err(anyhow::anyhow!(self.error_message.clone()))
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
            .context("Failed to connect to server for ping")?;

        let body = response.text().await.unwrap_or_default();
        extract_server_version(&body)
            .context("Could not extract Navidrome server version from API response")
    }
}

pub(crate) fn extract_server_version(json: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(json)
        .ok()
        .and_then(|parsed| parsed.get("subsonic-response").cloned())
        .and_then(|sub| sub.get("serverVersion").cloned())
        .and_then(|v| v.as_str().map(|s| s.to_string()))
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
}
