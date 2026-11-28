//! The emergency stop during a Python discovery.
//!
//! `kill_all_kernels` stops probes for the rest of the process, so this file
//! holds a single test that runs in a process of its own.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use europa_core::{DiscoveryEnv, DiscoveryError, DiscoveryRequest, PythonSource, Rejection};
use europa_kernel::{discover_python, kill_all_kernels};
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

/// Write an executable `/bin/sh` script.
fn script(path: &Path, body: &str) {
    std::fs::write(path, format!("#!/bin/sh\n{body}\n")).expect("writes");
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
}

#[tokio::test(flavor = "multi_thread")]
async fn kill_all_stops_discovery() {
    let (_dir, root, notebook) = repository();
    let configured = root.join("python");
    let pid_file = root.join("pid");
    script(
        &configured,
        &format!("echo $$ > '{}'; exec sleep 30", pid_file.display()),
    );
    let request = DiscoveryRequest::new(&notebook, Some(configured), DiscoveryEnv::default());
    // No other test forks in this process, so the script is never busy.
    let task = tokio::spawn(async move { discover_python(&request).await });
    let start = Instant::now();
    let pid: i32 = loop {
        let text = std::fs::read_to_string(&pid_file).unwrap_or_default();
        if let Ok(pid) = text.trim().parse() {
            break pid;
        }
        assert!(start.elapsed() < Duration::from_secs(5), "no pid written");
        tokio::time::sleep(Duration::from_millis(20)).await;
    };

    kill_all_kernels();

    let start = Instant::now();
    // SAFETY: signal 0 sends nothing and only probes.
    while unsafe { libc::kill(pid, 0) } == 0 {
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "process {pid} survives"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let result = task.await.expect("search task");
    assert!(
        matches!(result, Err(DiscoveryError::ConfigUnusable { .. })),
        "{result:?}"
    );

    let venv = root.join("repo/nb/.venv");
    std::fs::create_dir_all(venv.join("bin")).expect("creates");
    std::fs::set_permissions(&venv, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    let marker = root.join("started");
    script(
        &venv.join("bin/python"),
        &format!(": > '{}'", marker.display()),
    );
    let request = DiscoveryRequest::new(&notebook, None, DiscoveryEnv::default());
    let result = discover_python(&request).await;
    let Err(DiscoveryError::NoneUsable { reports, .. }) = result else {
        panic!("not a failed search: {result:?}");
    };
    let dot_venv = reports
        .iter()
        .find(|report| report.source == PythonSource::DotVenv)
        .expect(".venv is reported");
    assert_eq!(
        dot_venv.rejection,
        Rejection::NotExecutable {
            detail: "Europa is stopping".to_owned(),
        }
    );
    assert!(!marker.exists(), "a probe started after the stop");
}
