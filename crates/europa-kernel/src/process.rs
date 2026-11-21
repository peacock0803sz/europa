//! Child process supervision for one kernel.
//!
//! Each kernel is started on its own watcher thread with
//! `std::process::Command`, so Linux `PR_SET_PDEATHSIG` only fires when
//! Europa itself dies. Standard error is kept in a capped buffer for failure
//! reports, and stopping signals the whole process group.

use std::ffi::OsStr;
use std::io;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use europa_core::ProcessExit;

/// Upper bound of bytes kept from standard error.
const STDERR_LIMIT: usize = 65536;

/// Capped tail of a child standard error stream.
#[derive(Debug)]
pub(crate) struct StderrTail {
    /// Last bytes pushed, oldest first, at most [`STDERR_LIMIT`] long.
    buf: Vec<u8>,
}

impl StderrTail {
    /// Make an empty buffer.
    pub(crate) fn new() -> Self {
        Self { buf: Vec::new() }
    }

    /// Append bytes, dropping from the front past the limit.
    pub(crate) fn push(&mut self, bytes: &[u8]) {
        self.buf.extend_from_slice(bytes);
        if self.buf.len() > STDERR_LIMIT {
            let drop = self.buf.len() - STDERR_LIMIT;
            self.buf.drain(..drop);
        }
    }

    /// Read the tail as text.
    ///
    /// Undecodable bytes become `U+FFFD`, and a cut inside a character
    /// starts from the next boundary.
    pub(crate) fn tail(&self) -> String {
        let mut start = 0;
        while start < self.buf.len() && (self.buf[start] & 0xC0) == 0x80 {
            start += 1;
        }
        String::from_utf8_lossy(&self.buf[start..]).into_owned()
    }
}

/// A watched child process.
#[derive(Debug)]
pub(crate) struct Watched {
    /// Process id of the child.
    pub(crate) pid: u32,
    /// Fires once with how the child ended.
    pub(crate) exited: tokio::sync::watch::Receiver<Option<ProcessExit>>,
    /// Shared standard error tail.
    pub(crate) stderr: Arc<Mutex<StderrTail>>,
}

impl Watched {
    /// Start `<python> -m ipykernel_launcher -f <connection_file>`.
    pub(crate) fn spawn(python: &Path, connection_file: &Path) -> io::Result<Watched> {
        spawn_command(
            python,
            &[
                OsStr::new("-m"),
                OsStr::new("ipykernel_launcher"),
                OsStr::new("-f"),
                connection_file.as_os_str(),
            ],
        )
    }

    /// Send `SIGKILL` to the process group, unless already exited.
    ///
    /// The check and the signal are not atomic: the child may end between
    /// them while a grandchild keeps the group id alive, but a recycled
    /// group id colliding in that window is unlikely, so this is accepted.
    pub(crate) fn kill_group(&self) {
        if self.exited.borrow().is_none() {
            signal_group(self.pid, libc::SIGKILL);
        }
    }

    /// Wait up to `timeout` for the child to end.
    pub(crate) async fn wait_exit(&mut self, timeout: Duration) -> Option<ProcessExit> {
        let current = *self.exited.borrow();
        if current.is_some() {
            return current;
        }
        let _ = tokio::time::timeout(timeout, self.exited.wait_for(|exit| exit.is_some())).await;
        *self.exited.borrow()
    }
}

/// Send a signal to a process group, ignoring missing groups.
pub(crate) fn signal_group(pid: u32, signal: i32) {
    // SAFETY: `killpg` only sends a signal and touches no memory. A
    // positive pid names a process group, and `ESRCH` for a missing group
    // is ignored by the caller contract.
    let sent = unsafe { libc::killpg(pid as i32, signal) };
    if sent != 0 {
        debug_assert_eq!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::ESRCH)
        );
    }
}

