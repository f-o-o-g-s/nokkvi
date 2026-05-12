//! Subsonic REST API request helpers
//!
//! Provides POST-based request helpers that send credentials in the request body
//! rather than as URL query parameters. This leverages the OpenSubsonic `formPost`
//! extension supported by Navidrome, hiding credentials from server logs and URLs.

use std::sync::Arc;

use anyhow::{Context, Result};

use crate::types::error::NokkviError;

/// Send a POST request to a Subsonic REST API endpoint.
///
/// Credentials and standard parameters are sent as `application/x-www-form-urlencoded`
/// in the request body rather than in the URL, leveraging the OpenSubsonic `formPost`
/// extension. This prevents credentials from appearing in server access logs.
///
/// # Arguments
/// * `http_client` - Shared reqwest client
/// * `server_url` - Base Navidrome server URL (e.g., "http://localhost:4533")
/// * `endpoint` - Subsonic endpoint name (e.g., "star", "setRating", "getPlaylist")
/// * `subsonic_credential` - Pre-formatted credential string (e.g., "u=user&t=token&s=salt")
/// * `extra_params` - Additional endpoint-specific parameters as key-value pairs
pub async fn subsonic_post(
    http_client: &Arc<reqwest::Client>,
    server_url: &str,
    endpoint: &str,
    subsonic_credential: &str,
    extra_params: &[(&str, &str)],
) -> Result<reqwest::Response> {
    let url = format!("{server_url}/rest/{endpoint}");

    // Build form body: credentials + standard params + extra params
    let mut body = format!("{subsonic_credential}&f=json&v=1.8.0&c=nokkvi");
    for (key, value) in extra_params {
        body.push_str(&format!(
            "&{}={}",
            url::form_urlencoded::byte_serialize(key.as_bytes()).collect::<String>(),
            url::form_urlencoded::byte_serialize(value.as_bytes()).collect::<String>()
        ));
    }

    http_client
        .post(&url)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(body)
        .send()
        .await
        .with_context(|| format!("Failed to POST to Subsonic endpoint: {endpoint}"))
}

/// Send a POST request and verify the response was successful.
///
/// Convenience wrapper around [`subsonic_post`] that checks the HTTP status code
/// and returns a descriptive error on failure. Use for fire-and-forget mutation
/// endpoints (star, setRating, createPlaylist, deletePlaylist, updatePlaylist).
pub async fn subsonic_post_ok(
    http_client: &Arc<reqwest::Client>,
    server_url: &str,
    endpoint: &str,
    subsonic_credential: &str,
    extra_params: &[(&str, &str)],
    operation_label: &str,
) -> Result<()> {
    let response = subsonic_post(
        http_client,
        server_url,
        endpoint,
        subsonic_credential,
        extra_params,
    )
    .await?;

    let status = response.status();
    let body = response.text().await.unwrap_or_default();

    check_subsonic_response_status(status, &body, operation_label)
}

/// Apply Subsonic response status/body policy.
///
/// Pure (no I/O) so it can be unit-tested without an HTTP server. Returns:
/// - `Err(NokkviError::Unauthorized)` for HTTP 401, so the UI drops to login on
///   JWT expiry (mirrors [`crate::services::api::client::ApiClient`]).
/// - `Err(anyhow!(...))` for any other non-2xx status.
/// - `Err(anyhow!(...))` when the body wraps a Subsonic-envelope error
///   (`subsonic-response.status == "failed"`) inside an HTTP 200.
/// - `Ok(())` otherwise.
fn check_subsonic_response_status(
    status: reqwest::StatusCode,
    body: &str,
    operation_label: &str,
) -> Result<()> {
    if status == reqwest::StatusCode::UNAUTHORIZED {
        return Err(NokkviError::Unauthorized.into());
    }

    if !status.is_success() {
        return Err(anyhow::anyhow!(
            "{operation_label}: HTTP {status}, body: {body}"
        ));
    }

    // Subsonic API wraps errors inside a 200 OK response — check the inner status
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(body)
        && let Some(subsonic) = json.get("subsonic-response")
        && subsonic.get("status").and_then(|s| s.as_str()) == Some("failed")
    {
        let error_msg = subsonic
            .get("error")
            .and_then(|e| e.get("message"))
            .and_then(|m| m.as_str())
            .unwrap_or("unknown error");
        return Err(anyhow::anyhow!("{operation_label}: {error_msg}"));
    }

    Ok(())
}

