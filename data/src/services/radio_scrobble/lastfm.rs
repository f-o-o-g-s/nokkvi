//! Last.fm scrobbling client for direct radio scrobbling.
//!
//! Last.fm requires an MD5 `api_sig` on every authenticated call: drop
//! `format`/`callback` and empty values, sort the params by key, concatenate
//! `key+value` (no separators), append the shared secret, then MD5-hex
//! (lowercase). Verified against Navidrome (`adapters/lastfm/client.go: sign`)
//! and the user's Pano-style reference.
//!
//! Auth is the 3-step desktop flow: [`get_token`](LastfmClient::get_token) →
//! user authorizes at [`authorize_url`](LastfmClient::authorize_url) in a
//! browser → [`get_session`](LastfmClient::get_session) yields a per-user
//! session key. Thereafter [`scrobble`](LastfmClient::scrobble) /
//! [`update_now_playing`](LastfmClient::update_now_playing) carry
//! `api_key` + `sk` + `api_sig`.
//!
//! Reference: <https://www.last.fm/api>

use std::sync::Arc;

use anyhow::{Context, Result, anyhow};
use md5::{Digest, Md5};
use serde::Deserialize;

use super::ScrobbleTrack;

/// Last.fm API 2.0 root.
const API_ROOT: &str = "https://ws.audioscrobbler.com/2.0/";

/// A configured Last.fm app (api key + secret) plus an optional per-user
/// session key. Cheap to clone.
#[derive(Clone)]
pub struct LastfmClient {
    http: Arc<reqwest::Client>,
    api_key: String,
    api_secret: String,
    session_key: Option<String>,
}

/// A linked Last.fm session (returned by `auth.getSession`).
#[derive(Debug, Clone, Deserialize)]
pub struct LastfmSession {
    /// Last.fm username.
    pub name: String,
    /// The indefinite per-user session key used to sign scrobbles.
    pub key: String,
}

impl LastfmClient {
    /// Construct from the app `api_key` + `api_secret` (no session yet).
    pub fn new(
        http: Arc<reqwest::Client>,
        api_key: impl Into<String>,
        api_secret: impl Into<String>,
    ) -> Self {
        Self {
            http,
            api_key: api_key.into(),
            api_secret: api_secret.into(),
            session_key: None,
        }
    }

    /// Attach a per-user session key (required for scrobble / now-playing).
    #[must_use]
    pub fn with_session_key(mut self, sk: impl Into<String>) -> Self {
        self.session_key = Some(sk.into());
        self
    }

    /// The browser URL where the user authorizes a request `token`.
    pub fn authorize_url(&self, token: &str) -> String {
        format!(
            "https://www.last.fm/api/auth/?api_key={}&token={}",
            self.api_key, token
        )
    }

    /// Desktop-auth step 1: fetch an unauthorized request token.
    pub async fn get_token(&self) -> Result<String> {
        let params = vec![
            ("method", "auth.getToken".to_string()),
            ("api_key", self.api_key.clone()),
        ];
        let resp: TokenResponse = self.signed_get(params).await?;
        Ok(resp.token)
    }

    /// Desktop-auth step 3: exchange an AUTHORIZED `token` for a session.
    pub async fn get_session(&self, token: &str) -> Result<LastfmSession> {
        let params = vec![
            ("method", "auth.getSession".to_string()),
            ("api_key", self.api_key.clone()),
            ("token", token.to_string()),
        ];
        let resp: SessionResponse = self.signed_get(params).await?;
        Ok(resp.session)
    }

    /// Submit a now-playing update (ephemeral; does not scrobble).
    pub async fn update_now_playing(&self, track: &ScrobbleTrack) -> Result<()> {
        self.post(self.track_params("track.updateNowPlaying", track, None)?)
            .await
            .map(|_| ())
    }

