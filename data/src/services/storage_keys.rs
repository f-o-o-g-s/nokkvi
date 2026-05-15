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
    }
}
