//! Redirect policies for the HTTP clients that carry the Navidrome credential.
//!
//! reqwest follows up to ten redirects by default. On a 307 or 308 it sends
//! the request body again (a `/rest` form with the credential, the login's
//! password, a new API key), and it keeps custom headers such as
//! `X-ND-Authorization`: only `Authorization` and cookies are dropped on a hop
//! to another host. So a redirect off the server would hand the credential to
//! whatever host it names, over plain http too.
//!
//! These policies keep the credential on the server: a hop may change the
//! path or the port, or move http to https, but it may not change the host or
//! go from https to http. A hop they refuse is not followed: the redirect
//! comes back to the caller as the answer, which every caller already treats
//! as a failed request.

use reqwest::redirect::{Action, Attempt, Policy};
use url::Url;

/// How many hops a chain may take (reqwest's own default).
const MAX_REDIRECTS: usize = 10;

/// Whether a hop from `from` to `to` keeps the request on the same server:
/// the same host (the URL parser lowercases host names) and no step down
/// from https to http.
pub fn stays_on_server(from: &Url, to: &Url) -> bool {
    from.host_str() == to.host_str() && !(from.scheme() == "https" && to.scheme() == "http")
}

/// Whether `url`'s query carries a Subsonic credential (`s`, `t`, `apiKey`,
/// `p` or `jwt`).
pub fn carries_credential(url: &Url) -> bool {
    url.query_pairs()
        .any(|(key, _)| crate::utils::url_redaction::is_secret_param(&key))
}

/// The policy for a client that only talks to the Navidrome server and sends
/// its credential in a body or a header (the API client, the login, the event
/// stream): follow a redirect only while it stays on the server.
pub fn server_only() -> Policy {
    Policy::custom(|attempt| {
        if attempt.previous().len() >= MAX_REDIRECTS {
            attempt.error("too many redirects")
        } else if hop_stays_on_server(&attempt) {
            attempt.follow()
        } else {
            refuse(attempt)
        }
    })
}

/// The policy for a client that fetches both the server, with the credential
/// in the URL, and other hosts without one (the audio decoder also plays
/// radio stations, which redirect freely): a request that set out with a
/// credential in its URL follows only redirects that stay on the server; any
/// other follows as usual.
pub fn guard_credential_urls() -> Policy {
    Policy::custom(|attempt| {
        let credentialed = attempt.previous().first().is_some_and(carries_credential);
        if attempt.previous().len() >= MAX_REDIRECTS {
            attempt.error("too many redirects")
        } else if !credentialed || hop_stays_on_server(&attempt) {
            attempt.follow()
        } else {
            refuse(attempt)
        }
    })
}

/// The policy for a client that fetches untrusted public hosts with no
/// credential (an external image, LRCLIB): every hop is re-checked with
/// `is_blocked`, the entry guard's own predicate, so a public host can't
/// `302` the fetch onto a private or loopback address the entry guard would
/// have refused. `label` names the client in the error.
pub fn public_hosts_only(label: &'static str, is_blocked: fn(&str) -> bool) -> Policy {
    Policy::custom(move |attempt| {
        if attempt.previous().len() >= MAX_REDIRECTS {
            attempt.error(format!("{label}: too many redirects"))
        } else if is_blocked(attempt.url().as_str()) {
            attempt.error(format!("{label}: redirect to a private/loopback host"))
        } else {
            attempt.follow()
        }
    })
}

/// Hand a refused redirect back to the caller as the answer. Logs the host it
/// pointed at, never the URL (the caller's URL can hold the credential).
fn refuse(attempt: Attempt<'_>) -> Action {
    tracing::warn!(
        "Not following a redirect that leaves the server (to {})",
        attempt.url().host_str().unwrap_or("an unknown host")
    );
    attempt.stop()
}

/// Whether `attempt`'s next hop stays on the server the chain started on,
/// judged against the first URL and the hop just made (so an http → https
/// upgrade can't step back down).
fn hop_stays_on_server(attempt: &Attempt<'_>) -> bool {
    let next = attempt.url();
    match (attempt.previous().first(), attempt.previous().last()) {
        (Some(first), Some(last)) => stays_on_server(first, next) && stays_on_server(last, next),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(s: &str) -> Url {
        Url::parse(s).expect("a URL")
    }

    #[test]
    fn a_hop_on_the_same_host_stays() {
        let from = url("http://music.example.com:4533/rest/stream?id=1");
        for to in [
            "http://music.example.com:4533/navidrome/rest/stream?id=1",
            "https://music.example.com/rest/stream?id=1",
            "http://MUSIC.example.com:8080/x",
        ] {
            assert!(stays_on_server(&from, &url(to)), "{to}");
        }
    }

    #[test]
    fn a_hop_to_another_host_or_down_to_http_leaves() {
        let from = url("https://music.example.com/rest/stream?id=1");
        for to in [
            "https://evil.example.net/collect",
            "https://music.example.com.evil.net/rest/stream",
            "http://music.example.com/rest/stream?id=1",
            "https://127.0.0.1/rest/stream",
        ] {
            assert!(!stays_on_server(&from, &url(to)), "{to}");
        }
        assert!(!stays_on_server(
            &url("http://127.0.0.1:4533/x"),
            &url("http://localhost:4533/x")
        ));
    }

    #[test]
    fn credential_urls_are_told_apart() {
        assert!(carries_credential(&url(
            "http://h/rest/stream?id=1&apiKey=nds_x"
        )));
        assert!(carries_credential(&url(
            "http://h/rest/stream?id=1&u=a&s=b&t=c"
        )));
        assert!(!carries_credential(&url(
            "http://h/rest/stream?id=1&u=alice"
        )));
        assert!(!carries_credential(&url("http://radio.example/live.mp3")));
    }
}