    /// Submit a scrobble. `timestamp` is unix seconds when the track started.
    ///
    /// Last.fm returns HTTP 200 with `{"scrobbles":{"@attr":{"accepted":0,
    /// "ignored":1}, ...}}` when it *rejects* a scrobble (ignore-list artist,
    /// stale timestamp, blocked metadata). Treat `accepted < 1` as an error so
    /// it isn't silently reported as a success.
    pub async fn scrobble(&self, track: &ScrobbleTrack, timestamp: i64) -> Result<()> {
        let body = self
            .post(self.track_params("track.scrobble", track, Some(timestamp))?)
            .await?;
        match scrobbles_accepted(&body) {
            Some(n) if n >= 1 => Ok(()),
            Some(_) => Err(anyhow!(
                "Last.fm ignored the scrobble: {}",
                ignored_message(&body).unwrap_or("no reason given")
            )),
            // No parseable accepted count: a 200 without the expected @attr shape
            // gives no proof the scrobble was stored. Treat it as a (retryable)
            // failure rather than silently reporting success and dropping the
            // listen — the retry is bounded and the timestamp dedups server-side.
            None => Err(anyhow!(
                "Last.fm returned an unrecognized scrobble response (no accepted count)"
            )),
        }
    }

    // --- internals -----------------------------------------------------------

    fn session_key(&self) -> Result<&str> {
        self.session_key
            .as_deref()
            .ok_or_else(|| anyhow!("Last.fm is not connected (no session key)"))
    }

    fn track_params(
        &self,
        method: &'static str,
        track: &ScrobbleTrack,
        timestamp: Option<i64>,
    ) -> Result<Vec<(&'static str, String)>> {
        let sk = self.session_key()?.to_string();
        let mut params = vec![
            ("method", method.to_string()),
            ("artist", track.artist.clone()),
            ("track", track.title.clone()),
            ("api_key", self.api_key.clone()),
            ("sk", sk),
        ];
        if let Some(ts) = timestamp {
            params.push(("timestamp", ts.to_string()));
        }
        if let Some(album) = &track.album {
            params.push(("album", album.clone()));
        }
        Ok(params)
    }

