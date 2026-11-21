//! Round-trip, mapping, and state table tests for shared kernel types.

use std::time::Duration;

use europa_core::{
    admit, next_state, Admission, ExecEvent, IopubMsg, JsonMap, KernelError, KernelEvent,
    KernelInfo, KernelLaunch, KernelOp, KernelState, KernelStatus, KernelTransition, LanguageInfo,
    MsgId, ProcessExit, ReplyStatus, SpawnError, SpawnErrorKind, WarningReason,
};

fn sample_info() -> KernelInfo {
    KernelInfo {
        protocol_version: "5.3".to_owned(),
        implementation: "ipython".to_owned(),
        implementation_version: "9.7.0".to_owned(),
        language: LanguageInfo {
            name: "python".to_owned(),
            version: "3.12.0".to_owned(),
            file_extension: Some("py".to_owned()),
        },
        banner: String::new(),
    }
}

fn round_trip<T>(value: &T)
where
    T: serde::Serialize + serde::de::DeserializeOwned + PartialEq + std::fmt::Debug,
{
    let text = serde_json::to_string(value).expect("serializes");
    assert_eq!(
        &serde_json::from_str::<T>(&text).expect("loads"),
        value,
        "{text}"
    );
}

#[test]
fn round_trips_kernel_events() {
    let info = sample_info();
    round_trip(&KernelEvent::Status {
        parent: Some(MsgId::new("p1")),
        state: KernelStatus::Idle,
    });
    round_trip(&KernelEvent::Iopub(IopubMsg::Stream {
        parent: MsgId::new("p1"),
        name: "stdout".to_owned(),
        text: "hi\n".to_owned(),
    }));
    round_trip(&KernelEvent::Iopub(IopubMsg::DisplayData {
        parent: MsgId::new("p1"),
        data: JsonMap::from([("text/plain".to_owned(), "x".into())]),
        metadata: JsonMap::new(),
        display_id: None,
    }));
    round_trip(&KernelEvent::ExecuteReply {
        parent: MsgId::new("p1"),
        status: ReplyStatus::Ok,
        execution_count: Some(1),
    });
    round_trip(&KernelEvent::Died {
        exit: ProcessExit::Code(1),
        stderr_tail: "oops".to_owned(),
    });
    round_trip(&KernelEvent::Restarted { info });
    round_trip(&KernelEvent::Warning {
        reason: WarningReason::Malformed,
        msg_type: Some("stream".to_owned()),
        detail: "bad".to_owned(),
    });
}

#[test]
fn round_trips_info_launch_errors() {
    round_trip(&sample_info());
    round_trip(&KernelLaunch::new("/usr/bin/python3", ""));
    for error in [
        KernelError::Spawn(SpawnError {
            kind: SpawnErrorKind::InterpreterNotFound,
            python: "/opt/venv/bin/python".into(),
            stderr_tail: String::new(),
        }),
        KernelError::Dead {
            exit: ProcessExit::Signal(9),
        },
        KernelError::Restarting,
        KernelError::Timeout {
            op: KernelOp::KernelInfo,
        },
        KernelError::Closed,
        KernelError::Io {
            message: "broken".to_owned(),
        },
    ] {
        round_trip(&error);
    }
    for kind in [
        SpawnErrorKind::InterpreterNotFound,
        SpawnErrorKind::IpykernelMissing,
        SpawnErrorKind::EarlyExit {
            exit: ProcessExit::Code(1),
        },
        SpawnErrorKind::HandshakeTimeout {
            after: Duration::from_secs(30),
        },
        SpawnErrorKind::ConnectFailed {
            message: "refused".to_owned(),
        },
    ] {
        round_trip(&SpawnError {
            kind,
            python: "/opt/venv/bin/python".into(),
            stderr_tail: String::new(),
        });
    }
}

