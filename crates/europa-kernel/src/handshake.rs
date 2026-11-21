//! Socket connection and the three-step handshake.
//!
//! After spawning, five sockets connect to the kernel and three signals are
//! awaited: a `kernel_info_reply` on shell, any message on iopub, and a
//! heartbeat echo. iopub delivery proves the subscription is live, so the
//! first execution cannot lose its `busy` or outputs.

use std::time::Duration;

use europa_core::{KernelInfo, ProcessExit, SpawnErrorKind};
use zeromq::{Socket, SocketRecv, SocketSend};

use crate::connection::Ports;
use crate::convert::{convert, Channel, Converted};
use crate::process::Watched;
use crate::wire::{decode, encode, header, WireMessage};

/// Connected sockets for one kernel.
///
/// Send halves stay with the owner task; receive halves move into dedicated
/// tasks, so a slow channel never blocks the others behind one lock.
pub(crate) struct Sockets {
    /// Shell channel sender for requests.
    pub(crate) shell_send: zeromq::DealerSendHalf,
    /// Shell channel receiver, moved into its task.
    pub(crate) shell_recv: zeromq::DealerRecvHalf,
    /// Control channel sender for interrupt and shutdown.
    pub(crate) control_send: zeromq::DealerSendHalf,
    /// Control channel receiver, moved into its task.
    pub(crate) control_recv: zeromq::DealerRecvHalf,
    /// Stdin channel, connected but never read.
    pub(crate) stdin: zeromq::DealerSocket,
    /// Iopub channel for status and outputs, moved into its task.
    pub(crate) iopub: zeromq::SubSocket,
    /// Heartbeat channel, used only during the handshake.
    pub(crate) hb: zeromq::ReqSocket,
}

impl Sockets {
    /// Connect all five sockets to the kernel ports.
    ///
    /// The peer may not have bound yet; `zeromq` retries refused
    /// connections internally, so one call per socket is enough.
    async fn connect(ports: &Ports) -> Result<Self, String> {
        let mut shell = zeromq::DealerSocket::new();
        shell
            .connect(&format!("tcp://127.0.0.1:{}", ports.shell))
            .await
            .map_err(|error| error.to_string())?;
        let mut control = zeromq::DealerSocket::new();
        control
            .connect(&format!("tcp://127.0.0.1:{}", ports.control))
            .await
            .map_err(|error| error.to_string())?;
        let mut stdin = zeromq::DealerSocket::new();
        stdin
            .connect(&format!("tcp://127.0.0.1:{}", ports.stdin))
            .await
            .map_err(|error| error.to_string())?;
        let mut iopub = zeromq::SubSocket::new();
        iopub
            .connect(&format!("tcp://127.0.0.1:{}", ports.iopub))
            .await
            .map_err(|error| error.to_string())?;
        iopub
            .subscribe("")
            .await
            .map_err(|error| error.to_string())?;
        let mut hb = zeromq::ReqSocket::new();
        hb.connect(&format!("tcp://127.0.0.1:{}", ports.hb))
            .await
            .map_err(|error| error.to_string())?;
        let (shell_send, shell_recv) = shell.split();
        let (control_send, control_recv) = control.split();
        Ok(Self {
            shell_send,
            shell_recv,
            control_send,
            control_recv,
            stdin,
            iopub,
            hb,
        })
    }
}

/// Run the handshake: connect, then wait for all three signals.
///
/// Connecting and probing share one deadline, and an early child exit
/// beats the deadline with [`SpawnErrorKind::EarlyExit`].
pub(crate) async fn handshake(
    ports: &Ports,
    key: &[u8],
    session: &str,
    watched: &mut Watched,
    timeout: Duration,
) -> Result<(Sockets, KernelInfo), SpawnErrorKind> {
    let exited = watched.exited.clone();
    tokio::select! {
        result = run_handshake(ports, key, session, timeout) => result,
        exit = wait_child(exited) => Err(SpawnErrorKind::EarlyExit { exit }),
    }
}

/// Wait until the child ends and report how.
async fn wait_child(mut exited: tokio::sync::watch::Receiver<Option<ProcessExit>>) -> ProcessExit {
    let _ = exited.wait_for(|exit| exit.is_some()).await;
    exited.borrow().unwrap_or(ProcessExit::Unknown)
}

/// Connect and probe within one deadline.
async fn run_handshake(
    ports: &Ports,
    key: &[u8],
    session: &str,
    timeout: Duration,
) -> Result<(Sockets, KernelInfo), SpawnErrorKind> {
    match tokio::time::timeout(timeout, probe(ports, key, session)).await {
        Ok(result) => result,
        Err(_) => Err(SpawnErrorKind::HandshakeTimeout { after: timeout }),
    }
}

