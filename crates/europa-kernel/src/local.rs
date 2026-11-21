//! Local kernel behind the client window.
//!
//! One spawned task owns the send halves, the child watcher, and the
//! [`KernelState`]. Receive halves live in dedicated tasks that forward to
//! the owner, so a slow channel never blocks the others behind one lock.
//! Every command passes [`admit`] first and moves state only through
//! [`next_state`].

use std::collections::HashMap;
use std::future::Future;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use europa_core::{
    admit, next_state, Admission, KernelError, KernelEvent, KernelInfo, KernelLaunch, KernelOp,
    KernelState, KernelTransition, MsgId, ProcessExit, SpawnError, SpawnErrorKind,
};
use tokio::sync::{mpsc, oneshot, Notify};
use zeromq::{SocketRecv, SocketSend};

use crate::connection::{new_key, pick_ports, ConnectionFile};
use crate::convert::{convert, warning, Channel, Converted};
use crate::handshake::{handshake, Sockets};
use crate::process::{signal_group, Watched};
use crate::wire::{decode, encode, header, new_id, WireMessage};
use crate::KernelClient;

/// Tell a missing `ipykernel` apart from any other early exit.
pub(crate) fn classify(early: ProcessExit, stderr_tail: &str) -> SpawnErrorKind {
    if stderr_tail.contains("No module named ipykernel_launcher")
        || stderr_tail.contains("No module named 'ipykernel'")
    {
        SpawnErrorKind::IpykernelMissing
    } else {
        SpawnErrorKind::EarlyExit { exit: early }
    }
}

/// Whether a failure is worth one more spawn with fresh ports.
pub(crate) fn should_retry(kind: &SpawnErrorKind, stderr_tail: &str) -> bool {
    match kind {
        SpawnErrorKind::EarlyExit { .. } => stderr_tail.contains("Address already in use"),
        SpawnErrorKind::HandshakeTimeout { .. } => true,
        _ => false,
    }
}

/// Work for the owner task.
enum Command {
    /// Answer with the kernel description.
    KernelInfo {
        /// Where the answer goes.
        reply: oneshot::Sender<Result<KernelInfo, KernelError>>,
    },
    /// Run code and answer with the request id.
    Execute {
        /// Code to run.
        code: String,
        /// Where the request id goes.
        reply: oneshot::Sender<Result<MsgId, KernelError>>,
    },
    /// Interrupt the running execution.
    Interrupt {
        /// Where the answer goes.
        reply: oneshot::Sender<Result<(), KernelError>>,
    },
    /// Stop the kernel and start it again.
    Restart {
        /// Where the new kernel description goes.
        reply: oneshot::Sender<Result<KernelInfo, KernelError>>,
    },
    /// Stop the kernel and close.
    Shutdown {
        /// Where the answer goes.
        reply: oneshot::Sender<Result<(), KernelError>>,
    },
    /// Stop everything and end the task.
    Kill,
}

/// A message arriving from a receive task.
///
/// The generation lets the owner drop messages from sockets that a restart
/// already replaced.
enum Inbound {
    /// Something arrived on shell.
    Shell(u64, Result<zeromq::ZmqMessage, zeromq::ZmqError>),
    /// Something arrived on iopub.
    Iopub(u64, Result<zeromq::ZmqMessage, zeromq::ZmqError>),
    /// Something arrived on control.
    Control(u64, Result<zeromq::ZmqMessage, zeromq::ZmqError>),
}

/// Sockets kept by the owner task.
struct LiveSockets {
    /// Shell sender for requests.
    shell_send: zeromq::DealerSendHalf,
    /// Control sender for interrupt and shutdown.
    control_send: zeromq::DealerSendHalf,
    /// Stdin socket, held connected but never read.
    stdin: zeromq::DealerSocket,
    /// Heartbeat socket, used only during the handshake.
    hb: zeromq::ReqSocket,
}

/// State held by the owner task.
struct Owner {
    /// Liveness, changed only through [`next_state`].
    state: KernelState,
    /// Upper bound for info and interrupt replies.
    request_timeout: Duration,
    /// Grace period before a shutdown turns forceful.
    shutdown_grace: Duration,
    /// HMAC key bytes (hex text, per Jupyter convention).
    key: Vec<u8>,
    /// Session id shared by our messages.
    session: String,
    /// Launch arguments reused by restarts.
    launch: KernelLaunch,
    /// Send halves, dropped on close.
    live: Option<LiveSockets>,
    /// Child watcher, cleared on close.
    watched: Option<Watched>,
    /// Connection file, removed on close.
    conn: Option<ConnectionFile>,
    /// Pokes receive tasks to end on close.
    done: Arc<Notify>,
    /// Latest child exit broadcast, refreshed on restart.
    exited: tokio::sync::watch::Receiver<Option<ProcessExit>>,
    /// Current child and file for [`LocalKernel::process_id`].
    current: Arc<Mutex<Option<(u32, PathBuf)>>>,
    /// Channel carrying kernel events to the caller.
    events: std::sync::mpsc::Sender<KernelEvent>,
    /// Sender kept so the inbound channel never closes early.
    inbound_tx: mpsc::UnboundedSender<Inbound>,
    /// Socket generation, bumped on every restart.
    generation: u64,
    /// Info replies waiting for their message id, with deadlines.
    pending_info: HashMap<
        String,
        (
            oneshot::Sender<Result<KernelInfo, KernelError>>,
            tokio::time::Instant,
        ),
    >,
    /// Interrupt replies waiting for their message id, with deadlines.
    pending_interrupt: HashMap<
        String,
        (
            oneshot::Sender<Result<(), KernelError>>,
            tokio::time::Instant,
        ),
    >,
}

