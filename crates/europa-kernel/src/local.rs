//! Local kernel behind the client window.
//!
//! One spawned task owns the send halves, the child watcher, and the
//! [`KernelState`]. Receive halves live in dedicated tasks that forward to
//! the owner, so a slow channel never blocks the others behind one lock.
//! Every request passes [`admit`] first and moves state only through
//! [`next_state`].
//!
//! A restart runs in a task of its own, which owns the old generation while
//! stopping it and the new one until its handshake ends. Every inbound
//! message carries the generation of its socket, and the owner drops those
//! of generations that a restart, a death, or a close already ended.

use std::collections::HashMap;
use std::future::Future;
use std::io;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use europa_core::{
    admit, next_state, Admission, KernelError, KernelEvent, KernelInfo, KernelLaunch, KernelOp,
    KernelState, KernelTransition, MsgId, ProcessExit, SpawnError, SpawnErrorKind,
};
use tokio::sync::{mpsc, oneshot, watch};
use tokio::task::{JoinError, JoinHandle};
use tokio::time::Instant;
use zeromq::{SocketRecv, SocketSend};

use crate::connection::{new_key, pick_ports, ConnectionFile};
use crate::convert::{convert, warning, Channel, Converted};
use crate::handshake::{handshake, Sockets};
use crate::process::{signal_group, Watched};
use crate::registry::GLOBAL;
use crate::wire::{decode, encode, header, new_id, WireMessage};
use crate::KernelClient;

/// Longest wait after `SIGKILL` for the watcher to publish the exit.
const EXIT_WAIT: Duration = Duration::from_secs(5);

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
/// The generation lets the owner drop messages from sockets that a restart,
/// a death, or a close already ended.
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
    /// Stdin socket, kept connected but never read because `allow_stdin` is
    /// false.
    _stdin: zeromq::DealerSocket,
    /// Heartbeat socket, used only during the handshake and kept until the
    /// generation ends.
    _hb: zeromq::ReqSocket,
}

/// The child a kernel runs now.
///
/// Whoever replaces the child updates this at once, so `Drop` and a
/// cancelled restart always find the child to stop.
struct Current {
    /// Child pid, which is also its process group id.
    pid: u32,
    /// Connection file of the child.
    path: PathBuf,
    /// How the child ended, once it has.
    exited: watch::Receiver<Option<ProcessExit>>,
}

/// The current child, shared by the handle, the owner, and restarts.
type SharedCurrent = Arc<Mutex<Option<Current>>>;

/// A reply channel with the deadline of its request.
type Pending<T> = (oneshot::Sender<Result<T, KernelError>>, Instant);

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
    /// Send halves of the current generation.
    live: Option<LiveSockets>,
    /// Watcher of the current child.
    watched: Option<Watched>,
    /// Connection file of the current child.
    conn: Option<ConnectionFile>,
    /// Receive tasks of the current generation, aborted when it ends.
    receivers: Vec<JoinHandle<()>>,
    /// Current child and file for [`LocalKernel::process_id`].
    current: SharedCurrent,
    /// Channel carrying kernel events to the caller.
    events: std::sync::mpsc::Sender<KernelEvent>,
    /// Sender kept so the inbound channel never closes early.
    inbound_tx: mpsc::UnboundedSender<Inbound>,
    /// Socket generation, bumped whenever one ends.
    generation: u64,
    /// Info replies waiting for their message id, with deadlines.
    pending_info: HashMap<String, Pending<KernelInfo>>,
    /// Interrupt replies waiting for their message id, with deadlines.
    pending_interrupt: HashMap<String, Pending<()>>,
}

/// A restart running beside the owner task.
struct Restarting {
    /// The restart task, aborted on shutdown or drop.
    task: JoinHandle<Result<Started, SpawnError>>,
    /// Where the restart caller waits.
    reply: oneshot::Sender<Result<KernelInfo, KernelError>>,
}

/// The generation a restart stops before starting a new one.
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

