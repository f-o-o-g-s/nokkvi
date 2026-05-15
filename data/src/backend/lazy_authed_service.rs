//! LazyAuthedService — generic wrapper that lazily constructs an API
//! service from a shared AuthGateway on first use.
//!
//! Replaces the three near-identical `get_service()` lazy-init bodies
//! in AlbumsService / ArtistsService / SongsService that all share the
//! same shape: store an `Arc<OnceCell<AuthGateway>>` + an
//! `Arc<OnceCell<*ApiService>>`, attach the gateway via `.with_auth()`,
//! then on first `get_service()` call snapshot auth, snapshot client,
//! build the inner service, cache it.

use std::sync::Arc;

use anyhow::{Result, anyhow};
use tokio::sync::OnceCell;

use crate::{backend::auth::AuthGateway, services::api::client::ApiClient};

/// Builds an `A` from an authenticated `ApiClient`. Each API service
/// type uses its existing `new(ApiClient)` constructor — pass it as
/// `AlbumsApiService::new`, `ArtistsApiService::new`, etc.
pub type ServiceFactory<A> = fn(ApiClient) -> A;

/// Lazy wrapper that pairs a shared `AuthGateway` cell with a cached
/// API service `A`. The service is built on first call to `get()` via
/// `factory(client)` and cached for the lifetime of the wrapper.
///
/// Cloning the wrapper clones the inner `Arc`s — both `OnceCell`s are
/// shared, matching the previous `Arc<OnceCell<*>>` field-pair layout.
pub struct LazyAuthedService<A> {
    auth_gateway: Arc<OnceCell<AuthGateway>>,
    service: Arc<OnceCell<A>>,
    factory: ServiceFactory<A>,
}

impl<A> Clone for LazyAuthedService<A> {
    fn clone(&self) -> Self {
        Self {
            auth_gateway: self.auth_gateway.clone(),
            service: self.service.clone(),
            factory: self.factory,
        }
    }
}

