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
