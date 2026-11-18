//! Whether another process is still running, by PID.
//!
//! The boot-time sweeps (`services::mpris_art_writer`'s art cache, the rules
//! editor's orphan draft playlists) key leftovers on the PID of the instance
//! that made them and remove only those whose owner is gone. They once tested
//! `/proc/<pid>`, which doesn't exist on macOS, so every other running
//! instance read as dead there and lost its files and server-side drafts.
//! `kill(pid, 0)` sends no signal; it only runs the existence and permission
//! checks, and works on every unix.

/// `true` while a process with this PID exists, whoever owns it.
///
/// `EPERM` means the process exists but belongs to another user (launchd or
/// init as PID 1), so it counts as alive. PID 0 and PIDs past `i32::MAX`
/// name no single process (`kill(0, …)` targets the caller's process group),
/// so they read as dead.
pub(crate) fn pid_is_alive(pid: u32) -> bool {
    let Ok(pid) = libc::pid_t::try_from(pid) else {
        return false;
    };
    if pid <= 0 {
        return false;
    }
    // SAFETY: signal 0 delivers nothing; `kill` only checks that `pid` exists
    // and that we may signal it. No memory is passed or retained.
    let rc = unsafe { libc::kill(pid, 0) };
    rc == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

#[cfg(test)]
mod tests {
    use super::pid_is_alive;

    #[test]
    fn own_pid_and_pid_1_are_alive() {
        // PID 1 is init on Linux and launchd on macOS, owned by root, so it
        // also covers the EPERM branch.
        assert!(pid_is_alive(std::process::id()));
        assert!(pid_is_alive(1));
    }

    #[test]
    fn pids_naming_no_single_process_are_dead() {
        assert!(!pid_is_alive(0), "kill(0, 0) would probe our process group");
        assert!(!pid_is_alive(u32::MAX), "past i32::MAX is no valid pid_t");
    }

    #[test]
    fn an_exited_child_is_dead() {
        let mut child = std::process::Command::new("true")
            .spawn()
            .expect("spawn `true`");
        let pid = child.id();
        child.wait().expect("reap `true`");
        // Reaped, so the PID is free; reuse within this window is
        // vanishingly unlikely on both kernels' sequential allocators.
        assert!(!pid_is_alive(pid));
    }
}
