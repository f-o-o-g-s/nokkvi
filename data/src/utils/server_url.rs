//! Server-URL normalization and cleartext-HTTP detection for the login flow.
//!
//! Two pure helpers shared between the auth service (which probes connection
//! candidates) and the login view (which warns about cleartext credentials):
//!
//! - [`normalize_server_url_candidates`] turns whatever the user typed into an
//!   ordered list of full URLs to try. A trailing slash is stripped (so
//!   `http://host/` and `http://host` canonicalize) and a bare host with no
//!   scheme expands to `https://host` then `http://host`, so the user can type
//!   `navidrome.local:4533` and have it just work (HTTPS preferred, HTTP
//!   fallback). A URL that already carries a scheme is returned as the single
//!   canonical candidate.
//! - [`is_cleartext_http_url`] reports whether an EXPLICIT `http://` URL points
//!   at a non-local host, so the login view can warn that credentials would
//!   travel unencrypted. Bare-host input (no scheme) never warns — the
//!   candidate list prefers HTTPS, so cleartext is not decided at type time.

/// Trim surrounding whitespace and strip trailing slashes.
fn trimmed(raw: &str) -> &str {
    raw.trim().trim_end_matches('/')
}

/// True when `s` (case-insensitive) already begins with an http(s) scheme.
fn has_http_scheme(s: &str) -> bool {
    let lower = s.trim_start().to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://")
}

/// Ordered list of full URLs to attempt for the given user input.
///
/// - already-schemed input → one canonical candidate (trailing slash stripped)
/// - bare host → `["https://{host}", "http://{host}"]` (prefer TLS, fall back)
/// - empty/whitespace input → empty list (the caller treats this as a
///   validation error before any network attempt)
pub fn normalize_server_url_candidates(raw: &str) -> Vec<String> {
    let base = trimmed(raw);
    if base.is_empty() {
        return Vec::new();
    }
    if has_http_scheme(base) {
        vec![base.to_string()]
    } else {
        // Prefer HTTPS for a bare host. Fall back to plain HTTP ONLY for
        // local/LAN hosts, where cleartext is unremarkable — never silently
        // downgrade a REMOTE host to cleartext. Otherwise a TLS failure on the
        // https attempt (bad cert, blocked 443) would re-POST the password over
        // http to a public host with no warning. A user who genuinely runs a
        // remote server on plain HTTP can still opt in by typing `http://`
        // explicitly, which surfaces the cleartext advisory (informed consent).
        let mut candidates = vec![format!("https://{base}")];
        if host_is_local_or_lan(host_of(base)) {
            candidates.push(format!("http://{base}"));
        }
        candidates
    }
}

/// Whether `raw` is an explicit `http://` URL aimed at a non-local host, i.e.
/// credentials would cross the network unencrypted. Returns `false` for HTTPS,
/// for bare-host input (no scheme), and for loopback / `.local` / single-label
/// / private-LAN hosts where plain HTTP is the norm for self-hosters.
pub fn is_cleartext_http_url(raw: &str) -> bool {
    let lower = raw.trim().to_ascii_lowercase();
    let Some(after_scheme) = lower.strip_prefix("http://") else {
        return false;
    };
    let host = host_of(after_scheme);
    !host.is_empty() && !host_is_local_or_lan(host)
}

/// Extract the bare host from the authority part that follows `http://`
/// (drops any path/query/fragment, userinfo, and port; handles `[IPv6]`).
fn host_of(after_scheme: &str) -> &str {
    let authority = after_scheme
        .split(['/', '?', '#'])
        .next()
        .unwrap_or(after_scheme);
    let host_port = authority.rsplit('@').next().unwrap_or(authority);
    if let Some(rest) = host_port.strip_prefix('[') {
        // IPv6 literal: host runs up to the closing bracket.
        rest.split(']').next().unwrap_or(rest)
    } else {
        // host[:port] — host is everything before the first colon.
        host_port.split(':').next().unwrap_or(host_port)
    }
}

/// True for loopback, `.local`/`.localhost` mDNS, single-label hostnames, and
/// the private IPv4 ranges — the places where cleartext HTTP is unremarkable.
fn host_is_local_or_lan(host: &str) -> bool {
    let h = host.trim_end_matches('.'); // tolerate a fully-qualified trailing dot
    if h == "localhost" || h == "::1" {
        return true;
    }
    if h.ends_with(".local") || h.ends_with(".localhost") {
        return true;
    }
    // IPv6 literal: ULA (fc00::/7) and link-local (fe80::/10) are LAN ranges;
    // any other IPv6 (e.g. a global 2000::/3) is remote.
    if h.contains(':') {
        return h.starts_with("fc")
            || h.starts_with("fd")
            || h.starts_with("fe8")
            || h.starts_with("fe9")
            || h.starts_with("fea")
            || h.starts_with("feb");
    }
    // A single-label hostname (no dots) is almost always a LAN / mDNS name
    // rather than a public host.
    if !h.contains('.') {
        return true;
    }
    if let Some([a, b, _, _]) = parse_ipv4(h) {
        return a == 0                                  // 0.0.0.0/8 unspecified (non-routable)
            || a == 127                                // loopback
            || a == 10                                 // 10.0.0.0/8
            || (a == 192 && b == 168)                  // 192.168.0.0/16
            || (a == 172 && (16..=31).contains(&b))    // 172.16.0.0/12
            || (a == 169 && b == 254); // link-local
    }
    false
}