    /// Append `api_sig` (+ `format=json`) to a param set, ready for transmission.
    /// Values are signed RAW; reqwest URL-encodes them on the wire and Last.fm
    /// re-signs the decoded values, so the signature matches.
    fn finalize(&self, params: Vec<(&'static str, String)>) -> Vec<(String, String)> {
        let sig = api_sig(&params, &self.api_secret);
        let mut out: Vec<(String, String)> = params
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect();
        out.push(("api_sig".to_string(), sig));
        out.push(("format".to_string(), "json".to_string()));
        out
    }

    async fn signed_get<T: for<'de> Deserialize<'de>>(
        &self,
        params: Vec<(&'static str, String)>,
    ) -> Result<T> {
        let url = format!("{API_ROOT}?{}", encode_form(&self.finalize(params)));
        let resp = self
            .http
            .get(&url)
            .send()
            .await
            .context("Last.fm request failed")?;
        let body = resp.text().await.unwrap_or_default();
        parse_lastfm(&body)
    }

    /// POST a write call and return the parsed JSON body. Surfaces a top-level
    /// `{"error":N,...}` (which Last.fm returns even on HTTP 200) as an `Err`.
    async fn post(&self, params: Vec<(&'static str, String)>) -> Result<serde_json::Value> {
        let resp = self
            .http
            .post(API_ROOT)
            .header(
                reqwest::header::CONTENT_TYPE,
                "application/x-www-form-urlencoded",
            )
            .body(encode_form(&self.finalize(params)))
            .send()
            .await
            .context("Last.fm request failed")?;
        let body = resp.text().await.unwrap_or_default();
        parse_lastfm(&body)
    }
}

/// Extract `scrobbles.@attr.accepted` from a track.scrobble response. Last.fm
/// renders the count as a JSON number or a string depending on endpoint
/// version, so accept both.
fn scrobbles_accepted(body: &serde_json::Value) -> Option<i64> {
    let v = body.get("scrobbles")?.get("@attr")?.get("accepted")?;
    v.as_i64().or_else(|| v.as_str()?.parse().ok())
}

/// The human-readable reason Last.fm ignored a scrobble, if present
/// (`scrobbles.scrobble.ignoredMessage.#text`).
fn ignored_message(body: &serde_json::Value) -> Option<&str> {
    body.get("scrobbles")?
        .get("scrobble")?
        .get("ignoredMessage")?
        .get("#text")?
        .as_str()
        .filter(|s| !s.is_empty())
}

/// `application/x-www-form-urlencoded` encode the (already api_sig'd) params.
/// Values are encoded for transport only; the signature was computed over the
/// RAW values, which Last.fm reconstructs after decoding.
fn encode_form(params: &[(String, String)]) -> String {
    params
        .iter()
        .map(|(k, v)| {
            format!(
                "{}={}",
                url::form_urlencoded::byte_serialize(k.as_bytes()).collect::<String>(),
                url::form_urlencoded::byte_serialize(v.as_bytes()).collect::<String>()
            )
        })
        .collect::<Vec<_>>()
        .join("&")
}

#[derive(Deserialize)]
struct TokenResponse {
    token: String,
}

#[derive(Deserialize)]
struct SessionResponse {
    session: LastfmSession,
}

/// The error envelope Last.fm returns (with HTTP 200) on a failed call.
#[derive(Deserialize)]
struct LastfmErrorMaybe {
    error: Option<i32>,
    #[serde(default)]
    message: Option<String>,
}

/// Parse a Last.fm JSON body: surface `{"error":N,...}` as an `Err`, otherwise
/// deserialize the expected success shape.
fn parse_lastfm<T: for<'de> Deserialize<'de>>(body: &str) -> Result<T> {
    if let Ok(e) = serde_json::from_str::<LastfmErrorMaybe>(body)
        && let Some(code) = e.error
    {
        return Err(anyhow!(
            "Last.fm error {code}: {}",
            e.message.unwrap_or_default()
        ));
    }
    // Deliberately omit the raw body from the error: an auth.getSession body
    // carries the per-user session key, and this error surfaces in the file log
    // and an on-screen toast. The static context is enough to localize the fault
    // without leaking a credential.
    serde_json::from_str(body).context("Failed to parse Last.fm response")
}

/// Build the signing message: drop `format`/`callback` + empty values, sort by
/// key, concatenate `key+value`, append the shared secret. (The MD5 of this is
/// the `api_sig`.) Pulled out so it is unit-testable without hashing.
fn sign_message(params: &[(&'static str, String)], secret: &str) -> String {
    let mut filtered: Vec<&(&'static str, String)> = params
        .iter()
        .filter(|(k, v)| *k != "format" && *k != "callback" && !v.is_empty())
        .collect();
    filtered.sort_by(|a, b| a.0.cmp(b.0));
    let mut msg = String::new();
    for (k, v) in filtered {
        msg.push_str(k);
        msg.push_str(v);
    }
    msg.push_str(secret);
    msg
}

fn md5_hex(s: &str) -> String {
    let digest = Md5::digest(s.as_bytes());
    let mut out = String::with_capacity(32);
    for b in digest {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

fn api_sig(params: &[(&'static str, String)], secret: &str) -> String {
    md5_hex(&sign_message(params, secret))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn md5_hex_matches_known_vector() {
        // RFC 1321 test vector — proves the md-5 crate integration + hex enc.
        assert_eq!(md5_hex("abc"), "900150983cd24fb0d6963f7d28e17f72");
        assert_eq!(md5_hex(""), "d41d8cd98f00b204e9800998ecf8427e");
    }

    #[test]
    fn sign_message_sorts_and_drops_format_and_empty() {
        let params = vec![
            ("method", "auth.getToken".to_string()),
            ("api_key", "KEY".to_string()),
            ("format", "json".to_string()), // dropped
            ("callback", "x".to_string()),  // dropped
            ("empty", String::new()),       // dropped (blank)
        ];
        // Sorted by key (api_key, method), key+value concat, then secret.
        assert_eq!(
            sign_message(&params, "SECRET"),
            "api_keyKEYmethodauth.getTokenSECRET"
        );
    }

    #[test]
    fn api_sig_is_md5_of_sign_message() {
        let params = vec![
            ("method", "track.scrobble".to_string()),
            ("artist", "A".to_string()),
            ("track", "T".to_string()),
        ];
        let expected = md5_hex(&sign_message(&params, "s3cr3t"));
        assert_eq!(api_sig(&params, "s3cr3t"), expected);
        assert_eq!(expected.len(), 32);
    }

    #[test]
    fn authorize_url_includes_key_and_token() {
        let c = LastfmClient::new(Arc::new(reqwest::Client::new()), "APIKEY", "SECRET");
        assert_eq!(
            c.authorize_url("TOK"),
            "https://www.last.fm/api/auth/?api_key=APIKEY&token=TOK"
        );
    }

    #[test]
    fn scrobble_without_session_errors() {
        let c = LastfmClient::new(Arc::new(reqwest::Client::new()), "K", "S");
        let track = ScrobbleTrack {
            artist: "A".into(),
            title: "B".into(),
            album: None,
            station_name: None,
        };
        // track_params requires a session key.
        assert!(c.track_params("track.scrobble", &track, Some(1)).is_err());
    }

    #[test]
    fn parse_lastfm_surfaces_error_envelope() {
        let err =
            parse_lastfm::<serde_json::Value>(r#"{"error":9,"message":"Invalid session key"}"#)
                .expect_err("error envelope must become Err");
        assert!(format!("{err}").contains("Invalid session key"));
        // A success body parses fine.
        assert!(
            parse_lastfm::<serde_json::Value>(r#"{"scrobbles":{"@attr":{"accepted":1}}}"#).is_ok()
        );
    }

    #[test]
    fn scrobble_accepted_and_ignored_are_detected() {
        let ok = serde_json::json!({"scrobbles":{"@attr":{"accepted":1,"ignored":0}}});
        assert_eq!(scrobbles_accepted(&ok), Some(1));
        // Last.fm renders counts as strings on some endpoints + nests the reason.
        let ignored = serde_json::json!({"scrobbles":{
            "@attr":{"accepted":"0","ignored":"1"},
            "scrobble":{"ignoredMessage":{"code":"1","#text":"Artist was ignored"}}
        }});
        assert_eq!(scrobbles_accepted(&ignored), Some(0));
        assert_eq!(ignored_message(&ignored), Some("Artist was ignored"));
        // No @attr (e.g. a now-playing reply) → None, so callers stay lenient.
        assert_eq!(
            scrobbles_accepted(&serde_json::json!({"nowplaying":{}})),
            None
        );
    }

    /// Live signing check against the real Last.fm API: `auth.getToken` is
    /// signed but needs no session, so a returned token proves api_sig is
    /// correct. Non-destructive.
    ///
    /// Run with:
    ///   LASTFM_API_KEY=$(grep '^api_key:' ~/.config/yams/yams.yml | awk '{print $2}') \
    ///   LASTFM_API_SECRET=$(grep '^api_secret:' ~/.config/yams/yams.yml | awk '{print $2}') \
    ///   cargo test -p nokkvi-data lastfm_live -- --ignored --nocapture
    #[tokio::test]
    #[ignore = "needs LASTFM_API_KEY + LASTFM_API_SECRET env; hits the network"]
    async fn lastfm_live_get_token_proves_signing() {
        let (Ok(key), Ok(secret)) = (
            std::env::var("LASTFM_API_KEY"),
            std::env::var("LASTFM_API_SECRET"),
        ) else {
            eprintln!("LASTFM_API_KEY/SECRET unset — skipping live test");
            return;
        };
        let http = Arc::new(reqwest::Client::new());
        let client = LastfmClient::new(http, key, secret);
        let token = client
            .get_token()
            .await
            .expect("get_token (signed) must succeed");
        assert!(!token.is_empty(), "token must be non-empty");
        eprintln!(
            "got request token (signing OK): {}…",
            &token[..token.len().min(8)]
        );
        eprintln!("authorize URL: {}", client.authorize_url(&token));
    }

    /// Live end-to-end check of the SCROBBLE path (signing + session key + the
    /// POST write call), non-destructive: `track.updateNowPlaying` sets a
    /// transient now-playing indicator and never creates a permanent scrobble.
    ///
    /// Run with LASTFM_API_KEY + LASTFM_API_SECRET + LASTFM_SESSION_KEY set
    /// (the session key is line 2 of ~/.config/yams/.lastfm_session).
    #[tokio::test]
    #[ignore = "needs LASTFM_API_KEY/SECRET/SESSION_KEY env; hits the network"]
    async fn lastfm_live_now_playing_proves_session_path() {
        let (Ok(key), Ok(secret), Ok(sk)) = (
            std::env::var("LASTFM_API_KEY"),
            std::env::var("LASTFM_API_SECRET"),
            std::env::var("LASTFM_SESSION_KEY"),
        ) else {
            eprintln!("LASTFM_* env unset — skipping live test");
            return;
        };
        let http = Arc::new(reqwest::Client::new());
        let client = LastfmClient::new(http, key, secret).with_session_key(sk);
        let track = ScrobbleTrack {
            artist: "nokkvi".into(),
            title: "radio-scrobble signing probe".into(),
            album: None,
            station_name: Some("nokkvi test".into()),
        };
        client
            .update_now_playing(&track)
            .await
            .expect("updateNowPlaying (signed + session) must succeed");
        eprintln!("Last.fm now-playing accepted — full scrobble path works");
    }
}
