//! Integration tests driving a real ipykernel.
//!
//! These tests need a Python with `ipykernel` and never run inside plain
//! `cargo test`: they are all `#[ignore]`d and take their interpreter from
//! `EUROPA_KERNEL_PYTHON`. Without that variable they fail loudly instead
//! of passing silently. The one exception is `survives_parent_sigkill_child`,
//! the child side of the `SIGKILL` probes, which returns at once unless a
//! probe started it. Run them with `--test-threads=1`: `kill_all` kills
//! every kernel in the process, and several tests compare the count of
//! connection files in the shared temporary directory.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::Arc;
use std::time::{Duration, Instant};

use europa_core::{
    IopubMsg, KernelError, KernelEvent, KernelLaunch, KernelOp, KernelStatus, MsgId, ProcessExit,
    SpawnError, SpawnErrorKind,
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

/// Write an executable `/bin/sh` script into `dir`.
///
/// Scripts that `exec` the interpreter stand in for it. A symbolic link
/// would not do: CPython looks for `pyvenv.cfg` next to the link, so the
/// Python of a virtual environment would stop acting as one.
fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let path = dir.join(name);
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).expect("writes script");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    path
}

/// A script that runs the interpreter under test after a pause.
///
/// The pause keeps a restarted child in its handshake long enough to act on
/// it there.
fn slow_python(dir: &Path) -> PathBuf {
    script(
        dir,
        "slow-python",
        &format!("sleep 1\nexec '{}' \"$@\"", python().display()),
    )
}

/// Whether `pid` is gone right now.
fn is_gone(pid: u32) -> bool {
    // SAFETY: `kill` with signal 0 sends nothing and only probes.
    let probed = unsafe { libc::kill(pid as i32, 0) };
    probed != 0 && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
}

/// Assert that `pid` is already gone, after calls that wait for the exit.
fn assert_gone_now(pid: u32) {
    assert!(is_gone(pid), "process {pid} still alive");
}

/// Wait until `pid` is gone, after calls that kill without waiting.
fn assert_gone(pid: u32) {
    let start = Instant::now();
    while !is_gone(pid) {
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

/// Spawn a kernel or fail the test.
async fn spawn(launch: KernelLaunch) -> LocalKernel {
    LocalKernel::spawn(launch)
        .await
        .unwrap_or_else(|error| panic!("spawns: {error}"))
        .0
}

/// Send `sig` to one process.
fn signal(pid: u32, sig: i32) {
    // SAFETY: `kill` only sends the signal to that one process.
    unsafe { libc::kill(pid as i32, sig) };
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
    assert_gone_now(pid);
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
        assert_gone_now(pid);
        assert!(!file.exists());
    }
}

#[ignore]
#[tokio::test(flavor = "multi_thread")]
async fn drop_kills() {
    let kernel = spawn(launch()).await;
    let pid = kernel.process_id().expect("running");
    let file = kernel.connection_file().expect("file");
    drop(kernel);
    assert!(!file.exists());
    assert_gone(pid);
}

/// A runtime that runs tasks only inside `block_on`.
///
/// Between those calls no task runs, so whatever happens there is the work
/// of synchronous code alone.
fn idle_runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime")
}

#[ignore]
#[test]
fn drop_kills_without_runtime() {
    let runtime = idle_runtime();
    let kernel = runtime.block_on(spawn(launch()));
    let pid = kernel.process_id().expect("running");
    let file = kernel.connection_file().expect("file");
    // Nothing drives the owner task now, so only `Drop` itself can act.
    drop(kernel);
    assert!(!file.exists());
    assert_gone(pid);
    drop(runtime);
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
            Err(RecvTimeoutError::Timeout) => {}
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
    let mut kernel = spawn(launch()).await;
    let events = kernel.messages().expect("channel");
    run_execute_sequence(&kernel, &events).await;
    let pid = kernel.process_id().expect("running");
    let file = kernel.connection_file().expect("file");
    kernel.shutdown().await.expect("shutdown");
    assert_gone_now(pid);
    assert!(!file.exists());
}