/// A kernel generation that finished its handshake.
struct Started {
    /// Connected sockets.
    sockets: Sockets,
    /// Child watcher.
    watched: Watched,
    /// Connection file.
    conn: ConnectionFile,
    /// HMAC key bytes.
    key: Vec<u8>,
    /// Session id.
    session: String,
    /// Handshake info.
    info: KernelInfo,
}

/// A spawned local kernel.
///
/// Create it inside a Tokio runtime; a task of its own drives the kernel.
/// Each request is queued when its method is called, so requests run in
/// call order, and dropping a returned future does not cancel the request.
/// Dropping the kernel without [`KernelClient::shutdown`] synchronously
/// sends `SIGKILL` to the current child's process group and removes the
/// connection file, with no grace period and even after the runtime has
/// stopped.
pub struct LocalKernel {
    /// Commands for the owner task.
    commands: mpsc::UnboundedSender<Command>,
    /// Event channel handed out once.
    events: Mutex<Option<std::sync::mpsc::Receiver<KernelEvent>>>,
    /// Current child and file for cleanup and inspection.
    current: SharedCurrent,
}

impl LocalKernel {
    /// Spawn `<python> -m ipykernel_launcher` and shake hands.
    ///
    /// Retries port conflicts and handshake timeouts up to
    /// `launch.port_retries` times with fresh ports. On success the
    /// returned info is the handshake `kernel_info_reply`. Dropping the
    /// future during the handshake stops the child.
    pub async fn spawn(launch: KernelLaunch) -> Result<(Self, KernelInfo), SpawnError> {
        let current: SharedCurrent = Arc::new(Mutex::new(None));
        let started = start(&launch, &current).await?;
        let (commands, commands_rx) = mpsc::unbounded_channel();
        let (inbound_tx, inbound_rx) = mpsc::unbounded_channel();
        let (events_tx, events_rx) = std::sync::mpsc::channel();
        let mut owner = Owner {
            state: KernelState::Ready,
            request_timeout: launch.request_timeout,
            shutdown_grace: launch.shutdown_grace,
            key: Vec::new(),
            session: String::new(),
            launch,
            live: None,
            watched: None,
            conn: None,
            receivers: Vec::new(),
            current: Arc::clone(&current),
            events: events_tx,
            inbound_tx,
            generation: 0,
            pending_info: HashMap::new(),
            pending_interrupt: HashMap::new(),
        };
        let info = owner.adopt(started);
        tokio::spawn(run_owner(owner, commands_rx, inbound_rx));
        let kernel = Self {
            commands,
            events: Mutex::new(Some(events_rx)),
            current,
        };
        Ok((kernel, info))
    }

    /// Current child pid, if the kernel runs.
    #[doc(hidden)]
    #[must_use]
    pub fn process_id(&self) -> Option<u32> {
        lock(&self.current).as_ref().map(|current| current.pid)
    }

    /// Current connection file, if the kernel runs.
    #[doc(hidden)]
    #[must_use]
    pub fn connection_file(&self) -> Option<PathBuf> {
        lock(&self.current)
            .as_ref()
            .map(|current| current.path.clone())
    }

    /// Queue a command at once and wait for its answer when polled.
    ///
    /// Sending before the first poll keeps commands in call order. `gone`
    /// answers when the owner task has already ended.
    fn ask<T>(
        &self,
        command: impl FnOnce(oneshot::Sender<Result<T, KernelError>>) -> Command,
        gone: fn() -> Result<T, KernelError>,
    ) -> impl Future<Output = Result<T, KernelError>> + Send
    where
        T: Send + 'static,
    {
        let (reply, answer) = oneshot::channel();
        let sent = self.commands.send(command(reply)).is_ok();
        async move {
            if !sent {
                return gone();
            }
            answer.await.unwrap_or_else(|_| gone())
        }
    }
}

/// The answer of a request whose owner task has ended.
fn closed<T>() -> Result<T, KernelError> {
    Err(KernelError::Closed)
}

