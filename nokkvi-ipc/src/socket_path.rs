//! Resolution of the default IPC socket path.
//!
//! Strategy (XDG-aware, UID-namespaced for the fallback so multi-user systems
//! don't collide):
//!
//! 1. If `$XDG_RUNTIME_DIR` is set and non-empty, use `$XDG_RUNTIME_DIR/nokkvi.sock`.
//!    This is the freedesktop-blessed location on every modern Linux distro
//!    (systemd creates it per-login, mode `0700`).
//! 2. Otherwise fall back to `/tmp/nokkvi-<uid>.sock`. The uid suffix prevents
//!    a second user on the box from colliding with our socket name on `/tmp`
//!    (which is world-writable).
//!
//! Both branches return a path that is namespaced per the running login;
//! single-instance enforcement (refuse second launch) is layered on top.

use std::{env, path::PathBuf};

/// Returns the default socket path for the running login session.
///
/// See module docs for the resolution order.
#[must_use]
pub fn default_socket_path() -> PathBuf {
    if let Ok(dir) = env::var("XDG_RUNTIME_DIR")
        && !dir.is_empty()
    {
        return PathBuf::from(dir).join("nokkvi.sock");
    }

    // SAFETY: `libc::getuid` is always safe on Unix. We avoid a hard `libc`
    // dependency in this crate by reading `$UID` (set by most shells) and
    // falling back to a fixed name only if that's missing.
    let uid_suffix = env::var("UID").unwrap_or_else(|_| "default".into());
    PathBuf::from(format!("/tmp/nokkvi-{uid_suffix}.sock"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_env<F: FnOnce()>(key: &str, value: Option<&str>, f: F) {
        let prev = env::var(key).ok();
        // SAFETY: tests in this module are gated by a process-wide env mutex
        // implicitly (cargo test serial within one binary for env-mutating
        // tests). The risk is acceptable for a path-resolution unit test.
        unsafe {
            match value {
                Some(v) => env::set_var(key, v),
                None => env::remove_var(key),
            }
        }
        f();
        unsafe {
            match prev {
                Some(v) => env::set_var(key, v),
                None => env::remove_var(key),
            }
        }
    }

    #[test]
    fn xdg_runtime_dir_wins_when_set() {
        with_env("XDG_RUNTIME_DIR", Some("/run/user/4242"), || {
            let path = default_socket_path();
            assert_eq!(path, PathBuf::from("/run/user/4242/nokkvi.sock"));
        });
    }

    #[test]
    fn empty_xdg_runtime_dir_falls_through_to_tmp() {
        with_env("XDG_RUNTIME_DIR", Some(""), || {
            with_env("UID", Some("1000"), || {
                let path = default_socket_path();
                assert_eq!(path, PathBuf::from("/tmp/nokkvi-1000.sock"));
            });
        });
    }

    #[test]
    fn missing_xdg_runtime_dir_uses_uid_suffixed_tmp_fallback() {
        with_env("XDG_RUNTIME_DIR", None, || {
            with_env("UID", Some("31337"), || {
                let path = default_socket_path();
                assert_eq!(path, PathBuf::from("/tmp/nokkvi-31337.sock"));
            });
        });
    }
}
