//! Subsonic REST API request helpers
//!
//! Provides POST-based request helpers that send credentials in the request body
//! rather than as URL query parameters. This leverages the OpenSubsonic `formPost`
//! extension supported by Navidrome, hiding credentials from server logs and URLs.

use std::sync::Arc;

use anyhow::{Context, Result};

use crate::{services::api::parse, types::error::NokkviError};

/// Generic wrapper for the Subsonic `{"subsonic-response": ...}` envelope.
///
/// Every Subsonic endpoint wraps its payload in the same outer object; the
/// per-endpoint inner shape goes in `T` (declared file-private next to its
/// service). `pub(crate)` so modules outside `services::api` (e.g.
/// `services::auth`) can parse the envelope too.
#[derive(Debug, serde::Deserialize)]
pub(crate) struct SubsonicEnvelope<T> {
    #[serde(rename = "subsonic-response")]
    pub(crate) response: T,
}

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
pub(crate) async fn subsonic_post(
    http_client: &Arc<reqwest::Client>,
    server_url: &str,
    endpoint: &str,
    subsonic_credential: &str,
    extra_params: &[(&str, &str)],
) -> Result<reqwest::Response> {
    subsonic_post_request(
        http_client,
        server_url,
        endpoint,
        subsonic_credential,
        extra_params,
    )
    .send()
    .await
    .with_context(|| format!("Failed to POST to Subsonic endpoint: {endpoint}"))
}

/// Build (without sending) the [`subsonic_post`] request. Split out so the
/// request's shape (body, headers) can be tested without a server.
fn subsonic_post_request(
    http_client: &reqwest::Client,
    server_url: &str,
    endpoint: &str,
    subsonic_credential: &str,
    extra_params: &[(&str, &str)],
) -> reqwest::RequestBuilder {
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

    crate::services::api::client::with_client_unique_id(
        http_client
            .post(&url)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(body),
    )
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

/// Subsonic error codes that mean the credential itself was rejected, so the
/// session is dead: 40 (wrong username or password, which is also what
/// Navidrome answers a stale token with) and the OpenSubsonic 44 (invalid API
/// key). 41 (token auth not supported) stays a plain error: it is a server
/// setup problem that logging in again cannot fix.
const SESSION_REJECTED_CODES: [i64; 2] = [40, 44];

/// Apply Subsonic response status/body policy.
///
/// Pure (no I/O) so it can be unit-tested without an HTTP server. Returns:
/// - `Err(NokkviError::Unauthorized)` for HTTP 401, so the UI drops to login on
///   JWT expiry (mirrors [`crate::services::api::client::ApiClient`]).
/// - `Err(anyhow!(...))` for any other non-2xx status.
/// - `Err(NokkviError::Unauthorized)` when the body wraps a Subsonic-envelope
///   error (`subsonic-response.status == "failed"`) inside an HTTP 200 whose
///   code is in [`SESSION_REJECTED_CODES`]: a rejected token arrives this way,
///   not as a 401.
/// - `Err(anyhow!(...))` for any other failed envelope.
/// - `Ok(())` otherwise.
///
/// Shared by the checked readers and every Subsonic write (star, rating,
/// playlist and radio edits, the play-queue save), so a dead credential on
/// any of them routes to the login screen.
pub(crate) fn check_subsonic_response_status(
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
        let error = subsonic.get("error");
        let error_msg = error
            .and_then(|e| e.get("message"))
            .and_then(|m| m.as_str())
            .unwrap_or("unknown error");
        let code = error.and_then(|e| e.get("code")).and_then(|c| c.as_i64());
        if code.is_some_and(|c| SESSION_REJECTED_CODES.contains(&c)) {
            // The typed error carries no detail, so keep the server's reason
            // in the file log for "why was I logged out".
            tracing::debug!("{operation_label}: credential rejected ({code:?}): {error_msg}");
            return Err(NokkviError::Unauthorized.into());
        }
        return Err(anyhow::anyhow!("{operation_label}: {error_msg}"));
    }

    Ok(())
}