impl KernelClient for LocalKernel {
    fn kernel_info(&self) -> impl Future<Output = Result<KernelInfo, KernelError>> + Send {
        self.ask(|reply| Command::KernelInfo { reply }, closed)
    }

    fn execute(&self, code: &str) -> impl Future<Output = Result<MsgId, KernelError>> + Send {
        let code = code.to_owned();
        self.ask(move |reply| Command::Execute { code, reply }, closed)
    }

    fn interrupt(&self) -> impl Future<Output = Result<(), KernelError>> + Send {
        self.ask(|reply| Command::Interrupt { reply }, closed)
    }

    fn restart(&self) -> impl Future<Output = Result<KernelInfo, KernelError>> + Send {
        self.ask(|reply| Command::Restart { reply }, closed)
    }

    fn shutdown(&self) -> impl Future<Output = Result<(), KernelError>> + Send {
        self.ask(|reply| Command::Shutdown { reply }, || Ok(()))
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
        // Unregister before killing so that `kill_all_kernels` never keeps a
        // pid whose group may already be reaped and free.
        let current = lock(&self.current).take();
        if let Some(current) = current {
            GLOBAL.unregister(current.pid);
            signal_group(current.pid, libc::SIGKILL);
            let _ = std::fs::remove_file(&current.path);
        }
        let _ = self.commands.send(Command::Kill);
    }
}

/// Lock the shared current child, ignoring poison.
fn lock(current: &SharedCurrent) -> std::sync::MutexGuard<'_, Option<Current>> {
    current.lock().unwrap_or_else(|poison| poison.into_inner())
}

