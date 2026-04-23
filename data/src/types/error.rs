use thiserror::Error;

#[derive(Debug, Error)]
pub enum NokkviError {
    #[error("Unauthorized: Session has expired or credentials are invalid")]
    Unauthorized,
    #[error("API Request failed: {0}")]
    ApiError(String),
}
