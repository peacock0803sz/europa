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

use europa_core::{DiscoveryEnv, DiscoveryRequest, PythonSource};
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
