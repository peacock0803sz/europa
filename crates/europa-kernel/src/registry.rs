//! Process-wide registry of live kernels.
//!
//! `kill_all_kernels` walks this list for panic hooks and signal handling,
//! so it never waits on a lock and never waits for a kernel to exit.

use std::path::PathBuf;
use std::sync::{Mutex, TryLockError};

/// Pids and connection files of kernels started in this process.
pub(crate) struct Registry {
    /// Live children in spawn order.
    live: Mutex<Vec<(u32, PathBuf)>>,
}

impl Registry {
    /// Make an empty registry.
    pub(crate) const fn new() -> Self {
        Self {
            live: Mutex::new(Vec::new()),
        }
    }

    /// Remember a new child.
    pub(crate) fn register(&self, pid: u32, path: PathBuf) {
        self.live
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .push((pid, path));
    }

    /// Forget a child that ended or is being killed.
    pub(crate) fn unregister(&self, pid: u32) {
        self.live
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .retain(|(known, _)| *known != pid);
    }

    /// `SIGKILL` every known group and remove every known file.
    ///
    /// Does nothing when the lock is held elsewhere; best effort only.
    pub(crate) fn kill_all(&self) {
        let live = match self.live.try_lock() {
            Ok(live) => live,
            Err(TryLockError::Poisoned(poison)) => poison.into_inner(),
            Err(TryLockError::WouldBlock) => return,
        };
        for (pid, path) in live.iter() {
            crate::process::signal_group(*pid, libc::SIGKILL);
            let _ = std::fs::remove_file(path);
        }
    }
}

/// Kernels started in this process.
pub(crate) static GLOBAL: Registry = Registry::new();

/// Best-effort `SIGKILL` of every live kernel process group, then removal of
/// each connection file.
///
/// Meant for panic hooks and signal handling. It never blocks: when the
/// registry lock is held, even by the interrupted thread itself, it returns
/// without killing anything. It only calls `try_lock`, `kill`, and
/// `unlink`, and it does not wait for the kernels to exit. A kernel that has
/// just ended may stay listed until its owner forgets it, so in that short
/// window a signal can reach its freed group id. A long connection
/// file path makes the removal allocate, so a raw signal handler should
/// prefer a flag that a normal thread acts on.
pub fn kill_all_kernels() {
    GLOBAL.kill_all();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::spawn_command;
    use europa_core::ProcessExit;
    use std::ffi::OsStr;
    use std::path::Path;
    use std::time::Duration;

    #[tokio::test(flavor = "multi_thread")]
    async fn kills_registered_groups() {
        let registry = Registry::new();
        let dir = tempfile::tempdir().expect("temp dir");
        let mut watched = Vec::new();
        for (index, name) in ["k1.json", "k2.json"].iter().enumerate() {
            let path = dir.path().join(name);
            std::fs::write(&path, "{}").expect("writes");
            let child = spawn_command(
                Path::new("/bin/sh"),
                &[OsStr::new("-c"), OsStr::new("sleep 30")],
            )
            .expect("spawns");
            registry.register(child.pid, path);
            watched.push((child, index));
        }
        registry.kill_all();
        for (mut child, _) in watched {
            assert_eq!(
                child.wait_exit(Duration::from_secs(5)).await,
                Some(ProcessExit::Signal(9))
            );
        }
        assert!(dir.path().read_dir().expect("lists").next().is_none());
    }

    #[test]
    fn skips_when_locked() {
        let registry = Registry::new();
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("k.json");
        std::fs::write(&path, "{}").expect("writes");
        // No process has this group id, so a signal would find nothing.
        registry.register(u32::MAX >> 1, path.clone());
        let held = registry.live.lock().expect("locks");
        registry.kill_all();
        drop(held);
        assert!(path.exists(), "kill_all ran despite the held lock");
    }
}
