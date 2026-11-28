//! Child process supervision for one kernel.
//!
//! Each kernel is started on its own watcher thread with
//! `std::process::Command`, so Linux `PR_SET_PDEATHSIG` only fires when
//! Europa itself dies. Standard error is kept in a capped buffer for failure
//! reports, and stopping signals the whole process group. When the kernel
//! ends, the watcher also kills what is left of its group.

use std::ffi::OsStr;
use std::io;
use std::path::Path;
use std::process::{ChildStderr, ExitStatus};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use europa_core::ProcessExit;

/// Upper bound of bytes kept from standard error.
const STDERR_LIMIT: usize = 65536;

/// Longest wait for standard error to close after the child ends.
///
/// A process outside the kernel group may keep the pipe open; the exit is
/// published anyway once this passes.
const STDERR_DRAIN: Duration = Duration::from_millis(500);

/// Test-only switch that withholds `JPY_PARENT_PID` from the kernel.
///
/// It is read in every build because the integration tests link the normal
/// library. When set, the kernel gets no `JPY_PARENT_PID`, not even an
/// inherited one, which removes the only `SIGKILL` protection on macOS.
const NO_JPY_PARENT_PID: &str = "EUROPA_KERNEL_TEST_NO_JPY_PARENT_PID";

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
            None,
        )
    }

    /// Send `SIGKILL` to the process group, unless its exit is already
    /// published.
    ///
    /// Between reaping the child and publishing its exit, the watcher kills
    /// the group and waits for standard error to close. A signal sent in that
    /// window reaches surviving members of the old group, which keep its id
    /// reserved. With none left the id is free, and its reuse by a new group
    /// within that short window is accepted.
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

impl Drop for Watched {
    /// Stop a child that still runs when its handle goes away.
    ///
    /// This covers a spawn future dropped during the handshake, a restart
    /// task that is aborted or panics, the owner's kill path after the
    /// kernel handle is dropped, and an owner task dropped with its runtime.
    /// Paths that wait for the published exit only forget the registry entry
    /// here.
    fn drop(&mut self) {
        self.kill_group();
        crate::registry::GLOBAL.unregister(self.pid);
    }
}

/// Send a signal to a process group, ignoring failures.
///
/// `ESRCH` means the group is gone, and macOS answers `EPERM` when only
/// zombies are left in it; neither leaves anything to stop. It calls only
/// the async-signal-safe `kill`, so panic hooks and signal handlers may use
/// it.
pub(crate) fn signal_group(pid: u32, signal: i32) {
    // Negated, 0 would name our own group and 1 every process we may signal.
    if pid <= 1 {
        return;
    }
    // SAFETY: `kill` only sends a signal and touches no memory. The negated
    // pid names the process group whose id is `pid`.
    unsafe { libc::kill(-(pid as i32), signal) };
}

