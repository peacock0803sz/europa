//! Integration tests driving a real ipykernel.
//!
//! These tests need a Python with `ipykernel` and never run inside plain
//! `cargo test`: they are all `#[ignore]`d and take their interpreter from
//! `EUROPA_KERNEL_PYTHON`. Without that variable they fail loudly instead
//! of passing silently.

use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use europa_core::{IopubMsg, KernelEvent, KernelLaunch, KernelStatus, MsgId, SpawnErrorKind};
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
    let (kernel, info) = LocalKernel::spawn(launch())
        .await
        .unwrap_or_else(|error| panic!("spawns: {error}"));
    assert_eq!(info.language.name, "python");
    let mut again = kernel.kernel_info().await.expect("info");
    assert_eq!(again.language.name, "python");
    // The banner rotates tips on every call, so compare the rest.
    let mut expected = info.clone();
    expected.banner.clear();
    again.banner.clear();
    assert_eq!(again, expected);
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
    let Err(error) = LocalKernel::spawn(missing).await else {
        panic!("missing fails")
    };
    assert_eq!(error.kind, SpawnErrorKind::InterpreterNotFound);
    let dir = tempfile::tempdir().expect("temp dir");
    let bare = KernelLaunch::new(bare_python(dir.path()), "python3");
    let Err(error) = LocalKernel::spawn(bare).await else {
        panic!("bare fails")
    };
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
    let Err(error) = LocalKernel::spawn(args).await else {
        panic!("times out")
    };
    assert!(matches!(
        error.kind,
        SpawnErrorKind::HandshakeTimeout { .. }
    ));
    assert_eq!(connection_files(), before);
}

#[ignore]
#[tokio::test(flavor = "multi_thread")]
async fn repeat_spawn() {
    for round in 0..10 {
        let start = Instant::now();
        let (kernel, _) = LocalKernel::spawn(launch())
            .await
            .unwrap_or_else(|error| panic!("round {round} spawns: {error}"));
        kernel
            .kernel_info()
            .await
            .unwrap_or_else(|error| panic!("round {round} info: {error:?}"));
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
    let (kernel, _) = LocalKernel::spawn(launch())
        .await
        .unwrap_or_else(|error| panic!("spawns: {error}"));
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
        .expect("joins");
    let (owned, _) = owned.unwrap_or_else(|error| panic!("spawns: {error}"));
    let pid = owned.process_id().expect("running");
    let file = owned.connection_file().expect("file");
    runtime.shutdown_background();
    drop(owned);
    assert_gone(pid);
    assert!(!file.exists());
}

/// Gather events until the reply and idle for `parent` both arrive.
fn collect_until(
    events: &Receiver<KernelEvent>,
    parent: &MsgId,
    timeout: Duration,
) -> Vec<KernelEvent> {
    let mut out = Vec::new();
    let mut reply_seen = false;
    let mut idle_seen = false;
    let start = Instant::now();
    while !(reply_seen && idle_seen) {
        assert!(
            start.elapsed() < timeout,
            "timed out waiting for {parent:?}"
        );
        match events.recv_timeout(Duration::from_secs(1)) {
            Ok(event) => {
                match &event {
                    KernelEvent::ExecuteReply { parent: p, .. } if p == parent => {
                        reply_seen = true;
                    }
                    KernelEvent::Status {
                        parent: Some(p),
                        state: KernelStatus::Idle,
                    } if p == parent => {
                        idle_seen = true;
                    }
                    _ => {}
                }
                out.push(event);
            }
            Err(error) => panic!("channel failed: {error}"),
        }
    }
    out
}

/// One round of executions against a live kernel.
async fn run_execute_sequence(kernel: &LocalKernel, events: &Receiver<KernelEvent>) {
    let hello = kernel.execute("print(\"hi\")").await.expect("sends");
    let got = collect_until(events, &hello, Duration::from_secs(10));
    assert!(got.iter().any(|event| matches!(
        event,
        KernelEvent::Status { parent: Some(p), state: KernelStatus::Busy } if p == &hello
    )));
    assert!(got.iter().any(|event| matches!(
        event,
        KernelEvent::Iopub(IopubMsg::Stream { parent: p, name, text })
            if p == &hello && name == "stdout" && text == "hi\n"
    )));
    assert!(got.iter().any(|event| matches!(
        event,
        KernelEvent::ExecuteReply { parent: p, .. } if p == &hello
    )));
    assert!(got.iter().any(|event| matches!(
        event,
        KernelEvent::Status { parent: Some(p), state: KernelStatus::Idle } if p == &hello
    )));

    let error = kernel.execute("1/0").await.expect("sends");
    let got = collect_until(events, &error, Duration::from_secs(10));
    assert!(got.iter().any(|event| matches!(
        event,
        KernelEvent::Iopub(IopubMsg::Error { parent: p, ename, traceback, .. })
            if p == &error && ename == "ZeroDivisionError" && !traceback.is_empty()
    )));

    let stuck = kernel.execute("input()").await.expect("sends");
    let got = collect_until(events, &stuck, Duration::from_secs(10));
    assert!(got.iter().any(|event| matches!(
        event,
        KernelEvent::Iopub(IopubMsg::Error { parent: p, .. }) if p == &stuck
    )));

    let first = kernel.execute("a = 1").await.expect("sends");
    let second = kernel.execute("a + 1").await.expect("sends");
    collect_until(events, &first, Duration::from_secs(10));
    let got = collect_until(events, &second, Duration::from_secs(10));
    assert!(got.iter().any(|event| matches!(
        event,
        KernelEvent::Iopub(IopubMsg::ExecuteResult { parent: p, .. }) if p == &second
    )));
}

#[ignore]
#[tokio::test(flavor = "multi_thread")]
async fn execute_events() {
    let (mut kernel, _) = LocalKernel::spawn(launch())
        .await
        .unwrap_or_else(|error| panic!("spawns: {error}"));
    let events = kernel.messages().expect("channel");
    run_execute_sequence(&kernel, &events).await;
    let pid = kernel.process_id().expect("running");
    let file = kernel.connection_file().expect("file");
    kernel.shutdown().await.expect("shutdown");
    assert_gone(pid);
    assert!(!file.exists());
}

#[ignore]
#[tokio::test(flavor = "multi_thread")]
async fn execute_events_repeat() {
    let (mut kernel, _) = LocalKernel::spawn(launch())
        .await
        .unwrap_or_else(|error| panic!("spawns: {error}"));
    let events = kernel.messages().expect("channel");
    for _ in 0..10 {
        run_execute_sequence(&kernel, &events).await;
    }
    let pid = kernel.process_id().expect("running");
    let file = kernel.connection_file().expect("file");
    kernel.shutdown().await.expect("shutdown");
    assert_gone(pid);
    assert!(!file.exists());
}

#[ignore]
#[tokio::test(flavor = "multi_thread")]
async fn dropped_receiver() {
    let (mut kernel, _) = LocalKernel::spawn(launch())
        .await
        .unwrap_or_else(|error| panic!("spawns: {error}"));
    drop(kernel.messages());
    for _ in 0..3 {
        kernel.execute("1").await.expect("sends");
    }
    kernel.kernel_info().await.expect("info works");
    let pid = kernel.process_id().expect("running");
    let file = kernel.connection_file().expect("file");
    kernel.shutdown().await.expect("shutdown");
    assert_gone(pid);
    assert!(!file.exists());
}
