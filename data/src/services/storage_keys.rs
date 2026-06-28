//! Centralized redb storage-key constants.
//!
//! All keys used against `StateStorage` (the redb facade) live here so the
//! on-disk schema is enumerable in one place. Keys are referenced from
//! `credentials.rs`, `services/queue/`, and `services/settings.rs` via
//! re-exports or direct imports.
//!
//! # On-disk-compat invariant
//!
//! **The string values below must not change.** They are the literal keys
//! written into `app.redb` on every user's machine. Renaming a constant is
//! safe; mutating its string value silently orphans existing user data
//! (settings reset to defaults, queue lost, session forced to re-login).
//! The `storage_keys_string_values_are_pinned` test below enforces this.
//!
//! Add a new key by appending a `pub(crate) const`, then wiring it through
//! the call site; the test asserts the byte-identical value to prevent
//! drift.

pub(crate) const USER_SETTINGS: &str = "user_settings";
pub(crate) const JWT_TOKEN: &str = "jwt_token";
pub(crate) const SUBSONIC_CREDENTIAL: &str = "subsonic_credential";
pub(crate) const QUEUE_ORDER: &str = "queue_order";
pub(crate) const QUEUE_SONGS: &str = "queue_songs";
/// Bincode blob of remembered radio now-playing (ICY) artwork, keyed by
/// station id and namespaced by server URL. Written/read by
/// [`crate::services::radio_art_store::RadioArtStore`].
///
/// `_v2`: a short-lived earlier build wrote a pre-`_v2` `radio_art_index` blob.
/// `RadioArtStore::load_migrating` merges that forward into this key once (then
/// deletes it), so remembered art survives the rename. The rename existed to
/// escape a stale-ICY-on-switch bug that could persist a previous station's art
/// under a new station's id; with that bug fixed in code, migrating is safe and
/// any leftover wrong entry can be cleared per-station via "Refresh artwork".
pub(crate) const RADIO_ART_INDEX: &str = "radio_art_index_v2";
/// ListenBrainz user submission token for direct **radio** scrobbling. The
/// `radio_` prefix is deliberate: library scrobbling rides Navidrome's own
/// server-side keys, so these keys must read as radio-only and never be
/// mistaken for a library setting. This is the redb (GUI-entered) layer; the
/// effective value also honors a `[radio_scrobble]` config.toml entry and the
/// `NOKKVI_RADIO_*` env vars (see `radio_scrobble::source`). Empty = not set.
pub(crate) const LISTENBRAINZ_TOKEN: &str = "radio_listenbrainz_token";
/// Last.fm radio-scrobble credentials (redb layer — see [`LISTENBRAINZ_TOKEN`]
/// for the prefix rationale and the config.toml/env override layers). The app
/// key/secret are user-supplied (also config.toml/env-settable); the session
/// key + username come from the in-app browser auth flow and stay redb-only.
/// Empty = not set.
pub(crate) const LASTFM_API_KEY: &str = "radio_lastfm_api_key";
pub(crate) const LASTFM_API_SECRET: &str = "radio_lastfm_api_secret";
pub(crate) const LASTFM_SESSION_KEY: &str = "radio_lastfm_session_key";
pub(crate) const LASTFM_USERNAME: &str = "radio_lastfm_username";

#[cfg(test)]
mod tests {
    use super::*;

    /// Pins the redb key string values to prevent silent on-disk-compat
    /// breakage. A typo in any const value would orphan existing user data;
    /// this test catches it before merge.
    #[test]
    fn storage_keys_string_values_are_pinned() {
        assert_eq!(USER_SETTINGS, "user_settings");
        assert_eq!(JWT_TOKEN, "jwt_token");
        assert_eq!(SUBSONIC_CREDENTIAL, "subsonic_credential");
        assert_eq!(QUEUE_ORDER, "queue_order");
        assert_eq!(QUEUE_SONGS, "queue_songs");
        assert_eq!(RADIO_ART_INDEX, "radio_art_index_v2");
        assert_eq!(LISTENBRAINZ_TOKEN, "radio_listenbrainz_token");
        assert_eq!(LASTFM_API_KEY, "radio_lastfm_api_key");
        assert_eq!(LASTFM_API_SECRET, "radio_lastfm_api_secret");
        assert_eq!(LASTFM_SESSION_KEY, "radio_lastfm_session_key");
        assert_eq!(LASTFM_USERNAME, "radio_lastfm_username");
    }
}
