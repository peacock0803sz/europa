//! Kernel launch arguments, events, states, and errors.
//!
//! This module owns the data shared between the kernel adapter and its
//! callers. The adapter converts wire messages into [`KernelEvent`] and
//! reports state through [`KernelState`], while callers decide what to show.

use std::fmt;
use std::path::PathBuf;
use std::time::Duration;

use crate::{ExecEvent, IopubMsg, MsgId, ReplyStatus};

/// Arguments for spawning a local kernel.
///
/// `python` is the absolute path of the interpreter. `kernel_name` is only
/// recorded in the connection file, never resolved to a kernelspec.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct KernelLaunch {
    /// Python interpreter used to start the kernel.
    pub python: PathBuf,
    /// Kernel name recorded in the connection file.
    pub kernel_name: String,
    /// Upper bound for the handshake after spawning.
    pub handshake_timeout: Duration,
    /// Upper bound for waiting on a `kernel_info` or `interrupt` reply.
    pub request_timeout: Duration,
    /// Grace period between asking for shutdown and forcing it.
    pub shutdown_grace: Duration,
    /// How many times a port conflict is retried, after the first attempt.
    pub port_retries: u32,
}

impl KernelLaunch {
    /// Build launch arguments with defaults.
    ///
    /// Defaults are `handshake_timeout` 30 seconds, `request_timeout` 10
    /// seconds, `shutdown_grace` 5 seconds, and `port_retries` 3 times.
    /// An empty `kernel_name` becomes `python3`.
    #[must_use]
    pub fn new(python: impl Into<PathBuf>, kernel_name: impl Into<String>) -> Self {
        let name = kernel_name.into();
        Self {
            python: python.into(),
            kernel_name: if name.is_empty() {
                "python3".to_owned()
            } else {
                name
            },
            handshake_timeout: Duration::from_secs(30),
            request_timeout: Duration::from_secs(10),
            shutdown_grace: Duration::from_secs(5),
            port_retries: 3,
        }
    }
}

/// Self-description received during the handshake.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct KernelInfo {
    /// Protocol version reported by the kernel.
    pub protocol_version: String,
    /// Implementation name such as `ipython`.
    pub implementation: String,
    /// Implementation version.
    pub implementation_version: String,
    /// Language the kernel runs.
    pub language: LanguageInfo,
    /// Banner text shown at startup, empty when none.
    pub banner: String,
}

/// Language the kernel runs.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct LanguageInfo {
    /// Language name such as `python`.
    pub name: String,
    /// Language version.
    pub version: String,
    /// File extension without a leading dot, if known.
    pub file_extension: Option<String>,
}

/// Event observed from a kernel.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum KernelEvent {
    /// Execution state changed on the iopub channel.
    Status {
        /// Parent message id, if the status belongs to a request.
        parent: Option<MsgId>,
        /// New execution state.
        state: KernelStatus,
    },
    /// Output or execution count update from iopub.
    Iopub(IopubMsg),
    /// Reply to an `execute_request` on the shell channel.
    ExecuteReply {
        /// Parent message id of the request.
        parent: MsgId,
        /// Outcome of the execution.
        status: ReplyStatus,
        /// Execution count reported with the reply, if any.
        execution_count: Option<i64>,
    },
    /// The child process ended without being asked.
    Died {
        /// How the child ended.
        exit: ProcessExit,
        /// Tail of the standard error output.
        stderr_tail: String,
    },
    /// A restart finished and the handshake completed.
    Restarted {
        /// Self-description from the new kernel.
        info: KernelInfo,
    },
    /// A message that could not be delivered as data.
    Warning {
        /// Why the message was rejected.
        reason: WarningReason,
        /// Wire message type, if the header could be read.
        msg_type: Option<String>,
        /// What was wrong, for diagnostics.
        detail: String,
    },
}

impl KernelEvent {
    /// Map this event onto execution state, if it advances a run.
    ///
    /// Only `Status` with `Idle` and a parent, and `ExecuteReply`, map to an
    /// [`ExecEvent`]. Everything else returns `None`; iopub output goes
    /// through [`crate::apply_iopub`] instead.
    #[must_use]
    pub fn exec_event(&self) -> Option<ExecEvent> {
        match self {
            Self::Status {
                parent: Some(parent),
                state: KernelStatus::Idle,
            } => Some(ExecEvent::Idle {
                parent: parent.clone(),
            }),
            Self::ExecuteReply { parent, status, .. } => Some(ExecEvent::Reply {
                parent: parent.clone(),
                status: *status,
            }),
            _ => None,
        }
    }
}

/// Execution state reported on the iopub channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum KernelStatus {
    /// The kernel is starting.
    Starting,
    /// The kernel is running code.
    Busy,
    /// The kernel is waiting for input.
    Idle,
}

/// How a kernel child process ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ProcessExit {
    /// Exited with a status code.
    Code(i32),
    /// Killed by a signal number.
    Signal(i32),
    /// Neither a code nor a signal could be read.
    Unknown,
}