#[test]
fn maps_status_idle_and_reply_to_exec_events() {
    let parent = MsgId::new("p1");
    assert_eq!(
        KernelEvent::Status {
            parent: Some(parent.clone()),
            state: KernelStatus::Idle,
        }
        .exec_event(),
        Some(ExecEvent::Idle {
            parent: parent.clone()
        })
    );
    assert_eq!(
        KernelEvent::ExecuteReply {
            parent: parent.clone(),
            status: ReplyStatus::Ok,
            execution_count: Some(1),
        }
        .exec_event(),
        Some(ExecEvent::Reply {
            parent: parent.clone(),
            status: ReplyStatus::Ok,
        })
    );
    assert_eq!(
        KernelEvent::ExecuteReply {
            parent: parent.clone(),
            status: ReplyStatus::Error,
            execution_count: None,
        }
        .exec_event(),
        Some(ExecEvent::Reply {
            parent,
            status: ReplyStatus::Error,
        })
    );
}

#[test]
fn maps_nothing_else_to_exec_events() {
    for event in [
        KernelEvent::Status {
            parent: Some(MsgId::new("p1")),
            state: KernelStatus::Busy,
        },
        KernelEvent::Status {
            parent: Some(MsgId::new("p1")),
            state: KernelStatus::Starting,
        },
        KernelEvent::Status {
            parent: None,
            state: KernelStatus::Idle,
        },
        KernelEvent::Iopub(IopubMsg::Stream {
            parent: MsgId::new("p1"),
            name: "stdout".to_owned(),
            text: "hi".to_owned(),
        }),
        KernelEvent::Died {
            exit: ProcessExit::Code(1),
            stderr_tail: String::new(),
        },
        KernelEvent::Restarted {
            info: sample_info(),
        },
        KernelEvent::Warning {
            reason: WarningReason::BadSignature,
            msg_type: None,
            detail: "bad".to_owned(),
        },
    ] {
        assert_eq!(event.exec_event(), None);
    }
}

#[test]
fn launch_new_fills_defaults() {
    let launch = KernelLaunch::new("/usr/bin/python3", "");
    assert_eq!(launch.kernel_name, "python3");
    assert_eq!(launch.python.to_str().expect("path"), "/usr/bin/python3");
    assert_eq!(launch.handshake_timeout, Duration::from_secs(30));
    assert_eq!(launch.request_timeout, Duration::from_secs(10));
    assert_eq!(launch.shutdown_grace, Duration::from_secs(5));
    assert_eq!(launch.port_retries, 3);
}

#[test]
fn admits_each_state_and_operation() {
    let dead_exit = ProcessExit::Signal(9);
    let dead = KernelState::Dead { exit: dead_exit };
    let cases: Vec<(KernelState, KernelOp, Admission)> = vec![
        (KernelState::Ready, KernelOp::Execute, Admission::Send),
        (KernelState::Ready, KernelOp::KernelInfo, Admission::Send),
        (KernelState::Ready, KernelOp::Interrupt, Admission::Send),
        (
            KernelState::Ready,
            KernelOp::Restart,
            Admission::StopThenRestart,
        ),
        (
            KernelState::Ready,
            KernelOp::Shutdown,
            Admission::StopAndClose,
        ),
        (
            KernelState::Restarting,
            KernelOp::Execute,
            Admission::Reject(KernelError::Restarting),
        ),
        (
            KernelState::Restarting,
            KernelOp::KernelInfo,
            Admission::Reject(KernelError::Restarting),
        ),
        (
            KernelState::Restarting,
            KernelOp::Interrupt,
            Admission::Reject(KernelError::Restarting),
        ),
        (
            KernelState::Restarting,
            KernelOp::Restart,
            Admission::Reject(KernelError::Restarting),
        ),
        (
            KernelState::Restarting,
            KernelOp::Shutdown,
            Admission::AbortRestartAndStop,
        ),
        (
            dead.clone(),
            KernelOp::Execute,
            Admission::Reject(KernelError::Dead { exit: dead_exit }),
        ),
        (
            dead.clone(),
            KernelOp::KernelInfo,
            Admission::Reject(KernelError::Dead { exit: dead_exit }),
        ),
        (
            dead.clone(),
            KernelOp::Interrupt,
            Admission::Reject(KernelError::Dead { exit: dead_exit }),
        ),
        (dead.clone(), KernelOp::Restart, Admission::RestartFromDead),
        (dead.clone(), KernelOp::Shutdown, Admission::CleanUpDead),
        (
            KernelState::Closed,
            KernelOp::Execute,
            Admission::Reject(KernelError::Closed),
        ),
        (
            KernelState::Closed,
            KernelOp::KernelInfo,
            Admission::Reject(KernelError::Closed),
        ),
        (
            KernelState::Closed,
            KernelOp::Interrupt,
            Admission::Reject(KernelError::Closed),
        ),
        (
            KernelState::Closed,
            KernelOp::Restart,
            Admission::Reject(KernelError::Closed),
        ),
        (
            KernelState::Closed,
            KernelOp::Shutdown,
            Admission::AlreadyClosed,
        ),
    ];
    assert_eq!(cases.len(), 20);
    for (state, op, expected) in cases {
        assert_eq!(admit(&state, op), expected, "{state:?} {op:?}");
    }
}

