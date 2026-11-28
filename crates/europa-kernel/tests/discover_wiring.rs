//! Python discovery end to end, with `/bin/sh` scripts standing in for
//! interpreters, so that plain `cargo test` covers it without Python.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use europa_core::{
    Discovery, DiscoveryEnv, DiscoveryError, DiscoveryRequest, PythonSource, Rejection,
};
use europa_kernel::discover_python;
use tempfile::TempDir;

/// A canonical temporary repository and the notebook in `repo/nb`.
fn repository() -> (TempDir, PathBuf, PathBuf) {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = std::fs::canonicalize(dir.path()).expect("resolves");
    std::fs::create_dir_all(root.join("repo/.git")).expect("creates");
    std::fs::create_dir_all(root.join("repo/nb")).expect("creates");
    let notebook = root.join("repo/nb/a.ipynb");
    (dir, root, notebook)
}

fn set_mode(path: &Path, mode: u32) {
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).expect("chmod");
}

/// Write an executable `/bin/sh` script.
fn script(path: &Path, body: &str) {
    std::fs::write(path, format!("#!/bin/sh\n{body}\n")).expect("writes");
    set_mode(path, 0o755);
}

/// A virtual environment at `root` whose interpreter runs `body`.
fn venv(root: &Path, mode: u32, body: &str) -> PathBuf {
    std::fs::create_dir_all(root.join("bin")).expect("creates");
    set_mode(root, mode);
    let python = root.join("bin/python");
    script(&python, body);
    python
}

/// Script that records that it ran.
fn leave_marker(marker: &Path) -> String {
    format!(": > '{}'", marker.display())
}

fn request(notebook: &Path) -> DiscoveryRequest {
    DiscoveryRequest::new(notebook, None, DiscoveryEnv::default())
}

/// Whether a script could not start because another test's `fork` still held
/// it open for writing.
fn busy(result: &Result<Discovery, DiscoveryError>) -> bool {
    let reports = match result {
        Ok(found) => found.skipped.as_slice(),
        Err(DiscoveryError::NoneUsable { reports, .. }) => reports,
        Err(DiscoveryError::ConfigUnusable { report }) => std::slice::from_ref(report),
        Err(DiscoveryError::ConfigNotAbsolute { .. }) => &[],
    };
    reports.iter().any(|report| {
        matches!(&report.rejection, Rejection::NotExecutable { detail }
            if detail.contains("Text file busy"))
    })
}

/// Search, once more when a script was busy.
// It returns what `discover_python` returns, large error included.
#[allow(clippy::result_large_err)]
async fn discover(request: &DiscoveryRequest) -> Result<Discovery, DiscoveryError> {
    let result = discover_python(request).await;
    if busy(&result) {
        discover_python(request).await
    } else {
        result
    }
}

fn reports(result: Result<Discovery, DiscoveryError>) -> Vec<europa_core::CandidateReport> {
    match result {
        Err(DiscoveryError::NoneUsable { reports, .. }) => reports,
        other => panic!("not a failed search: {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn selects_fake_dot_venv() {
    let (_dir, root, notebook) = repository();
    let python = venv(&root.join("repo/nb/.venv"), 0o755, "exit 0");
    let found = discover(&request(&notebook)).await.expect("finds one");
    assert_eq!(found.source, PythonSource::DotVenv);
    assert_eq!(found.python, python);
    let first = &found.skipped[0];
    assert_eq!(
        (&first.source, &first.rejection),
        (&PythonSource::Config, &Rejection::NotSet)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn reports_fake_failure() {
    let (_dir, root, notebook) = repository();
    venv(&root.join("repo/nb/.venv"), 0o755, "exit 1");
    let text = discover(&request(&notebook))
        .await
        .expect_err("nothing is usable")
        .to_string();
    let first = text
        .lines()
        .skip_while(|line| *line != "Checked in this order:")
        .nth(1)
        .expect("a first candidate");
    assert!(first.trim_start().starts_with("kernel.python "), "{first}");
    assert!(first.ends_with(" not set"), "{first}");
}

#[tokio::test(flavor = "multi_thread")]
async fn config_skips_dot_venv() {
    let (_dir, root, notebook) = repository();
    let configured = root.join("python");
    script(&configured, "exit 0");
    let marker = root.join("started");
    venv(&root.join("repo/nb/.venv"), 0o755, &leave_marker(&marker));
    let request = DiscoveryRequest::new(&notebook, Some(configured), DiscoveryEnv::default());
    let found = discover(&request).await.expect("finds one");
    assert_eq!(found.source, PythonSource::Config);
    assert!(!marker.exists(), "the .venv was probed");
}

#[tokio::test(flavor = "multi_thread")]
async fn selects_repo_root_dot_venv() {
    let (_dir, root, notebook) = repository();
    let python = venv(&root.join("repo/.venv"), 0o755, "exit 0");
    let found = discover(&request(&notebook)).await.expect("finds one");
    assert_eq!(
        (found.source, found.python),
        (PythonSource::DotVenv, python)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn skips_writable_dot_venv() {
    // SAFETY: `geteuid` takes nothing and touches no memory.
    if unsafe { libc::geteuid() } == 0 {
        eprintln!("skipped: root ignores permissions");
        return;
    }
    let (_dir, root, notebook) = repository();
    let marker = root.join("started");
    venv(&root.join("repo/nb/.venv"), 0o757, &leave_marker(&marker));
    let reports = reports(discover(&request(&notebook)).await);
    let dot_venv = reports
        .iter()
        .find(|report| report.source == PythonSource::DotVenv)
        .expect(".venv is reported");
    assert_eq!(dot_venv.rejection, Rejection::NotOwned);
    assert!(!marker.exists(), "the .venv was run");
}

#[tokio::test(flavor = "multi_thread")]
async fn bounded_total_wait() {
    let (_dir, root, notebook) = repository();
    venv(&root.join("repo/nb/.venv"), 0o755, "exec sleep 30");
    let bin = root.join("bin");
    std::fs::create_dir(&bin).expect("creates");
    script(&bin.join("python3"), "exec sleep 30");
    let env = DiscoveryEnv {
        path: Some(bin.into_os_string()),
        ..DiscoveryEnv::default()
    };
    let mut request = DiscoveryRequest::new(&notebook, None, env);
    request.probe_timeout = Duration::from_millis(500);
    let start = Instant::now();
    let result = discover(&request).await;
    assert!(start.elapsed() < Duration::from_secs(3));
    let timed_out = reports(result)
        .iter()
        .filter(|report| matches!(report.rejection, Rejection::TimedOut { .. }))
        .count();
    assert_eq!(timed_out, 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn dropping_discovery_stops_probe() {
    let (_dir, root, notebook) = repository();
    let configured = root.join("python");
    let pid_file = root.join("pid");
    script(
        &configured,
        &format!("echo $$ > '{}'; exec sleep 30", pid_file.display()),
    );
    let request = DiscoveryRequest::new(&notebook, Some(configured), DiscoveryEnv::default());
    let task = tokio::spawn(async move { discover(&request).await });
    let start = Instant::now();
    let pid: i32 = loop {
        let text = std::fs::read_to_string(&pid_file).unwrap_or_default();
        if let Ok(pid) = text.trim().parse() {
            break pid;
        }
        assert!(start.elapsed() < Duration::from_secs(5), "no pid written");
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    task.abort();
    let start = Instant::now();
    // SAFETY: signal 0 sends nothing and only probes.
    while unsafe { libc::kill(pid, 0) } == 0 {
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "process {pid} survives"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}