/// A restart running beside the owner task.
struct Restarting {
    /// The restart task, aborted on shutdown or drop.
    task: tokio::task::JoinHandle<()>,
    /// Where the restart caller waits.
    reply: oneshot::Sender<Result<KernelInfo, KernelError>>,
}

/// How one kernel generation ends for the restart task.
struct OldLive {
    /// Send halves used to stop the old kernel.
    live: LiveSockets,
    /// Old watcher whose exit ends the stop phase.
    watched: Watched,
    /// Old file removed after the old child ends.
    conn: ConnectionFile,
    /// Old HMAC key for the shutdown request.
    key: Vec<u8>,
    /// Old session for the shutdown request.
    session: String,
}

/// What a restart task hands back.
enum RestartOutcome {
    /// A fresh kernel generation.
    Ok(Box<RestartedOk>),
    /// Starting over failed; the owner goes `Dead`.
    Err(SpawnError),
}

/// A fresh kernel generation from a restart task.
struct RestartedOk {
    /// Fresh connected sockets.
    sockets: Sockets,
    /// Fresh child watcher.
    watched: Watched,
    /// Fresh connection file.
    conn: ConnectionFile,
    /// Fresh HMAC key.
    key: Vec<u8>,
    /// Fresh session.
    session: String,
    /// Fresh handshake info.
    info: KernelInfo,
}

/// Stop all live kernels without waiting.
///
/// Temporary stub for the registry (T039); panics until then.
pub fn kill_all_kernels() {
    todo!()
}

/// A spawned local kernel.
pub struct LocalKernel {
    /// Commands for the owner task.
    commands: mpsc::UnboundedSender<Command>,
    /// Event channel handed out once.
    events: Mutex<Option<std::sync::mpsc::Receiver<KernelEvent>>>,
    /// Current child and file for cleanup and inspection.
    current: Arc<Mutex<Option<(u32, PathBuf)>>>,
}

impl LocalKernel {
    /// Spawn `<python> -m ipykernel_launcher` and shake hands.
    ///
    /// Retries port conflicts and handshake timeouts up to
    /// `launch.port_retries` times with fresh ports. On success the
    /// returned info is the handshake `kernel_info_reply`.
    pub async fn spawn(launch: KernelLaunch) -> Result<(Self, KernelInfo), SpawnError> {
        let mut attempt = 0;
        loop {
            let ports = pick_ports().map_err(|error| SpawnError {
                kind: SpawnErrorKind::ConnectFailed {
                    message: error.to_string(),
                },
                python: launch.python.clone(),
                stderr_tail: String::new(),
            })?;
            let key = new_key();
            let conn = ConnectionFile::create(ports, key.clone(), &launch.kernel_name).map_err(
                |error| SpawnError {
                    kind: SpawnErrorKind::ConnectFailed {
                        message: error.to_string(),
                    },
                    python: launch.python.clone(),
                    stderr_tail: String::new(),
                },
            )?;
            let mut watched = match Watched::spawn(&launch.python, conn.path()) {
                Ok(watched) => watched,
                Err(error)
                    if error.kind() == std::io::ErrorKind::NotFound
                        || error.kind() == std::io::ErrorKind::PermissionDenied =>
                {
                    let _ = conn.remove();
                    return Err(SpawnError {
                        kind: SpawnErrorKind::InterpreterNotFound,
                        python: launch.python.clone(),
                        stderr_tail: String::new(),
                    });
                }
                Err(error) => {
                    let _ = conn.remove();
                    return Err(SpawnError {
                        kind: SpawnErrorKind::ConnectFailed {
                            message: error.to_string(),
                        },
                        python: launch.python.clone(),
                        stderr_tail: String::new(),
                    });
                }
            };
            let session = new_id();
            match handshake(
                &ports,
                key.as_bytes(),
                &session,
                &mut watched,
                launch.handshake_timeout,
            )
            .await
            {
                Ok((sockets, info)) => {
                    let (commands, commands_rx) = mpsc::unbounded_channel();
                    let (inbound_tx, inbound_rx) = mpsc::unbounded_channel();
                    let (events_tx, events_rx) = std::sync::mpsc::channel();
                    let current =
                        Arc::new(Mutex::new(Some((watched.pid, conn.path().to_path_buf()))));
                    let done = Arc::new(Notify::new());
                    tokio::spawn(recv_shell(
                        0,
                        sockets.shell_recv,
                        inbound_tx.clone(),
                        Arc::clone(&done),
                    ));
                    tokio::spawn(recv_iopub(
                        0,
                        sockets.iopub,
                        inbound_tx.clone(),
                        Arc::clone(&done),
                    ));
                    tokio::spawn(recv_control(
                        0,
                        sockets.control_recv,
                        inbound_tx.clone(),
                        Arc::clone(&done),
                    ));
                    let exited = watched.exited.clone();
                    let owner = Owner {
                        state: KernelState::Ready,
                        request_timeout: launch.request_timeout,
                        shutdown_grace: launch.shutdown_grace,
                        key: key.into_bytes(),
                        session,
                        launch: launch.clone(),
                        live: Some(LiveSockets {
                            shell_send: sockets.shell_send,
                            control_send: sockets.control_send,
                            stdin: sockets.stdin,
                            hb: sockets.hb,
                        }),
                        watched: Some(watched),
                        conn: Some(conn),
                        done,
                        exited,
                        current: Arc::clone(&current),
                        events: events_tx,
                        inbound_tx,
                        generation: 0,
                        pending_info: HashMap::new(),
                        pending_interrupt: HashMap::new(),
                    };
                    tokio::spawn(run_owner(owner, commands_rx, inbound_rx));
                    let kernel = Self {
                        commands,
                        events: Mutex::new(Some(events_rx)),
                        current,
                    };
                    return Ok((kernel, info));
                }
                Err(kind) => {
                    let tail = watched
                        .stderr
                        .lock()
                        .unwrap_or_else(|poison| poison.into_inner())
                        .tail();
                    let kind = match kind {
                        SpawnErrorKind::EarlyExit { exit } => classify(exit, &tail),
                        kind => kind,
                    };
                    watched.kill_group();
                    let _ = watched.wait_exit(Duration::from_secs(5)).await;
                    let _ = conn.remove();
                    if should_retry(&kind, &tail) && attempt < launch.port_retries {
                        attempt += 1;
                        continue;
                    }
                    return Err(SpawnError {
                        kind,
                        python: launch.python.clone(),
                        stderr_tail: tail,
                    });
                }
            }
        }
    }

