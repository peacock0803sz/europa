//! Integration tests driving a real ipykernel.
//!
//! These tests need a Python with `ipykernel` and never run inside plain
//! `cargo test`: they are all `#[ignore]`d and take their interpreter from
//! `EUROPA_KERNEL_PYTHON`. Without that variable they fail loudly instead
//! of passing silently.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use europa_core::{KernelLaunch, SpawnErrorKind};
use europa_kernel::{KernelClient, LocalKernel};

/// Interpreter under test, or a loud failure.
fn python() -> PathBuf {
    std::env::var_os("EUROPA_KERNEL_PYTHON").map_or_else(
        || panic!("EUROPA_KERNEL_PYTHON is not set; point it at a Python with ipykernel installed"),
        PathBuf::from,
    )
}

/// A virtual environment without `ipykernel`.
fn bare_python(dir: &Path) -> PathBuf {
    let venv = dir.join("bare");
    let status = std::process::Command::new(python())
        .args(["-m", "venv"])
        .arg(&venv)
        .status()
        .expect("venv spawns");
    assert!(status.success());
    venv.join("bin").join("python")
}

/// Wait until `kill(pid, 0)` reports the process is gone.
fn assert_gone(pid: u32) {
    let start = Instant::now();
    loop {
        // SAFETY: `kill` with signal 0 sends nothing and only probes.
        let probed = unsafe { libc::kill(pid as i32, 0) };
        if probed != 0 {
            assert_eq!(
                std::io::Error::last_os_error().raw_os_error(),
                Some(libc::ESRCH)
            );
            return;
        }
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "process {pid} still alive"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Count our connection files in the temporary directory.
fn connection_files() -> usize {
    std::fs::read_dir(std::env::temp_dir())
        .expect("temp dir lists")
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.starts_with("europa-kernel-") && name.ends_with(".json"))
        })
        .count()
}

/// Default launch arguments for the tests.
fn launch() -> KernelLaunch {
    KernelLaunch::new(python(), "python3")
}

#[ignore]
#[tokio::test(flavor = "multi_thread")]
async fn spawn_and_info() {
    let (kernel, info) = LocalKernel::spawn(launch()).await.expect("spawns");
    assert_eq!(info.language.name, "python");
    assert_eq!(kernel.kernel_info().await.expect("info"), info);
    let pid = kernel.process_id().expect("running");
    let file = kernel.connection_file().expect("file");
    kernel.shutdown().await.expect("shutdown");
    assert_gone(pid);
    assert!(!file.exists());
    kernel.shutdown().await.expect("second shutdown");
}

#[ignore]
#[tokio::test(flavor = "multi_thread")]
async fn spawn_failures() {
    let before = connection_files();
    let missing = KernelLaunch::new("/nonexistent-europa-python", "python3");
    let error = LocalKernel::spawn(missing)
        .await
        .expect_err("missing fails");
    assert_eq!(error.kind, SpawnErrorKind::InterpreterNotFound);
    let dir = tempfile::tempdir().expect("temp dir");
    let bare = KernelLaunch::new(bare_python(dir.path()), "python3");
    let error = LocalKernel::spawn(bare).await.expect_err("bare fails");
    assert_eq!(error.kind, SpawnErrorKind::IpykernelMissing);
    assert!(error.stderr_tail.contains("ipykernel"));
    assert_eq!(connection_files(), before);
}

#[ignore]
#[tokio::test(flavor = "multi_thread")]
async fn handshake_timeout() {
    let before = connection_files();
    let mut args = launch();
    args.handshake_timeout = Duration::from_millis(1);
    args.port_retries = 0;
    let error = LocalKernel::spawn(args).await.expect_err("times out");
    assert!(matches!(
        error.kind,
        SpawnErrorKind::HandshakeTimeout { .. }
    ));
    assert_eq!(connection_files(), before);
}

#[ignore]
#[tokio::test(flavor = "multi_thread")]
async fn repeat_spawn() {
    for _ in 0..10 {
        let start = Instant::now();
        let (kernel, _) = LocalKernel::spawn(launch()).await.expect("spawns");
        kernel.kernel_info().await.expect("info");
        assert!(start.elapsed() < Duration::from_secs(10));
        let pid = kernel.process_id().expect("running");
        let file = kernel.connection_file().expect("file");
        kernel.shutdown().await.expect("shutdown");
        assert_gone(pid);
        assert!(!file.exists());
    }
}

#[ignore]
#[tokio::test(flavor = "multi_thread")]
async fn drop_kills() {
    let (kernel, _) = LocalKernel::spawn(launch()).await.expect("spawns");
    let pid = kernel.process_id().expect("running");
    let file = kernel.connection_file().expect("file");
    drop(kernel);
    assert_gone(pid);
    assert!(!file.exists());

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let owned = runtime
        .spawn(LocalKernel::spawn(launch()))
        .await
        .expect("joins")
        .expect("spawns")
        .0;
    let pid = owned.process_id().expect("running");
    let file = owned.connection_file().expect("file");
    runtime.shutdown_background();
    drop(owned);
    assert_gone(pid);
    assert!(!file.exists());
}
