//! Integration tests of Python discovery against a real interpreter.
//!
//! These tests need a Python with `ipykernel` and never run inside plain
//! `cargo test`: they are all `#[ignore]`d and take their interpreter from
//! `EUROPA_KERNEL_PYTHON`. Without that variable they fail loudly instead of
//! passing silently. Run them with `--test-threads=1`, because some change the
//! current directory of the process.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use europa_core::{
    CandidateReport, Discovery, DiscoveryEnv, DiscoveryError, DiscoveryRequest, InstallHint,
    PythonSource, Rejection,
};
use europa_kernel::discover_python;
use tempfile::TempDir;

/// Interpreter under test, or a loud failure.
fn python() -> PathBuf {
    std::env::var_os("EUROPA_KERNEL_PYTHON").map_or_else(
        || panic!("EUROPA_KERNEL_PYTHON is not set; point it at a Python with ipykernel installed"),
        PathBuf::from,
    )
}

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

/// A `.venv` at `root` whose interpreter is a `/bin/sh` script running `body`.
///
/// A symbolic link to the interpreter under test would not do: CPython looks
/// for `pyvenv.cfg` next to the link, so the Python of a virtual environment
/// would stop acting as one.
fn script_venv(root: &Path, body: &str) -> PathBuf {
    std::fs::create_dir_all(root.join("bin")).expect("creates");
    set_mode(root, 0o755);
    let python = root.join("bin/python");
    std::fs::write(&python, format!("#!/bin/sh\n{body}\n")).expect("writes script");
    set_mode(&python, 0o755);
    python
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs EUROPA_KERNEL_PYTHON"]
async fn finds_dot_venv() {
    let (_dir, root, notebook) = repository();
    let expected = script_venv(
        &root.join("repo/nb/.venv"),
        &format!("exec '{}' \"$@\"", python().display()),
    );
    let request = DiscoveryRequest::new(&notebook, None, DiscoveryEnv::default());
    let start = Instant::now();
    let found = discover_python(&request).await.expect("finds one");
    assert!(
        start.elapsed() < Duration::from_secs(3),
        "took {:?}",
        start.elapsed()
    );
    assert_eq!(found.source, PythonSource::DotVenv);
    assert_eq!(found.python, expected);

    let here = std::env::current_dir().expect("current dir");
    std::env::set_current_dir("/").expect("enters the root");
    let again = discover_python(&request).await;
    std::env::set_current_dir(here).expect("returns");
    assert_eq!(again, Ok(found));
}

/// An environment of the interpreter under test that lacks `ipykernel`.
fn bare_venv(dir: &Path) -> PathBuf {
    let status = std::process::Command::new(python())
        .args(["-m", "venv", "--without-pip"])
        .arg(dir)
        .status()
        .expect("venv spawns");
    assert!(status.success());
    // A group-writable directory of a shared group would be skipped unprobed.
    set_mode(dir, 0o755);
    dir.join("bin/python")
}

/// The report of `source` in a search that found nothing usable.
fn report_of(result: &Result<Discovery, DiscoveryError>, source: PythonSource) -> &CandidateReport {
    let Err(DiscoveryError::NoneUsable { reports, .. }) = result else {
        panic!("not a failed search: {result:?}");
    };
    reports
        .iter()
        .find(|report| report.source == source)
        .unwrap_or_else(|| panic!("no report of {source:?} in {reports:?}"))
}

/// Assert that `.venv` was probed and lacks `ipykernel`.
fn assert_lacks_ipykernel(result: &Result<Discovery, DiscoveryError>) {
    let report = report_of(result, PythonSource::DotVenv);
    let Rejection::CannotImport { stderr_tail, .. } = &report.rejection else {
        panic!("{report:?}");
    };
    assert!(
        stderr_tail.contains("No module named 'ipykernel'"),
        "{stderr_tail}"
    );
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs EUROPA_KERNEL_PYTHON"]
async fn reports_missing_ipykernel() {
    let (_dir, root, notebook) = repository();
    let bare = bare_venv(&root.join("repo/nb/.venv"));
    let request = DiscoveryRequest::new(&notebook, None, DiscoveryEnv::default());
    let result = discover_python(&request).await;
    assert_lacks_ipykernel(&result);
    assert_eq!(
        report_of(&result, PythonSource::DotVenv).install,
        Some(InstallHint::Pip)
    );
    let text = result.expect_err("nothing is usable").to_string();
    assert!(
        text.contains(&format!(
            "install it: {} -m pip install ipykernel",
            bare.display()
        )),
        "{text}"
    );
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs EUROPA_KERNEL_PYTHON"]
async fn ignores_cwd_module() {
    let (_dir, root, notebook) = repository();
    bare_venv(&root.join("repo/nb/.venv"));
    let elsewhere = tempfile::tempdir().expect("temp dir");
    for dir in [elsewhere.path(), &root.join("repo/nb")] {
        std::fs::write(dir.join("ipykernel.py"), "").expect("writes");
    }
    let request = DiscoveryRequest::new(&notebook, None, DiscoveryEnv::default());
    let here = std::env::current_dir().expect("current dir");
    std::env::set_current_dir(elsewhere.path()).expect("enters");
    let result = discover_python(&request).await;
    std::env::set_current_dir(here).expect("returns");
    assert_lacks_ipykernel(&result);
}

/// Names of the distributions that the interpreter under test sees.
fn distributions() -> String {
    let output = std::process::Command::new(python())
        .args([
            "-c",
            "import importlib.metadata as m; \
             print(sorted(d.metadata['Name'] for d in m.distributions()))",
        ])
        .output()
        .expect("python spawns");
    assert!(output.status.success());
    String::from_utf8(output.stdout).expect("UTF-8")
}

/// Every path below `dir`.
fn listing(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir(dir).expect("lists") {
        let path = entry.expect("entry").path();
        if path.is_dir() {
            found.extend(listing(&path));
        }
        found.push(path);
    }
    found.sort();
    found
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs EUROPA_KERNEL_PYTHON"]
async fn leaves_environment_untouched() {
    let (_dir, root, notebook) = repository();
    let bare = root.join("bare");
    let python = bare_venv(&bare);
    let env = DiscoveryEnv {
        path: Some(bare.join("bin").into_os_string()),
        ..DiscoveryEnv::default()
    };
    // `lib` holds `site-packages`, where an install would land.
    let before = (distributions(), listing(&bare.join("lib")));
    let request = DiscoveryRequest::new(&notebook, None, env);
    let result = discover_python(&request).await;
    assert_eq!((distributions(), listing(&bare.join("lib"))), before);
    assert!(!root.join("repo/.venv").exists());
    assert!(!root.join("repo/nb/.venv").exists());
    assert_eq!(
        report_of(&result, PythonSource::PathPython3).python,
        Some(python.with_file_name("python3"))
    );
    assert_eq!(report_of(&result, PythonSource::PathPython3).install, None);
    let text = result.expect_err("nothing is usable").to_string();
    assert!(
        text.contains(&format!("-m venv {}", root.join("repo/.venv").display())),
        "{text}"
    );
    assert!(!text.contains("install it:"), "{text}");
}