/// Start a command on a watcher thread, for tests, probes, and kernels.
///
/// The command starts in `cwd`, or in the current directory without one.
///
/// Spawning and `wait` run on the same watcher thread, which lives until
/// the child ends, so a Linux death signal fires only when Europa itself
/// dies and never when a runtime thread goes idle.
///
/// When the child ends, the watcher reaps it and kills the rest of its
/// process group. Members that outlive the child keep the group id reserved
/// on Linux and macOS, so the signal, sent right after the reap, reaches
/// them rather than a recycled group. The watcher then waits a little for
/// standard error to close and publishes the exit. `waitid` with `WNOWAIT`
/// would keep the zombie and its id in place instead, but macOS reports a
/// stopped child to it as exited.
pub(crate) fn spawn_command(
    program: &Path,
    args: &[&OsStr],
    cwd: Option<&Path>,
) -> io::Result<Watched> {
    use std::os::unix::process::CommandExt;
    use std::process::Stdio;

    let (exit_tx, exit_rx) = tokio::sync::watch::channel(None);
    let stderr = Arc::new(Mutex::new(StderrTail::new()));
    let (pid_tx, pid_rx) = mpsc::channel();
    let program = program.to_path_buf();
    let cwd = cwd.map(Path::to_path_buf);
    let args: Vec<std::ffi::OsString> = args.iter().map(|arg| arg.to_os_string()).collect();
    let stderr_reader = Arc::clone(&stderr);
    let parent_pid = std::process::id();
    let watcher = move || {
        let mut command = std::process::Command::new(&program);
        command.args(&args);
        if let Some(cwd) = cwd {
            command.current_dir(cwd);
        }
        command.stdin(Stdio::null());
        command.stdout(Stdio::null());
        command.stderr(Stdio::piped());
        command.process_group(0);
        if std::env::var_os(NO_JPY_PARENT_PID).is_some() {
            command.env_remove("JPY_PARENT_PID");
        } else {
            command.env("JPY_PARENT_PID", parent_pid.to_string());
        }
        #[cfg(target_os = "linux")]
        // SAFETY: runs after `fork` in the child. It only calls the raw
        // system call wrappers `prctl`, `getppid`, and `_exit`, allocates
        // nothing, and `_exit`s without running destructors when the
        // parent is already gone, closing the race between setting the
        // death signal and checking the parent.
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
        let pid = child.id();
        let _ = pid_tx.send(Ok(pid));
        let drained = drain_stderr(child.stderr.take(), stderr_reader);
        // A failed `wait` (for example with `SIGCHLD` ignored) is still
        // published as `Unknown`, so the owner never mistakes the child for
        // a running one.
        let exit = child.wait().map_or(ProcessExit::Unknown, exit_of);
        signal_group(pid, libc::SIGKILL);
        let _ = drained.recv_timeout(STDERR_DRAIN);
        let _ = exit_tx.send_replace(Some(exit));
    };
    std::thread::Builder::new()
        .name("europa-kernel-watch".to_owned())
        .spawn(watcher)?;
    pid_rx
        .recv()
        .map_err(|_| io::Error::other("watcher thread died"))?
        .map(|pid| Watched {
            pid,
            exited: exit_rx,
            stderr,
        })
}

/// Copy standard error into `tail` on a thread of its own.
///
/// The returned receiver disconnects once the pipe closes. Without a reader
/// thread the pipe is dropped, and the kernel's writes to it fail.
fn drain_stderr(piped: Option<ChildStderr>, tail: Arc<Mutex<StderrTail>>) -> mpsc::Receiver<()> {
    use std::io::Read;

    let (done_tx, done_rx) = mpsc::channel::<()>();
    let reader = move || {
        let _done = done_tx;
        let Some(mut piped) = piped else {
            return;
        };
        let mut chunk = [0_u8; 8192];
        loop {
            match piped.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(read) => tail
                    .lock()
                    .unwrap_or_else(|poison| poison.into_inner())
                    .push(&chunk[..read]),
            }
        }
    };
    let _ = std::thread::Builder::new()
        .name("europa-kernel-stderr".to_owned())
        .spawn(reader);
    done_rx
}

