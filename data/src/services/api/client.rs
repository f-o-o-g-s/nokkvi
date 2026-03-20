use std::sync::Arc;

use anyhow::{Context, Result};
use parking_lot::RwLock;
use reqwest::Client;
use tracing::debug;
use url::Url;

/// Callback invoked when a refreshed JWT is received from the server.
/// Called with the new token string so callers can persist it to redb.
pub type TokenRefreshCallback = Arc<dyn Fn(&str) + Send + Sync>;

pub struct ApiClient {
    client: Arc<Client>,
    base_url: Url,
    /// JWT token, wrapped in RwLock for interior mutability.
    /// Updated transparently by response interceptor when Navidrome returns
    /// a refreshed token via the `X-ND-Authorization` header.
    token: Arc<RwLock<String>>,
    /// Optional callback invoked when token is refreshed, for persistence.
    on_token_refresh: Option<TokenRefreshCallback>,
}

impl ApiClient {
    pub fn new(base_url: Url, token: String) -> Self {
        // Configure client with shorter idle timeout for faster shutdown
        let client = Client::builder()
            .pool_idle_timeout(std::time::Duration::from_secs(5))
            .build()
            .expect("Failed to build HTTP client");

        Self {
            client: Arc::new(client),
            base_url,
            token: Arc::new(RwLock::new(token)),
            on_token_refresh: None,
        }
    }

    /// Set a callback to be invoked when a refreshed JWT is received.
    /// Used to persist the new token to redb.
    pub fn set_on_token_refresh(&mut self, callback: TokenRefreshCallback) {
        self.on_token_refresh = Some(callback);
    }
}

impl Clone for ApiClient {
    fn clone(&self) -> Self {
        Self {
            client: self.client.clone(),
            base_url: self.base_url.clone(),
            token: self.token.clone(),
            on_token_refresh: self.on_token_refresh.clone(),
        }
    }
}

impl ApiClient {
    /// Get the underlying HTTP client for making raw requests
    pub fn http_client(&self) -> Arc<Client> {
        self.client.clone()
    }

    /// Get the current bearer token string (read lock)
    fn bearer_header(&self) -> String {
        format!("Bearer {}", self.token.read())
    }

    /// Check response headers for a refreshed JWT and update if found.
    /// Navidrome's JWTRefresher middleware returns a fresh token in the
    /// `X-ND-Authorization` header on every authenticated response.
    fn intercept_token_refresh(&self, response: &reqwest::Response) {
        if let Some(header_value) = response.headers().get("x-nd-authorization")
            && let Ok(new_token) = header_value.to_str()
        {
            // Strip "Bearer " prefix if present
            let token_str = new_token.strip_prefix("Bearer ").unwrap_or(new_token);

            // Only update if the token actually changed
            {
                let current = self.token.read();
                if *current == token_str {
                    return;
                }
            }

            debug!("JWT refreshed from server response header");
            {
                let mut token = self.token.write();
                *token = token_str.to_string();
            }

            // Notify callback for persistence (e.g., save to redb)
            if let Some(ref callback) = self.on_token_refresh {
                callback(token_str);
            }
        }
    }

    /// Make a GET request to the Navidrome REST API
    /// endpoint: API path (e.g., "/api/album")
    /// params: Query parameters as key-value pairs
    pub async fn get(&self, endpoint: &str, params: &[(&str, &str)]) -> Result<String> {
        let mut url = self
            .base_url
            .join(endpoint)
            .with_context(|| format!("Failed to join endpoint: {endpoint}"))?;

        // Build query string manually to avoid Send issues with query_pairs_mut
        if !params.is_empty() {
            let mut query_parts = Vec::new();
            for (key, value) in params {
                query_parts.push(format!(
                    "{}={}",
                    url::form_urlencoded::byte_serialize(key.as_bytes()).collect::<String>(),
                    url::form_urlencoded::byte_serialize(value.as_bytes()).collect::<String>()
                ));
            }
            url.set_query(Some(&query_parts.join("&")));
        }

        let response = self
            .client
            .get(url.as_str())
            .header("X-ND-Authorization", self.bearer_header())
            .send()
            .await
            .context("Failed to send GET request")?;

        // Intercept refreshed JWT from response header
        self.intercept_token_refresh(&response);

        let status = response.status();
        let body = response
            .text()
            .await
            .context("Failed to read response body")?;

        if status.is_success() {
            Ok(body)
        } else {
            Err(anyhow::anyhow!(
                "API request failed with status {status}: {body}"
            ))
        }
    }

