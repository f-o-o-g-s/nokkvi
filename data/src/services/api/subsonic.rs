//! Subsonic REST API request helpers
//!
//! Provides POST-based request helpers that send credentials in the request body
//! rather than as URL query parameters. This leverages the OpenSubsonic `formPost`
//! extension supported by Navidrome, hiding credentials from server logs and URLs.

use std::sync::Arc;

use anyhow::{Context, Result};

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

    if !status.is_success() {
        return Err(anyhow::anyhow!(
            "{operation_label}: HTTP {status}, body: {body}"
        ));
    }

    // Subsonic API wraps errors inside a 200 OK response — check the inner status
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&body)
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
    use super::*;

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
}
