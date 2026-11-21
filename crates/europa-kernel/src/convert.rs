//! Convert wire messages into kernel events.
//!
//! The channel and the message type decide first; content is only read for
//! the twelve known kinds. Anything else becomes a warning, except the
//! shutdown reply on iopub, which is silently ignored.

use europa_core::{KernelEvent, KernelInfo, MsgId};

use crate::wire::{DecodeError, WireMessage};

/// Channel a message arrived on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Channel {
    /// Shell channel for `execute_reply` and `kernel_info_reply`.
    Shell,
    /// Iopub channel for status, outputs, and execution counts.
    Iopub,
    /// Control channel for `interrupt_reply` and `shutdown_reply`.
    Control,
}

/// Outcome of converting one wire message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Converted {
    /// A kernel event for the channel handed to callers.
    Event(KernelEvent),
    /// A `kernel_info_reply`, kept for the waiting request.
    KernelInfoReply {
        /// Parent message id, if the reply carries one.
        parent: Option<MsgId>,
        /// Kernel description, or the failure text.
        result: Result<KernelInfo, String>,
    },
    /// An `interrupt_reply`, kept for the waiting request.
    InterruptReply {
        /// Parent message id, if the reply carries one.
        parent: Option<MsgId>,
    },
    /// A `shutdown_reply`, kept for the waiting request.
    ShutdownReply {
        /// Parent message id, if the reply carries one.
        parent: Option<MsgId>,
    },
    /// A message to drop without a warning.
    Ignored,
}

/// Convert one wire message into an event or a pending reply.
pub(crate) fn convert(channel: Channel, msg: &WireMessage) -> Converted {
    let _ = (channel, msg);
    todo!()
}

/// Turn a decoding failure into a warning event.
pub(crate) fn warning(error: &DecodeError) -> KernelEvent {
    let _ = error;
    todo!()
}
