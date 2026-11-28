//! One probe: does an interpreter import `ipykernel`?

use std::path::Path;
use std::time::Duration;

use europa_core::ProbeOutcome;

use crate::registry::Registry;

/// Start `python` once and report whether it imports `ipykernel`.
///
/// The child is listed in `registry` while it runs, and it is stopped when it
/// outlives `timeout` or when the future is dropped.
pub(crate) async fn probe(python: &Path, timeout: Duration, registry: &Registry) -> ProbeOutcome {
    let _ = (python, timeout, registry);
    todo!()
}

/// [`probe`] with a hook between the start of the child and its registration.
///
/// Tests use the hook to run an emergency stop while the child is not listed
/// yet.
pub(crate) async fn probe_with(
    python: &Path,
    timeout: Duration,
    registry: &Registry,
    before_register: impl FnOnce(),
) -> ProbeOutcome {
    let _ = (python, timeout, registry, before_register);
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;
    use europa_core::ProcessExit;
    use std::future::Future;
    use std::path::PathBuf;
    use std::time::Instant;
    use tempfile::TempDir;

    /// A probe timeout that no test waits out.
    const LONG: Duration = Duration::from_secs(30);

    /// A temporary directory holding an executable fake Python.
    fn fake(body: &str) -> (TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = write(&dir, "python", format!("#!/bin/sh\n{body}\n").as_bytes());
        (dir, path)
    }

    /// An executable file in `dir`.
    fn write(dir: &TempDir, name: &str, content: &[u8]) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;

        let path = dir.path().join(name);
        std::fs::write(&path, content).expect("writes");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        path
    }

    /// A sleeper that records its pid in `pid_file`.
    fn sleeper(pid_file: &Path) -> (TempDir, PathBuf) {
        fake(&format!(
            "echo $$ > '{}'; exec sleep 30",
            pid_file.display()
        ))
    }

    /// Probe again once when another test's `fork` still held the new script
    /// open for writing.
    async fn unbusy<F>(probe: impl Fn() -> F) -> ProbeOutcome
    where
        F: Future<Output = ProbeOutcome>,
    {
        let outcome = probe().await;
        match &outcome {
            ProbeOutcome::NotExecutable { detail } if detail.contains("Text file busy") => {
                probe().await
            }
            _ => outcome,
        }
    }

    async fn run(python: &Path, timeout: Duration, registry: &Registry) -> ProbeOutcome {
        unbusy(|| probe(python, timeout, registry)).await
    }

    /// Wait for the pid that a sleeper writes.
    async fn recorded_pid(file: &Path) -> i32 {
        let start = Instant::now();
        loop {
            if let Ok(pid) = std::fs::read_to_string(file)
                .unwrap_or_default()
                .trim()
                .parse()
            {
                return pid;
            }
            assert!(start.elapsed() < Duration::from_secs(5), "no pid written");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    /// Wait up to two seconds for `pid` to vanish.
    async fn vanishes(pid: i32) -> bool {
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(2) {
            // SAFETY: signal 0 sends nothing and only probes.
            let probed = unsafe { libc::kill(pid, 0) };
            if probed != 0 && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH) {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        false
    }

    fn stopping() -> ProbeOutcome {
        ProbeOutcome::NotExecutable {
            detail: "Europa is stopping".to_owned(),
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn accepts_a_clean_exit() {
        static REGISTRY: Registry = Registry::new();
        let (_dir, python) = fake("exit 0");
        assert_eq!(run(&python, LONG, &REGISTRY).await, ProbeOutcome::Usable);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn keeps_the_last_five_lines_of_a_failure() {
        static REGISTRY: Registry = Registry::new();
        let (_dir, python) = fake("for i in 1 2 3 4 5 6 7; do echo line$i >&2; done; exit 1");
        assert_eq!(
            run(&python, LONG, &REGISTRY).await,
            ProbeOutcome::CannotImport {
                exit: ProcessExit::Code(1),
                stderr_tail: "line3\nline4\nline5\nline6\nline7".to_owned(),
            }
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn ignores_standard_output() {
        static REGISTRY: Registry = Registry::new();
        let (_dir, python) = fake("echo out; echo err >&2; exit 1");
        let outcome = run(&python, LONG, &REGISTRY).await;
        let ProbeOutcome::CannotImport { stderr_tail, .. } = outcome else {
            panic!("{outcome:?}");
        };
        assert_eq!(stderr_tail, "err");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn imports_from_the_root_directory_without_isolation() {
        static REGISTRY: Registry = Registry::new();
        let record = tempfile::NamedTempFile::new().expect("temp file");
        let (_dir, python) = fake(&format!(
            "printf '%s\\n' \"$PWD\" \"$@\" > '{}'",
            record.path().display()
        ));
        assert_eq!(run(&python, LONG, &REGISTRY).await, ProbeOutcome::Usable);
        assert_eq!(
            std::fs::read_to_string(record.path()).expect("reads"),
            "/\n-B\n-c\nimport sys; sys.path[:] = [p for p in sys.path if p != '']; import ipykernel\n"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn reports_why_an_interpreter_did_not_start() {
        static REGISTRY: Registry = Registry::new();
        let outcome = run(Path::new("/nonexistent-europa-python"), LONG, &REGISTRY).await;
        let ProbeOutcome::NotExecutable { detail } = outcome else {
            panic!("{outcome:?}");
        };
        assert!(!detail.is_empty());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn explains_a_file_that_is_no_program() {
        static REGISTRY: Registry = Registry::new();
        let dir = tempfile::tempdir().expect("temp dir");
        let python = write(&dir, "python", b"\x7fXYZ");
        // Linux hands such a file to `/bin/sh`, while macOS refuses to start
        // it.
        match run(&python, LONG, &REGISTRY).await {
            ProbeOutcome::CannotImport { stderr_tail, .. } if cfg!(target_os = "linux") => {
                assert!(!stderr_tail.is_empty());
            }
            ProbeOutcome::NotExecutable { detail } if cfg!(target_os = "macos") => {
                assert!(!detail.is_empty());
            }
            other => panic!("{other:?}"),
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn stops_a_probe_that_runs_too_long() {
        static REGISTRY: Registry = Registry::new();
        let record = tempfile::tempdir().expect("temp dir");
        let pid_file = record.path().join("pid");
        let (_dir, python) = sleeper(&pid_file);
        let limit = Duration::from_millis(500);
        let start = Instant::now();
        let outcome = run(&python, limit, &REGISTRY).await;
        assert!(start.elapsed() < Duration::from_millis(1500));
        assert_eq!(outcome, ProbeOutcome::TimedOut { after: limit });
        assert!(pid_file.exists(), "stopped before it wrote its pid");
        let pid = recorded_pid(&pid_file).await;
        assert!(vanishes(pid).await, "process {pid} survives");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn stops_the_child_when_the_probe_is_dropped() {
        static REGISTRY: Registry = Registry::new();
        let record = tempfile::tempdir().expect("temp dir");
        let pid_file = record.path().join("pid");
        let (_dir, python) = sleeper(&pid_file);
        let task = tokio::spawn(async move { run(&python, LONG, &REGISTRY).await });
        let pid = recorded_pid(&pid_file).await;
        task.abort();
        assert!(vanishes(pid).await, "process {pid} survives");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn starts_nothing_after_an_emergency_stop() {
        static REGISTRY: Registry = Registry::new();
        let record = tempfile::tempdir().expect("temp dir");
        let pid_file = record.path().join("pid");
        let (_dir, python) = sleeper(&pid_file);
        let task = tokio::spawn(async move { run(&python, LONG, &REGISTRY).await });
        let pid = recorded_pid(&pid_file).await;
        REGISTRY.kill_all();
        let outcome = task.await.expect("probe task");
        assert!(
            matches!(
                outcome,
                ProbeOutcome::CannotImport {
                    exit: ProcessExit::Signal(9),
                    ..
                }
            ),
            "{outcome:?}"
        );
        assert!(vanishes(pid).await, "process {pid} survives");

        let marker = record.path().join("started");
        let (_dir, python) = fake(&format!(": > '{}'; exit 0", marker.display()));
        assert_eq!(run(&python, LONG, &REGISTRY).await, stopping());
        assert!(!marker.exists(), "a probe started after the stop");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn stops_a_child_that_the_emergency_stop_missed() {
        static REGISTRY: Registry = Registry::new();
        let record = tempfile::tempdir().expect("temp dir");
        let pid_file = record.path().join("pid");
        let (_dir, python) = sleeper(&pid_file);
        let limit = Duration::from_secs(5);
        let start = Instant::now();
        let outcome =
            unbusy(|| probe_with(&python, limit, &REGISTRY, || REGISTRY.kill_all())).await;
        assert!(start.elapsed() < limit);
        assert_eq!(outcome, stopping());
        if pid_file.exists() {
            let pid = recorded_pid(&pid_file).await;
            assert!(vanishes(pid).await, "process {pid} survives");
        }
    }
}