/// Send requests until shell, iopub, and heartbeat all answer.
///
/// The heartbeat round trip runs as one pinned future: cancelling a `REQ`
/// receive drops its in-flight request, so it is never re-polled.
async fn probe(
    ports: &Ports,
    key: &[u8],
    session: &str,
) -> Result<(Sockets, KernelInfo), SpawnErrorKind> {
    let mut sockets = Sockets::connect(ports)
        .await
        .map_err(|message| SpawnErrorKind::ConnectFailed { message })?;
    send_kernel_info(&mut sockets.shell_send, key, session).await?;
    let idle_hb = zeromq::ReqSocket::new();
    let mut hb = std::mem::replace(&mut sockets.hb, idle_hb);
    let mut hb_roundtrip = Box::pin(async move {
        hb.send(ping())
            .await
            .map_err(|error| SpawnErrorKind::ConnectFailed {
                message: error.to_string(),
            })?;
        let echo = hb
            .recv()
            .await
            .map_err(|error| SpawnErrorKind::ConnectFailed {
                message: error.to_string(),
            })?;
        let frames: Vec<Vec<u8>> = echo
            .into_vec()
            .into_iter()
            .map(|frame| frame.to_vec())
            .collect();
        Ok::<_, SpawnErrorKind>((hb, frames == vec![b"ping".to_vec()]))
    });
    let mut info = None;
    let mut iopub_seen = false;
    let mut hb_seen = false;
    let mut resend = tokio::time::interval(Duration::from_secs(1));
    resend.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        if info.is_some() && iopub_seen && hb_seen {
            break;
        }
        tokio::select! {
            incoming = sockets.shell_recv.recv() => {
                let frames = into_frames(incoming)?;
                let Ok(message) = decode(&frames, key) else {
                    continue;
                };
                match convert(Channel::Shell, &message) {
                    Converted::KernelInfoReply { result: Ok(found), .. } => {
                        info = Some(found);
                    }
                    Converted::KernelInfoReply { result: Err(detail), .. } => {
                        return Err(SpawnErrorKind::ConnectFailed { message: detail });
                    }
                    _ => {}
                }
            }
            incoming = sockets.iopub.recv() => {
                let frames = into_frames(incoming)?;
                if decode(&frames, key).is_ok() {
                    iopub_seen = true;
                }
            }
            result = &mut hb_roundtrip, if !hb_seen => {
                let (back, ok) = result?;
                sockets.hb = back;
                hb_seen = ok;
            }
            _ = resend.tick(), if info.is_none() => {
                send_kernel_info(&mut sockets.shell_send, key, session).await?;
            }
        }
    }
    Ok((sockets, info.expect("checked above")))
}

/// Send one `kernel_info_request` on a shell sender.
async fn send_kernel_info(
    shell: &mut zeromq::DealerSendHalf,
    key: &[u8],
    session: &str,
) -> Result<(), SpawnErrorKind> {
    let message = WireMessage {
        identities: Vec::new(),
        header: header("kernel_info_request", session),
        parent_header: None,
        metadata: serde_json::Value::Object(Default::default()),
        content: serde_json::Value::Object(Default::default()),
        buffers: Vec::new(),
    };
    let frames = encode(&message, key);
    shell
        .send(into_zmq(frames))
        .await
        .map_err(|error| SpawnErrorKind::ConnectFailed {
            message: error.to_string(),
        })
}

/// Heartbeat ping content.
fn ping() -> zeromq::ZmqMessage {
    into_zmq(vec![b"ping".to_vec()])
}

/// Pack frames into a socket message.
fn into_zmq(frames: Vec<Vec<u8>>) -> zeromq::ZmqMessage {
    use bytes::Bytes;
    let parts: Vec<Bytes> = frames.into_iter().map(Bytes::from).collect();
    zeromq::ZmqMessage::try_from(parts).expect("handshake sends frames")
}

/// Unpack a socket message into frames.
fn into_frames(
    incoming: Result<zeromq::ZmqMessage, zeromq::ZmqError>,
) -> Result<Vec<Vec<u8>>, SpawnErrorKind> {
    incoming
        .map(|message| {
            message
                .into_vec()
                .into_iter()
                .map(|frame| frame.to_vec())
                .collect()
        })
        .map_err(|error| SpawnErrorKind::ConnectFailed {
            message: error.to_string(),
        })
}
