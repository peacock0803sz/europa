//! Snapshot tests for kernel error messages.

use std::time::Duration;

use europa_core::{KernelError, KernelOp, ProcessExit, SpawnError, SpawnErrorKind};

fn spawn(kind: SpawnErrorKind, tail: &str) -> SpawnError {
    SpawnError {
        kind,
        python: "/opt/venv/bin/python".into(),
        stderr_tail: tail.to_owned(),
    }
}

#[test]
fn snapshots_spawn_error_messages() {
    let short_tail = "line1\nline2\nline3\n";
    insta::assert_snapshot!(
        "spawn_interpreter_not_found",
        spawn(SpawnErrorKind::InterpreterNotFound, "").to_string()
    );
    insta::assert_snapshot!(
        "spawn_ipykernel_missing",
        spawn(SpawnErrorKind::IpykernelMissing, short_tail).to_string()
    );
    insta::assert_snapshot!(
        "spawn_early_exit",
        spawn(
            SpawnErrorKind::EarlyExit {
                exit: ProcessExit::Code(1),
            },
            short_tail
        )
        .to_string()
    );
    insta::assert_snapshot!(
        "spawn_handshake_timeout",
        spawn(
            SpawnErrorKind::HandshakeTimeout {
                after: Duration::from_secs(30),
            },
            ""
        )
        .to_string()
    );
    insta::assert_snapshot!(
        "spawn_connect_failed",
        spawn(
            SpawnErrorKind::ConnectFailed {
                message: "refused".to_owned(),
            },
            ""
        )
        .to_string()
    );
}

#[test]
fn snapshots_long_stderr_tail() {
    let lines: Vec<String> = (1..=25).map(|n| format!("line{n}")).collect();
    let tail = format!("{}\n", lines.join("\n"));
    let text = spawn(
        SpawnErrorKind::EarlyExit {
            exit: ProcessExit::Code(1),
        },
        &tail,
    )
    .to_string();
    insta::assert_snapshot!("spawn_early_exit_long", text);
    assert!(text.contains("line6\n"), "{text}");
    assert!(text.contains("line25"), "{text}");
    assert!(!text.contains("line1\n"), "{text}");
    assert!(!text.contains("line5\n"), "{text}");
}

#[test]
fn states_next_action_in_message() {
    let missing = spawn(SpawnErrorKind::InterpreterNotFound, "").to_string();
    assert!(missing.contains("/opt/venv/bin/python"), "{missing}");
    assert!(missing.contains("check the path"), "{missing}");
    let ipykernel = spawn(SpawnErrorKind::IpykernelMissing, "").to_string();
    assert!(ipykernel.contains("/opt/venv/bin/python"), "{ipykernel}");
    assert!(
        ipykernel.contains("-m pip install ipykernel"),
        "{ipykernel}"
    );
}

#[test]
fn snapshots_kernel_error_messages() {
    insta::assert_snapshot!(
        "kernel_error_spawn",
        KernelError::Spawn(spawn(SpawnErrorKind::InterpreterNotFound, "")).to_string()
    );
    insta::assert_snapshot!(
        "kernel_error_dead",
        KernelError::Dead {
            exit: ProcessExit::Signal(9),
        }
        .to_string()
    );
    insta::assert_snapshot!(
        "kernel_error_restarting",
        KernelError::Restarting.to_string()
    );
    insta::assert_snapshot!(
        "kernel_error_timeout",
        KernelError::Timeout {
            op: KernelOp::KernelInfo,
        }
        .to_string()
    );
    insta::assert_snapshot!("kernel_error_closed", KernelError::Closed.to_string());
    insta::assert_snapshot!(
        "kernel_error_io",
        KernelError::Io {
            message: "broken".to_owned(),
        }
        .to_string()
    );
}