#[ignore]
#[tokio::test(flavor = "multi_thread")]
async fn execute_events_repeat() {
    let mut kernel = spawn(launch()).await;
    let events = kernel.messages().expect("channel");
    for _ in 0..10 {
        run_execute_sequence(&kernel, &events).await;
    }
    let pid = kernel.process_id().expect("running");
    let file = kernel.connection_file().expect("file");
    kernel.shutdown().await.expect("shutdown");
    assert_gone_now(pid);
    assert!(!file.exists());
}

#[ignore]
#[tokio::test(flavor = "multi_thread")]
async fn dropped_receiver() {
    let mut kernel = spawn(launch()).await;
    drop(kernel.messages());
    for _ in 0..3 {
        kernel.execute("1").await.expect("sends");
    }
    kernel.kernel_info().await.expect("info works");
    let pid = kernel.process_id().expect("running");
    let file = kernel.connection_file().expect("file");
    kernel.shutdown().await.expect("shutdown");
    assert_gone_now(pid);
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

/// Count the `Died` and `Restarted` events among `events`.
fn lifecycle_counts(events: &[KernelEvent]) -> (usize, usize) {
    let died = events
        .iter()
        .filter(|event| matches!(event, KernelEvent::Died { .. }))
        .count();
    let restarted = events
        .iter()
        .filter(|event| matches!(event, KernelEvent::Restarted { .. }))
        .count();
    (died, restarted)
}

/// Wait until a restart has started a child other than `old_pid`.
async fn wait_for_new_child(kernel: &LocalKernel, old_pid: u32) -> (u32, PathBuf) {
    let start = Instant::now();
    loop {
        if let (Some(pid), Some(file)) = (kernel.process_id(), kernel.connection_file()) {
            if pid != old_pid {
                return (pid, file);
            }
        }
        assert!(start.elapsed() < Duration::from_secs(30), "no new child");
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

#[ignore]
#[tokio::test(flavor = "multi_thread")]
async fn interrupt_running() {
    let mut kernel = spawn(launch()).await;
    let events = kernel.messages().expect("channel");
    let running = kernel
        .execute("import time\nprint('started', flush=True)\ntime.sleep(60)")
        .await
        .expect("sends");
    // Interrupt only once the cell itself runs: a SIGINT that lands while the
    // kernel prepares the cell is logged instead of raised in it.
    recv_match(&events, Duration::from_secs(10), |event| {
        matches!(
            event,
            KernelEvent::Iopub(IopubMsg::Stream { parent: p, text, .. })
                if p == &running && text.contains("started")
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
    assert_gone_now(pid);
    assert!(!file.exists());
}

#[ignore]
#[tokio::test(flavor = "multi_thread")]
async fn restart_flow() {
    let mut kernel = spawn(launch()).await;
    let events = kernel.messages().expect("channel");
    let old_pid = kernel.process_id().expect("running");
    let old_file = kernel.connection_file().expect("file");
    let kernel = Arc::new(kernel);
    let restarting = Arc::clone(&kernel);
    let restarted = tokio::spawn(async move { restarting.restart().await });
    let mut execute_rejected = false;
    let mut info_rejected = false;
    let mut interrupt_rejected = false;
    let outcome = tokio::time::timeout(Duration::from_secs(30), async {
        while !restarted.is_finished() {
            match kernel.execute("1").await {
                Err(KernelError::Restarting) => {
                    execute_rejected = true;
                    // The restart still runs, so these are turned away too
                    // unless it ends right now.
                    match kernel.kernel_info().await {
                        Err(KernelError::Restarting) => info_rejected = true,
                        Ok(_) => {}
                        Err(error) => panic!("unexpected info failure: {error:?}"),
                    }
                    match kernel.interrupt().await {
                        Err(KernelError::Restarting) => interrupt_rejected = true,
                        Ok(()) => {}
                        Err(error) => panic!("unexpected interrupt failure: {error:?}"),
                    }
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
        execute_rejected,
        "expected Restarting while restart runs; restart may have finished before any execute was rejected"
    );
    assert!(
        info_rejected,
        "expected kernel_info to fail with Restarting"
    );
    assert!(
        interrupt_rejected,
        "expected interrupt to fail with Restarting"
    );
    assert_eq!(info.language.name, "python");
    let new_pid = kernel.process_id().expect("running");
    assert_ne!(new_pid, old_pid);
    assert_gone_now(old_pid);
    let new_file = kernel.connection_file().expect("file");
    assert_ne!(new_file, old_file);
    assert!(!old_file.exists());
    let drained = drain(&events, Duration::from_millis(500));
    assert_eq!(
        lifecycle_counts(&drained),
        (0, 1),
        "one Restarted and no Died"
    );
    let again = kernel.execute("print(1)").await.expect("sends");
    collect_until(&events, &again, Duration::from_secs(10));
    let pid = kernel.process_id().expect("running");
    let file = kernel.connection_file().expect("file");
    let kernel = Arc::try_unwrap(kernel).unwrap_or_else(|_| panic!("sole owner"));
    kernel.shutdown().await.expect("shutdown");
    assert_gone_now(pid);
    assert!(!file.exists());
}

/// Whether `event` is output, a reply, or a status of the run `parent`.
fn reports_on(event: &KernelEvent, parent: &MsgId) -> bool {
    match event {
        KernelEvent::Iopub(IopubMsg::Stream { parent: p, .. })
        | KernelEvent::ExecuteReply { parent: p, .. }
        | KernelEvent::Status {
            parent: Some(p), ..
        } => p == parent,
        _ => false,
    }
}

#[ignore]
#[tokio::test(flavor = "multi_thread")]
async fn restart_drops_old_runs() {
    let mut args = launch();
    args.shutdown_grace = Duration::from_secs(1);
    let mut kernel = spawn(args).await;
    let events = kernel.messages().expect("channel");
    let running = kernel
        // One write, so the marker never arrives split from its newline. The
        // run keeps printing, so the old kernel still sends output while the
        // restart stops it.
        .execute(
            "import sys, time\nsys.stdout.write('started\\n')\nsys.stdout.flush()\nwhile True:\n    print('tick', flush=True)\n    time.sleep(0.05)",
        )
        .await
        .expect("sends");
    recv_match(&events, Duration::from_secs(10), |event| {
        matches!(
            event,
            KernelEvent::Iopub(IopubMsg::Stream { parent: p, text, .. })
                if p == &running && text.contains("started")
        )
    });
    kernel
        .restart()
        .await
        .unwrap_or_else(|error| panic!("restarts: {error}"));
    // Ticks routed before the restart began may come first; skip them.
    recv_match(&events, Duration::from_secs(5), |event| {
        matches!(event, KernelEvent::Restarted { .. })
    });
    // Anything of the old generation that still passed would show up here.
    let later = drain(&events, Duration::from_millis(2500));
    assert!(
        !later.iter().any(|event| reports_on(event, &running)),
        "the old run reports after the restart: {later:?}"
    );
    let pid = kernel.process_id().expect("running");
    kernel.shutdown().await.expect("shutdown");
    assert_gone_now(pid);
}

#[ignore]
#[tokio::test(flavor = "multi_thread")]
async fn restart_stuck_kernel() {
    let mut args = launch();
    args.request_timeout = Duration::from_secs(30);
    args.shutdown_grace = Duration::from_secs(1);
    let mut kernel = spawn(args).await;
    let events = kernel.messages().expect("channel");
    let old_pid = kernel.process_id().expect("running");
    let old_file = kernel.connection_file().expect("file");
    // A stopped kernel answers neither the request nor the shutdown request.
    signal(old_pid, libc::SIGSTOP);
    let waiting = kernel.kernel_info();
    // Let the owner send the request before the restart begins.
    tokio::time::sleep(Duration::from_millis(200)).await;
    let start = Instant::now();
    let info = kernel
        .restart()
        .await
        .unwrap_or_else(|error| panic!("restarts: {error}"));
    let waited = waiting.await;
    assert!(
        matches!(waited, Err(KernelError::Restarting)),
        "the waiting request fails with Restarting, got {waited:?}"
    );
    let took = start.elapsed();
    assert!(
        took >= Duration::from_millis(900) && took < Duration::from_secs(20),
        "restart took {took:?} with a one second grace"
    );
    assert_eq!(info.language.name, "python");
    assert_ne!(kernel.process_id(), Some(old_pid));
    assert_gone_now(old_pid);
    assert!(!old_file.exists());
    let drained = drain(&events, Duration::from_millis(500));
    assert_eq!(
        lifecycle_counts(&drained),
        (0, 1),
        "one Restarted and no Died"
    );
    kernel.kernel_info().await.expect("the new kernel answers");
    let pid = kernel.process_id().expect("running");
    let file = kernel.connection_file().expect("file");
    kernel.shutdown().await.expect("shutdown");
    assert_gone_now(pid);
    assert!(!file.exists());
}

#[ignore]
#[tokio::test(flavor = "multi_thread")]
async fn shutdown_during_restart() {
    let before = connection_files();
    let dir = tempfile::tempdir().expect("temp dir");
    let kernel = spawn(KernelLaunch::new(slow_python(dir.path()), "python3")).await;
    let old_pid = kernel.process_id().expect("running");
    let old_file = kernel.connection_file().expect("file");
    let kernel = Arc::new(kernel);
    let restarting = Arc::clone(&kernel);
    let restarted = tokio::spawn(async move { restarting.restart().await });
    let (new_pid, new_file) = wait_for_new_child(&kernel, old_pid).await;
    kernel.shutdown().await.expect("shutdown");
    let restart_result = restarted.await.expect("joins");
    assert!(
        matches!(restart_result, Err(KernelError::Closed)),
        "restart is aborted, got {restart_result:?}"
    );
    assert_gone_now(old_pid);
    assert_gone_now(new_pid);
    assert!(!old_file.exists());
    assert!(!new_file.exists());
    assert!(kernel.process_id().is_none());
    assert!(kernel.connection_file().is_none());
    assert_eq!(connection_files(), before);
}

#[ignore]
#[tokio::test(flavor = "multi_thread")]
async fn shutdown_while_stopping() {
    let before = connection_files();
    let mut args = launch();
    args.shutdown_grace = Duration::from_secs(30);
    let kernel = spawn(args).await;
    let old_pid = kernel.process_id().expect("running");
    let old_file = kernel.connection_file().expect("file");
    // A stopped kernel ignores the shutdown request, so the restart stays in
    // its stop phase for the whole grace period.
    signal(old_pid, libc::SIGSTOP);
    let kernel = Arc::new(kernel);
    let restarting = Arc::clone(&kernel);
    let restarted = tokio::spawn(async move { restarting.restart().await });
    let start = Instant::now();
    while !matches!(kernel.execute("1").await, Err(KernelError::Restarting)) {
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "restart never began"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let start = Instant::now();
    kernel.shutdown().await.expect("shutdown");
    assert!(
        start.elapsed() < Duration::from_secs(10),
        "shutdown waited out the grace period"
    );
    let restart_result = restarted.await.expect("joins");
    assert!(
        matches!(restart_result, Err(KernelError::Closed)),
        "restart is aborted, got {restart_result:?}"
    );
    assert_gone_now(old_pid);
    assert!(!old_file.exists());
    assert!(kernel.process_id().is_none());
    assert_eq!(connection_files(), before);
}

#[ignore]
#[test]
fn drop_during_restart() {
    let before = connection_files();
    let dir = tempfile::tempdir().expect("temp dir");
    let runtime = idle_runtime();
    let kernel = runtime.block_on(spawn(KernelLaunch::new(slow_python(dir.path()), "python3")));
    let old_pid = kernel.process_id().expect("running");
    // The restart is queued at the call, so its future is not needed.
    drop(kernel.restart());
    let (new_pid, new_file) = runtime.block_on(async {
        let child = wait_for_new_child(&kernel, old_pid).await;
        assert!(matches!(
            kernel.execute("1").await,
            Err(KernelError::Restarting)
        ));
        child
    });
    // The restart task now sits in its handshake, and nothing drives it, so
    // only `Drop` itself can stop the new child.
    drop(kernel);
    assert!(!new_file.exists());
    assert_gone(new_pid);
    assert_gone(old_pid);
    drop(runtime);
    assert_eq!(connection_files(), before);
}

#[ignore]
#[tokio::test(flavor = "multi_thread")]
async fn spawn_dropped_during_handshake() {
    let before = connection_files();
    let dir = tempfile::tempdir().expect("temp dir");
    let pid_file = dir.path().join("pid");
    let slow = script(
        dir.path(),
        "pid-python",
        &format!(
            "echo $$ > '{}'\nsleep 1\nexec '{}' \"$@\"",
            pid_file.display(),
            python().display()
        ),
    );
    let wrote_pid = async {
        loop {
            let text = std::fs::read_to_string(&pid_file).unwrap_or_default();
            if let Some(pid) = text.strip_suffix('\n').and_then(|pid| pid.parse().ok()) {
                return pid;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    };
    // The script sleeps after writing its pid, so the spawn is still in its
    // handshake when the pid wins and the spawn future is dropped.
    let pid: u32 = tokio::select! {
        spawned = LocalKernel::spawn(KernelLaunch::new(&slow, "python3")) => {
            panic!("the spawn finished first: {:?}", spawned.map(|_| ()))
        }
        pid = tokio::time::timeout(Duration::from_secs(10), wrote_pid) => {
            pid.expect("the script writes its pid")
        }
    };
    assert_gone(pid);
    assert_eq!(connection_files(), before);
}

#[ignore]
#[tokio::test(flavor = "multi_thread")]
async fn death_and_recover() {
    let mut kernel = spawn(launch()).await;
    let events = kernel.messages().expect("channel");
    let pid = kernel.process_id().expect("running");
    let file = kernel.connection_file().expect("file");
    // A stopped kernel cannot answer, so the request still waits when the
    // kernel dies.
    signal(pid, libc::SIGSTOP);
    let waiting = kernel.kernel_info();
    tokio::time::sleep(Duration::from_millis(200)).await;
    signal(pid, libc::SIGKILL);
    let start = Instant::now();
    let waited = waiting.await;
    assert!(
        matches!(waited, Err(KernelError::Dead { .. })),
        "the waiting request fails with Dead, got {waited:?}"
    );
    assert!(
        start.elapsed() < Duration::from_secs(5),
        "the waiting request outlived the death"
    );
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
    assert_gone_now(pid);
    assert!(!file.exists());
    assert_eq!(
        lifecycle_counts(&drain(&events, Duration::from_millis(500))),
        (0, 0),
        "exactly one Died"
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
    signal(pid, libc::SIGKILL);
    recv_match(&events, Duration::from_secs(10), |event| {
        matches!(event, KernelEvent::Died { .. })
    });
    kernel.shutdown().await.expect("shutdown after death");
}

#[ignore]
#[tokio::test(flavor = "multi_thread")]
async fn restart_failure() {
    let before = connection_files();
    let dir = tempfile::tempdir().expect("temp dir");
    let flag = dir.path().join("fail");
    let wrapper = script(
        dir.path(),
        "python-wrapper",
        &format!(
            "if [ -e '{}' ]; then exit 3; fi\nexec '{}' \"$@\"",
            flag.display(),
            python().display()
        ),
    );
    let mut kernel = spawn(KernelLaunch::new(&wrapper, "python3")).await;
    let events = kernel.messages().expect("channel");
    let old_pid = kernel.process_id().expect("running");
    let old_file = kernel.connection_file().expect("file");

    // The new child starts and exits before its handshake.
    std::fs::write(&flag, "").expect("writes flag");
    let error = kernel.restart().await.expect_err("restart fails");
    assert!(
        matches!(
            error,
            KernelError::Spawn(SpawnError {
                kind: SpawnErrorKind::EarlyExit {
                    exit: ProcessExit::Code(3)
                },
                ..
            })
        ),
        "got {error:?}"
    );
    assert_gone_now(old_pid);
    assert!(!old_file.exists());
    assert!(
        kernel.process_id().is_none(),
        "no child after failed restart"
    );
    assert!(matches!(
        kernel.execute("1").await,
        Err(KernelError::Dead { .. })
    ));

    // The interpreter is gone, so restarting from dead fails before a child
    // starts.
    std::fs::remove_file(&wrapper).expect("removes wrapper");
    let error = kernel.restart().await.expect_err("restart fails again");
    assert!(
        matches!(
            error,
            KernelError::Spawn(SpawnError {
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
    assert_eq!(
        lifecycle_counts(&drain(&events, Duration::from_millis(500))),
        (0, 0),
        "failed restarts emit neither Died nor Restarted"
    );
    kernel.shutdown().await.expect("shutdown");
    assert_eq!(connection_files(), before);
}

#[ignore]
#[tokio::test(flavor = "multi_thread")]
async fn kill_all() {
    let mut first = spawn(launch()).await;
    let mut second = spawn(launch()).await;
    let _ = first.messages();
    let _ = second.messages();
    let replaced = first.process_id().expect("running");
    // A restarted generation must be registered as well.
    first
        .restart()
        .await
        .unwrap_or_else(|error| panic!("restarts: {error}"));
    let first_pid = first.process_id().expect("running");
    assert_ne!(first_pid, replaced);
    let second_pid = second.process_id().expect("running");
    let first_file = first.connection_file().expect("file");
    let second_file = second.connection_file().expect("file");
    kill_all_kernels();
    // Checked at once, before the owners notice the deaths and clean up.
    assert!(!first_file.exists());
    assert!(!second_file.exists());
    assert_gone(first_pid);
    assert_gone(second_pid);
}

#[ignore]
#[tokio::test(flavor = "multi_thread")]
async fn request_timeout() {
    let mut args = launch();
    args.request_timeout = Duration::from_secs(1);
    args.shutdown_grace = Duration::from_secs(1);

    let mut kernel = spawn(args.clone()).await;
    let events = kernel.messages().expect("channel");
    let pid = kernel.process_id().expect("running");
    signal(pid, libc::SIGSTOP);
    let start = Instant::now();
    assert!(matches!(
        kernel.kernel_info().await,
        Err(KernelError::Timeout {
            op: KernelOp::KernelInfo
        })
    ));
    let waited = start.elapsed();
    assert!(
        waited >= Duration::from_millis(900) && waited < Duration::from_secs(5),
        "info waited {waited:?}"
    );
    signal(pid, libc::SIGCONT);
    // The resumed kernel first answers the request that already timed out,
    // which can take longer than the one-second timeout on a slow runner.
    let resumed = Instant::now();
    loop {
        match kernel.kernel_info().await {
            Ok(_) => break,
            Err(KernelError::Timeout { .. }) if resumed.elapsed() < Duration::from_secs(10) => {}
            Err(error) => panic!("info after resume: {error:?}"),
        }
    }
    assert_eq!(
        lifecycle_counts(&drain(&events, Duration::from_millis(500))),
        (0, 0),
        "a timeout is no death"
    );
    let file = kernel.connection_file().expect("file");
    kernel.shutdown().await.expect("shutdown");
    assert_gone_now(pid);
    assert!(!file.exists());

    // This kernel never resumes, so the interrupt it receives is never
    // handled, and shutdown has to kill it after the grace period.
    let stuck = spawn(args).await;
    let stuck_pid = stuck.process_id().expect("running");
    let stuck_file = stuck.connection_file().expect("file");
    signal(stuck_pid, libc::SIGSTOP);
    let start = Instant::now();
    assert!(matches!(
        stuck.interrupt().await,
        Err(KernelError::Timeout {
            op: KernelOp::Interrupt
        })
    ));
    let waited = start.elapsed();
    assert!(
        waited >= Duration::from_millis(900) && waited < Duration::from_secs(5),
        "interrupt waited {waited:?}"
    );
    // A request still waiting when the kernel closes ends either way, by its
    // own deadline or by the close.
    let pending = stuck.kernel_info();
    let start = Instant::now();
    stuck.shutdown().await.expect("forced shutdown");
    let waited = start.elapsed();
    assert!(
        waited >= Duration::from_millis(900) && waited < Duration::from_secs(10),
        "shutdown took {waited:?}"
    );
    let pending = pending.await;
    assert!(
        matches!(
            pending,
            Err(KernelError::Closed
                | KernelError::Timeout {
                    op: KernelOp::KernelInfo
                })
        ),
        "a request waiting at close fails with Closed or Timeout, got {pending:?}"
    );
    assert_gone_now(stuck_pid);
    assert!(!stuck_file.exists());
}

/// Cleans up after a kernel whose parent was killed, even when the test
/// fails.
struct OrphanCleanup {
    /// Kernel group to kill, until the test has seen the kernel end.
    pid: Option<u32>,
    /// Connection file that nobody else is left to remove.
    file: PathBuf,
}

impl Drop for OrphanCleanup {
    fn drop(&mut self) {
        if let Some(pid) = self.pid {
            // SAFETY: `killpg` only sends the signal to that group.
            unsafe { libc::killpg(pid as i32, libc::SIGKILL) };
        }
        let _ = std::fs::remove_file(&self.file);
    }
}

/// Kills and reaps a child process when dropped.
struct KillOnDrop(std::process::Child);

impl Drop for KillOnDrop {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
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
    let mut child = KillOnDrop(command.spawn().expect("spawns child"));
    let stdout = child.0.stdout.take().expect("piped");
    let mut kernel_file = None;
    let mut kernel_pid = None;
    for line in BufReader::new(stdout).lines() {
        let line = line.expect("reads");
        // libtest may print the test name in front of our text on one line.
        if let Some((_, file)) = line.split_once("kernel-file=") {
            kernel_file = Some(PathBuf::from(file.trim()));
        }
        if let Some((_, pid)) = line.split_once("kernel-pid=") {
            kernel_pid = Some(pid.trim().parse::<u32>().expect("number"));
            break;
        }
    }
    let kernel_pid = kernel_pid.expect("pid line");
    let mut orphan = OrphanCleanup {
        pid: Some(kernel_pid),
        file: kernel_file.expect("file line"),
    };
    signal(child.0.id(), libc::SIGKILL);
    let _ = child.0.wait();
    let start = Instant::now();
    while !is_gone(kernel_pid) {
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "kernel {kernel_pid} survives parent SIGKILL"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    orphan.pid = None;
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
    let kernel = spawn(launch()).await;
    let pid = kernel.process_id().expect("running");
    let file = kernel.connection_file().expect("file");
    println!("kernel-file={}", file.display());
    println!("kernel-pid={pid}");
    std::io::stdout().flush().expect("flush");
    tokio::time::sleep(Duration::from_secs(60)).await;
    drop(kernel);
}