    /// Current child pid, if the kernel runs.
    #[doc(hidden)]
    #[must_use]
    pub fn process_id(&self) -> Option<u32> {
        self.current
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .as_ref()
            .map(|(pid, _)| *pid)
    }

    /// Current connection file, if the kernel runs.
    #[doc(hidden)]
    #[must_use]
    pub fn connection_file(&self) -> Option<PathBuf> {
        self.current
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .as_ref()
            .map(|(_, path)| path.clone())
    }
}

impl KernelClient for LocalKernel {
    fn kernel_info(&self) -> impl Future<Output = Result<KernelInfo, KernelError>> + Send {
        let (reply, answer) = oneshot::channel();
        let sent = self.commands.send(Command::KernelInfo { reply });
        async move {
            if sent.is_err() {
                return Err(KernelError::Closed);
            }
            answer.await.unwrap_or(Err(KernelError::Closed))
        }
    }

    fn execute(&self, code: &str) -> impl Future<Output = Result<MsgId, KernelError>> + Send {
        let (reply, answer) = oneshot::channel();
        let sent = self.commands.send(Command::Execute {
            code: code.to_owned(),
            reply,
        });
        async move {
            if sent.is_err() {
                return Err(KernelError::Closed);
            }
            answer.await.unwrap_or(Err(KernelError::Closed))
        }
    }

    async fn interrupt(&self) -> Result<(), KernelError> {
        let (reply, answer) = oneshot::channel();
        let sent = self.commands.send(Command::Interrupt { reply });
        if sent.is_err() {
            return Err(KernelError::Closed);
        }
        answer.await.unwrap_or(Err(KernelError::Closed))
    }

    async fn restart(&self) -> Result<KernelInfo, KernelError> {
        let (reply, answer) = oneshot::channel();
        let sent = self.commands.send(Command::Restart { reply });
        if sent.is_err() {
            return Err(KernelError::Closed);
        }
        answer.await.unwrap_or(Err(KernelError::Closed))
    }

    fn shutdown(&self) -> impl Future<Output = Result<(), KernelError>> + Send {
        let (reply, answer) = oneshot::channel();
        let sent = self.commands.send(Command::Shutdown { reply });
        async move {
            if sent.is_err() {
                return Ok(());
            }
            answer.await.unwrap_or(Ok(()))
        }
    }

    fn messages(&mut self) -> Option<std::sync::mpsc::Receiver<KernelEvent>> {
        self.events
            .get_mut()
            .unwrap_or_else(|poison| poison.into_inner())
            .take()
    }
}

impl Drop for LocalKernel {
    fn drop(&mut self) {
        if let Some((pid, path)) = self
            .current
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .take()
        {
            signal_group(pid, libc::SIGKILL);
            let _ = std::fs::remove_file(&path);
        }
        let _ = self.commands.send(Command::Kill);
    }
}

