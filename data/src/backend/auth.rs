//! AuthGateway — authentication state and API client management
//!
//! Wraps `AuthService` behind `Arc<Mutex>` for shared access across services.

use std::sync::Arc;

use anyhow::Result;
use tokio::sync::Mutex;

use crate::services::{api::client::ApiClient, auth::AuthService};

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

    /// Get the initial JWT token (as received from login or resume)
    pub async fn get_token(&self) -> String {
        let auth_service = self.auth_service.lock().await;
        auth_service.get_token().to_string()
    }

    /// Set the token refresh callback (for persisting refreshed JWT to redb)
    pub async fn set_token_refresh_callback(
        &self,
        callback: crate::services::api::client::TokenRefreshCallback,
    ) {
        let mut auth_service = self.auth_service.lock().await;
        auth_service.set_token_refresh_callback(callback);
    }

    /// Get API client (cloned) - for initializing other ViewModels
    pub async fn get_client(&self) -> Option<ApiClient> {
        let auth_service = self.auth_service.lock().await;
        auth_service.get_client().cloned()
    }

    /// Fetches the server version dynamically using the Subsonic /rest/ping endpoint
    pub async fn fetch_server_version(&self) -> Result<String> {
        let auth_service = self.auth_service.lock().await;
        auth_service.fetch_server_version().await
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
}
