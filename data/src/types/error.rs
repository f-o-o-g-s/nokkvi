use thiserror::Error;

/// Errors emitted by the data layer at the boundary between
/// `crate::services::api` and consumers.
///
/// Currently a single variant — every other API failure path uses
/// `anyhow::Result`. The 401 case is broken out so the UI can downcast
/// and drop to the login screen instead of merely surfacing a toast.
#[derive(Debug, Error)]
pub enum NokkviError {
    #[error("Unauthorized: Session has expired or credentials are invalid")]
    Unauthorized,
}