/// Run commands, inbound messages, and deadlines until killed or abandoned.
async fn run_owner(
    mut owner: Owner,
    mut commands: mpsc::UnboundedReceiver<Command>,
    mut inbound: mpsc::UnboundedReceiver<Inbound>,
) {
    let mut restarting: Option<Restarting> = None;
    let mut restart_rx: Option<mpsc::UnboundedReceiver<RestartOutcome>> = None;
    loop {
        let deadline = owner.pending_deadline();
        tokio::select! {
            command = commands.recv() => {
                let Some(command) = command else {
                    abort_restart(&mut restarting, &mut restart_rx).await;
                    kill_live(&mut owner);
                    finish_close(&mut owner);
                    break;
                };
                match command {
                    Command::KernelInfo { reply } => handle_kernel_info(&mut owner, reply).await,
                    Command::Execute { code, reply } => {
                        handle_execute(&mut owner, code, reply).await;
                    }
                    Command::Interrupt { reply } => {
                        handle_interrupt(&mut owner, reply).await;
                    }
                    Command::Restart { reply } => {
                        handle_restart(&mut owner, &mut restarting, &mut restart_rx, reply).await;
                    }
                    Command::Shutdown { reply } => {
                        handle_shutdown(&mut owner, &mut restarting, &mut restart_rx, reply).await;
                    }
                    Command::Kill => {
                        abort_restart(&mut restarting, &mut restart_rx).await;
                        kill_live(&mut owner);
                        finish_close(&mut owner);
                        break;
                    }
                }
            }
            message = inbound.recv() => {
                let Some(message) = message else {
                    continue;
                };
                match message {
                    Inbound::Shell(gen, result) if gen == owner.generation => {
                        handle_shell(&mut owner, result);
                    }
                    Inbound::Iopub(gen, result) if gen == owner.generation => {
                        handle_channel(&mut owner, Channel::Iopub, result);
                    }
                    Inbound::Control(gen, result) if gen == owner.generation => {
                        handle_channel(&mut owner, Channel::Control, result);
                    }
                    _ => {}
                }
            }
            result = async {
                match restart_rx.as_mut() {
                    Some(rx) => rx.recv().await,
                    None => std::future::pending().await,
                }
            } => {
                restart_rx = None;
                let taken = restarting.take();
                if let Some(state) = taken {
                    let _ = state.task.await;
                    apply_restart(&mut owner, result, Some(state.reply));
                }
            }
            _ = sleep_until(deadline) => {
                expire_info(&mut owner);
            }
            exit = wait_child_exited(owner.exited.clone()),
                if matches!(owner.state, KernelState::Ready) =>
            {
                handle_child_exit(&mut owner, exit).await;
            }
        }
    }
}

/// Abort a running restart and tell its caller it was closed.
async fn abort_restart(
    restarting: &mut Option<Restarting>,
    restart_rx: &mut Option<mpsc::UnboundedReceiver<RestartOutcome>>,
) {
    restart_rx.take();
    if let Some(state) = restarting.take() {
        state.task.abort();
        let _ = state.task.await;
        let _ = state.reply.send(Err(KernelError::Closed));
    }
}

/// Forward shell messages until the receive task ends or is told to stop.
async fn recv_shell(
    gen: u64,
    mut recv: zeromq::DealerRecvHalf,
    tx: mpsc::UnboundedSender<Inbound>,
    done: Arc<Notify>,
) {
    loop {
        tokio::select! {
            result = recv.recv() => {
                if tx.send(Inbound::Shell(gen, result)).is_err() {
                    break;
                }
            }
            _ = done.notified() => break,
        }
    }
}

/// Forward iopub messages until the receive task ends or is told to stop.
async fn recv_iopub(
    gen: u64,
    mut recv: zeromq::SubSocket,
    tx: mpsc::UnboundedSender<Inbound>,
    done: Arc<Notify>,
) {
    loop {
        tokio::select! {
            result = recv.recv() => {
                if tx.send(Inbound::Iopub(gen, result)).is_err() {
                    break;
                }
            }
            _ = done.notified() => break,
        }
    }
}

/// Forward control messages until the receive task ends or is told to stop.
async fn recv_control(
    gen: u64,
    mut recv: zeromq::DealerRecvHalf,
    tx: mpsc::UnboundedSender<Inbound>,
    done: Arc<Notify>,
) {
    loop {
        tokio::select! {
            result = recv.recv() => {
                if tx.send(Inbound::Control(gen, result)).is_err() {
                    break;
                }
            }
            _ = done.notified() => break,
        }
    }
}

/// Wait until the child ends, reporting how.
async fn wait_child_exited(
    mut exited: tokio::sync::watch::Receiver<Option<ProcessExit>>,
) -> Option<ProcessExit> {
    let settled = exited.wait_for(|exit| exit.is_some()).await;
    match settled {
        Ok(guard) => *guard,
        Err(_) => None,
    }
}

/// Turn an unexpected child exit into one `Died` event.
async fn handle_child_exit(owner: &mut Owner, exit: Option<ProcessExit>) {
    let Some(exit) = exit else {
        return;
    };
    if !matches!(owner.state, KernelState::Ready) {
        return;
    }
    let tail = owner
        .watched
        .as_ref()
        .map(|watched| Arc::clone(&watched.stderr))
        .map(|stderr| {
            stderr
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .tail()
        })
        .unwrap_or_default();
    send_event(
        owner,
        KernelEvent::Died {
            exit,
            stderr_tail: tail,
        },
    );
    for (_, (reply, _)) in owner.pending_info.drain() {
        let _ = reply.send(Err(KernelError::Dead { exit }));
    }
    for (_, (reply, _)) in owner.pending_interrupt.drain() {
        let _ = reply.send(Err(KernelError::Dead { exit }));
    }
    if let Some(conn) = owner.conn.take() {
        let _ = conn.remove();
    }
    let _ = owner
        .current
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .take();
    owner.watched = None;
    if let Some(live) = owner.live.take() {
        let _ = (&live.stdin, &live.hb);
    }
    owner.done.notify_waiters();
    owner.state = next_state(&owner.state, KernelTransition::ChildExited { exit });
}

/// Sleep until a pending deadline, or never when idle.
async fn sleep_until(deadline: Option<tokio::time::Instant>) {
    match deadline {
        Some(at) => tokio::time::sleep_until(at).await,
        None => std::future::pending().await,
    }
}

impl Owner {
    /// Earliest pending deadline, if any request waits.
    fn pending_deadline(&self) -> Option<tokio::time::Instant> {
        self.pending_info
            .values()
            .map(|(_, at)| *at)
            .chain(self.pending_interrupt.values().map(|(_, at)| *at))
            .min()
    }
}

