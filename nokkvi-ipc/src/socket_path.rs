//! Resolution of per-instance IPC socket paths.
//!
//! # Strategy: PID-suffixed paths (rmpc pattern)
//!
//! Each running nokkvi daemon binds a uniquely-named socket
//! (`nokkvi-{pid}.sock`) rather than a single fixed `nokkvi.sock`. This
//! eliminates the orphan-socket bug class entirely: a second invocation
//! never unlinks the live socket of a different PID, so no amount of CLI /
//! daemon binary drift can leave a healthy daemon stranded.
//!
//! Trade-off vs. the single-fixed-path design we used through Phase 2:
//! external tools and scripts (`socat UNIX-CONNECT:…`, `nc -U …`) need to
//! enumerate `{dir}/nokkvi-*.sock` and probe instead of hardcoding one path.
//! [`find_live_socket`] does that for our own CLI; downstream consumers can
//! re-use the same `read_dir` + `connect`-probe pattern.
//!
//! # Directory resolution (XDG-aware, UID-namespaced fallback)
//!
//! 1. `$XDG_RUNTIME_DIR` if set and non-empty (mode `0700`, per-login).
//! 2. `/tmp` otherwise. The filename suffix on this branch carries the UID
//!    so two users on the same box don't see each other's sockets.
//!
//! See [`socket_path`] / [`socket_dir`] / [`find_live_socket`].

use std::{
    env, fs,
    os::unix::net::UnixStream,
    path::{Path, PathBuf},
    time::Duration,
};

/// Filename prefix shared by every nokkvi socket in [`socket_dir`].
const SOCKET_PREFIX: &str = "nokkvi-";
const SOCKET_SUFFIX: &str = ".sock";

/// Returns the directory in which nokkvi sockets live for this login session.
///
/// `$XDG_RUNTIME_DIR` when set, otherwise `/tmp`. The fallback branch is
/// only hit on environments that lack a proper user-runtime dir (rare on
/// modern Linux — systemd creates one per login at boot).
#[must_use]
pub fn socket_dir() -> PathBuf {
    if let Ok(dir) = env::var("XDG_RUNTIME_DIR")
        && !dir.is_empty()
    {
        return PathBuf::from(dir);
    }
    PathBuf::from("/tmp")
}

/// Per-PID socket path. The daemon binds this on startup; the CLI never
/// constructs one directly (it uses [`find_live_socket`] instead).
///
/// In `$XDG_RUNTIME_DIR` the filename is `nokkvi-{pid}.sock`. In the `/tmp`
/// fallback it's `nokkvi-{uid}-{pid}.sock` so two users sharing a box don't
/// collide on the same filename prefix.
#[must_use]
pub fn socket_path(pid: u32) -> PathBuf {
    let dir = socket_dir();
    let filename = if dir == Path::new("/tmp") {
        let uid = env::var("UID").unwrap_or_else(|_| "default".into());
        format!("{SOCKET_PREFIX}{uid}-{pid}{SOCKET_SUFFIX}")
    } else {
        format!("{SOCKET_PREFIX}{pid}{SOCKET_SUFFIX}")
    };
    dir.join(filename)
}

/// Enumerate every nokkvi-shaped socket in [`socket_dir`].
///
/// Returns paths that match `nokkvi-*.sock`. Liveness is not checked here —
/// callers that need it should layer [`is_alive`] on top, or use
/// [`find_live_socket`] which does both.
pub fn all_socket_paths() -> impl Iterator<Item = PathBuf> {
    let dir = socket_dir();
    let entries = fs::read_dir(&dir).ok().into_iter().flatten().flatten();
    entries.filter_map(|entry| {
        let path = entry.path();
        let name = path.file_name()?.to_string_lossy().into_owned();
        if name.starts_with(SOCKET_PREFIX) && name.ends_with(SOCKET_SUFFIX) {
            Some(path)
        } else {
            None
        }
    })
}

/// Quick liveness probe: returns `true` iff a process is accepting
/// connections on the given socket path right now. Dead sockets (kernel
/// inode gone, file remains as a corpse) and stale paths both fail with
/// `ECONNREFUSED` / `ENOENT` and report `false`.
///
/// Uses a 100ms read/write timeout on the resulting stream so a wedged peer
/// can't hang the CLI if it ever exchanges bytes. The connect itself is
/// blocking, but a local Unix-socket connect either succeeds in <1ms or
/// fails immediately — there's no slow-network path.
#[must_use]
pub fn is_alive(path: &Path) -> bool {
    let Ok(addr) = std::os::unix::net::SocketAddr::from_pathname(path) else {
        return false;
    };
    let Ok(stream) = UnixStream::connect_addr(&addr) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_millis(100)));
    let _ = stream.set_write_timeout(Some(Duration::from_millis(100)));
    true
}

