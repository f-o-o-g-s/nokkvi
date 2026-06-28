//! Layered credential sources for radio scrobbling.
//!
//! A user-supplied radio-scrobble credential can come from three places, in
//! descending priority:
//!
//! 1. **Environment variable** — `NOKKVI_RADIO_*` (scriptable, never on disk).
//! 2. **`config.toml`** — the `[radio_scrobble]` section (hand-editable, the
//!    "obvious" home; mirrors how Navidrome keeps `lastfm.apikey`/`secret`).
//! 3. **redb** — the GUI write target (kept out of the user's tracked dotfiles).
//!
//! [`resolve`] / [`resolve_pair`] apply that precedence over already-read
//! values so the choice logic stays pure and unit-testable. Only the
//! hand-configurable credentials participate (ListenBrainz token, Last.fm app
//! key + secret). The Last.fm **session key** and username are browser-auth
//! output, not something a user hand-types, so they stay redb-only and are not
//! resolved here.

use serde::Deserialize;

/// Env var overriding the ListenBrainz radio-scrobble token.
pub const ENV_LISTENBRAINZ_TOKEN: &str = "NOKKVI_RADIO_LISTENBRAINZ_TOKEN";
/// Env var overriding the Last.fm app API key.
pub const ENV_LASTFM_API_KEY: &str = "NOKKVI_RADIO_LASTFM_API_KEY";
/// Env var overriding the Last.fm app API secret.
pub const ENV_LASTFM_API_SECRET: &str = "NOKKVI_RADIO_LASTFM_API_SECRET";

/// The `[radio_scrobble]` section of `config.toml`. Every field is optional —
/// a user can set any subset (or none). Unknown sibling keys are ignored, so
/// this coexists with `[settings]`, `server_url`, etc. in the same file.
#[derive(Debug, Default, Deserialize, PartialEq, Eq)]
pub struct RadioScrobbleToml {
    #[serde(default)]
    pub listenbrainz_token: Option<String>,
    #[serde(default)]
    pub lastfm_api_key: Option<String>,
    #[serde(default)]
    pub lastfm_api_secret: Option<String>,
}

/// Just enough of `config.toml` to pull out our section without depending on
/// the full settings shape (and without `deny_unknown_fields`, so every other
/// table parses away to nothing here).
#[derive(Debug, Default, Deserialize)]
struct ConfigEnvelope {
    #[serde(default)]
    radio_scrobble: RadioScrobbleToml,
}

impl RadioScrobbleToml {
    /// Parse the `[radio_scrobble]` section out of a full `config.toml` string.
    /// A missing section or a parse error yields all-`None` (no creds from
    /// config) rather than failing — a malformed config must never break
    /// scrobbling, it just stops contributing credentials.
    pub fn parse(toml_str: &str) -> Self {
        toml::from_str::<ConfigEnvelope>(toml_str)
            .map(|c| c.radio_scrobble)
            .unwrap_or_default()
    }

    /// Read `[radio_scrobble]` from the user's `config.toml` on disk. Returns
    /// all-`None` when the file is absent or unreadable.
    pub fn load() -> Self {
        let Ok(path) = crate::utils::paths::get_config_path() else {
            return Self::default();
        };
        match std::fs::read_to_string(&path) {
            Ok(content) => Self::parse(&content),
            Err(_) => Self::default(),
        }
    }
}

/// Which layer supplied a resolved credential. Drives the settings badges so a
/// user can see *where* a credential comes from — and understand why a GUI
/// "disconnect" that only clears redb may not take effect while a higher layer
/// still resolves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CredSource {
    /// No layer supplies it.
    #[default]
    Unset,
    /// Entered in the GUI (redb — the write target).
    Redb,
    /// `[radio_scrobble]` in config.toml.
    Config,
    /// A `NOKKVI_RADIO_*` environment variable.
    Env,
}

impl CredSource {
    /// Short badge label for a *set* credential, or `None` when unset.
    pub fn badge(self) -> Option<&'static str> {
        match self {
            CredSource::Unset => None,
            CredSource::Redb => Some("Saved"),
            CredSource::Config => Some("Set in config.toml"),
            CredSource::Env => Some("Set via env var"),
        }
    }

    /// True when a layer *above* redb (env or config.toml) supplies the value —
    /// i.e. a GUI write to redb would be shadowed.
    pub fn shadows_redb(self) -> bool {
        matches!(self, CredSource::Env | CredSource::Config)
    }
}