/// Why a wire message became a warning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum WarningReason {
    /// The HMAC signature did not match.
    BadSignature,
    /// The message type has no core representation.
    UnknownType,
    /// The message could not be parsed.
    Malformed,
}

/// Operation a caller asks the kernel to perform.
///
/// Only `KernelInfo` and `Interrupt` wait for a reply and can appear in
/// [`KernelError::Timeout`]. Restart waits are reported as
/// [`SpawnErrorKind::HandshakeTimeout`] and shutdown waits end by force.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum KernelOp {
    /// Run code.
    Execute,
    /// Ask for the kernel description.
    KernelInfo,
    /// Interrupt the running execution.
    Interrupt,
    /// Stop the kernel and start it again.
    Restart,
    /// Ask the kernel to exit.
    Shutdown,
}

/// Liveness of a kernel.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum KernelState {
    /// Running and accepting requests.
    Ready,
    /// A restart task is running; new work is rejected.
    Restarting,
    /// The child ended unexpectedly.
    Dead {
        /// How the child ended.
        exit: ProcessExit,
    },
    /// Shut down; only shutdown is accepted, as a no-op.
    Closed,
}

/// What the owner task should do with a request in a given state.
///
/// | State | `Execute`, `Interrupt`, `KernelInfo` | `Restart` | `Shutdown` |
/// |---|---|---|---|
/// | `Ready` | `Send` | `StopThenRestart` | `StopAndClose` |
/// | `Restarting` | `Reject(Restarting)` | `Reject(Restarting)` | `AbortRestartAndStop` |
/// | `Dead` | `Reject(Dead)` | `RestartFromDead` | `CleanUpDead` |
/// | `Closed` | `Reject(Closed)` | `Reject(Closed)` | `AlreadyClosed` |
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Admission {
    /// Send the request to the kernel.
    Send,
    /// Reject the request with this error.
    Reject(KernelError),
    /// Stop the old kernel and start a new one.
    StopThenRestart,
    /// Start a new kernel without stopping, from the dead state.
    RestartFromDead,
    /// Cancel the running restart and close.
    AbortRestartAndStop,
    /// Stop the kernel and close.
    StopAndClose,
    /// Move to closed after cleaning up the dead kernel.
    CleanUpDead,
    /// Already closed; shutdown stays a no-op.
    AlreadyClosed,
}

/// Decide what to do with an operation in a state.
///
/// Has no IO and returns the same result for the same input.
#[must_use]
pub fn admit(state: &KernelState, op: KernelOp) -> Admission {
    match (state, op) {
        (KernelState::Ready, KernelOp::Execute)
        | (KernelState::Ready, KernelOp::KernelInfo)
        | (KernelState::Ready, KernelOp::Interrupt) => Admission::Send,
        (KernelState::Ready, KernelOp::Restart) => Admission::StopThenRestart,
        (KernelState::Ready, KernelOp::Shutdown) => Admission::StopAndClose,
        (KernelState::Restarting, KernelOp::Shutdown) => Admission::AbortRestartAndStop,
        (KernelState::Restarting, _) => Admission::Reject(KernelError::Restarting),
        (KernelState::Dead { exit: _ }, KernelOp::Restart) => Admission::RestartFromDead,
        (KernelState::Dead { .. }, KernelOp::Shutdown) => Admission::CleanUpDead,
        (KernelState::Dead { exit }, _) => Admission::Reject(KernelError::Dead { exit: *exit }),
        (KernelState::Closed, KernelOp::Shutdown) => Admission::AlreadyClosed,
        (KernelState::Closed, _) => Admission::Reject(KernelError::Closed),
    }
}

/// Transition of kernel liveness.
///
/// `ChildExited` in `Restarting` is part of stopping and leaves the state
/// unchanged. `Closed` from any state closes the kernel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum KernelTransition {
    /// The child process ended.
    ChildExited {
        /// How the child ended.
        exit: ProcessExit,
    },
    /// A restart began.
    RestartStarted,
    /// A restart finished its handshake.
    RestartSucceeded,
    /// A restart failed.
    RestartFailed,
    /// The kernel was shut down.
    Closed,
}

/// Move liveness forward by one transition.
///
/// Has no IO and returns the same result for the same input. Combinations
/// outside the table leave the state unchanged: `RestartSucceeded` and
/// `RestartFailed` only apply in `Restarting`, `ChildExited` only maps
/// `Ready` to `Dead`, and `RestartStarted` only applies in `Ready` and `Dead`.
#[must_use]
pub fn next_state(state: &KernelState, transition: KernelTransition) -> KernelState {
    match (state, transition) {
        (_, KernelTransition::Closed) => KernelState::Closed,
        (KernelState::Ready, KernelTransition::ChildExited { exit }) => KernelState::Dead { exit },
        (KernelState::Ready, KernelTransition::RestartStarted)
        | (KernelState::Dead { .. }, KernelTransition::RestartStarted) => KernelState::Restarting,
        (KernelState::Restarting, KernelTransition::RestartSucceeded) => KernelState::Ready,
        (KernelState::Restarting, KernelTransition::RestartFailed) => KernelState::Dead {
            exit: ProcessExit::Unknown,
        },
        _ => state.clone(),
    }
}