/// Forward one event unless the caller dropped the channel.
fn send_event(owner: &Owner, event: KernelEvent) {
    let _ = owner.events.send(event);
}

/// Route one shell message to its waiter or the event channel.
fn handle_shell(owner: &mut Owner, incoming: Result<zeromq::ZmqMessage, zeromq::ZmqError>) {
    let frames = match incoming {
        Ok(message) => to_frames(message),
        Err(_) => return,
    };
    let message = match decode(&frames, &owner.key) {
        Ok(message) => message,
        Err(error) => {
            send_event(owner, warning(&error));
            return;
        }
    };
    match convert(Channel::Shell, &message) {
        Converted::Event(event) => send_event(owner, event),
        Converted::KernelInfoReply { parent, result } => {
            let Some(id) = parent.map(|parent| parent.as_str().to_owned()) else {
                return;
            };
            if let Some((reply, _)) = owner.pending_info.remove(&id) {
                let _ = reply.send(result.map_err(|message| KernelError::Io { message }));
            }
        }
        _ => {}
    }
}

/// Route one iopub or control message to the event channel.
fn handle_channel(
    owner: &mut Owner,
    channel: Channel,
    incoming: Result<zeromq::ZmqMessage, zeromq::ZmqError>,
) {
    let frames = match incoming {
        Ok(message) => to_frames(message),
        Err(_) => return,
    };
    let message = match decode(&frames, &owner.key) {
        Ok(message) => message,
        Err(error) => {
            send_event(owner, warning(&error));
            return;
        }
    };
    if let Converted::Event(event) = convert(channel, &message) {
        send_event(owner, event);
        return;
    }
    if let Converted::InterruptReply { parent } = convert(channel, &message) {
        let Some(id) = parent.map(|parent| parent.as_str().to_owned()) else {
            return;
        };
        if let Some((reply, _)) = owner.pending_interrupt.remove(&id) {
            let _ = reply.send(Ok(()));
        }
    }
}

/// Run code when admitted, answering with the request id at once.
async fn handle_execute(
    owner: &mut Owner,
    code: String,
    reply: oneshot::Sender<Result<MsgId, KernelError>>,
) {
    match admit(&owner.state, KernelOp::Execute) {
        Admission::Send => {}
        Admission::Reject(error) => {
            let _ = reply.send(Err(error));
            return;
        }
        _ => {
            let _ = reply.send(Err(KernelError::Closed));
            return;
        }
    }
    let req_header = header("execute_request", &owner.session);
    let msg_id = req_header.msg_id.clone();
    let message = WireMessage {
        identities: Vec::new(),
        header: req_header,
        parent_header: None,
        metadata: serde_json::Value::Object(Default::default()),
        content: serde_json::json!({
            "silent": false,
            "store_history": true,
            "user_expressions": {},
            "allow_stdin": false,
            "stop_on_error": true,
            "code": code,
        }),
        buffers: Vec::new(),
    };
    let frames = encode(&message, &owner.key);
    let sent = match owner.live.as_mut() {
        Some(live) => send_zmq(&mut live.shell_send, frames).await.is_ok(),
        None => false,
    };
    if sent {
        let _ = reply.send(Ok(MsgId::new(msg_id)));
    } else {
        let _ = reply.send(Err(KernelError::Closed));
    }
}

/// Interrupt the running execution when admitted.
async fn handle_interrupt(owner: &mut Owner, reply: oneshot::Sender<Result<(), KernelError>>) {
    match admit(&owner.state, KernelOp::Interrupt) {
        Admission::Send => {}
        Admission::Reject(error) => {
            let _ = reply.send(Err(error));
            return;
        }
        _ => {
            let _ = reply.send(Err(KernelError::Closed));
            return;
        }
    }
    let req_header = header("interrupt_request", &owner.session);
    let msg_id = req_header.msg_id.clone();
    let message = WireMessage {
        identities: Vec::new(),
        header: req_header,
        parent_header: None,
        metadata: serde_json::Value::Object(Default::default()),
        content: serde_json::Value::Object(Default::default()),
        buffers: Vec::new(),
    };
    let frames = encode(&message, &owner.key);
    let sent = match owner.live.as_mut() {
        Some(live) => send_zmq(&mut live.control_send, frames).await.is_ok(),
        None => false,
    };
    if !sent {
        let _ = reply.send(Err(KernelError::Closed));
        return;
    }
    let deadline = tokio::time::Instant::now() + owner.request_timeout;
    owner.pending_interrupt.insert(msg_id, (reply, deadline));
}

/// Ask for the kernel description when admitted.
async fn handle_kernel_info(
    owner: &mut Owner,
    reply: oneshot::Sender<Result<KernelInfo, KernelError>>,
) {
    match admit(&owner.state, KernelOp::KernelInfo) {
        Admission::Send => {}
        Admission::Reject(error) => {
            let _ = reply.send(Err(error));
            return;
        }
        _ => {
            let _ = reply.send(Err(KernelError::Closed));
            return;
        }
    }
    let req_header = header("kernel_info_request", &owner.session);
    let msg_id = req_header.msg_id.clone();
    let message = WireMessage {
        identities: Vec::new(),
        header: req_header,
        parent_header: None,
        metadata: serde_json::Value::Object(Default::default()),
        content: serde_json::Value::Object(Default::default()),
        buffers: Vec::new(),
    };
    let frames = encode(&message, &owner.key);
    let sent = match owner.live.as_mut() {
        Some(live) => send_zmq(&mut live.shell_send, frames).await.is_ok(),
        None => false,
    };
    if !sent {
        let _ = reply.send(Err(KernelError::Closed));
        return;
    }
    let deadline = tokio::time::Instant::now() + owner.request_timeout;
    owner.pending_info.insert(msg_id, (reply, deadline));
}