#[test]
fn moves_state_by_transition_table() {
    let dead_exit = ProcessExit::Signal(9);
    let new_exit = ProcessExit::Code(1);
    let dead = KernelState::Dead { exit: dead_exit };
    let cases: Vec<(KernelState, KernelTransition, KernelState)> = vec![
        (
            KernelState::Ready,
            KernelTransition::ChildExited { exit: new_exit },
            KernelState::Dead { exit: new_exit },
        ),
        (
            KernelState::Ready,
            KernelTransition::RestartStarted,
            KernelState::Restarting,
        ),
        (
            KernelState::Ready,
            KernelTransition::RestartSucceeded,
            KernelState::Ready,
        ),
        (
            KernelState::Ready,
            KernelTransition::RestartFailed,
            KernelState::Ready,
        ),
        (
            KernelState::Ready,
            KernelTransition::Closed,
            KernelState::Closed,
        ),
        (
            KernelState::Restarting,
            KernelTransition::ChildExited { exit: new_exit },
            KernelState::Restarting,
        ),
        (
            KernelState::Restarting,
            KernelTransition::RestartStarted,
            KernelState::Restarting,
        ),
        (
            KernelState::Restarting,
            KernelTransition::RestartSucceeded,
            KernelState::Ready,
        ),
        (
            KernelState::Restarting,
            KernelTransition::RestartFailed,
            KernelState::Dead {
                exit: ProcessExit::Unknown,
            },
        ),
        (
            KernelState::Restarting,
            KernelTransition::Closed,
            KernelState::Closed,
        ),
        (
            dead.clone(),
            KernelTransition::ChildExited { exit: new_exit },
            dead.clone(),
        ),
        (
            dead.clone(),
            KernelTransition::RestartStarted,
            KernelState::Restarting,
        ),
        (
            dead.clone(),
            KernelTransition::RestartSucceeded,
            dead.clone(),
        ),
        (dead.clone(), KernelTransition::RestartFailed, dead.clone()),
        (dead.clone(), KernelTransition::Closed, KernelState::Closed),
        (
            KernelState::Closed,
            KernelTransition::ChildExited { exit: new_exit },
            KernelState::Closed,
        ),
        (
            KernelState::Closed,
            KernelTransition::RestartStarted,
            KernelState::Closed,
        ),
        (
            KernelState::Closed,
            KernelTransition::RestartSucceeded,
            KernelState::Closed,
        ),
        (
            KernelState::Closed,
            KernelTransition::RestartFailed,
            KernelState::Closed,
        ),
        (
            KernelState::Closed,
            KernelTransition::Closed,
            KernelState::Closed,
        ),
    ];
    assert_eq!(cases.len(), 20);
    for (state, transition, expected) in cases {
        assert_eq!(next_state(&state, transition), expected, "{state:?}");
    }
}