/// Find the first live nokkvi daemon socket in [`socket_dir`].
///
/// Enumerates `nokkvi-*.sock` files and returns the first one that accepts
/// a connection. Dead corpse files (left over from `SIGKILL`'d daemons) are
/// skipped silently — caller doesn't need to know about them.
///
/// Returns `None` when no daemon is running. Callers use this for both the
/// CLI's "where do I send my request" lookup and the daemon's "is another
/// instance already alive" guard.
#[must_use]
pub fn find_live_socket() -> Option<PathBuf> {
    all_socket_paths().find(|path| is_alive(path))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_env<F: FnOnce()>(key: &str, value: Option<&str>, f: F) {
        let prev = env::var(key).ok();
        // SAFETY: env mutation is process-wide; cargo test runs these tests
        // serially within one binary by default. The risk is acceptable for a
        // path-resolution unit test.
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
    fn socket_path_uses_xdg_runtime_dir_when_set() {
        with_env("XDG_RUNTIME_DIR", Some("/run/user/4242"), || {
            assert_eq!(
                socket_path(12345),
                PathBuf::from("/run/user/4242/nokkvi-12345.sock"),
            );
        });
    }

    #[test]
    fn socket_path_falls_back_to_tmp_with_uid_and_pid() {
        with_env("XDG_RUNTIME_DIR", Some(""), || {
            with_env("UID", Some("1000"), || {
                assert_eq!(socket_path(99), PathBuf::from("/tmp/nokkvi-1000-99.sock"));
            });
        });
    }

    #[test]
    fn socket_path_tmp_fallback_handles_missing_uid_env() {
        with_env("XDG_RUNTIME_DIR", None, || {
            with_env("UID", None, || {
                assert_eq!(socket_path(7), PathBuf::from("/tmp/nokkvi-default-7.sock"));
            });
        });
    }

    #[test]
    fn socket_dir_prefers_xdg_then_tmp() {
        with_env("XDG_RUNTIME_DIR", Some("/run/user/4242"), || {
            assert_eq!(socket_dir(), PathBuf::from("/run/user/4242"));
        });
        with_env("XDG_RUNTIME_DIR", Some(""), || {
            assert_eq!(socket_dir(), PathBuf::from("/tmp"));
        });
        with_env("XDG_RUNTIME_DIR", None, || {
            assert_eq!(socket_dir(), PathBuf::from("/tmp"));
        });
    }

    #[test]
    fn is_alive_reports_false_for_nonexistent_path() {
        assert!(!is_alive(Path::new("/tmp/definitely-nonexistent.sock")));
    }

    #[test]
    fn is_alive_reports_false_for_stale_socket_file() {
        // A regular file isn't a socket — connect_addr should fail.
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("not-a-socket.sock");
        std::fs::write(&path, b"corpse").expect("write corpse");
        assert!(!is_alive(&path));
    }

    #[test]
    fn find_live_socket_returns_none_in_empty_dir() {
        // Point XDG at an empty tempdir so the enumerator finds nothing.
        let tmp = tempfile::tempdir().expect("tempdir");
        with_env(
            "XDG_RUNTIME_DIR",
            Some(tmp.path().to_str().unwrap()),
            || {
                assert!(find_live_socket().is_none());
            },
        );
    }

    #[test]
    fn all_socket_paths_filters_by_prefix_and_suffix() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let dir = tmp.path();
        // Plant a mix: matching, non-matching prefix, non-matching suffix.
        std::fs::write(dir.join("nokkvi-100.sock"), b"").unwrap();
        std::fs::write(dir.join("nokkvi-200.sock"), b"").unwrap();
        std::fs::write(dir.join("other-300.sock"), b"").unwrap();
        std::fs::write(dir.join("nokkvi-400.txt"), b"").unwrap();

        with_env("XDG_RUNTIME_DIR", Some(dir.to_str().unwrap()), || {
            let mut found: Vec<_> = all_socket_paths()
                .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
                .collect();
            found.sort();
            assert_eq!(found, vec!["nokkvi-100.sock", "nokkvi-200.sock"]);
        });
    }
}