/// POST to a Subsonic endpoint and parse the enveloped JSON payload.
///
/// Shared fetch pipeline for the typed list endpoints (getGenres,
/// getMusicFolders, getInternetRadioStations, getSimilarSongs2, getPlaylist):
/// send via [`subsonic_post`], read the body, parse a
/// [`SubsonicEnvelope<T>`] with the UTF-8-safe preview parser, and return the
/// inner payload. `label` names the response in every error context
/// (`Failed to fetch {label}` / `Failed to read {label} response` /
/// `Failed to parse {label}: <preview>`).
pub(crate) async fn subsonic_get_envelope<T: serde::de::DeserializeOwned>(
    http_client: &Arc<reqwest::Client>,
    server_url: &str,
    endpoint: &str,
    subsonic_credential: &str,
    extra_params: &[(&str, &str)],
    label: &str,
) -> Result<T> {
    let (_status, body) = fetch_subsonic_body(
        http_client,
        server_url,
        endpoint,
        subsonic_credential,
        extra_params,
        label,
    )
    .await?;
    let envelope: SubsonicEnvelope<T> = parse::parse_json_with_preview(&body, label)?;
    Ok(envelope.response)
}

/// [`subsonic_get_envelope`] for a read whose empty answer would be taken as
/// fact: it applies [`check_subsonic_response_status`] before parsing, so
/// HTTP 401 is `NokkviError::Unauthorized` and any other non-2xx status, or
/// a `status: "failed"` envelope (a rejected token, an unknown id), is an
/// error rather than an empty payload. The lenient twin checks neither.
pub(crate) async fn subsonic_get_envelope_checked<T: serde::de::DeserializeOwned>(
    http_client: &Arc<reqwest::Client>,
    server_url: &str,
    endpoint: &str,
    subsonic_credential: &str,
    extra_params: &[(&str, &str)],
    label: &str,
) -> Result<T> {
    let (status, body) = fetch_subsonic_body(
        http_client,
        server_url,
        endpoint,
        subsonic_credential,
        extra_params,
        label,
    )
    .await?;
    check_subsonic_response_status(status, &body, label)?;
    let envelope: SubsonicEnvelope<T> = parse::parse_json_with_preview(&body, label)?;
    Ok(envelope.response)
}

/// POST to a Subsonic endpoint and read the status and body, with `label`
/// in the error contexts. The shared front half of the envelope readers.
async fn fetch_subsonic_body(
    http_client: &Arc<reqwest::Client>,
    server_url: &str,
    endpoint: &str,
    subsonic_credential: &str,
    extra_params: &[(&str, &str)],
    label: &str,
) -> Result<(reqwest::StatusCode, String)> {
    let response = subsonic_post(
        http_client,
        server_url,
        endpoint,
        subsonic_credential,
        extra_params,
    )
    .await
    .with_context(|| format!("Failed to fetch {label}"))?;
    let status = response.status();
    let body = response
        .text()
        .await
        .with_context(|| format!("Failed to read {label} response"))?;
    Ok((status, body))
}