/// Build a Subsonic REST API URL (GET-style, credentials in query string).
///
/// Only used for streaming URLs where POST is not possible due to HTTP Range
/// request requirements. For all other endpoints, use [`subsonic_post`] instead.
pub fn build_subsonic_url(
    server_url: &str,
    endpoint: &str,
    song_id: &str,
    subsonic_credential: &str,
) -> String {
    format!(
        "{server_url}/rest/{endpoint}?id={song_id}&{subsonic_credential}&f=json&v=1.8.0&c=nokkvi"
    )
}

#[cfg(test)]
mod tests {
    use reqwest::StatusCode;

    use super::*;
    use crate::types::error::NokkviError;

    #[test]
    fn test_build_subsonic_url() {
        let url = build_subsonic_url(
            "http://localhost:4533",
            "star",
            "song123",
            "u=admin&p=enc:hex123",
        );
        assert_eq!(
            url,
            "http://localhost:4533/rest/star?id=song123&u=admin&p=enc:hex123&f=json&v=1.8.0&c=nokkvi"
        );
    }

    /// HTTP 401 from a Subsonic mutation endpoint must downcast to
    /// [`NokkviError::Unauthorized`] so the UI can drop to login.
    ///
    /// Mirrors [`crate::services::api::client::ApiClient`]'s discipline; without
    /// this routing, star/unstar/setRating + radio CRUD + replace_playlist_tracks
    /// surface a generic toast on JWT expiry instead of returning to the login screen.
    #[test]
    fn check_subsonic_response_status_routes_401_to_unauthorized() {
        let err = check_subsonic_response_status(StatusCode::UNAUTHORIZED, "", "star song")
            .expect_err("401 must produce an error");

        let nokkvi_err = err
            .downcast_ref::<NokkviError>()
            .expect("401 should downcast to NokkviError");
        assert!(
            matches!(nokkvi_err, NokkviError::Unauthorized),
            "expected NokkviError::Unauthorized, got {nokkvi_err:?}"
        );
    }

    /// Non-401 HTTP failures must keep the existing descriptive error format.
    #[test]
    fn check_subsonic_response_status_500_returns_generic_error() {
        let err = check_subsonic_response_status(
            StatusCode::INTERNAL_SERVER_ERROR,
            "server boom",
            "star song",
        )
        .expect_err("500 must produce an error");

        // Not a NokkviError — caller's downcast for Unauthorized must miss.
        assert!(err.downcast_ref::<NokkviError>().is_none());

        let msg = format!("{err}");
        assert!(msg.contains("star song"), "missing label in: {msg}");
        assert!(msg.contains("500"), "missing status in: {msg}");
        assert!(msg.contains("server boom"), "missing body in: {msg}");
    }

    /// HTTP 200 with a healthy Subsonic envelope returns `Ok(())`.
    #[test]
    fn check_subsonic_response_status_ok_envelope_is_ok() {
        let body = r#"{"subsonic-response":{"status":"ok","version":"1.8.0"}}"#;
        let result = check_subsonic_response_status(StatusCode::OK, body, "star song");
        assert!(result.is_ok(), "expected Ok, got {result:?}");
    }

    /// HTTP 200 with a `failed` Subsonic envelope surfaces the inner error message.
    #[test]
    fn check_subsonic_response_status_failed_envelope_returns_error() {
        let body = r#"{"subsonic-response":{"status":"failed","error":{"code":70,"message":"Song not found"}}}"#;
        let err = check_subsonic_response_status(StatusCode::OK, body, "star song")
            .expect_err("failed envelope must produce an error");

        // Envelope failures stay as plain anyhow errors (not session-expiry).
        assert!(err.downcast_ref::<NokkviError>().is_none());
        let msg = format!("{err}");
        assert!(msg.contains("star song"));
        assert!(msg.contains("Song not found"));
    }
}
