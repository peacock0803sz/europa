//! Process-wide registry of live kernels.
//!
//! `kill_all_kernels` walks this list from panic hooks and signal handlers,
//! so it never waits on a lock and only sends `SIGKILL`.

use std::path::PathBuf;

/// Pids and connection files of kernels started in this process.
pub(crate) struct Registry {
    /// Live children in spawn order.
    live: std::sync::Mutex<Vec<(u32, PathBuf)>>,
}

impl Registry {
    /// Make an empty registry.
    pub(crate) const fn new() -> Self {
        Self {
            live: std::sync::Mutex::new(Vec::new()),
        }
    }

    /// Remember a new child.
    pub(crate) fn register(&self, pid: u32, path: PathBuf) {
        if let Ok(mut live) = self.live.lock() {
            live.push((pid, path));
        }
    }

    /// Forget a reaped child.
    pub(crate) fn unregister(&self, pid: u32) {
        if let Ok(mut live) = self.live.lock() {
            live.retain(|(known, _)| *known != pid);
        }
    }

    /// `SIGKILL` every known group and remove every known file.
    ///
    /// Takes no lock when one is held elsewhere; best effort only.
    pub(crate) fn kill_all(&self) {
        let Ok(live) = self.live.try_lock() else {
            return;
        };
        for (pid, path) in live.iter() {
            crate::process::signal_group(*pid, libc::SIGKILL);
            let _ = std::fs::remove_file(path);
        }
    }
}

/// Kernels started in this process.
pub(crate) static GLOBAL: Registry = Registry::new();

/// `SIGKILL` every live kernel group and remove every live file.
///
/// Takes no lock when one is held, so panic hooks and signal handlers can
/// call it. It never waits for a grace period.
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
        let _held = registry.live.lock().expect("locks");
        registry.kill_all();
    }
}