/// Failure of an operation after the kernel was spawned.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum KernelError {
    /// A restart failed while starting the new kernel.
    Spawn(SpawnError),
    /// The kernel is dead; the exit tells how it ended.
    Dead {
        /// How the child ended.
        exit: ProcessExit,
    },
    /// A restart is running; no request was queued.
    Restarting,
    /// A waited reply did not arrive in time.
    Timeout {
        /// Operation that timed out.
        op: KernelOp,
    },
    /// The kernel is closed.
    Closed,
    /// Sending on a socket failed.
    Io {
        /// What failed.
        message: String,
    },
}

impl fmt::Display for KernelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Spawn(error) => write!(f, "{error}"),
            Self::Dead { exit } => write!(f, "Kernel is dead ({}).", exit_text(*exit)),
            Self::Restarting => write!(f, "Kernel is restarting."),
            Self::Timeout { op } => write!(f, "Timed out waiting for {}.", op_name(*op)),
            Self::Closed => write!(f, "Kernel is closed."),
            Self::Io { message } => write!(f, "Kernel I/O failed: {message}."),
        }
    }
}

impl std::error::Error for KernelError {}

/// Failure to spawn a kernel.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SpawnError {
    /// What kind of failure this is.
    pub kind: SpawnErrorKind,
    /// Interpreter path that was used.
    pub python: PathBuf,
    /// Tail of the child standard error output, empty when never started.
    pub stderr_tail: String,
}

/// Why spawning a kernel failed.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SpawnErrorKind {
    /// The interpreter could not be started.
    InterpreterNotFound,
    /// The child ended because `ipykernel` is missing.
    IpykernelMissing,
    /// The child ended before the handshake.
    EarlyExit {
        /// How the child ended.
        exit: ProcessExit,
    },
    /// The handshake did not finish in time.
    HandshakeTimeout {
        /// How long the handshake waited.
        after: Duration,
    },
    /// Connecting to the kernel failed.
    ConnectFailed {
        /// What failed.
        message: String,
    },
}

impl fmt::Display for SpawnError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let python = self.python.display();
        match &self.kind {
            SpawnErrorKind::InterpreterNotFound => write!(
                f,
                "Failed to spawn kernel with {python}: interpreter not found; check the path or pass an absolute path to a Python interpreter.{}",
                stderr_section(&self.stderr_tail)
            ),
            SpawnErrorKind::IpykernelMissing => write!(
                f,
                "Failed to spawn kernel with {python}: ipykernel is missing. Install it with `{python} -m pip install ipykernel`; Europa does not install packages.{}",
                stderr_section(&self.stderr_tail)
            ),
            SpawnErrorKind::EarlyExit { exit } => write!(
                f,
                "Failed to spawn kernel with {python}: process {} before handshake. Check the Python version and that ipykernel imports.{}",
                exit_text(*exit),
                stderr_section(&self.stderr_tail)
            ),
            SpawnErrorKind::HandshakeTimeout { after } => write!(
                f,
                "Failed to spawn kernel with {python}: handshake timed out after {}s. The kernel may be slow to start or blocked by a firewall on 127.0.0.1.{}",
                after.as_secs(),
                stderr_section(&self.stderr_tail)
            ),
            SpawnErrorKind::ConnectFailed { message } => write!(
                f,
                "Failed to spawn kernel with {python}: connection failed: {message}.{}",
                stderr_section(&self.stderr_tail)
            ),
        }
    }
}

/// Short human text for how a child ended.
fn exit_text(exit: ProcessExit) -> String {
    match exit {
        ProcessExit::Code(code) => format!("exited with code {code}"),
        ProcessExit::Signal(signal) => format!("killed by signal {signal}"),
        ProcessExit::Unknown => "exited with unknown status".to_owned(),
    }
}

/// Human name of a kernel operation.
fn op_name(op: KernelOp) -> &'static str {
    match op {
        KernelOp::Execute => "execute",
        KernelOp::KernelInfo => "kernel info",
        KernelOp::Interrupt => "interrupt",
        KernelOp::Restart => "restart",
        KernelOp::Shutdown => "shutdown",
    }
}

/// Last 20 lines of stderr, prefixed for display.
fn stderr_section(tail: &str) -> String {
    if tail.is_empty() {
        return String::new();
    }
    let lines: Vec<&str> = tail.lines().collect();
    let start = lines.len().saturating_sub(20);
    let mut out = String::from("\n\nstderr:\n");
    out.push_str(&lines[start..].join("\n"));
    if tail.ends_with('\n') {
        out.push('\n');
    }
    out
}

impl std::error::Error for SpawnError {}