/// Spawn a kernel and shake hands, retrying with fresh ports when useful.
///
/// Each attempt stores its child in `current` right after the spawn, so an
/// aborted restart or a dropped handle can stop it during the handshake.
async fn start(launch: &KernelLaunch, current: &SharedCurrent) -> Result<Started, SpawnError> {
    let mut attempt = 0;
    loop {
        let ports = pick_ports().map_err(|error| setup_failure(launch, &error))?;
        let key = new_key();
        let conn = ConnectionFile::create(ports, key.clone(), &launch.kernel_name)
            .map_err(|error| setup_failure(launch, &error))?;
        let mut watched = Watched::spawn(&launch.python, conn.path())
            .map_err(|error| start_failure(launch, &error))?;
        *lock(current) = Some(Current {
            pid: watched.pid,
            path: conn.path().to_path_buf(),
            exited: watched.exited.clone(),
        });
        GLOBAL.register(watched.pid, conn.path().to_path_buf());
        let session = new_id();
        let shaken = handshake(
            &ports,
            key.as_bytes(),
            &session,
            &mut watched,
            launch.handshake_timeout,
        )
        .await;
        match shaken {
            Ok((sockets, info)) => {
                return Ok(Started {
                    sockets,
                    watched,
                    conn,
                    key: key.into_bytes(),
                    session,
                    info,
                });
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
                let _ = watched.wait_exit(EXIT_WAIT).await;
                let _ = conn.remove();
                *lock(current) = None;
                // Dropping the watcher forgets the registry entry.
                drop(watched);
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

/// Report ports or a connection file that could not be prepared.
fn setup_failure(launch: &KernelLaunch, error: &io::Error) -> SpawnError {
    SpawnError {
        kind: SpawnErrorKind::ConnectFailed {
            message: error.to_string(),
        },
        python: launch.python.clone(),
        stderr_tail: String::new(),
    }
}

/// Report an interpreter that could not be started.
fn start_failure(launch: &KernelLaunch, error: &io::Error) -> SpawnError {
    let kind = match error.kind() {
        io::ErrorKind::NotFound | io::ErrorKind::PermissionDenied => {
            SpawnErrorKind::InterpreterNotFound
        }
        _ => SpawnErrorKind::ConnectFailed {
            message: error.to_string(),
        },
    };
    SpawnError {
        kind,
        python: launch.python.clone(),
        stderr_tail: String::new(),
    }
}

/// Run commands, inbound messages, and deadlines until killed or abandoned.
async fn run_owner(
    mut owner: Owner,
    mut commands: mpsc::UnboundedReceiver<Command>,
    mut inbound: mpsc::UnboundedReceiver<Inbound>,
) {
    let mut restarting: Option<Restarting> = None;
    loop {
        let deadline = owner.pending_deadline();
        tokio::select! {
            command = commands.recv() => {
                let Some(command) = command else {
                    abort_and_cleanup(&mut owner, &mut restarting).await;
                    break;
                };
                match command {
                    Command::KernelInfo { reply } => handle_kernel_info(&mut owner, reply).await,
                    Command::Execute { code, reply } => {
                        handle_execute(&mut owner, code, reply).await;
                    }
                    Command::Interrupt { reply } => handle_interrupt(&mut owner, reply).await,
                    Command::Restart { reply } => handle_restart(&mut owner, &mut restarting, reply),
                    Command::Shutdown { reply } => {
                        handle_shutdown(&mut owner, &mut restarting, reply).await;
                    }
                    Command::Kill => {
                        abort_and_cleanup(&mut owner, &mut restarting).await;
                        break;
                    }
                }
            }
            Some(message) = inbound.recv() => route(&mut owner, message),
            joined = restart_finished(&mut restarting) => {
                if let Some(state) = restarting.take() {
                    apply_restart(&mut owner, joined, state.reply);
                }
            }
            () = sleep_until(deadline) => expire_pending(&mut owner),
            exit = wait_child_exited(owner.watched.as_ref().map(|watched| watched.exited.clone())),
                if matches!(owner.state, KernelState::Ready) =>
            {
                handle_child_exit(&mut owner, &mut inbound, exit);
            }
        }
    }
}

/// Wait for a running restart to finish, or forever when none runs.
async fn restart_finished(
    restarting: &mut Option<Restarting>,
) -> Result<Result<Started, SpawnError>, JoinError> {
    match restarting.as_mut() {
        Some(state) => (&mut state.task).await,
        None => std::future::pending().await,
    }
}

/// Abort a running restart and tell its caller it was closed.
///
/// `abort` only takes effect at the task's next `.await`, and a restart
/// inside the blocking `Watched::spawn` can still store a new child in
/// `current`, so this waits for the task to stop before anyone reads
/// `current`. A generation that started anyway is dropped here, which stops
/// its child.
async fn abort_restart(restarting: &mut Option<Restarting>) {
    if let Some(mut state) = restarting.take() {
        state.task.abort();
        let _ = (&mut state.task).await;
        let _ = state.reply.send(Err(KernelError::Closed));
    }
}

/// Abort a restart, kill the current child, and close, without waiting for
/// the child to exit.
async fn abort_and_cleanup(owner: &mut Owner, restarting: &mut Option<Restarting>) {
    abort_restart(restarting).await;
    if let Some(current) = take_current(owner) {
        signal_group(current.pid, libc::SIGKILL);
        let _ = std::fs::remove_file(&current.path);
    }
    // Dropping the watcher kills a child that still runs.
    drop_live(owner);
    finish_close(owner);
}

/// Forward one socket's messages to the owner until aborted.
async fn forward<S: SocketRecv>(
    mut socket: S,
    tx: mpsc::UnboundedSender<Inbound>,
    tag: impl Fn(Result<zeromq::ZmqMessage, zeromq::ZmqError>) -> Inbound,
) {
    loop {
        let received = socket.recv().await;
        if tx.send(tag(received)).is_err() {
            break;
        }
    }
}

/// Wait until the child ends, reporting how; never ends without a child.
///
/// A watch that closes without an exit counts as an unknown exit, so the
/// owner never spins on it.
async fn wait_child_exited(exited: Option<watch::Receiver<Option<ProcessExit>>>) -> ProcessExit {
    let Some(mut exited) = exited else {
        return std::future::pending().await;
    };
    let settled = exited.wait_for(Option::is_some).await.map(|exit| *exit);
    settled.ok().flatten().unwrap_or(ProcessExit::Unknown)
}

/// Take the current child, forgetting it in the registry.
fn take_current(owner: &Owner) -> Option<Current> {
    let taken = lock(&owner.current).take();
    if let Some(current) = taken.as_ref() {
        GLOBAL.unregister(current.pid);
    }
    taken
}

/// Fail every waiting info and interrupt request with `error`.
fn fail_pending(owner: &mut Owner, error: &KernelError) {
    for (_, (reply, _)) in owner.pending_info.drain() {
        let _ = reply.send(Err(error.clone()));
    }
    for (_, (reply, _)) in owner.pending_interrupt.drain() {
        let _ = reply.send(Err(error.clone()));
    }
}

/// Turn an unexpected child exit into one `Died` event.
///
/// Messages already queued for the owner are routed first. Frames still in
/// the socket buffers end with the generation, so no frame of the dead
/// kernel follows `Died`. The file and the current child are cleaned up
/// before `Died` is sent, so whoever sees it finds the kernel already dead.
fn handle_child_exit(
    owner: &mut Owner,
    inbound: &mut mpsc::UnboundedReceiver<Inbound>,
    exit: ProcessExit,
) {
    if !matches!(owner.state, KernelState::Ready) {
        return;
    }
    owner.stop_receivers();
    while let Ok(message) = inbound.try_recv() {
        route(owner, message);
    }
    let tail = owner
        .watched
        .as_ref()
        .map(|watched| {
            watched
                .stderr
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .tail()
        })
        .unwrap_or_default();
    let _ = take_current(owner);
    drop_live(owner);
    owner.state = next_state(&owner.state, KernelTransition::ChildExited { exit });
    fail_pending(owner, &KernelError::Dead { exit });
    send_event(
        owner,
        KernelEvent::Died {
            exit,
            stderr_tail: tail,
        },
    );
}

/// Sleep until a pending deadline, or never when idle.
async fn sleep_until(deadline: Option<Instant>) {
    match deadline {
        Some(at) => tokio::time::sleep_until(at).await,
        None => std::future::pending().await,
    }
}

impl Owner {
    /// Earliest pending deadline, if any request waits.
    fn pending_deadline(&self) -> Option<Instant> {
        self.pending_info
            .values()
            .map(|(_, at)| *at)
            .chain(self.pending_interrupt.values().map(|(_, at)| *at))
            .min()
    }

    /// Explain a failed send, preferring the exit when the child already
    /// ended and the owner has not seen it yet.
    fn send_failure(&self, error: KernelError) -> KernelError {
        match self
            .watched
            .as_ref()
            .and_then(|watched| *watched.exited.borrow())
        {
            Some(exit) => KernelError::Dead { exit },
            None => error,
        }
    }

    /// Install a generation that finished its handshake, returning its info.
    ///
    /// Its receive tasks tag messages with the current generation.
    fn adopt(&mut self, started: Started) -> KernelInfo {
        let Started {
            sockets,
            watched,
            conn,
            key,
            session,
            info,
        } = started;
        let gen = self.generation;
        let tx = &self.inbound_tx;
        self.receivers = vec![
            tokio::spawn(forward(sockets.shell_recv, tx.clone(), move |received| {
                Inbound::Shell(gen, received)
            })),
            tokio::spawn(forward(sockets.iopub, tx.clone(), move |received| {
                Inbound::Iopub(gen, received)
            })),
            tokio::spawn(forward(sockets.control_recv, tx.clone(), move |received| {
                Inbound::Control(gen, received)
            })),
        ];
        self.live = Some(LiveSockets {
            shell_send: sockets.shell_send,
            control_send: sockets.control_send,
            _stdin: sockets.stdin,
            _hb: sockets.hb,
        });
        self.watched = Some(watched);
        self.conn = Some(conn);
        self.key = key;
        self.session = session;
        info
    }

    /// Abort the receive tasks of the current generation.
    ///
    /// An abort is remembered, so a task busy forwarding when the generation
    /// ends still stops at its next `.await`, and [`route`] drops what it
    /// sent meanwhile.
    fn stop_receivers(&mut self) {
        for task in self.receivers.drain(..) {
            task.abort();
        }
    }

    /// End the current generation so none of its messages pass any more.
    fn end_generation(&mut self) {
        self.stop_receivers();
        self.generation += 1;
    }
}

/// Route one inbound message, dropping those of ended generations.
fn route(owner: &mut Owner, message: Inbound) {
    match message {
        Inbound::Shell(gen, received) if gen == owner.generation => handle_shell(owner, received),
        Inbound::Iopub(gen, received) if gen == owner.generation => {
            handle_channel(owner, Channel::Iopub, received);
        }
        Inbound::Control(gen, received) if gen == owner.generation => {
            handle_channel(owner, Channel::Control, received);
        }
        _ => {}
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

/// Route one iopub or control message to its waiter or the event channel.
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
    match convert(channel, &message) {
        Converted::Event(event) => send_event(owner, event),
        Converted::InterruptReply { parent } => {
            let Some(id) = parent.map(|parent| parent.as_str().to_owned()) else {
                return;
            };
            if let Some((reply, _)) = owner.pending_interrupt.remove(&id) {
                let _ = reply.send(Ok(()));
            }
        }
        _ => {}
    }
}

/// Admit a request that only a ready kernel answers.
fn admitted(state: &KernelState, op: KernelOp) -> Result<(), KernelError> {
    match admit(state, op) {
        Admission::Send => Ok(()),
        Admission::Reject(error) => Err(error),
        _ => Err(KernelError::Closed),
    }
}

/// An empty JSON object.
fn empty() -> serde_json::Value {
    serde_json::Value::Object(serde_json::Map::new())
}

/// Frame a request on `session`, returning its message id and frames.
fn frame(
    msg_type: &str,
    session: &str,
    key: &[u8],
    content: serde_json::Value,
) -> (String, Vec<Vec<u8>>) {
    let header = header(msg_type, session);
    let msg_id = header.msg_id.clone();
    let message = WireMessage {
        identities: Vec::new(),
        header,
        parent_header: None,
        metadata: empty(),
        content,
        buffers: Vec::new(),
    };
    (msg_id, encode(&message, key))
}

/// Send a request, reporting a failed socket as [`KernelError::Io`].
async fn send_on(
    socket: Option<&mut zeromq::DealerSendHalf>,
    frames: Vec<Vec<u8>>,
) -> Result<(), KernelError> {
    let Some(socket) = socket else {
        return Err(KernelError::Closed);
    };
    send_zmq(socket, frames)
        .await
        .map_err(|error| KernelError::Io {
            message: error.to_string(),
        })
}

/// Run code when admitted, answering with the request id at once.
async fn handle_execute(
    owner: &mut Owner,
    code: String,
    reply: oneshot::Sender<Result<MsgId, KernelError>>,
) {
    if let Err(error) = admitted(&owner.state, KernelOp::Execute) {
        let _ = reply.send(Err(error));
        return;
    }
    let content = serde_json::json!({
        "silent": false,
        "store_history": true,
        "user_expressions": {},
        "allow_stdin": false,
        "stop_on_error": true,
        "code": code,
    });
    let (msg_id, frames) = frame("execute_request", &owner.session, &owner.key, content);
    let shell = owner.live.as_mut().map(|live| &mut live.shell_send);
    let sent = send_on(shell, frames).await;
    let answer = match sent {
        Ok(()) => Ok(MsgId::new(msg_id)),
        Err(error) => Err(owner.send_failure(error)),
    };
    let _ = reply.send(answer);
}

/// Interrupt the running execution when admitted.
async fn handle_interrupt(owner: &mut Owner, reply: oneshot::Sender<Result<(), KernelError>>) {
    if let Err(error) = admitted(&owner.state, KernelOp::Interrupt) {
        let _ = reply.send(Err(error));
        return;
    }
    let (msg_id, frames) = frame("interrupt_request", &owner.session, &owner.key, empty());
    let control = owner.live.as_mut().map(|live| &mut live.control_send);
    match send_on(control, frames).await {
        Ok(()) => {
            let deadline = Instant::now() + owner.request_timeout;
            owner.pending_interrupt.insert(msg_id, (reply, deadline));
        }
        Err(error) => {
            let _ = reply.send(Err(owner.send_failure(error)));
        }
    }
}

/// Ask for the kernel description when admitted.
async fn handle_kernel_info(
    owner: &mut Owner,
    reply: oneshot::Sender<Result<KernelInfo, KernelError>>,
) {
    if let Err(error) = admitted(&owner.state, KernelOp::KernelInfo) {
        let _ = reply.send(Err(error));
        return;
    }
    let (msg_id, frames) = frame("kernel_info_request", &owner.session, &owner.key, empty());
    let shell = owner.live.as_mut().map(|live| &mut live.shell_send);
    match send_on(shell, frames).await {
        Ok(()) => {
            let deadline = Instant::now() + owner.request_timeout;
            owner.pending_info.insert(msg_id, (reply, deadline));
        }
        Err(error) => {
            let _ = reply.send(Err(owner.send_failure(error)));
        }
    }
}

/// Restart the kernel, stopping the old generation in a separate task.
///
/// Requests still waiting on the old kernel fail with
/// [`KernelError::Restarting`], since its replies are dropped from now on.
fn handle_restart(
    owner: &mut Owner,
    restarting: &mut Option<Restarting>,
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
                // `Ready` always holds all three; a partial set is dropped
                // here, and dropping its watcher stops the child.
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
    owner.end_generation();
    fail_pending(owner, &KernelError::Restarting);
    let task = tokio::spawn(run_restart(
        owner.launch.clone(),
        stop_old,
        Arc::clone(&owner.current),
        owner.shutdown_grace,
    ));
    *restarting = Some(Restarting { task, reply });
}

/// Stop the old generation, if any, then start a new one.
async fn run_restart(
    launch: KernelLaunch,
    stop_old: Option<OldLive>,
    current: SharedCurrent,
    grace: Duration,
) -> Result<Started, SpawnError> {
    if let Some(old) = stop_old {
        stop_old_generation(old, &current, grace).await;
    }
    start(&launch, &current).await
}

/// Stop one old generation with a shutdown request, then forcefully.
///
/// Once the wait ends, its file is removed and `current` is cleared, so a
/// cancelled restart does not signal the old group later.
async fn stop_old_generation(old: OldLive, current: &SharedCurrent, grace: Duration) {
    let OldLive {
        mut live,
        mut watched,
        conn,
        key,
        session,
    } = old;
    let (_, frames) = frame(
        "shutdown_request",
        &session,
        &key,
        serde_json::json!({"restart": false}),
    );
    let _ = send_zmq(&mut live.control_send, frames).await;
    see_child_end(&mut watched, grace).await;
    let _ = conn.remove();
    *lock(current) = None;
}

/// Apply a finished restart to the owner.
fn apply_restart(
    owner: &mut Owner,
    joined: Result<Result<Started, SpawnError>, JoinError>,
    reply: oneshot::Sender<Result<KernelInfo, KernelError>>,
) {
    match joined {
        Ok(Ok(started)) => {
            let info = owner.adopt(started);
            owner.state = next_state(&owner.state, KernelTransition::RestartSucceeded);
            send_event(owner, KernelEvent::Restarted { info: info.clone() });
            let _ = reply.send(Ok(info));
        }
        Ok(Err(error)) => {
            owner.state = next_state(&owner.state, KernelTransition::RestartFailed);
            let _ = reply.send(Err(KernelError::Spawn(error)));
        }
        Err(error) => {
            // The panicking task dropped its watcher, which killed its
            // child; forget the child and remove its file here.
            if let Some(current) = take_current(owner) {
                signal_group(current.pid, libc::SIGKILL);
                let _ = std::fs::remove_file(&current.path);
            }
            owner.state = next_state(&owner.state, KernelTransition::RestartFailed);
            let _ = reply.send(Err(KernelError::Spawn(SpawnError {
                kind: SpawnErrorKind::ConnectFailed {
                    message: format!("restart task failed: {error}"),
                },
                python: owner.launch.python.clone(),
                stderr_tail: String::new(),
            })));
        }
    }
}

/// Stop the kernel and close.
///
/// A ready kernel gets a shutdown request and the grace period; past it,
/// its group is killed and the exit waited for up to [`EXIT_WAIT`] more. A
/// restart in progress is cancelled, and whichever child it holds is killed
/// and waited for the same way.
async fn handle_shutdown(
    owner: &mut Owner,
    restarting: &mut Option<Restarting>,
    reply: oneshot::Sender<Result<(), KernelError>>,
) {
    match admit(&owner.state, KernelOp::Shutdown) {
        Admission::StopAndClose => {
            let (_, frames) = frame(
                "shutdown_request",
                &owner.session,
                &owner.key,
                serde_json::json!({"restart": false}),
            );
            if let Some(live) = owner.live.as_mut() {
                let _ = send_zmq(&mut live.control_send, frames).await;
            }
            if let Some(watched) = owner.watched.as_mut() {
                see_child_end(watched, owner.shutdown_grace).await;
            }
        }
        Admission::CleanUpDead => {}
        Admission::AlreadyClosed => {
            let _ = reply.send(Ok(()));
            return;
        }
        Admission::AbortRestartAndStop => {
            abort_restart(restarting).await;
            if let Some(current) = take_current(owner) {
                stop_current(current).await;
            }
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
    drop_live(owner);
    finish_close(owner);
    let _ = reply.send(Ok(()));
}

/// Give a child asked to exit the grace period, then kill its group and
/// wait up to [`EXIT_WAIT`] for the exit.
async fn see_child_end(watched: &mut Watched, grace: Duration) {
    if watched.wait_exit(grace).await.is_none() {
        watched.kill_group();
        let _ = watched.wait_exit(EXIT_WAIT).await;
    }
}

/// Kill the child a cancelled restart left behind and wait up to
/// [`EXIT_WAIT`] for its exit.
async fn stop_current(current: Current) {
    signal_group(current.pid, libc::SIGKILL);
    let mut exited = current.exited;
    let _ = tokio::time::timeout(EXIT_WAIT, exited.wait_for(Option::is_some)).await;
    let _ = std::fs::remove_file(&current.path);
}

/// Drop the current generation: receive tasks, sockets, watcher, and file.
fn drop_live(owner: &mut Owner) {
    owner.end_generation();
    owner.live = None;
    owner.watched = None;
    if let Some(conn) = owner.conn.take() {
        let _ = conn.remove();
    }
}

/// Forget the current child, fail waiters, and close the state.
fn finish_close(owner: &mut Owner) {
    let _ = take_current(owner);
    fail_pending(owner, &KernelError::Closed);
    owner.state = next_state(&owner.state, KernelTransition::Closed);
}

/// Fail requests past their deadline.
fn expire_pending(owner: &mut Owner) {
    let now = Instant::now();
    expire(&mut owner.pending_info, now, KernelOp::KernelInfo);
    expire(&mut owner.pending_interrupt, now, KernelOp::Interrupt);
}

/// Fail the requests in `pending` whose deadline passed.
fn expire<T>(pending: &mut HashMap<String, Pending<T>>, now: Instant, op: KernelOp) {
    for (_, (reply, _)) in pending.extract_if(|_, (_, at)| *at <= now) {
        let _ = reply.send(Err(KernelError::Timeout { op }));
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
        .map(|part| part.to_vec())
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