/// Parse a dotted-quad IPv4 string into its four octets, or `None`.
fn parse_ipv4(h: &str) -> Option<[u8; 4]> {
    let mut parts = h.split('.');
    let a = parts.next()?.parse().ok()?;
    let b = parts.next()?.parse().ok()?;
    let c = parts.next()?.parse().ok()?;
    let d = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some([a, b, c, d])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schemed_url_is_single_candidate() {
        assert_eq!(
            normalize_server_url_candidates("http://localhost:4533"),
            vec!["http://localhost:4533".to_string()]
        );
        assert_eq!(
            normalize_server_url_candidates("https://music.example.com"),
            vec!["https://music.example.com".to_string()]
        );
    }

    #[test]
    fn trailing_slashes_are_stripped() {
        assert_eq!(
            normalize_server_url_candidates("http://host/"),
            vec!["http://host".to_string()]
        );
        assert_eq!(
            normalize_server_url_candidates("https://host/sub/"),
            vec!["https://host/sub".to_string()]
        );
        assert_eq!(
            normalize_server_url_candidates("  http://host///  "),
            vec!["http://host".to_string()]
        );
    }

    #[test]
    fn bare_local_host_prefers_https_then_http() {
        // .local / LAN-IP bare hosts keep the cleartext HTTP fallback.
        assert_eq!(
            normalize_server_url_candidates("navidrome.local:4533"),
            vec![
                "https://navidrome.local:4533".to_string(),
                "http://navidrome.local:4533".to_string(),
            ]
        );
        assert_eq!(
            normalize_server_url_candidates("192.168.1.50:4533"),
            vec![
                "https://192.168.1.50:4533".to_string(),
                "http://192.168.1.50:4533".to_string(),
            ]
        );
        // Single-label LAN name also keeps the fallback.
        assert_eq!(
            normalize_server_url_candidates("mediaserver"),
            vec![
                "https://mediaserver".to_string(),
                "http://mediaserver".to_string(),
            ]
        );
    }

    #[test]
    fn bare_remote_host_is_https_only() {
        // A REMOTE bare host must NOT silently fall back to cleartext HTTP — a
        // TLS failure there would otherwise leak the password over http.
        assert_eq!(
            normalize_server_url_candidates("music.example.com:4533"),
            vec!["https://music.example.com:4533".to_string()]
        );
        assert_eq!(
            normalize_server_url_candidates("navidrome.mydomain.org"),
            vec!["https://navidrome.mydomain.org".to_string()]
        );
    }

    #[test]
    fn scheme_detection_is_case_insensitive() {
        assert_eq!(
            normalize_server_url_candidates("HTTP://Host/"),
            vec!["HTTP://Host".to_string()]
        );
    }

    #[test]
    fn empty_input_yields_no_candidates() {
        assert!(normalize_server_url_candidates("").is_empty());
        assert!(normalize_server_url_candidates("   ").is_empty());
        assert!(normalize_server_url_candidates("  ///  ").is_empty());
    }

    #[test]
    fn cleartext_warns_only_for_remote_http() {
        assert!(is_cleartext_http_url("http://example.com"));
        assert!(is_cleartext_http_url("http://example.com:4533/"));
        assert!(is_cleartext_http_url("HTTP://Example.com"));
        assert!(is_cleartext_http_url(
            "http://music.example.com:443/path?x=1"
        ));
        // 172.32 is outside the private 172.16/12 block.
        assert!(is_cleartext_http_url("http://172.32.0.1"));
        // A global IPv6 host is remote.
        assert!(is_cleartext_http_url("http://[2001:db8::1]:4533"));
    }

    #[test]
    fn cleartext_silent_for_https_and_bare_host() {
        assert!(!is_cleartext_http_url("https://example.com"));
        assert!(!is_cleartext_http_url("example.com")); // no scheme → not decided
        assert!(!is_cleartext_http_url(""));
    }

    #[test]
    fn cleartext_silent_for_local_and_lan() {
        for url in [
            "http://localhost:4533",
            "http://127.0.0.1:4533",
            "http://192.168.1.50:4533",
            "http://10.0.0.5",
            "http://172.16.0.1",
            "http://172.31.255.255",
            "http://169.254.1.1",
            "http://nas.local",
            "http://mediaserver", // single-label LAN name
            "http://[::1]:4533",
            "http://[fc00::1]:4533", // IPv6 ULA
            "http://[fe80::1]",      // IPv6 link-local
            "http://0.0.0.0:4533",   // unspecified / non-routable
        ] {
            assert!(
                !is_cleartext_http_url(url),
                "expected {url} to be treated as local/LAN (no cleartext warning)"
            );
        }
    }
}
