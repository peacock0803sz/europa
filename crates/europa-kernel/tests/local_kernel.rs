//! Integration tests driving a real ipykernel.
//!
//! These tests need a Python with `ipykernel` and never run inside plain
//! `cargo test`: they are all `#[ignore]`d and take their interpreter from
//! `EUROPA_KERNEL_PYTHON`. Without that variable they fail loudly instead
//! of passing silently.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::Arc;
use std::time::{Duration, Instant};

use europa_core::{
    IopubMsg, KernelError, KernelEvent, KernelLaunch, KernelOp, KernelStatus, MsgId, ProcessExit,
    SpawnErrorKind,
};
use europa_kernel::{kill_all_kernels, KernelClient, LocalKernel};

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

/// Read events until `f` matches one or `timeout` passes.
fn recv_match(
    events: &Receiver<KernelEvent>,
    timeout: Duration,
    mut f: impl FnMut(&KernelEvent) -> bool,
) -> KernelEvent {
    let start = Instant::now();
    loop {
        assert!(start.elapsed() < timeout, "timed out waiting for an event");
        match events.recv_timeout(Duration::from_secs(1)) {
            Ok(event) => {
                if f(&event) {
                    return event;
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(error) => panic!("channel failed: {error}"),
        }
    }
}

/// Drain events until quiet for `settle`.
fn drain(events: &Receiver<KernelEvent>, settle: Duration) -> Vec<KernelEvent> {
    let mut out = Vec::new();
    loop {
        match events.recv_timeout(settle) {
            Ok(event) => out.push(event),
            Err(_) => return out,
        }
    }
}

#[ignore]
#[tokio::test(flavor = "multi_thread")]
async fn interrupt_running() {
    let (mut kernel, _) = LocalKernel::spawn(launch())
        .await
        .unwrap_or_else(|error| panic!("spawns: {error}"));
    let events = kernel.messages().expect("channel");
    let running = kernel
        .execute("import time; time.sleep(60)")
        .await
        .expect("sends");
    recv_match(&events, Duration::from_secs(10), |event| {
        matches!(
            event,
            KernelEvent::Status { parent: Some(p), state: KernelStatus::Busy } if p == &running
        )
    });
    kernel.interrupt().await.expect("interrupt");
    let got = recv_match(&events, Duration::from_secs(10), |event| {
        matches!(
            event,
            KernelEvent::Iopub(IopubMsg::Error { parent: p, ename, .. })
                if p == &running && ename == "KeyboardInterrupt"
        )
    });
    assert!(matches!(got, KernelEvent::Iopub(IopubMsg::Error { .. })));
    let pid = kernel.process_id().expect("running");
    let file = kernel.connection_file().expect("file");
    kernel.shutdown().await.expect("shutdown");
    assert_gone(pid);
    assert!(!file.exists());
}

#[ignore]
#[tokio::test(flavor = "multi_thread")]
async fn restart_flow() {
    let (mut kernel, _) = LocalKernel::spawn(launch())
        .await
        .unwrap_or_else(|error| panic!("spawns: {error}"));
    let events = kernel.messages().expect("channel");
    let old_pid = kernel.process_id().expect("running");
    let old_file = kernel.connection_file().expect("file");
    let kernel = Arc::new(kernel);
    let restarting = Arc::clone(&kernel);
    let restarted = tokio::spawn(async move { restarting.restart().await });
    let mut restarting_seen = false;
    let outcome = tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            if restarted.is_finished() {
                break;
            }
            match kernel.execute("1").await {
                Err(KernelError::Restarting) => {
                    restarting_seen = true;
                }
                Err(error) => panic!("unexpected execute failure: {error:?}"),
                Ok(_) => {}
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await;
    assert!(outcome.is_ok(), "restart takes too long");
    let info = restarted
        .await
        .expect("joins")
        .unwrap_or_else(|error| panic!("restarts: {error}"));
    assert!(
        restarting_seen,
        "expected Restarting while restart runs; restart may have finished before any execute was rejected"
    );
    assert_eq!(info.language.name, "python");
    let new_pid = kernel.process_id().expect("running");
    assert_ne!(new_pid, old_pid);
    assert_gone(old_pid);
    let new_file = kernel.connection_file().expect("file");
    assert_ne!(new_file, old_file);
    assert!(!old_file.exists());
    let drained = drain(&events, Duration::from_millis(500));
    assert_eq!(
        drained
            .iter()
            .filter(|event| matches!(event, KernelEvent::Restarted { .. }))
            .count(),
        1
    );
    assert!(
        !drained
            .iter()
            .any(|event| matches!(event, KernelEvent::Died { .. })),
        "no Died across a restart"
    );
    let again = kernel.execute("print(1)").await.expect("sends");
    collect_until(&events, &again, Duration::from_secs(10));
    let pid = kernel.process_id().expect("running");
    let file = kernel.connection_file().expect("file");
    let kernel = Arc::try_unwrap(kernel).unwrap_or_else(|_| panic!("sole owner"));
    let mut kernel = kernel;
    kernel.shutdown().await.expect("shutdown");
    assert_gone(pid);
    assert!(!file.exists());
}

#[ignore]
#[tokio::test(flavor = "multi_thread")]
async fn shutdown_during_restart() {
    let (mut kernel, _) = LocalKernel::spawn(launch())
        .await
        .unwrap_or_else(|error| panic!("spawns: {error}"));
    let old_pid = kernel.process_id().expect("running");
    let kernel = Arc::new(kernel);
    let restarting = Arc::clone(&kernel);
    let shutting = Arc::clone(&kernel);
    let (restarted, shut) = tokio::join!(
        tokio::spawn(async move { restarting.restart().await }),
        tokio::spawn(async move { shutting.shutdown().await }),
    );
    let restart_result = restarted.expect("joins");
    let shutdown_result = shut.expect("joins");
    assert!(shutdown_result.is_ok(), "shutdown wins");
    assert!(
        matches!(restart_result, Err(KernelError::Closed)),
        "restart is aborted, got {restart_result:?}"
    );
    assert_gone(old_pid);
    assert!(kernel.process_id().is_none());
    assert!(kernel.connection_file().is_none());
}

#[ignore]
#[tokio::test(flavor = "multi_thread")]
async fn death_and_recover() {
    let (mut kernel, _) = LocalKernel::spawn(launch())
        .await
        .unwrap_or_else(|error| panic!("spawns: {error}"));
    let events = kernel.messages().expect("channel");
    let pid = kernel.process_id().expect("running");
    // SAFETY: `kill` with SIGKILL only terminates the child.
    unsafe { libc::kill(pid as i32, libc::SIGKILL) };
    let got = recv_match(&events, Duration::from_secs(10), |event| {
        matches!(event, KernelEvent::Died { .. })
    });
    assert!(
        matches!(
            got,
            KernelEvent::Died {
                exit: ProcessExit::Signal(9),
                ..
            }
        ),
        "got {got:?}"
    );
    assert!(matches!(
        kernel.execute("1").await,
        Err(KernelError::Dead { .. })
    ));
    assert!(matches!(
        kernel.kernel_info().await,
        Err(KernelError::Dead { .. })
    ));
    assert!(matches!(
        kernel.interrupt().await,
        Err(KernelError::Dead { .. })
    ));
    assert!(kernel.connection_file().is_none());
    let info = kernel
        .restart()
        .await
        .unwrap_or_else(|error| panic!("recovers: {error}"));
    assert_eq!(info.language.name, "python");
    recv_match(&events, Duration::from_secs(10), |event| {
        matches!(event, KernelEvent::Restarted { .. })
    });
    kernel.execute("1").await.expect("runs again");
    kernel.kernel_info().await.expect("info again");
    let pid = kernel.process_id().expect("running");
    // SAFETY: `kill` with SIGKILL only terminates the child.
    unsafe { libc::kill(pid as i32, libc::SIGKILL) };
    recv_match(&events, Duration::from_secs(10), |event| {
        matches!(event, KernelEvent::Died { .. })
    });
    kernel.shutdown().await.expect("shutdown after death");
}

#[ignore]
#[tokio::test(flavor = "multi_thread")]
async fn restart_failure() {
    let dir = tempfile::tempdir().expect("temp dir");
    let wrapper = dir.path().join("python-wrapper");
    std::fs::write(
        &wrapper,
        format!("#!/bin/sh\nexec {} \"$@\"\n", python().display()),
    )
    .expect("writes wrapper");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    }
    let (mut kernel, _) = LocalKernel::spawn(KernelLaunch::new(&wrapper, "python3"))
        .await
        .unwrap_or_else(|error| panic!("spawns: {error}"));
    std::fs::remove_file(&wrapper).expect("removes wrapper");
    let error = kernel.restart().await.expect_err("restart fails");
    assert!(
        matches!(
            error,
            KernelError::Spawn(europa_core::SpawnError {
                kind: SpawnErrorKind::InterpreterNotFound,
                ..
            })
        ),
        "got {error:?}"
    );
    assert!(matches!(
        kernel.execute("1").await,
        Err(KernelError::Dead { .. })
    ));
    let pid = kernel.process_id();
    assert!(pid.is_none(), "no child after failed restart");
    kernel.shutdown().await.expect("shutdown");
}

#[ignore]
#[tokio::test(flavor = "multi_thread")]
async fn kill_all() {
    let (mut first, _) = LocalKernel::spawn(launch())
        .await
        .unwrap_or_else(|error| panic!("spawns: {error}"));
    let (mut second, _) = LocalKernel::spawn(launch())
        .await
        .unwrap_or_else(|error| panic!("spawns: {error}"));
    let _ = first.messages();
    let _ = second.messages();
    let first_pid = first.process_id().expect("running");
    let second_pid = second.process_id().expect("running");
    let first_file = first.connection_file().expect("file");
    let second_file = second.connection_file().expect("file");
    kill_all_kernels();
    assert_gone(first_pid);
    assert_gone(second_pid);
    assert!(!first_file.exists());
    assert!(!second_file.exists());
}

#[ignore]
#[tokio::test(flavor = "multi_thread")]
async fn request_timeout() {
    let mut args = launch();
    args.request_timeout = Duration::from_secs(1);
    let (mut kernel, _) = LocalKernel::spawn(args)
        .await
        .unwrap_or_else(|error| panic!("spawns: {error}"));
    let pid = kernel.process_id().expect("running");
    // SAFETY: `kill` with SIGSTOP only stops the child.
    unsafe { libc::kill(pid as i32, libc::SIGSTOP) };
    assert!(matches!(
        kernel.kernel_info().await,
        Err(KernelError::Timeout {
            op: KernelOp::KernelInfo
        })
    ));
    assert!(matches!(
        kernel.interrupt().await,
        Err(KernelError::Timeout {
            op: KernelOp::Interrupt
        })
    ));
    let mut args = launch();
    args.shutdown_grace = Duration::from_secs(1);
    let (mut slow, _) = LocalKernel::spawn(args)
        .await
        .unwrap_or_else(|error| panic!("spawns: {error}"));
    let slow_pid = slow.process_id().expect("running");
    let slow_file = slow.connection_file().expect("file");
    // SAFETY: `kill` with SIGSTOP only stops the child.
    unsafe { libc::kill(slow_pid as i32, libc::SIGSTOP) };
    slow.shutdown().await.expect("forced shutdown");
    assert_gone(slow_pid);
    assert!(!slow_file.exists());
    // SAFETY: `kill` with SIGCONT only resumes the child.
    unsafe { libc::kill(pid as i32, libc::SIGCONT) };
    kernel.kernel_info().await.expect("info after resume");
    let file = kernel.connection_file().expect("file");
    kernel.shutdown().await.expect("shutdown");
    assert_gone(pid);
    assert!(!file.exists());
}

/// Kill a child copy of this test binary and wait for its kernel to go.
async fn run_sigkill_probe(extra_env: Option<(&str, &str)>) {
    let exe = std::env::current_exe().expect("exe");
    let mut command = std::process::Command::new(exe);
    command
        .args([
            "--exact",
            "survives_parent_sigkill_child",
            "--ignored",
            "--nocapture",
        ])
        .env("EUROPA_KERNEL_SIGKILL_CHILD", "1")
        .env("EUROPA_KERNEL_PYTHON", python())
        .stdout(std::process::Stdio::piped());
    if let Some((key, value)) = extra_env {
        command.env(key, value);
    }
    let mut child = command.spawn().expect("spawns child");
    let stdout = child.stdout.take().expect("piped");
    let lines = BufReader::new(stdout).lines();
    let start = Instant::now();
    let mut kernel_pid = None;
    for line in lines {
        let line = line.expect("reads");
        if let Some(pid) = line.strip_prefix("kernel-pid=") {
            kernel_pid = Some(pid.parse().expect("number"));
            break;
        }
        assert!(
            start.elapsed() < Duration::from_secs(30),
            "no kernel pid line"
        );
    }
    let kernel_pid: u32 = kernel_pid.expect("pid line");
    // SAFETY: `kill` with SIGKILL only terminates the child test process.
    unsafe { libc::kill(child.id() as i32, libc::SIGKILL) };
    let _ = child.wait();
    let start = Instant::now();
    loop {
        // SAFETY: `kill` with signal 0 sends nothing and only probes.
        let probed = unsafe { libc::kill(kernel_pid as i32, 0) };
        if probed != 0 {
            assert_eq!(
                std::io::Error::last_os_error().raw_os_error(),
                Some(libc::ESRCH)
            );
            return;
        }
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "kernel {kernel_pid} survives parent SIGKILL"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[ignore]
#[tokio::test(flavor = "multi_thread")]
async fn survives_parent_sigkill() {
    run_sigkill_probe(None).await;
}

#[cfg(target_os = "linux")]
#[ignore]
#[tokio::test(flavor = "multi_thread")]
async fn survives_parent_sigkill_pdeathsig() {
    run_sigkill_probe(Some(("EUROPA_KERNEL_TEST_NO_JPY_PARENT_PID", "1"))).await;
}

#[ignore]
#[tokio::test(flavor = "multi_thread")]
async fn survives_parent_sigkill_child() {
    if std::env::var_os("EUROPA_KERNEL_SIGKILL_CHILD").is_none() {
        return;
    }
    let (_kernel, _) = LocalKernel::spawn(launch())
        .await
        .unwrap_or_else(|error| panic!("spawns: {error}"));
    let pid = _kernel.process_id().expect("running");
    println!("kernel-pid={pid}");
    std::io::stdout().flush().expect("flush");
    tokio::time::sleep(Duration::from_secs(60)).await;
}
