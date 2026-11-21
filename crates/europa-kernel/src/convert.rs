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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::Header;
    use europa_core::{IopubMsg, JsonMap, KernelStatus, ProcessExit, ReplyStatus, WarningReason};
    use serde_json::json;

    fn parent(msg_id: &str) -> Header {
        Header {
            msg_id: msg_id.to_owned(),
            session: "sess".to_owned(),
            username: "europa".to_owned(),
            date: "2026-01-02T03:04:05.000006Z".to_owned(),
            msg_type: "execute_request".to_owned(),
            version: "5.3".to_owned(),
        }
    }

    fn wire(
        channel_msg_type: &str,
        content: serde_json::Value,
        parent_id: Option<&str>,
    ) -> WireMessage {
        WireMessage {
            identities: Vec::new(),
            header: Header {
                msg_id: "m".to_owned(),
                session: "sess".to_owned(),
                username: "kernel".to_owned(),
                date: "2026-01-02T03:04:05.000006Z".to_owned(),
                msg_type: channel_msg_type.to_owned(),
                version: "5.3".to_owned(),
            },
            parent_header: parent_id.map(parent),
            metadata: json!({}),
            content,
            buffers: Vec::new(),
        }
    }

    fn info_json() -> serde_json::Value {
        json!({
            "status": "ok",
            "protocol_version": "5.3",
            "implementation": "ipython",
            "implementation_version": "9.7.0",
            "language_info": {"name": "python", "version": "3.12.0", "file_extension": "py"},
            "banner": ""
        })
    }

    #[test]
    fn maps_iopub_outputs() {
        let stream = wire(
            "stream",
            json!({"name": "stdout", "text": "hi\n"}),
            Some("p1"),
        );
        assert_eq!(
            convert(Channel::Iopub, &stream),
            Converted::Event(KernelEvent::Iopub(IopubMsg::Stream {
                parent: MsgId::new("p1"),
                name: "stdout".to_owned(),
                text: "hi\n".to_owned(),
            }))
        );
        let plain: JsonMap = [("text/plain".to_owned(), "x".into())].into();
        let display = wire(
            "display_data",
            json!({"data": {"text/plain": "x"}, "metadata": {}, "transient": {"display_id": "d1"}}),
            Some("p1"),
        );
        assert_eq!(
            convert(Channel::Iopub, &display),
            Converted::Event(KernelEvent::Iopub(IopubMsg::DisplayData {
                parent: MsgId::new("p1"),
                data: plain.clone(),
                metadata: JsonMap::new(),
                display_id: Some("d1".to_owned()),
            }))
        );
        let bare = wire(
            "display_data",
            json!({"data": {"text/plain": "x"}, "metadata": {}}),
            Some("p1"),
        );
        assert_eq!(
            convert(Channel::Iopub, &bare),
            Converted::Event(KernelEvent::Iopub(IopubMsg::DisplayData {
                parent: MsgId::new("p1"),
                data: plain.clone(),
                metadata: JsonMap::new(),
                display_id: None,
            }))
        );
        let empty = wire(
            "display_data",
            json!({"data": {"text/plain": "x"}, "metadata": {}, "transient": {"display_id": ""}}),
            Some("p1"),
        );
        assert_eq!(
            convert(Channel::Iopub, &empty),
            Converted::Event(KernelEvent::Iopub(IopubMsg::DisplayData {
                parent: MsgId::new("p1"),
                data: plain,
                metadata: JsonMap::new(),
                display_id: Some(String::new()),
            }))
        );
        let update = wire(
            "update_display_data",
            json!({"data": {"text/plain": "y"}, "metadata": {}, "transient": {"display_id": "d1"}}),
            Some("p2"),
        );
        assert_eq!(
            convert(Channel::Iopub, &update),
            Converted::Event(KernelEvent::Iopub(IopubMsg::UpdateDisplayData {
                parent: MsgId::new("p2"),
                data: [("text/plain".to_owned(), "y".into())].into(),
                metadata: JsonMap::new(),
                display_id: "d1".to_owned(),
            }))
        );
        let result = wire(
            "execute_result",
            json!({"execution_count": 2, "data": {"text/plain": "42"}, "metadata": {}}),
            Some("p1"),
        );
        assert_eq!(
            convert(Channel::Iopub, &result),
            Converted::Event(KernelEvent::Iopub(IopubMsg::ExecuteResult {
                parent: MsgId::new("p1"),
                data: [("text/plain".to_owned(), "42".into())].into(),
                metadata: JsonMap::new(),
                execution_count: Some(2),
            }))
        );
        let error = wire(
            "error",
            json!({"ename": "E", "evalue": "v", "traceback": ["t"]}),
            Some("p1"),
        );
        assert_eq!(
            convert(Channel::Iopub, &error),
            Converted::Event(KernelEvent::Iopub(IopubMsg::Error {
                parent: MsgId::new("p1"),
                ename: "E".to_owned(),
                evalue: "v".to_owned(),
                traceback: vec!["t".to_owned()],
            }))
        );
        for wait in [true, false] {
            let clear = wire("clear_output", json!({"wait": wait}), Some("p1"));
            assert_eq!(
                convert(Channel::Iopub, &clear),
                Converted::Event(KernelEvent::Iopub(IopubMsg::ClearOutput {
                    parent: MsgId::new("p1"),
                    wait,
                }))
            );
        }
        let input = wire(
            "execute_input",
            json!({"code": "a=1", "execution_count": 3}),
            Some("p1"),
        );
        assert_eq!(
            convert(Channel::Iopub, &input),
            Converted::Event(KernelEvent::Iopub(IopubMsg::ExecuteInput {
                parent: MsgId::new("p1"),
                execution_count: 3,
            }))
        );
    }

    #[test]
    fn maps_status_states() {
        for (state, expected) in [
            ("busy", KernelStatus::Busy),
            ("idle", KernelStatus::Idle),
            ("starting", KernelStatus::Starting),
        ] {
            let msg = wire("status", json!({"execution_state": state}), Some("p1"));
            assert_eq!(
                convert(Channel::Iopub, &msg),
                Converted::Event(KernelEvent::Status {
                    parent: Some(MsgId::new("p1")),
                    state: expected,
                })
            );
        }
        let bare = wire("status", json!({"execution_state": "idle"}), None);
        assert_eq!(
            convert(Channel::Iopub, &bare),
            Converted::Event(KernelEvent::Status {
                parent: None,
                state: KernelStatus::Idle,
            })
        );
        let dead = wire("status", json!({"execution_state": "dead"}), Some("p1"));
        assert_eq!(
            convert(Channel::Iopub, &dead),
            Converted::Event(KernelEvent::Warning {
                reason: WarningReason::Malformed,
                msg_type: Some("status".to_owned()),
                detail: "unknown execution_state".to_owned(),
            })
        );
    }

    #[test]
    fn rejects_outputs_without_a_parent() {
        let msg = wire("stream", json!({"name": "stdout", "text": "hi"}), None);
        assert_eq!(
            convert(Channel::Iopub, &msg),
            Converted::Event(KernelEvent::Warning {
                reason: WarningReason::Malformed,
                msg_type: Some("stream".to_owned()),
                detail: "missing parent".to_owned(),
            })
        );
    }

    #[test]
    fn maps_execute_replies() {
        for (status, expected) in [
            ("ok", ReplyStatus::Ok),
            ("error", ReplyStatus::Error),
            ("aborted", ReplyStatus::Aborted),
        ] {
            let reply = wire(
                "execute_reply",
                json!({"status": status, "execution_count": 1}),
                Some("p1"),
            );
            assert_eq!(
                convert(Channel::Shell, &reply),
                Converted::Event(KernelEvent::ExecuteReply {
                    parent: MsgId::new("p1"),
                    status: expected,
                    execution_count: Some(1),
                })
            );
        }
        let weird = wire("execute_reply", json!({"status": "weird"}), Some("p1"));
        assert_eq!(
            convert(Channel::Shell, &weird),
            Converted::Event(KernelEvent::Warning {
                reason: WarningReason::Malformed,
                msg_type: Some("execute_reply".to_owned()),
                detail: "unknown status".to_owned(),
            })
        );
    }

    #[test]
    fn maps_kernel_info_replies() {
        let ok = wire("kernel_info_reply", info_json(), Some("p1"));
        let converted = convert(Channel::Shell, &ok);
        let Converted::KernelInfoReply {
            parent,
            result: Ok(info),
        } = converted
        else {
            panic!("expected ok reply, got {converted:?}");
        };
        assert_eq!(parent, Some(MsgId::new("p1")));
        assert_eq!(info.protocol_version, "5.3");
        assert_eq!(info.language.name, "python");
        assert_eq!(info.language.file_extension, Some("py".to_owned()));
        let mut without_extension = info_json();
        without_extension["language_info"]
            .as_object_mut()
            .expect("object")
            .remove("file_extension");
        let missing = wire("kernel_info_reply", without_extension, Some("p1"));
        let Converted::KernelInfoReply {
            result: Ok(info), ..
        } = convert(Channel::Shell, &missing)
        else {
            panic!("expected ok reply without extension");
        };
        assert_eq!(info.language.file_extension, None);
        let failed = wire(
            "kernel_info_reply",
            json!({"status": "error", "ename": "E", "evalue": "v", "traceback": []}),
            Some("p1"),
        );
        assert!(matches!(
            convert(Channel::Shell, &failed),
            Converted::KernelInfoReply { result: Err(_), .. }
        ));
    }

    #[test]
    fn maps_control_replies() {
        let interrupt = wire("interrupt_reply", json!({"status": "ok"}), Some("p1"));
        assert_eq!(
            convert(Channel::Control, &interrupt),
            Converted::InterruptReply {
                parent: Some(MsgId::new("p1")),
            }
        );
        let shutdown = wire(
            "shutdown_reply",
            json!({"status": "ok", "restart": false}),
            Some("p1"),
        );
        assert_eq!(
            convert(Channel::Control, &shutdown),
            Converted::ShutdownReply {
                parent: Some(MsgId::new("p1")),
            }
        );
    }

    #[test]
    fn warns_on_unknown_types() {
        let comm = wire(
            "comm_open",
            json!({"comm_id": "c", "target_name": "t", "data": {}}),
            Some("p1"),
        );
        assert_eq!(
            convert(Channel::Iopub, &comm),
            Converted::Event(KernelEvent::Warning {
                reason: WarningReason::UnknownType,
                msg_type: Some("comm_open".to_owned()),
                detail: String::new(),
            })
        );
        let info = wire(
            "comm_info_reply",
            json!({"status": "ok", "comms": {}}),
            Some("p1"),
        );
        assert_eq!(
            convert(Channel::Shell, &info),
            Converted::Event(KernelEvent::Warning {
                reason: WarningReason::UnknownType,
                msg_type: Some("comm_info_reply".to_owned()),
                detail: String::new(),
            })
        );
    }

    #[test]
    fn ignores_iopub_shutdown_replies() {
        let msg = wire(
            "shutdown_reply",
            json!({"status": "ok", "restart": false}),
            Some("p1"),
        );
        assert_eq!(convert(Channel::Iopub, &msg), Converted::Ignored);
    }

    #[test]
    fn warns_on_malformed_content_without_panicking() {
        let broken = wire("stream", json!({"name": "stdout"}), Some("p1"));
        assert_eq!(
            convert(Channel::Iopub, &broken),
            Converted::Event(KernelEvent::Warning {
                reason: WarningReason::Malformed,
                msg_type: Some("stream".to_owned()),
                detail: "invalid content".to_owned(),
            })
        );
    }

    #[test]
    fn maps_decoding_failures_to_warnings() {
        assert_eq!(
            warning(&DecodeError::BadSignature),
            KernelEvent::Warning {
                reason: WarningReason::BadSignature,
                msg_type: None,
                detail: String::new(),
            }
        );
        assert_eq!(
            warning(&DecodeError::Malformed("broken".to_owned())),
            KernelEvent::Warning {
                reason: WarningReason::Malformed,
                msg_type: None,
                detail: "broken".to_owned(),
            }
        );
        let _ = ProcessExit::Code(0);
    }
}