/// Restart the kernel, stopping the old generation in a separate task.
async fn handle_restart(
    owner: &mut Owner,
    restarting: &mut Option<Restarting>,
    restart_rx: &mut Option<mpsc::UnboundedReceiver<RestartOutcome>>,
    reply: oneshot::Sender<Result<KernelInfo, KernelError>>,
) {
    let stop_old = match admit(&owner.state, KernelOp::Restart) {
        Admission::StopThenRestart => {
            match (owner.live.take(), owner.watched.take(), owner.conn.take()) {
                (Some(live), Some(watched), Some(conn)) => Some(OldLive {
                    live,
                    watched,
                    conn,
                    key: owner.key.clone(),
                    session: owner.session.clone(),
                }),
                _ => None,
            }
        }
        Admission::RestartFromDead => None,
        Admission::Reject(error) => {
            let _ = reply.send(Err(error));
            return;
        }
        _ => {
            let _ = reply.send(Err(KernelError::Closed));
            return;
        }
    };
    owner.state = next_state(&owner.state, KernelTransition::RestartStarted);
    owner.generation += 1;
    owner.done.notify_waiters();
    let (result_tx, result_rx) = mpsc::unbounded_channel();
    let task = tokio::spawn(run_restart(
        owner.launch.clone(),
        stop_old,
        Arc::clone(&owner.current),
        owner.shutdown_grace,
        result_tx,
    ));
    *restarting = Some(Restarting { task, reply });
    *restart_rx = Some(result_rx);
}

/// Stop the old generation, then spawn and shake hands on a new one.
async fn run_restart(
    launch: KernelLaunch,
    stop_old: Option<OldLive>,
    current: Arc<Mutex<Option<(u32, PathBuf)>>>,
    grace: Duration,
    result: mpsc::UnboundedSender<RestartOutcome>,
) {
    if let Some(old) = stop_old {
        stop_old_generation(old, &current, grace).await;
    }
    let key = new_key();
    let session = new_id();
    let mut attempt = 0;
    loop {
        let ports = match pick_ports() {
            Ok(ports) => ports,
            Err(error) => {
                let _ = result.send(RestartOutcome::Err(SpawnError {
                    kind: SpawnErrorKind::ConnectFailed {
                        message: error.to_string(),
                    },
                    python: launch.python.clone(),
                    stderr_tail: String::new(),
                }));
                return;
            }
        };
        let conn = match ConnectionFile::create(ports, key.clone(), &launch.kernel_name) {
            Ok(conn) => conn,
            Err(error) => {
                let _ = result.send(RestartOutcome::Err(SpawnError {
                    kind: SpawnErrorKind::ConnectFailed {
                        message: error.to_string(),
                    },
                    python: launch.python.clone(),
                    stderr_tail: String::new(),
                }));
                return;
            }
        };
        let mut watched = match Watched::spawn(&launch.python, conn.path()) {
            Ok(watched) => watched,
            Err(error)
                if error.kind() == std::io::ErrorKind::NotFound
                    || error.kind() == std::io::ErrorKind::PermissionDenied =>
            {
                let _ = conn.remove();
                let _ = result.send(RestartOutcome::Err(SpawnError {
                    kind: SpawnErrorKind::InterpreterNotFound,
                    python: launch.python.clone(),
                    stderr_tail: String::new(),
                }));
                return;
            }
            Err(error) => {
                let _ = conn.remove();
                let _ = result.send(RestartOutcome::Err(SpawnError {
                    kind: SpawnErrorKind::ConnectFailed {
                        message: error.to_string(),
                    },
                    python: launch.python.clone(),
                    stderr_tail: String::new(),
                }));
                return;
            }
        };
        current
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .replace((watched.pid, conn.path().to_path_buf()));
        match handshake(
            &ports,
            key.as_bytes(),
            &session,
            &mut watched,
            launch.handshake_timeout,
        )
        .await
        {
            Ok((sockets, info)) => {
                let outcome = RestartedOk {
                    sockets,
                    watched,
                    conn,
                    key: key.into_bytes(),
                    session,
                    info,
                };
                if let Err(returned) = result.send(RestartOutcome::Ok(Box::new(outcome))) {
                    let RestartOutcome::Ok(mut restarted) = returned.0 else {
                        return;
                    };
                    restarted.watched.kill_group();
                    let _ = restarted.watched.wait_exit(Duration::from_secs(5)).await;
                    let _ = restarted.conn.remove();
                    let _ = current
                        .lock()
                        .unwrap_or_else(|poison| poison.into_inner())
                        .take();
                }
                return;
            }
            Err(kind) => {
                let tail = watched
                    .stderr
                    .lock()
                    .unwrap_or_else(|poison| poison.into_inner())
                    .tail();
                let kind = match kind {
                    SpawnErrorKind::EarlyExit { exit } => classify(exit, &tail),
                    kind => kind,
                };
                watched.kill_group();
                let _ = watched.wait_exit(Duration::from_secs(5)).await;
                let _ = conn.remove();
                let _ = current
                    .lock()
                    .unwrap_or_else(|poison| poison.into_inner())
                    .take();
                if should_retry(&kind, &tail) && attempt < launch.port_retries {
                    attempt += 1;
                    continue;
                }
                let _ = result.send(RestartOutcome::Err(SpawnError {
                    kind,
                    python: launch.python.clone(),
                    stderr_tail: tail,
                }));
                return;
            }
        }
    }
}