    /// Make a GET request and return both body and headers
    /// Returns (body, total_count_from_header)
    pub async fn get_with_headers(
        &self,
        endpoint: &str,
        params: &[(&str, &str)],
    ) -> Result<(String, Option<u32>)> {
        let mut url = self
            .base_url
            .join(endpoint)
            .with_context(|| format!("Failed to join endpoint: {endpoint}"))?;

        // Build query string manually to avoid Send issues with query_pairs_mut
        if !params.is_empty() {
            let mut query_parts = Vec::new();
            for (key, value) in params {
                query_parts.push(format!(
                    "{}={}",
                    url::form_urlencoded::byte_serialize(key.as_bytes()).collect::<String>(),
                    url::form_urlencoded::byte_serialize(value.as_bytes()).collect::<String>()
                ));
            }
            url.set_query(Some(&query_parts.join("&")));
        }

        let response = self
            .client
            .get(url.as_str())
            .header("X-ND-Authorization", self.bearer_header())
            .send()
            .await
            .context("Failed to send GET request")?;

        // Intercept refreshed JWT from response header
        self.intercept_token_refresh(&response);

        let status = response.status();

        // Extract X-Total-Count header if present
        let total_count = response
            .headers()
            .get("X-Total-Count")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u32>().ok());

        let body = response
            .text()
            .await
            .context("Failed to read response body")?;

        if status.is_success() {
            Ok((body, total_count))
        } else {
            Err(anyhow::anyhow!(
                "API request failed with status {status}: {body}"
            ))
        }
    }

    /// Make a POST request with a JSON body to the Navidrome REST API
    pub async fn post_json(
        &self,
        endpoint: &str,
        json_body: &impl serde::Serialize,
    ) -> Result<String> {
        let url = self
            .base_url
            .join(endpoint)
            .with_context(|| format!("Failed to join endpoint: {endpoint}"))?;

        let response = self
            .client
            .post(url.as_str())
            .header("X-ND-Authorization", self.bearer_header())
            .json(json_body)
            .send()
            .await
            .context("Failed to send POST request")?;

        self.intercept_token_refresh(&response);

        let status = response.status();
        let body = response
            .text()
            .await
            .context("Failed to read response body")?;

        if status.is_success() {
            Ok(body)
        } else {
            Err(anyhow::anyhow!(
                "API POST {endpoint} failed with status {status}: {body}"
            ))
        }
    }

    /// Make a PUT request with a JSON body to the Navidrome REST API
    pub async fn put_json(
        &self,
        endpoint: &str,
        json_body: &impl serde::Serialize,
    ) -> Result<String> {
        let url = self
            .base_url
            .join(endpoint)
            .with_context(|| format!("Failed to join endpoint: {endpoint}"))?;

        let response = self
            .client
            .put(url.as_str())
            .header("X-ND-Authorization", self.bearer_header())
            .json(json_body)
            .send()
            .await
            .context("Failed to send PUT request")?;

        self.intercept_token_refresh(&response);

        let status = response.status();
        let body = response
            .text()
            .await
            .context("Failed to read response body")?;

        if status.is_success() {
            Ok(body)
        } else {
            Err(anyhow::anyhow!(
                "API PUT {endpoint} failed with status {status}: {body}"
            ))
        }
    }

    /// Make a DELETE request to the Navidrome REST API
    pub async fn delete(&self, endpoint: &str) -> Result<()> {
        let url = self
            .base_url
            .join(endpoint)
            .with_context(|| format!("Failed to join endpoint: {endpoint}"))?;

        let response = self
            .client
            .delete(url.as_str())
            .header("X-ND-Authorization", self.bearer_header())
            .send()
            .await
            .context("Failed to send DELETE request")?;

        self.intercept_token_refresh(&response);

        let status = response.status();
        if status.is_success() {
            Ok(())
        } else {
            let body = response.text().await.unwrap_or_default();
            Err(anyhow::anyhow!(
                "API DELETE {endpoint} failed with status {status}: {body}"
            ))
        }
    }
}
