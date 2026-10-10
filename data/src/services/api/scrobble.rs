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
pub fn scrobble_request(
    http_client: &reqwest::Client,
    server_url: &str,
    subsonic_credential: &str,
    song_id: &str,
    submission: bool,
) -> reqwest::RequestBuilder {
    let url = format!(
        "{server_url}/rest/scrobble?id={song_id}&submission={submission}&{subsonic_credential}&f=json&v=1.8.0&c=nokkvi"
    );
    crate::services::api::client::with_client_unique_id(http_client.get(url))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::api::client::{CLIENT_UNIQUE_ID_HEADER, client_unique_id};

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