/// Stop one old generation with a shutdown request, then forcefully.
async fn stop_old_generation(
    mut old: OldLive,
    current: &Arc<Mutex<Option<(u32, PathBuf)>>>,
    grace: Duration,
) {
    let message = WireMessage {
        identities: Vec::new(),
        header: header("shutdown_request", &old.session),
        parent_header: None,
        metadata: serde_json::Value::Object(Default::default()),
        content: serde_json::json!({"restart": false}),
        buffers: Vec::new(),
    };
    let frames = encode(&message, &old.key);
    let mut live = old.live;
    let _ = send_zmq(&mut live.control_send, frames).await;
    let mut exited = old.watched.exited.clone();
    let done = tokio::select! {
        _ = exited.wait_for(|exit| exit.is_some()) => true,
        _ = tokio::time::sleep(grace) => false,
    };
    if !done {
        old.watched.kill_group();
    }
    let _ = old.watched.wait_exit(Duration::from_secs(5)).await;
    let _ = old.conn.remove();
    let _ = current
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .take();
    let _ = (&live.stdin, &live.hb);
}

/// Apply a finished restart to the owner.
fn apply_restart(
    owner: &mut Owner,
    outcome: Option<RestartOutcome>,
    reply: Option<oneshot::Sender<Result<KernelInfo, KernelError>>>,
) {
    let Some(reply) = reply else {
        return;
    };
    match outcome {
        Some(RestartOutcome::Ok(restarted)) => {
            let gen = owner.generation;
            let inbound_tx = owner.inbound_tx.clone();
            let done = Arc::clone(&owner.done);
            tokio::spawn(recv_shell(
                gen,
                restarted.sockets.shell_recv,
                inbound_tx.clone(),
                Arc::clone(&done),
            ));
            tokio::spawn(recv_iopub(
                gen,
                restarted.sockets.iopub,
                inbound_tx.clone(),
                Arc::clone(&done),
            ));
            tokio::spawn(recv_control(
                gen,
                restarted.sockets.control_recv,
                inbound_tx,
                Arc::clone(&done),
            ));
            owner.live = Some(LiveSockets {
                shell_send: restarted.sockets.shell_send,
                control_send: restarted.sockets.control_send,
                stdin: restarted.sockets.stdin,
                hb: restarted.sockets.hb,
            });
            owner.exited = restarted.watched.exited.clone();
            owner.watched = Some(restarted.watched);
            owner.conn = Some(restarted.conn);
            owner.key = restarted.key;
            owner.session = restarted.session;
            owner.state = next_state(&owner.state, KernelTransition::RestartSucceeded);
            send_event(
                owner,
                KernelEvent::Restarted {
                    info: restarted.info.clone(),
                },
            );
            let _ = reply.send(Ok(restarted.info));
        }
        Some(RestartOutcome::Err(error)) => {
            owner.state = next_state(&owner.state, KernelTransition::RestartFailed);
            let _ = reply.send(Err(KernelError::Spawn(error)));
        }
        None => {
            owner.state = next_state(&owner.state, KernelTransition::RestartFailed);
            let _ = reply.send(Err(KernelError::Spawn(SpawnError {
                kind: SpawnErrorKind::ConnectFailed {
                    message: "restart task ended".to_owned(),
                },
                python: owner.launch.python.clone(),
                stderr_tail: String::new(),
            })));
        }
    }
}

/// Stop the kernel and close, waiting out the grace period first.
async fn handle_shutdown(
    owner: &mut Owner,
    restarting: &mut Option<Restarting>,
    restart_rx: &mut Option<mpsc::UnboundedReceiver<RestartOutcome>>,
    reply: oneshot::Sender<Result<(), KernelError>>,
) {
    match admit(&owner.state, KernelOp::Shutdown) {
        Admission::StopAndClose => {}
        Admission::CleanUpDead => {
            drop_live(owner);
            finish_close(owner);
            let _ = reply.send(Ok(()));
            return;
        }
        Admission::AlreadyClosed => {
            let _ = reply.send(Ok(()));
            return;
        }
        Admission::AbortRestartAndStop => {
            abort_restart(restarting, restart_rx).await;
            if let Some((pid, path)) = owner
                .current
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .take()
            {
                signal_group(pid, libc::SIGKILL);
                let _ = std::fs::remove_file(&path);
            }
            drop_live(owner);
            finish_close(owner);
            let _ = reply.send(Ok(()));
            return;
        }
        Admission::Reject(error) => {
            let _ = reply.send(Err(error));
            return;
        }
        _ => {
            let _ = reply.send(Err(KernelError::Closed));
            return;
        }
    }
    send_shutdown_request(owner).await;
    wait_child_exit(owner).await;
    drop_live(owner);
    finish_close(owner);
    let _ = reply.send(Ok(()));
}

/// Send `shutdown_request(restart: false)` on control.
async fn send_shutdown_request(owner: &mut Owner) {
    let message = WireMessage {
        identities: Vec::new(),
        header: header("shutdown_request", &owner.session),
        parent_header: None,
        metadata: serde_json::Value::Object(Default::default()),
        content: serde_json::json!({"restart": false}),
        buffers: Vec::new(),
    };
    let frames = encode(&message, &owner.key);
    if let Some(live) = owner.live.as_mut() {
        let _ = send_zmq(&mut live.control_send, frames).await;
    }
}

