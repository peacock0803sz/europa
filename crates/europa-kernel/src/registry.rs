//! Process-wide registry of live kernels and discovery probes.
//!
//! `kill_all_kernels` walks this list for panic hooks and signal handling,
//! so it never waits on a lock and never waits for a child to exit.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, TryLockError};

/// Pids and connection files of children started in this process.
pub(crate) struct Registry {
    /// Live children in spawn order, with the connection file of those that
    /// have one.
    live: Mutex<Vec<(u32, Option<PathBuf>)>>,
    /// Set once by an emergency stop and never cleared.
    stopped: AtomicBool,
}

impl Registry {
    /// Make an empty registry.
    pub(crate) const fn new() -> Self {
        Self {
            live: Mutex::new(Vec::new()),
            stopped: AtomicBool::new(false),
        }
    }

    /// Remember a new child.
    pub(crate) fn register(&self, pid: u32, path: Option<PathBuf>) {
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

    /// Whether an emergency stop happened.
    pub(crate) fn is_stopped(&self) -> bool {
        self.stopped.load(Ordering::SeqCst)
    }

    /// Mark the registry stopped, then `SIGKILL` every known group and remove
    /// every known file.
    ///
    /// Only the mark is set when the lock is held elsewhere; the rest is best
    /// effort.
    pub(crate) fn kill_all(&self) {
        // The mark comes before the list is read, and a probe reads it after
        // registering, so a child cannot slip between the two unnoticed.
        self.stopped.store(true, Ordering::SeqCst);
        let live = match self.live.try_lock() {
            Ok(live) => live,
            Err(TryLockError::Poisoned(poison)) => poison.into_inner(),
            Err(TryLockError::WouldBlock) => return,
        };
        for (pid, path) in live.iter() {
            crate::process::signal_group(*pid, libc::SIGKILL);
            if let Some(path) = path {
                let _ = std::fs::remove_file(path);
            }
        }
    }
}

/// Kernels and probes started in this process.
pub(crate) static GLOBAL: Registry = Registry::new();

/// Best-effort `SIGKILL` of every live kernel and discovery probe process
/// group, then removal of each connection file.
///
/// Meant for panic hooks and signal handling. After it, discovery starts no
/// new probe for the rest of the process. It never blocks: when the registry
/// lock is held, even by the interrupted thread itself, it returns without
/// killing anything, though probes still stop starting. Besides setting an
/// atomic flag, it only calls `try_lock`, `kill`, and `unlink`, and it does
/// not wait for the children to exit. A child that has just ended may stay
/// listed until its owner forgets it, so in that short window a signal can
/// reach its freed group id. A long connection file path makes the removal
/// allocate, so a raw signal handler should prefer a flag that a normal
/// thread acts on.
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
                None,
            )
            .expect("spawns");
            registry.register(child.pid, Some(path));
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

    #[tokio::test(flavor = "multi_thread")]
    async fn kills_a_child_without_a_file() {
        let registry = Registry::new();
        let mut child = spawn_command(
            Path::new("/bin/sh"),
            &[OsStr::new("-c"), OsStr::new("sleep 30")],
            None,
        )
        .expect("spawns");
        registry.register(child.pid, None);
        registry.kill_all();
        assert_eq!(
            child.wait_exit(Duration::from_secs(5)).await,
            Some(ProcessExit::Signal(9))
        );
    }

    #[test]
    fn remembers_an_emergency_stop() {
        let registry = Registry::new();
        assert!(!registry.is_stopped());
        registry.kill_all();
        assert!(registry.is_stopped());
    }

    #[test]
    fn skips_when_locked() {
        let registry = Registry::new();
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("k.json");
        std::fs::write(&path, "{}").expect("writes");
        // No process has this group id, so a signal would find nothing.
        registry.register(u32::MAX >> 1, Some(path.clone()));
        let held = registry.live.lock().expect("locks");
        registry.kill_all();
        drop(held);
        assert!(path.exists(), "kill_all ran despite the held lock");
        assert!(registry.is_stopped(), "the stop needs no lock");
    }
}