impl<A: Send + Sync + 'static> LazyAuthedService<A> {
    pub fn new(factory: ServiceFactory<A>) -> Self {
        Self {
            auth_gateway: Arc::new(OnceCell::new()),
            service: Arc::new(OnceCell::new()),
            factory,
        }
    }

    /// Attach the auth gateway. Idempotent on success; subsequent calls
    /// against an already-initialized cell are no-ops (preserves the
    /// silent-double-init behavior of the previous `Arc<OnceCell<_>>::set`
    /// pattern at the three migrated call sites).
    pub fn with_auth(self, gw: AuthGateway) -> Self {
        let _ = self.auth_gateway.set(gw);
        self
    }

    /// Lazily build the cached `A` on first call.
    ///
    /// Uses `OnceCell::get_or_try_init` for atomic init-once semantics
    /// and lock-free reads on subsequent calls.
    pub async fn get(&self) -> Result<&A> {
        self.service
            .get_or_try_init(|| async {
                let auth = self.auth_gateway.get().ok_or_else(|| {
                    anyhow!("LazyAuthedService not initialized. Please authenticate first.")
                })?;
                let client = auth.get_client().await.ok_or_else(|| {
                    anyhow!("LazyAuthedService not initialized. Please authenticate first.")
                })?;
                Ok((self.factory)(client))
            })
            .await
    }

    /// Single-lock `(server_url, subsonic_credential)` pair, or the
    /// empty-pair fallback when the gateway hasn't been attached yet.
    pub async fn server_config(&self) -> (String, String) {
        match self.auth_gateway.get() {
            Some(auth) => auth.server_config().await,
            None => (String::new(), String::new()),
        }
    }

    /// Borrow the attached `AuthGateway`, if any. Required by the few
    /// call sites that build an ad-hoc API service from a fresh client
    /// snapshot (e.g. `AlbumsService::load_album_songs`).
    pub fn auth(&self) -> Option<&AuthGateway> {
        self.auth_gateway.get()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Stand-in API service for the cache-pointer identity check. The
    /// inner `marker` lets us prove the second `get()` returns the same
    /// cached instance (pointer equality on the `&TestApi` borrow).
    struct TestApi {
        marker: u64,
    }

    /// Factory that always builds the same marker — only the address of
    /// the cached `TestApi` matters for cache-identity assertions.
    const TEST_FACTORY: ServiceFactory<TestApi> = |_client| TestApi { marker: 42 };

    /// Constructing a `LazyAuthedService<TestApi>` must NOT invoke the
    /// factory — it only runs on first `get()` after `with_auth()`.
    /// Verified indirectly: the inner cell stays uninitialized so `get()`
    /// without `with_auth` errors instead of silently materialising a
    /// stub.
    #[tokio::test]
    async fn lazy_authed_service_new_does_not_init() {
        let svc: LazyAuthedService<TestApi> = LazyAuthedService::new(TEST_FACTORY);
        // No `with_auth` ⇒ the cell must be empty, so `get()` errors.
        assert!(
            svc.get().await.is_err(),
            "fresh LazyAuthedService must error on get() before with_auth"
        );
    }

    /// Calling `.get().await` before `.with_auth()` returns the
    /// "not initialized" error.
    #[tokio::test]
    async fn lazy_authed_service_get_without_auth_errors() {
        let svc: LazyAuthedService<TestApi> = LazyAuthedService::new(TEST_FACTORY);
        let res = svc.get().await;
        assert!(res.is_err(), "expected error when auth not attached");
        let err_msg = res.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(
            err_msg.contains("not initialized"),
            "expected 'not initialized' in error, got: {err_msg}"
        );
    }

    /// `server_config()` against an un-attached gateway returns the
    /// empty-pair fallback. Pins the pre-migration behavior of the
    /// three `get_server_config()` empty arms.
    #[tokio::test]
    async fn lazy_authed_service_server_config_without_auth_returns_empty_pair() {
        let svc: LazyAuthedService<TestApi> = LazyAuthedService::new(TEST_FACTORY);

        let (url, cred) = svc.server_config().await;
        assert!(url.is_empty(), "server_url should be empty pre-auth");
        assert!(
            cred.is_empty(),
            "subsonic_credential should be empty pre-auth"
        );
        assert!(svc.auth().is_none(), "auth() returns None pre-with_auth");
    }

    /// Happy path: after `with_auth(resumed_gateway)`, the factory runs
    /// on first `get()` and the cached service is returned on subsequent
    /// calls (pointer equality on the cell-owned `&TestApi`). Also pins
    /// `server_config()` returning the resumed pair (delegated to
    /// AuthGateway). Uses `resume_session` so no network call is needed.
    #[tokio::test]
    async fn lazy_authed_service_get_after_auth_inits_once_and_caches() {
        let gateway = AuthGateway::new().expect("auth gateway");
        gateway
            .resume_session(
                "http://localhost:4533".to_string(),
                "alice".to_string(),
                "jwt-test".to_string(),
                "u=alice&s=salt&t=token".to_string(),
            )
            .await
            .expect("resume session");

        let svc = LazyAuthedService::<TestApi>::new(TEST_FACTORY).with_auth(gateway);

        // First get builds via factory.
        let first = svc.get().await.expect("first get");
        assert_eq!(first.marker, 42);

        // Second get hits the cache — must be the same cell-owned ref.
        let second = svc.get().await.expect("second get");
        assert!(
            std::ptr::eq(first, second),
            "second get must return the cached instance"
        );

        // server_config() returns the resumed pair via the gateway.
        let (url, cred) = svc.server_config().await;
        assert_eq!(url, "http://localhost:4533");
        assert_eq!(cred, "u=alice&s=salt&t=token");

        // auth() exposes the attached gateway.
        assert!(svc.auth().is_some(), "auth() returns Some after with_auth");
    }
}