/// Start a command on a watcher thread, for tests and kernels.
///
/// Spawning and `wait` run on the same watcher thread, which lives until
/// the child ends, so a Linux death signal fires only when Europa itself
/// dies and never when a runtime thread goes idle.
///
/// Only the SIGKILL tests set `EUROPA_KERNEL_TEST_NO_JPY_PARENT_PID` to
/// skip the parent pid hand-off; it is never set in production.
pub(crate) fn spawn_command(program: &Path, args: &[&OsStr]) -> io::Result<Watched> {
    use std::os::unix::process::CommandExt;
    use std::process::Stdio;

    let (exit_tx, exit_rx) = tokio::sync::watch::channel(None);
    let stderr = Arc::new(Mutex::new(StderrTail::new()));
    let (pid_tx, pid_rx) = std::sync::mpsc::channel();
    let program = program.to_path_buf();
    let args: Vec<std::ffi::OsString> = args.iter().map(|arg| arg.to_os_string()).collect();
    let stderr_reader = Arc::clone(&stderr);
    let parent_pid = std::process::id();
    std::thread::spawn(move || {
        let mut command = std::process::Command::new(&program);
        command.args(&args);
        command.stdin(Stdio::null());
        command.stdout(Stdio::null());
        command.stderr(Stdio::piped());
        command.process_group(0);
        // Only the SIGKILL tests set this variable, to prove Linux
        // `PR_SET_PDEATHSIG` alone; it is never set in production.
        if std::env::var_os("EUROPA_KERNEL_TEST_NO_JPY_PARENT_PID").is_none() {
            command.env("JPY_PARENT_PID", parent_pid.to_string());
        }
        #[cfg(target_os = "linux")]
        // SAFETY: runs after `fork` in the child. It only calls the
        // async-signal-safe `prctl` and `getppid`, allocates nothing,
        // and `_exit`s without running destructors when the parent is
        // already gone, closing the race between setting the death
        // signal and checking the parent.
        unsafe {
            command.pre_exec(move || {
                if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                if libc::getppid() as u32 != parent_pid {
                    libc::_exit(1)
                }
                Ok(())
            });
        }
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(error) => {
                let _ = pid_tx.send(Err(error));
                return;
            }
        };
        let _ = pid_tx.send(Ok(child.id()));
        let piped = child.stderr.take();
        let reader = std::thread::spawn(move || {
            use std::io::Read;
            if let Some(mut piped) = piped {
                let mut chunk = [0_u8; 8192];
                loop {
                    match piped.read(&mut chunk) {
                        Ok(0) => break,
                        Ok(read) => stderr_reader
                            .lock()
                            .unwrap_or_else(|poison| poison.into_inner())
                            .push(&chunk[..read]),
                        Err(_) => break,
                    }
                }
            }
        });
        let status = child.wait();
        let _ = reader.join();
        let exit = status.map(|status| {
            use std::os::unix::process::ExitStatusExt;
            match (status.code(), status.signal()) {
                (Some(code), _) => ProcessExit::Code(code),
                (None, Some(signal)) => ProcessExit::Signal(signal),
                (None, None) => ProcessExit::Unknown,
            }
        });
        let _ = exit_tx.send_replace(exit.ok());
    });
    pid_rx
        .recv()
        .map_err(|_| io::Error::other("watcher thread died"))?
        .map(|pid| Watched {
            pid,
            exited: exit_rx,
            stderr,
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_short_output_verbatim() {
        let mut tail = StderrTail::new();
        tail.push(b"hello\n");
        assert_eq!(tail.tail(), "hello\n");
    }

    #[test]
    fn keeps_only_the_last_window() {
        let mut tail = StderrTail::new();
        tail.push(&vec![b'x'; 70000]);
        let text = tail.tail();
        assert_eq!(text.len(), STDERR_LIMIT);
        assert!(text.bytes().all(|byte| byte == b'x'));
    }

    #[test]
    fn starts_after_a_split_character() {
        let mut tail = StderrTail::new();
        tail.push("é".as_bytes());
        tail.push(&vec![b'a'; 65535]);
        assert_eq!(tail.tail(), "a".repeat(65535));
    }

    #[test]
    fn replaces_undecodable_bytes() {
        let mut tail = StderrTail::new();
        tail.push(b"\xff\xfehello");
        let text = tail.tail();
        assert!(text.contains('\u{FFFD}'));
        assert!(text.contains("hello"));
    }

    #[tokio::test]
    async fn reports_the_exit_code() {
        let mut watched = spawn_command(
            Path::new("/bin/sh"),
            &[OsStr::new("-c"), OsStr::new("echo err >&2; exit 3")],
        )
        .expect("spawns");
        assert_eq!(
            watched.wait_exit(Duration::from_secs(5)).await,
            Some(ProcessExit::Code(3))
        );
        assert_eq!(watched.stderr.lock().expect("unpoisoned").tail(), "err\n");
    }

    #[tokio::test]
    async fn reports_a_fatal_signal() {
        let mut watched = spawn_command(
            Path::new("/bin/sh"),
            &[OsStr::new("-c"), OsStr::new("kill -9 $$")],
        )
        .expect("spawns");
        assert_eq!(
            watched.wait_exit(Duration::from_secs(5)).await,
            Some(ProcessExit::Signal(9))
        );
    }

    #[tokio::test]
    async fn kill_group_stops_a_sleeper() {
        let mut watched = spawn_command(
            Path::new("/bin/sh"),
            &[OsStr::new("-c"), OsStr::new("sleep 30")],
        )
        .expect("spawns");
        watched.kill_group();
        assert_eq!(
            watched.wait_exit(Duration::from_secs(5)).await,
            Some(ProcessExit::Signal(9))
        );
    }

    #[test]
    fn reports_a_missing_program() {
        let error = spawn_command(Path::new("/nonexistent-europa-python"), &[])
            .expect_err("missing program fails");
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
    }

    #[tokio::test]
    async fn passes_the_parent_pid() {
        let mut watched = spawn_command(
            Path::new("/bin/sh"),
            &[OsStr::new("-c"), OsStr::new("echo \"$JPY_PARENT_PID\" >&2")],
        )
        .expect("spawns");
        watched.wait_exit(Duration::from_secs(5)).await;
        assert_eq!(
            watched.stderr.lock().expect("unpoisoned").tail(),
            format!("{}\n", std::process::id())
        );
    }
}
