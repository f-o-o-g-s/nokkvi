//! Subsonic `scrobble` requests (now-playing and play submissions).

/// Build (without sending) the `GET /rest/scrobble` request for `song_id`:
/// `submission = false` is the now-playing heartbeat, `true` counts a play.
///
/// The request carries the client unique id like every other event-causing
/// `/rest` request (see `client::with_client_unique_id`, which also notes
/// how Navidrome keys now-playing on it). A confirmed submission makes Navidrome send a `refreshResource`
/// for the song, its album and its artist; with the id it skips nokkvi's own
/// stream and stays with this user's other sessions, so the UI refreshes its
/// lists itself (`handle_scrobble_submission_result`).
fn scrobble_request(
    http_client: &reqwest::Client,
    server_url: &str,
    subsonic_credential: &str,
    song_id: &str,
    submission: bool,
) -> reqwest::RequestBuilder {
    let standard = crate::services::api::subsonic::STANDARD_PARAMS;
    let url = format!(
        "{server_url}/rest/scrobble?id={song_id}&submission={submission}&{subsonic_credential}&{standard}"
    );
    crate::services::api::client::with_client_unique_id(http_client.get(url))
}

/// Why a scrobble request did not count.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ScrobbleError {
    /// The server rejected the credential: the session is dead.
    #[error("the server rejected the session")]
    SessionRejected,
    /// The server answered and refused this scrobble (a failed Subsonic
    /// envelope, such as an unknown song): sending it again gets the same
    /// answer.
    #[error("refused: {0}")]
    Refused(String),
    /// No usable answer (a transport error or a non-2xx status): worth
    /// another try.
    #[error("{0}")]
    Failed(String),
}

/// Send a [`scrobble_request`] and read its answer: the HTTP status, then the
/// Subsonic envelope, because Navidrome answers a dead credential with
/// HTTP 200 and a failed envelope (`check_subsonic_response_status` sorts
/// out which failures are a dead session). The request URL carries the
/// Subsonic credential (an API key or the token pair), and
/// `reqwest::Error`'s text includes the URL, so a transport error is reported
/// without it, and a non-2xx answer by its status alone: the UI logs these
/// errors, at warn level for a submission.
pub async fn send_scrobble(
    http_client: &reqwest::Client,
    server_url: &str,
    subsonic_credential: &str,
    song_id: &str,
    submission: bool,
) -> Result<(), ScrobbleError> {
    let response = scrobble_request(
        http_client,
        server_url,
        subsonic_credential,
        song_id,
        submission,
    )
    .send()
    .await
    .map_err(|e| ScrobbleError::Failed(e.without_url().to_string()))?;
    let status = response.status();
    if status == reqwest::StatusCode::UNAUTHORIZED {
        return Err(ScrobbleError::SessionRejected);
    }
    if !status.is_success() {
        return Err(ScrobbleError::Failed(format!("HTTP {status}")));
    }
    let body = response
        .text()
        .await
        .map_err(|e| ScrobbleError::Failed(e.without_url().to_string()))?;
    crate::services::api::subsonic::check_subsonic_response_status(status, &body, "scrobble")
        .map_err(|e| {
            if crate::types::error::NokkviError::is_unauthorized(&e) {
                ScrobbleError::SessionRejected
            } else {
                ScrobbleError::Refused(e.to_string())
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::api::client::{CLIENT_UNIQUE_ID_HEADER, client_unique_id};

    /// A transport failure (server down, network gone) must not carry the
    /// request URL into the error, which the UI logs: the URL holds the
    /// Subsonic credential, an API key or the token pair.
    #[tokio::test]
    async fn a_failed_scrobble_never_reports_its_credential() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        drop(listener);
        let client = reqwest::Client::new();
        for credential in ["apiKey=nds_HIDEME", "u=alice&s=SALTHIDE&t=TOKHIDE"] {
            for submission in [false, true] {
                let err = send_scrobble(
                    &client,
                    &format!("http://127.0.0.1:{port}"),
                    credential,
                    "s1",
                    submission,
                )
                .await
                .expect_err("a closed port fails");
                let err = err.to_string();
                assert!(!err.contains("HIDE"), "credential in the error: {err}");
            }
        }
    }

    async fn send_to(status_line: &'static str, body: &'static str) -> Result<(), ScrobbleError> {
        let url = crate::services::api::subsonic::tests::one_shot_server(status_line, body).await;
        send_scrobble(
            &reqwest::Client::new(),
            &url,
            "u=alice&t=tok&s=salt",
            "s1",
            true,
        )
        .await
    }

    /// Navidrome answers a dead credential on `/rest` with HTTP 200 and a
    /// failed envelope (code 40, or 44 for a revoked API key), never a 401.
    /// Read as a success, a rejected scrobble counted as a confirmed play.
    #[tokio::test]
    async fn a_rejected_credential_is_a_dead_session_not_a_success() {
        for body in [
            r#"{"subsonic-response":{"status":"failed","error":{"code":40,"message":"Wrong username or password"}}}"#,
            r#"{"subsonic-response":{"status":"failed","error":{"code":44,"message":"Invalid API key"}}}"#,
        ] {
            assert_eq!(
                send_to("200 OK", body).await,
                Err(ScrobbleError::SessionRejected),
                "{body}"
            );
        }
        assert_eq!(
            send_to("401 Unauthorized", "{}").await,
            Err(ScrobbleError::SessionRejected)
        );
    }

    /// Any other failed envelope is the server's answer for this scrobble:
    /// sending it again would get the same one.
    #[tokio::test]
    async fn a_failed_envelope_is_a_refusal() {
        let refused = send_to(
            "200 OK",
            r#"{"subsonic-response":{"status":"failed","error":{"code":70,"message":"Song not found"}}}"#,
        )
        .await;
        assert!(
            matches!(refused, Err(ScrobbleError::Refused(ref m)) if m.contains("Song not found")),
            "{refused:?}"
        );
    }

    #[tokio::test]
    async fn an_ok_envelope_counts_and_a_server_error_is_retryable() {
        assert_eq!(
            send_to("200 OK", r#"{"subsonic-response":{"status":"ok"}}"#).await,
            Ok(())
        );
        assert!(matches!(
            send_to("500 Internal Server Error", "boom").await,
            Err(ScrobbleError::Failed(_))
        ));
    }

    #[test]
    fn scrobble_request_carries_the_client_unique_id() {
        let client = reqwest::Client::new();
        for submission in [false, true] {
            let request = scrobble_request(
                &client,
                "http://localhost:4533",
                "u=alice&t=tok&s=salt",
                "s1",
                submission,
            )
            .build()
            .expect("request builds");
            assert_eq!(
                request.url().as_str(),
                format!(
                    "http://localhost:4533/rest/scrobble?id=s1&submission={submission}&u=alice&t=tok&s=salt&f=json&v=1.8.0&c=nokkvi"
                )
            );
            assert_eq!(
                request
                    .headers()
                    .get(CLIENT_UNIQUE_ID_HEADER)
                    .and_then(|v| v.to_str().ok()),
                Some(client_unique_id()),
                "submission={submission}"
            );
        }
    }
}
