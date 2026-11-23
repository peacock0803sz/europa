//! Window onto a running kernel.
//!
//! [`KernelClient`] is the window callers drive kernels through. Callers send
//! requests through it and read [`KernelEvent`] from the channel handed out by
//! [`KernelClient::messages`]. Every request method takes `&self`, so a
//! restart in progress can still be asked about and answered with
//! [`KernelError::Restarting`].

use std::future::Future;

use europa_core::{KernelError, KernelEvent, KernelInfo, MsgId};

/// Window onto a running kernel.
///
/// The trait carries no data types of its own; every value crossing it is a
/// core type or a standard library type. Implementations keep their sockets
/// and child processes private and report everything as [`KernelEvent`].
pub trait KernelClient {
    /// Ask for the kernel description.
    ///
    /// Sends `kernel_info_request` on the shell channel and waits for the
    /// reply up to the launch `request_timeout`. Returns the reply content.
    fn kernel_info(&self) -> impl Future<Output = Result<KernelInfo, KernelError>> + Send;

    /// Run code and return the request id without waiting for results.
    ///
    /// Results arrive on the [`KernelClient::messages`] channel as events
    /// carrying this id as their parent.
    fn execute(&self, code: &str) -> impl Future<Output = Result<MsgId, KernelError>> + Send;

    /// Interrupt the running execution.
    ///
    /// Sends `interrupt_request` on the control channel and waits for the
    /// reply up to the launch `request_timeout`. The kernel keeps running.
    fn interrupt(&self) -> impl Future<Output = Result<(), KernelError>> + Send;

    /// Stop the kernel and start it again with the same launch arguments.
    ///
    /// Emits [`KernelEvent::Restarted`] with the new kernel description and
    /// returns the same value. Messages from the old kernel are dropped once
    /// the restart begins, so runs in flight get no reply or idle status,
    /// and `kernel_info` or `interrupt` calls still waiting fail with
    /// [`KernelError::Restarting`]. Requests made while restarting fail the
    /// same way instead of queueing, except `shutdown`, which cancels the
    /// restart so that it returns [`KernelError::Closed`]. On failure it
    /// returns [`KernelError::Spawn`] and leaves the kernel dead until the
    /// next restart.
    fn restart(&self) -> impl Future<Output = Result<KernelInfo, KernelError>> + Send;

    /// Ask the kernel to exit and wait until it is gone.
    ///
    /// Sends `shutdown_request` on the control channel. If the child still
    /// runs past the launch `shutdown_grace`, it sends `SIGKILL` to the
    /// process group and waits up to five more seconds; it returns `Ok`
    /// after that even if the child has not ended. During a restart it
    /// cancels the restart and kills whichever child the restart holds,
    /// with the same wait. A second call succeeds without doing anything.
    fn shutdown(&self) -> impl Future<Output = Result<(), KernelError>> + Send;

    /// Take the event channel, only the first call returns `Some`.
    ///
    /// Iopub and shell events arrive in the order they were received. The
    /// channel is unbounded, so unread events are kept without blocking the
    /// receiver task.
    fn messages(&mut self) -> Option<std::sync::mpsc::Receiver<KernelEvent>>;
}