/// Describe how a reaped child ended.
fn exit_of(status: ExitStatus) -> ProcessExit {
    use std::os::unix::process::ExitStatusExt;

    match (status.code(), status.signal()) {
        (Some(code), _) => ProcessExit::Code(code),
        (None, Some(signal)) => ProcessExit::Signal(signal),
        (None, None) => ProcessExit::Unknown,
    }
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
            None,
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
            None,
        )
        .expect("spawns");
        assert_eq!(
            watched.wait_exit(Duration::from_secs(5)).await,
            Some(ProcessExit::Signal(9))
        );
    }

    /// Wait up to five seconds for `pid` to vanish.
    fn vanishes(pid: i32) -> bool {
        let start = std::time::Instant::now();
        while start.elapsed() < Duration::from_secs(5) {
            // SAFETY: signal 0 sends nothing and only probes.
            if unsafe { libc::kill(pid, 0) } != 0 {
                return true;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        false
    }

    /// Read the pid a script printed on standard error.
    async fn printed_pid(watched: &Watched) -> i32 {
        let start = std::time::Instant::now();
        loop {
            let text = watched.stderr.lock().expect("unpoisoned").tail();
            if let Some(line) = text.lines().next() {
                return line.trim().parse().expect("pid");
            }
            assert!(start.elapsed() < Duration::from_secs(5), "no pid printed");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    #[tokio::test]
    async fn kill_group_stops_a_sleeper() {
        let mut watched = spawn_command(
            Path::new("/bin/sh"),
            &[OsStr::new("-c"), OsStr::new("sleep 30 & echo $! >&2; wait")],
            None,
        )
        .expect("spawns");
        let grandchild = printed_pid(&watched).await;
        watched.kill_group();
        assert_eq!(
            watched.wait_exit(Duration::from_secs(5)).await,
            Some(ProcessExit::Signal(9))
        );
        assert!(vanishes(grandchild), "grandchild {grandchild} survives");
    }

    #[tokio::test]
    async fn ends_the_group_with_its_leader() {
        let mut watched = spawn_command(
            Path::new("/bin/sh"),
            &[
                OsStr::new("-c"),
                OsStr::new("sleep 30 & echo $! >&2; exit 0"),
            ],
            None,
        )
        .expect("spawns");
        // The background sleeper keeps standard error open until the group
        // kill ends it, so the exit arrives long before the sleep would.
        assert_eq!(
            watched.wait_exit(Duration::from_secs(5)).await,
            Some(ProcessExit::Code(0))
        );
        let grandchild = printed_pid(&watched).await;
        assert!(vanishes(grandchild), "grandchild {grandchild} survives");
    }

    #[tokio::test]
    async fn a_stopped_child_still_runs() {
        let mut watched = spawn_command(
            Path::new("/bin/sh"),
            &[OsStr::new("-c"), OsStr::new("sleep 30")],
            None,
        )
        .expect("spawns");
        // SAFETY: `kill` only sends the signal to that one process.
        unsafe { libc::kill(watched.pid as i32, libc::SIGSTOP) };
        assert_eq!(watched.wait_exit(Duration::from_millis(500)).await, None);
        // SAFETY: `kill` only sends the signal to that one process.
        unsafe { libc::kill(watched.pid as i32, libc::SIGCONT) };
        watched.kill_group();
        assert_eq!(
            watched.wait_exit(Duration::from_secs(5)).await,
            Some(ProcessExit::Signal(9))
        );
    }

    #[tokio::test]
    async fn dropping_the_handle_stops_the_child() {
        let watched = spawn_command(
            Path::new("/bin/sh"),
            &[OsStr::new("-c"), OsStr::new("sleep 30")],
            None,
        )
        .expect("spawns");
        let mut exited = watched.exited.clone();
        drop(watched);
        let settled = tokio::time::timeout(
            Duration::from_secs(5),
            exited.wait_for(|exit| exit.is_some()),
        )
        .await
        .expect("ends in time")
        .map(|exit| *exit)
        .expect("watch stays readable");
        assert_eq!(settled, Some(ProcessExit::Signal(9)));
    }

    #[test]
    fn reports_a_missing_program() {
        let error = spawn_command(Path::new("/nonexistent-europa-python"), &[], None)
            .expect_err("missing program fails");
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
    }

    #[tokio::test]
    async fn passes_the_parent_pid() {
        let mut watched = spawn_command(
            Path::new("/bin/sh"),
            &[OsStr::new("-c"), OsStr::new("echo \"$JPY_PARENT_PID\" >&2")],
            None,
        )
        .expect("spawns");
        watched.wait_exit(Duration::from_secs(5)).await;
        assert_eq!(
            watched.stderr.lock().expect("unpoisoned").tail(),
            format!("{}\n", std::process::id())
        );
    }

    #[tokio::test]
    async fn starts_in_the_given_directory() {
        let mut watched = spawn_command(
            Path::new("/bin/sh"),
            &[OsStr::new("-c"), OsStr::new("pwd >&2")],
            Some(Path::new("/")),
        )
        .expect("spawns");
        watched.wait_exit(Duration::from_secs(5)).await;
        assert_eq!(watched.stderr.lock().expect("unpoisoned").tail(), "/\n");
    }
}
