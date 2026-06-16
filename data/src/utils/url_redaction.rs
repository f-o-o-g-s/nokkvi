//! Redact Subsonic auth tokens (`s=<salt>` / `t=<md5(password+salt)>`) from URLs
//! before they reach the log file.
//!
//! Users attach `~/.local/state/nokkvi/nokkvi.log` to bug reports per
//! `CONTRIBUTING.md`. The Subsonic stream URLs we log embed `&s=<salt>&t=<token>`,
//! which together are enough to impersonate the user against the Navidrome API
//! until they rotate their password. Anywhere a Subsonic URL crosses into a
//! `tracing` call site, wrap it with `redact_subsonic_url`.
//!
//! The helper preserves the path and every other query parameter (`id=`, `u=`,
//! `f=`, `v=`, `c=`, `_=`, etc.) so logs remain useful for debugging.

use std::borrow::Cow;

/// Strip the `s=` (salt) and `t=` (token) query parameters from a Subsonic URL.
///
/// Returns `Cow::Borrowed` when no redaction is needed (no query string, or
/// no `s=` / `t=` params present) and `Cow::Owned` when at least one of the
/// sensitive params was removed.
///
/// # Behavior
/// - Non-URL inputs (no `?`) are returned unchanged.
/// - Param ordering is preserved for the params that remain.
/// - If `s=` and `t=` are the only params, the trailing `?` is dropped so the
///   result is a clean base URL.
/// - Matching is exact on the key name (`s` and `t`), case-sensitive, so an
///   `id=...` containing the substring `s=` is not mistakenly stripped.
pub fn redact_subsonic_url(url: &str) -> Cow<'_, str> {
    let Some(query_start) = url.find('?') else {
        return Cow::Borrowed(url);
    };

    let (base, query_with_q) = url.split_at(query_start);
    // query_with_q starts with '?'; skip it for splitting.
    let query = &query_with_q[1..];

    let mut kept: Vec<&str> = Vec::new();
    let mut redacted_any = false;
    for pair in query.split('&') {
        let key = pair.split_once('=').map_or(pair, |(k, _)| k);
        if key == "s" || key == "t" {
            redacted_any = true;
        } else {
            kept.push(pair);
        }
    }

    if !redacted_any {
        return Cow::Borrowed(url);
    }

    if kept.is_empty() {
        Cow::Owned(base.to_string())
    } else {
        Cow::Owned(format!("{base}?{}", kept.join("&")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn happy_path_strips_salt_and_token() {
        let url = "http://nav.example.com/rest/stream?id=al-1&u=alice&t=deadbeef&s=cafe&f=json&v=1.8.0&c=nokkvi&_=123";
        let redacted = redact_subsonic_url(url);
        assert_eq!(
            redacted,
            "http://nav.example.com/rest/stream?id=al-1&u=alice&f=json&v=1.8.0&c=nokkvi&_=123"
        );
        // `t=deadbeef` and `s=cafe` are gone; every other param survives.
        assert!(!redacted.contains("t=deadbeef"));
        assert!(!redacted.contains("s=cafe"));
        assert!(redacted.contains("id=al-1"));
        assert!(redacted.contains("u=alice"));
        assert!(redacted.contains("f=json"));
        assert!(redacted.contains("v=1.8.0"));
        assert!(redacted.contains("c=nokkvi"));
        assert!(redacted.contains("_=123"));
    }

    #[test]
    fn no_query_string_unchanged() {
        let url = "http://nav.example.com/rest/stream";
        let redacted = redact_subsonic_url(url);
        assert_eq!(redacted, url);
        assert!(matches!(redacted, Cow::Borrowed(_)));
    }

    #[test]
    fn non_url_input_unchanged() {
        let input = "not a url at all";
        let redacted = redact_subsonic_url(input);
        assert_eq!(redacted, input);
        assert!(matches!(redacted, Cow::Borrowed(_)));
    }

    #[test]
    fn only_salt_and_token_strips_question_mark() {
        let url = "http://nav.example.com/rest/stream?t=deadbeef&s=cafe";
        let redacted = redact_subsonic_url(url);
        assert_eq!(redacted, "http://nav.example.com/rest/stream");
    }

    #[test]
    fn only_salt_strips_question_mark() {
        let url = "http://nav.example.com/rest/stream?s=cafe";
        let redacted = redact_subsonic_url(url);
        assert_eq!(redacted, "http://nav.example.com/rest/stream");
    }

    #[test]
    fn params_in_any_order() {
        // s= and t= scattered around other params, not adjacent.
        let url = "http://srv/rest/stream?s=cafe&id=al-1&t=deadbeef&u=alice";
        let redacted = redact_subsonic_url(url);
        assert_eq!(redacted, "http://srv/rest/stream?id=al-1&u=alice");

        // Reverse order.
        let url = "http://srv/rest/stream?u=alice&t=deadbeef&id=al-1&s=cafe";
        let redacted = redact_subsonic_url(url);
        assert_eq!(redacted, "http://srv/rest/stream?u=alice&id=al-1");
    }

    #[test]
    fn no_sensitive_params_borrowed() {
        let url = "http://srv/rest/stream?id=al-1&u=alice&f=json";
        let redacted = redact_subsonic_url(url);
        assert_eq!(redacted, url);
        assert!(matches!(redacted, Cow::Borrowed(_)));
    }

    #[test]
    fn empty_string_unchanged() {
        let redacted = redact_subsonic_url("");
        assert_eq!(redacted, "");
        assert!(matches!(redacted, Cow::Borrowed(_)));
    }

    #[test]
    fn longer_keys_containing_s_or_t_not_stripped() {
        // An `id=...` value or a key like `state=` must not be confused with `s=`.
        let url = "http://srv/rest/stream?state=ok&things=cool&id=t=fake";
        let redacted = redact_subsonic_url(url);
        // Nothing was redacted — input returned as-is.
        assert_eq!(redacted, url);
        assert!(matches!(redacted, Cow::Borrowed(_)));
    }

    #[test]
    fn empty_value_still_redacted() {
        let url = "http://srv/rest/stream?id=al-1&s=&t=";
        let redacted = redact_subsonic_url(url);
        assert_eq!(redacted, "http://srv/rest/stream?id=al-1");
    }

    /// The string helper above only protects URLs we log *directly*. The other
    /// half of the credential surface is `reqwest::Error`, whose `Display`/`Debug`
    /// fold the full failing request URL (including `s=`/`t=`) into their output —
    /// so logging such an error, or baking it into an `anyhow`/`io::Error` message,
    /// leaks the credential even though no URL literal appears at the call site.
    /// The fix everywhere is `reqwest::Error::without_url()`. This test pins that
    /// contract so a future bare `{e}`/`.context(e)` on a credentialed request is
    /// caught by the suite rather than in a user's pasted `nokkvi.log`.
    #[test]
    fn reqwest_error_without_url_drops_subsonic_credential() {
        // A connect-refused request to a closed local port yields a real
        // `reqwest::Error` that carries the request URL — exactly the shape the
        // artwork-fetch and stream-decode error paths produce on a network drop.
        // Bind :0 to get an OS-assigned port, then drop the listener so the port is
        // guaranteed closed for this test (avoids a hardcoded ephemeral-range port
        // colliding with a real listener under parallel/CI runs).
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
        let port = listener.local_addr().expect("local addr").port();
        drop(listener);
        let url = format!(
            "http://127.0.0.1:{port}/rest/getCoverArt\
             ?id=al-1&u=demo&s=SALT_SECRET&t=TOKEN_SECRET&f=json"
        );
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(2))
            .build()
            .expect("blocking client builds");
        let err = client
            .get(url)
            .send()
            .expect_err("connection to a closed port must fail");

        // Sanity: the error must actually carry the URL, or the test proves nothing.
        assert!(
            err.url().is_some(),
            "expected the transport error to carry the request URL"
        );
        let raw = format!("{err:?}");
        assert!(
            raw.contains("SALT_SECRET") && raw.contains("TOKEN_SECRET"),
            "precondition: the raw reqwest error should embed the credential, got: {raw}"
        );

        // `.without_url()` must strip the credential from both representations.
        let stripped = err.without_url();
        let debug = format!("{stripped:?}");
        let display = format!("{stripped}");
        for needle in ["SALT_SECRET", "TOKEN_SECRET"] {
            assert!(
                !debug.contains(needle),
                "credential leaked via Debug: {debug}"
            );
            assert!(
                !display.contains(needle),
                "credential leaked via Display: {display}"
            );
        }
    }
}