/// Coerce a Subsonic JSON field that may be either a single object or an
/// array into a `Vec<T>`.
///
/// Subsonic's XML→JSON bridge collapses a single-element collection to a bare
/// object (e.g. one `<genre>` / `<internetRadioStation>` / playlist `<entry>`)
/// instead of a one-element array, so every "list" field needs the same
/// one-or-many fan-out. This centralizes that quirk: an array deserializes
/// straight to `Vec<T>`; a single value is wrapped in a one-element Vec. Parse
/// failures propagate via `?` rather than panicking.
pub(crate) fn deserialize_one_or_many<T: serde::de::DeserializeOwned>(
    value: serde_json::Value,
) -> Result<Vec<T>> {
    if value.is_array() {
        Ok(serde_json::from_value(value)?)
    } else {
        Ok(vec![serde_json::from_value(value)?])
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use reqwest::StatusCode;

    use super::*;
    use crate::types::error::NokkviError;

    /// Every `/rest` POST carries the client unique id, so the event a write
    /// causes (star, setRating) skips nokkvi's own stream and stays with the
    /// same user instead of reaching every user on the server.
    #[test]
    fn subsonic_post_request_carries_the_client_unique_id() {
        let client = reqwest::Client::new();
        let request = subsonic_post_request(
            &client,
            "http://localhost:4533",
            "star",
            "u=alice&t=tok&s=salt",
            &[("id", "s1")],
        )
        .build()
        .expect("request builds");
        assert_eq!(request.url().as_str(), "http://localhost:4533/rest/star");
        assert_eq!(
            request
                .headers()
                .get(crate::services::api::client::CLIENT_UNIQUE_ID_HEADER)
                .and_then(|v| v.to_str().ok()),
            Some(crate::services::api::client::client_unique_id())
        );
    }

    /// The generic envelope must parse a canonical `{"subsonic-response":
    /// ...}` body and reject a body missing the key — this is the single
    /// declaration every typed list endpoint now flows through.
    #[test]
    fn subsonic_envelope_parses_canonical_body_and_rejects_missing_key() {
        let body = r#"{"subsonic-response":{"status":"ok","version":"1.8.0"}}"#;
        let envelope: SubsonicEnvelope<serde_json::Value> =
            serde_json::from_str(body).expect("canonical envelope must parse");
        assert_eq!(envelope.response["status"], "ok");

        let missing = r#"{"status":"ok","version":"1.8.0"}"#;
        assert!(
            serde_json::from_str::<SubsonicEnvelope<serde_json::Value>>(missing).is_err(),
            "body without the subsonic-response key must fail to parse"
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

    /// A rejected credential comes back as HTTP 200 with a failed envelope:
    /// code 40 (wrong username or password, which is also what a stale token
    /// gets) or the OpenSubsonic code 44 (invalid API key). Both mean the
    /// session is dead, so they route to login like HTTP 401, on reads and
    /// writes alike.
    #[test]
    fn check_subsonic_response_status_routes_auth_codes_to_unauthorized() {
        for code in [40, 44] {
            let body = format!(
                r#"{{"subsonic-response":{{"status":"failed","error":{{"code":{code},"message":"rejected"}}}}}}"#
            );
            let err = check_subsonic_response_status(StatusCode::OK, &body, "star song")
                .expect_err("a failed envelope must produce an error");
            assert!(
                matches!(
                    err.downcast_ref::<NokkviError>(),
                    Some(NokkviError::Unauthorized)
                ),
                "code {code} should be Unauthorized, got: {err:#}"
            );
        }
    }

    /// Every other failure code stays a plain error: 41 (token auth not
    /// supported) is a server setup problem a re-login cannot fix, and 50
    /// (not authorized) is a permission refusal on one operation.
    #[test]
    fn check_subsonic_response_status_keeps_other_codes_plain() {
        for code in [0, 41, 50, 70] {
            let body = format!(
                r#"{{"subsonic-response":{{"status":"failed","error":{{"code":{code},"message":"nope"}}}}}}"#
            );
            let err = check_subsonic_response_status(StatusCode::OK, &body, "star song")
                .expect_err("a failed envelope must produce an error");
            assert!(
                err.downcast_ref::<NokkviError>().is_none(),
                "code {code} must stay a plain error, got: {err:#}"
            );
            assert!(format!("{err}").contains("nope"), "got: {err}");
        }
    }

    /// A JSON array deserializes element-for-element into the target Vec —
    /// both scalar and struct element types round-trip.
    #[test]
    fn deserialize_one_or_many_handles_array() {
        let nums: Vec<i64> = deserialize_one_or_many(serde_json::json!([1, 2, 3]))
            .expect("array of scalars must deserialize");
        assert_eq!(nums, vec![1, 2, 3]);

        #[derive(serde::Deserialize, PartialEq, Debug)]
        struct Item {
            id: String,
        }
        let items: Vec<Item> =
            deserialize_one_or_many(serde_json::json!([{ "id": "a" }, { "id": "b" }]))
                .expect("array of structs must deserialize");
        assert_eq!(
            items,
            vec![
                Item {
                    id: "a".to_string()
                },
                Item {
                    id: "b".to_string()
                }
            ]
        );
    }

    /// A single JSON object (NOT wrapped in `[]`) returns a one-element Vec —
    /// this is the Subsonic single-element quirk the helper exists to absorb.
    #[test]
    fn deserialize_one_or_many_handles_single_object() {
        #[derive(serde::Deserialize, PartialEq, Debug)]
        struct Item {
            id: String,
        }
        let items: Vec<Item> = deserialize_one_or_many(serde_json::json!({ "id": "solo" }))
            .expect("single object must deserialize to a one-element Vec");
        assert_eq!(
            items,
            vec![Item {
                id: "solo".to_string()
            }]
        );
    }

    /// A value whose shape does not match `T` returns `Err` (via `?`) rather
    /// than panicking — guards the propagation path.
    #[test]
    fn deserialize_one_or_many_propagates_parse_error() {
        // i64 target fed a string element: serde_json must error, not panic.
        let result: Result<Vec<i64>> = deserialize_one_or_many(serde_json::json!(["not a number"]));
        assert!(result.is_err(), "shape mismatch must produce an error");

        // Single-object branch is equally guarded.
        let result: Result<Vec<i64>> = deserialize_one_or_many(serde_json::json!({ "k": "v" }));
        assert!(
            result.is_err(),
            "single-object shape mismatch must produce an error"
        );
    }

    /// Answer one HTTP request on an ephemeral port with `status_line` and
    /// `body`, reading the whole request first so closing the socket never
    /// resets it. Returns the base URL.
    pub(crate) async fn one_shot_server(status_line: &'static str, body: &'static str) -> String {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind ephemeral port");
        let addr = listener.local_addr().expect("local addr");
        tokio::spawn(async move {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            let mut request = Vec::new();
            let mut chunk = [0u8; 4096];
            while let Ok(read) = socket.read(&mut chunk).await
                && read > 0
            {
                request.extend_from_slice(&chunk[..read]);
                let text = String::from_utf8_lossy(&request);
                if let Some(head_end) = text.find("\r\n\r\n") {
                    let length = text[..head_end]
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .and_then(|v| v.trim().parse::<usize>().ok())
                        })
                        .unwrap_or(0);
                    if request.len() >= head_end + 4 + length {
                        break;
                    }
                }
            }
            let response = format!(
                "HTTP/1.1 {status_line}\r\nContent-Type: application/json\r\n\
                 Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = socket.write_all(response.as_bytes()).await;
            let _ = socket.shutdown().await;
        });
        format!("http://{addr}")
    }

    async fn get_playlist_envelope(base_url: &str, checked: bool) -> Result<serde_json::Value> {
        let client = Arc::new(reqwest::Client::new());
        let params = [("id", "pl1")];
        if checked {
            subsonic_get_envelope_checked(
                &client,
                base_url,
                "getPlaylist",
                "u=x",
                &params,
                "playlist",
            )
            .await
        } else {
            subsonic_get_envelope(&client, base_url, "getPlaylist", "u=x", &params, "playlist")
                .await
        }
    }

    /// Navidrome's answer to a rejected token: HTTP 200, failed envelope,
    /// code 40.
    pub(crate) const FAILED_ENVELOPE: &str = r#"{"subsonic-response":{"status":"failed","version":"1.16.1","error":{"code":40,"message":"Wrong username or password"}}}"#;

    /// Navidrome's answer to an unknown id: HTTP 200, failed envelope, code 70.
    pub(crate) const NOT_FOUND_ENVELOPE: &str = r#"{"subsonic-response":{"status":"failed","version":"1.16.1","error":{"code":70,"message":"data not found"}}}"#;

    /// A failed envelope comes back as HTTP 200. The lenient reader hands it
    /// on as a payload (a playlist would read as empty); the checked one
    /// makes it an error that carries the server's message.
    #[tokio::test]
    async fn checked_envelope_turns_a_failed_envelope_into_an_error() {
        let lenient =
            get_playlist_envelope(&one_shot_server("200 OK", NOT_FOUND_ENVELOPE).await, false)
                .await
                .expect("the lenient reader never looks at the status");
        assert_eq!(lenient["status"], "failed");

        let err = get_playlist_envelope(&one_shot_server("200 OK", NOT_FOUND_ENVELOPE).await, true)
            .await
            .expect_err("a failed envelope must be an error");
        assert!(
            format!("{err:#}").contains("data not found"),
            "got: {err:#}"
        );
    }

    /// A rejected token (code 40 inside HTTP 200) routes to the
    /// session-expired path through the checked reader, like a 401.
    #[tokio::test]
    async fn checked_envelope_routes_a_rejected_token_to_unauthorized() {
        let err = get_playlist_envelope(&one_shot_server("200 OK", FAILED_ENVELOPE).await, true)
            .await
            .expect_err("a rejected token must be an error");
        assert!(
            matches!(
                err.downcast_ref::<NokkviError>(),
                Some(NokkviError::Unauthorized)
            ),
            "got: {err:#}"
        );
    }

    /// A 401 (a reverse proxy in front of Navidrome) routes to the
    /// session-expired path through the checked reader.
    #[tokio::test]
    async fn checked_envelope_routes_401_to_unauthorized() {
        let err = get_playlist_envelope(&one_shot_server("401 Unauthorized", "").await, true)
            .await
            .expect_err("401 must be an error");
        assert!(
            matches!(
                err.downcast_ref::<NokkviError>(),
                Some(NokkviError::Unauthorized)
            ),
            "got: {err:#}"
        );
    }

    #[tokio::test]
    async fn checked_envelope_parses_an_ok_body() {
        let ok =
            r#"{"subsonic-response":{"status":"ok","version":"1.16.1","playlist":{"id":"pl1"}}}"#;
        let payload = get_playlist_envelope(&one_shot_server("200 OK", ok).await, true)
            .await
            .expect("an ok envelope parses");
        assert_eq!(payload["playlist"]["id"], "pl1");
    }
}