/// Wait for the child to end, killing its group past the grace period.
async fn wait_child_exit(owner: &mut Owner) {
    let grace = owner.shutdown_grace;
    let mut exited = match owner.watched.as_ref() {
        Some(watched) => watched.exited.clone(),
        None => return,
    };
    let done = tokio::select! {
        _ = exited.wait_for(|exit| exit.is_some()) => true,
        _ = tokio::time::sleep(grace) => false,
    };
    if done {
        return;
    }
    if let Some(watched) = owner.watched.as_ref() {
        watched.kill_group();
    }
}

/// Drop send halves and watcher, stopping receive tasks and removing the file.
fn drop_live(owner: &mut Owner) {
    if let Some(live) = owner.live.take() {
        // `stdin` is never read because `allow_stdin` is false, and `hb`
        // is only used during the handshake; touch them so the channels
        // stay connected until the sockets close here.
        let _ = (&live.stdin, &live.hb);
    }
    owner.watched = None;
    if let Some(conn) = owner.conn.take() {
        let _ = conn.remove();
    }
    owner.done.notify_waiters();
}

/// Kill the group without waiting, for task end.
fn kill_live(owner: &mut Owner) {
    if let Some(watched) = owner.watched.as_ref() {
        watched.kill_group();
    }
    drop_live(owner);
}

/// Clear the current child, fail waiters, and close the state.
fn finish_close(owner: &mut Owner) {
    let _ = owner
        .current
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .take();
    for (_, (reply, _)) in owner.pending_info.drain() {
        let _ = reply.send(Err(KernelError::Closed));
    }
    for (_, (reply, _)) in owner.pending_interrupt.drain() {
        let _ = reply.send(Err(KernelError::Closed));
    }
    owner.state = next_state(&owner.state, KernelTransition::Closed);
}

/// Fail requests past their deadline.
fn expire_info(owner: &mut Owner) {
    let now = tokio::time::Instant::now();
    let expired: Vec<String> = owner
        .pending_info
        .iter()
        .filter(|(_, (_, at))| *at <= now)
        .map(|(id, _)| id.clone())
        .collect();
    for id in expired {
        if let Some((reply, _)) = owner.pending_info.remove(&id) {
            let _ = reply.send(Err(KernelError::Timeout {
                op: KernelOp::KernelInfo,
            }));
        }
    }
    let expired: Vec<String> = owner
        .pending_interrupt
        .iter()
        .filter(|(_, (_, at))| *at <= now)
        .map(|(id, _)| id.clone())
        .collect();
    for id in expired {
        if let Some((reply, _)) = owner.pending_interrupt.remove(&id) {
            let _ = reply.send(Err(KernelError::Timeout {
                op: KernelOp::Interrupt,
            }));
        }
    }
}

/// Send frames on a dealer sender.
async fn send_zmq(
    socket: &mut zeromq::DealerSendHalf,
    frames: Vec<Vec<u8>>,
) -> Result<(), zeromq::ZmqError> {
    use bytes::Bytes;
    let parts: Vec<Bytes> = frames.into_iter().map(Bytes::from).collect();
    let message = zeromq::ZmqMessage::try_from(parts).expect("requests carry frames");
    socket.send(message).await
}

/// Unpack a socket message into frames.
fn to_frames(message: zeromq::ZmqMessage) -> Vec<Vec<u8>> {
    message
        .into_vec()
        .into_iter()
        .map(|frame| frame.to_vec())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tells_missing_ipykernel_apart() {
        assert_eq!(
            classify(
                ProcessExit::Code(1),
                "Traceback: No module named ipykernel_launcher\n"
            ),
            SpawnErrorKind::IpykernelMissing
        );
        assert_eq!(
            classify(ProcessExit::Code(1), "No module named 'ipykernel'\n"),
            SpawnErrorKind::IpykernelMissing
        );
        assert_eq!(
            classify(ProcessExit::Code(1), "SyntaxError: bad\n"),
            SpawnErrorKind::EarlyExit {
                exit: ProcessExit::Code(1),
            }
        );
        assert_eq!(
            classify(ProcessExit::Signal(9), ""),
            SpawnErrorKind::EarlyExit {
                exit: ProcessExit::Signal(9),
            }
        );
    }

    #[test]
    fn retries_only_port_conflicts_and_timeouts() {
        assert!(should_retry(
            &SpawnErrorKind::EarlyExit {
                exit: ProcessExit::Code(1),
            },
            "Address already in use\n",
        ));
        assert!(should_retry(
            &SpawnErrorKind::HandshakeTimeout {
                after: std::time::Duration::from_secs(30),
            },
            "",
        ));
        assert!(!should_retry(&SpawnErrorKind::InterpreterNotFound, "",));
        assert!(!should_retry(&SpawnErrorKind::IpykernelMissing, "",));
        assert!(!should_retry(
            &SpawnErrorKind::EarlyExit {
                exit: ProcessExit::Code(1),
            },
            "SyntaxError: bad\n",
        ));
        assert!(!should_retry(
            &SpawnErrorKind::ConnectFailed {
                message: "refused".to_owned(),
            },
            "",
        ));
    }

    #[test]
    fn kernel_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<LocalKernel>();
    }
}
