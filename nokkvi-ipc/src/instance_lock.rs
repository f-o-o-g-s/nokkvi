//! The single-instance launch lock.
//!
//! The IPC socket is bound inside the app's iced subscription, seconds into
//! boot, so the socket probe alone cannot tell two near-simultaneous first
//! launches apart: both find no socket and both boot. Every launch that is
//! about to start the app therefore takes an exclusive `flock` on
//! [`lock_path`] BEFORE it probes, and the winner holds it for its whole
//! lifetime. A launch that cannot take it knows another instance is booting
//! or running and waits for that instance's socket instead of booting.
//!
//! - `flock` is released by the kernel when the holder exits or crashes, so
//!   there is no stale-lock cleanup. The file itself is never deleted:
//!   unlinking a lock file while another process waits on it is a race.
//! - The file lives in [`socket_dir`] (per user: `$XDG_RUNTIME_DIR`, or a
//!   UID-suffixed name in the `/tmp` fallback, like the socket).
//! - [`File`] opens with `O_CLOEXEC`, so a child the app spawns cannot
//!   inherit the lock and keep it after the app exits.
//! - The `nokkvi <verb>` client path never takes it: a verb run while the app
//!   boots must not be mistaken for a second launch.

use std::{
    env,
    fs::{File, OpenOptions, TryLockError},
    io,
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
};

use crate::socket_path::socket_dir;

/// The outcome of [`try_acquire`].
#[derive(Debug)]
pub enum InstanceLock {
    /// This process holds the lock. Keep the file open for the process's
    /// lifetime: dropping it releases the lock.
    Won(File),
    /// Another process holds it: an instance is booting or running.
    Lost,
    /// The lock file could not be opened or locked (no usable runtime dir,
    /// a filesystem without `flock`). The caller falls back to the socket
    /// probe alone.
    Unavailable(io::Error),
}

/// Where the lock lives: `nokkvi.lock` beside the sockets, or
/// `nokkvi-{uid}.lock` in the `/tmp` fallback.
#[must_use]
pub fn lock_path() -> PathBuf {
    let dir = socket_dir();
    if dir == Path::new("/tmp") {
        let uid = env::var("UID").unwrap_or_else(|_| "default".into());
        dir.join(format!("nokkvi-{uid}.lock"))
    } else {
        dir.join("nokkvi.lock")
    }
}

/// Try to take the launch lock at [`lock_path`] without blocking.
#[must_use]
pub fn try_acquire() -> InstanceLock {
    try_acquire_at(&lock_path())
}

/// [`try_acquire`] at an explicit path.
#[must_use]
pub fn try_acquire_at(path: &Path) -> InstanceLock {
    let file = match OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .mode(0o600)
        .open(path)
    {
        Ok(file) => file,
        Err(err) => return InstanceLock::Unavailable(err),
    };
    match file.try_lock() {
        Ok(()) => InstanceLock::Won(file),
        Err(TryLockError::WouldBlock) => InstanceLock::Lost,
        Err(TryLockError::Error(err)) => InstanceLock::Unavailable(err),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_launch_wins_and_a_second_loses_until_it_exits() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("nokkvi.lock");

        // flock conflicts between two open file descriptions even inside one
        // process, so a second open stands in for a second launch.
        let first = try_acquire_at(&path);
        assert!(matches!(first, InstanceLock::Won(_)), "{first:?}");
        let second = try_acquire_at(&path);
        assert!(matches!(second, InstanceLock::Lost), "{second:?}");

        drop(first);
        let third = try_acquire_at(&path);
        assert!(
            matches!(third, InstanceLock::Won(_)),
            "released when the holder goes away: {third:?}"
        );
    }

    #[test]
    fn an_unusable_directory_reports_unavailable() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("missing-dir").join("nokkvi.lock");
        assert!(matches!(
            try_acquire_at(&path),
            InstanceLock::Unavailable(_)
        ));
    }
}