/// All radio-scrobble credentials resolved together from a **single**
/// config.toml read (avoids the per-getter, per-field disk re-reads the layered
/// source otherwise incurs on every now-playing heartbeat and settings render).
#[derive(Debug, Clone, Default)]
pub struct RadioCreds {
    pub listenbrainz_token: Option<String>,
    pub listenbrainz_source: CredSource,
    /// `(api_key, api_secret)` resolved as an atomic pair.
    pub lastfm: Option<(String, String)>,
    pub lastfm_source: CredSource,
    /// Last.fm session key + username are browser-auth output → redb-only.
    pub lastfm_session: Option<String>,
    pub lastfm_username: Option<String>,
}

/// Trim a candidate and reject it if blank — a blank layer must not shadow a
/// populated lower-priority one.
fn non_blank(value: Option<String>) -> Option<String> {
    value
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

/// Like [`resolve`], but also reports which layer won (for the settings badges).
pub fn resolve_with_source(
    env_value: Option<String>,
    toml_value: Option<&str>,
    redb_value: Option<String>,
) -> (Option<String>, CredSource) {
    if let Some(v) = non_blank(env_value) {
        return (Some(v), CredSource::Env);
    }
    if let Some(v) = non_blank(toml_value.map(str::to_string)) {
        return (Some(v), CredSource::Config);
    }
    if let Some(v) = non_blank(redb_value) {
        return (Some(v), CredSource::Redb);
    }
    (None, CredSource::Unset)
}

/// Like [`resolve_pair`], but also reports which layer supplied the pair.
pub fn resolve_pair_with_source(
    env_key: Option<String>,
    env_secret: Option<String>,
    toml_key: Option<&str>,
    toml_secret: Option<&str>,
    redb_key: Option<String>,
    redb_secret: Option<String>,
) -> (Option<(String, String)>, CredSource) {
    let layers: [(Option<String>, Option<String>, CredSource); 3] = [
        (non_blank(env_key), non_blank(env_secret), CredSource::Env),
        (
            non_blank(toml_key.map(str::to_string)),
            non_blank(toml_secret.map(str::to_string)),
            CredSource::Config,
        ),
        (
            non_blank(redb_key),
            non_blank(redb_secret),
            CredSource::Redb,
        ),
    ];
    for (k, s, src) in layers {
        if let (Some(k), Some(s)) = (k, s) {
            return (Some((k, s)), src);
        }
    }
    (None, CredSource::Unset)
}

/// Resolve a single credential by precedence: env > config.toml > redb. Each
/// layer is trimmed and blank-filtered before it can win.
pub fn resolve(
    env_value: Option<String>,
    toml_value: Option<&str>,
    redb_value: Option<String>,
) -> Option<String> {
    non_blank(env_value)
        .or_else(|| non_blank(toml_value.map(str::to_string)))
        .or_else(|| non_blank(redb_value))
}

/// Resolve the Last.fm `(api_key, api_secret)` pair **atomically** from the
/// highest-priority layer that supplies *both*. Resolving the pair together
/// (rather than each field independently) prevents a stale redb secret from
/// pairing with a fresh env key — the two must belong to the same registered
/// Last.fm app or every request's signature is wrong.
pub fn resolve_pair(
    env_key: Option<String>,
    env_secret: Option<String>,
    toml_key: Option<&str>,
    toml_secret: Option<&str>,
    redb_key: Option<String>,
    redb_secret: Option<String>,
) -> Option<(String, String)> {
    let layers: [(Option<String>, Option<String>); 3] = [
        (non_blank(env_key), non_blank(env_secret)),
        (
            non_blank(toml_key.map(str::to_string)),
            non_blank(toml_secret.map(str::to_string)),
        ),
        (non_blank(redb_key), non_blank(redb_secret)),
    ];
    layers.into_iter().find_map(|(k, s)| Some((k?, s?)))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── RadioScrobbleToml::parse ────────────────────────────────────────────

    #[test]
    fn parse_full_section() {
        let toml = r#"
            server_url = "https://nav.example"
            [settings]
            volume = 80
            [radio_scrobble]
            listenbrainz_token = "lb-tok"
            lastfm_api_key = "key"
            lastfm_api_secret = "sec"
        "#;
        assert_eq!(
            RadioScrobbleToml::parse(toml),
            RadioScrobbleToml {
                listenbrainz_token: Some("lb-tok".into()),
                lastfm_api_key: Some("key".into()),
                lastfm_api_secret: Some("sec".into()),
            }
        );
    }

    #[test]
    fn parse_missing_section_is_all_none() {
        let toml = "server_url = \"x\"\n[settings]\nvolume = 50\n";
        assert_eq!(RadioScrobbleToml::parse(toml), RadioScrobbleToml::default());
    }

    #[test]
    fn parse_partial_section() {
        let toml = "[radio_scrobble]\nlistenbrainz_token = \"only-lb\"\n";
        let got = RadioScrobbleToml::parse(toml);
        assert_eq!(got.listenbrainz_token.as_deref(), Some("only-lb"));
        assert!(got.lastfm_api_key.is_none());
        assert!(got.lastfm_api_secret.is_none());
    }

    #[test]
    fn parse_malformed_toml_is_all_none() {
        // Unterminated string — a hard parse error must degrade to no creds,
        // not panic or propagate.
        assert_eq!(
            RadioScrobbleToml::parse("[radio_scrobble]\nlistenbrainz_token = \"oops"),
            RadioScrobbleToml::default()
        );
    }

    // ── resolve (single value) ──────────────────────────────────────────────

    #[test]
    fn resolve_env_wins() {
        assert_eq!(
            resolve(Some("env".into()), Some("cfg"), Some("redb".into())),
            Some("env".into())
        );
    }

    #[test]
    fn resolve_config_over_redb() {
        assert_eq!(
            resolve(None, Some("cfg"), Some("redb".into())),
            Some("cfg".into())
        );
    }

    #[test]
    fn resolve_redb_fallback() {
        assert_eq!(
            resolve(None, None, Some("redb".into())),
            Some("redb".into())
        );
    }

    #[test]
    fn resolve_blank_layer_does_not_shadow() {
        // Blank env + blank config must fall through to redb, not win-as-empty.
        assert_eq!(
            resolve(Some("   ".into()), Some(""), Some("redb".into())),
            Some("redb".into())
        );
    }

    #[test]
    fn resolve_trims_winner() {
        assert_eq!(
            resolve(Some("  spaced  ".into()), None, None),
            Some("spaced".into())
        );
    }

    #[test]
    fn resolve_all_none() {
        assert_eq!(resolve(None, None, None), None);
    }

    // ── resolve_pair (atomic key+secret) ────────────────────────────────────

    #[test]
    fn resolve_pair_env_both() {
        assert_eq!(
            resolve_pair(
                Some("ek".into()),
                Some("es".into()),
                Some("ck"),
                Some("cs"),
                Some("rk".into()),
                Some("rs".into()),
            ),
            Some(("ek".into(), "es".into()))
        );
    }

    #[test]
    fn resolve_pair_falls_through_incomplete_env_layer() {
        // env has only the key, not the secret → the env layer is incomplete,
        // so the whole pair comes from config (never env-key + config-secret).
        assert_eq!(
            resolve_pair(
                Some("ek".into()),
                None,
                Some("ck"),
                Some("cs"),
                Some("rk".into()),
                Some("rs".into()),
            ),
            Some(("ck".into(), "cs".into()))
        );
    }

    #[test]
    fn resolve_pair_redb_fallback() {
        assert_eq!(
            resolve_pair(None, None, None, None, Some("rk".into()), Some("rs".into())),
            Some(("rk".into(), "rs".into()))
        );
    }

    #[test]
    fn resolve_pair_partial_everywhere_is_none() {
        // No single layer has both halves → unusable.
        assert_eq!(
            resolve_pair(
                Some("ek".into()),
                None,
                None,
                Some("cs"),
                Some("rk".into()),
                None,
            ),
            None
        );
    }
}
