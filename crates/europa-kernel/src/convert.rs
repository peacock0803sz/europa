//! Convert wire messages into kernel events.
//!
//! The channel and the message type decide first; content is only read for
//! the twelve known kinds. Anything else becomes a warning, except the
//! shutdown reply on iopub, which is silently ignored.

use europa_core::{
    IopubMsg, JsonMap, KernelEvent, KernelInfo, KernelStatus, LanguageInfo, MsgId, ReplyStatus,
    WarningReason,
};

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
    let msg_type = msg.header.msg_type.as_str();
    let parent = msg
        .parent_header
        .as_ref()
        .map(|header| MsgId::new(header.msg_id.clone()));
    match (channel, msg_type) {
        (Channel::Iopub, "status") => convert_status(msg, parent),
        (Channel::Iopub, "stream") => convert_stream(msg, parent),
        (Channel::Iopub, "display_data") => convert_display_data(msg, parent),
        (Channel::Iopub, "update_display_data") => convert_update_display_data(msg, parent),
        (Channel::Iopub, "execute_result") => convert_execute_result(msg, parent),
        (Channel::Iopub, "error") => convert_error(msg, parent),
        (Channel::Iopub, "clear_output") => convert_clear_output(msg, parent),
        (Channel::Iopub, "execute_input") => convert_execute_input(msg, parent),
        (Channel::Iopub, "shutdown_reply") => Converted::Ignored,
        (Channel::Shell, "execute_reply") => convert_execute_reply(msg, parent),
        (Channel::Shell, "kernel_info_reply") => convert_kernel_info_reply(msg, parent),
        (Channel::Control, "interrupt_reply") => Converted::InterruptReply { parent },
        (Channel::Control, "shutdown_reply") => Converted::ShutdownReply { parent },
        _ => Converted::Event(KernelEvent::Warning {
            reason: WarningReason::UnknownType,
            msg_type: Some(msg_type.to_owned()),
            detail: String::new(),
        }),
    }
}

/// Turn a decoding failure into a warning event.
pub(crate) fn warning(error: &DecodeError) -> KernelEvent {
    match error {
        DecodeError::BadSignature => KernelEvent::Warning {
            reason: WarningReason::BadSignature,
            msg_type: None,
            detail: String::new(),
        },
        DecodeError::Malformed(detail) => KernelEvent::Warning {
            reason: WarningReason::Malformed,
            msg_type: None,
            detail: detail.clone(),
        },
    }
}

/// Parent id or a missing-parent warning.
fn missing_parent(msg_type: &str) -> Converted {
    Converted::Event(KernelEvent::Warning {
        reason: WarningReason::Malformed,
        msg_type: Some(msg_type.to_owned()),
        detail: "missing parent".to_owned(),
    })
}

/// Malformed content warning.
fn malformed(msg_type: &str) -> Converted {
    Converted::Event(KernelEvent::Warning {
        reason: WarningReason::Malformed,
        msg_type: Some(msg_type.to_owned()),
        detail: "invalid content".to_owned(),
    })
}

/// Read a MIME bundle from content.
fn bundle(content: &serde_json::Value, key: &str) -> Option<(JsonMap, JsonMap)> {
    let data = serde_json::from_value(content.get("data")?.clone()).ok()?;
    let metadata = match content.get("metadata") {
        None => JsonMap::new(),
        Some(value) => serde_json::from_value(value.clone()).ok()?,
    };
    let _ = key;
    Some((data, metadata))
}

/// Convert a status message.
fn convert_status(msg: &WireMessage, parent: Option<MsgId>) -> Converted {
    let parsed: Result<jupyter_protocol::Status, _> = serde_json::from_value(msg.content.clone());
    let Ok(status) = parsed else {
        return malformed("status");
    };
    let state = match status.execution_state {
        jupyter_protocol::ExecutionState::Busy => KernelStatus::Busy,
        jupyter_protocol::ExecutionState::Idle => KernelStatus::Idle,
        jupyter_protocol::ExecutionState::Starting => KernelStatus::Starting,
        _ => {
            return Converted::Event(KernelEvent::Warning {
                reason: WarningReason::Malformed,
                msg_type: Some("status".to_owned()),
                detail: "unknown execution_state".to_owned(),
            });
        }
    };
    Converted::Event(KernelEvent::Status { parent, state })
}

/// Convert a stream message.
fn convert_stream(msg: &WireMessage, parent: Option<MsgId>) -> Converted {
    let Some(parent) = parent else {
        return missing_parent("stream");
    };
    let Ok(content) =
        serde_json::from_value::<jupyter_protocol::StreamContent>(msg.content.clone())
    else {
        return malformed("stream");
    };
    let name = match content.name {
        jupyter_protocol::Stdio::Stdout => "stdout".to_owned(),
        jupyter_protocol::Stdio::Stderr => "stderr".to_owned(),
    };
    Converted::Event(KernelEvent::Iopub(IopubMsg::Stream {
        parent,
        name,
        text: content.text,
    }))
}

/// Convert a display_data message.
fn convert_display_data(msg: &WireMessage, parent: Option<MsgId>) -> Converted {
    let Some(parent) = parent else {
        return missing_parent("display_data");
    };
    let Ok(content) = serde_json::from_value::<jupyter_protocol::DisplayData>(msg.content.clone())
    else {
        return malformed("display_data");
    };
    let Some((data, metadata)) = bundle(&msg.content, "display_data") else {
        return malformed("display_data");
    };
    Converted::Event(KernelEvent::Iopub(IopubMsg::DisplayData {
        parent,
        data,
        metadata,
        display_id: content.transient.and_then(|transient| transient.display_id),
    }))
}

/// Convert an update_display_data message.
fn convert_update_display_data(msg: &WireMessage, parent: Option<MsgId>) -> Converted {
    let Some(parent) = parent else {
        return missing_parent("update_display_data");
    };
    let Ok(content) =
        serde_json::from_value::<jupyter_protocol::UpdateDisplayData>(msg.content.clone())
    else {
        return malformed("update_display_data");
    };
    let Some((data, metadata)) = bundle(&msg.content, "update_display_data") else {
        return malformed("update_display_data");
    };
    Converted::Event(KernelEvent::Iopub(IopubMsg::UpdateDisplayData {
        parent,
        data,
        metadata,
        display_id: content.transient.display_id.unwrap_or_default(),
    }))
}

/// Convert an execute_result message.
fn convert_execute_result(msg: &WireMessage, parent: Option<MsgId>) -> Converted {
    let Some(parent) = parent else {
        return missing_parent("execute_result");
    };
    let Ok(content) =
        serde_json::from_value::<jupyter_protocol::ExecuteResult>(msg.content.clone())
    else {
        return malformed("execute_result");
    };
    let Some((data, metadata)) = bundle(&msg.content, "execute_result") else {
        return malformed("execute_result");
    };
    Converted::Event(KernelEvent::Iopub(IopubMsg::ExecuteResult {
        parent,
        data,
        metadata,
        execution_count: Some(usize::from(content.execution_count) as i64),
    }))
}

/// Convert an error message.
fn convert_error(msg: &WireMessage, parent: Option<MsgId>) -> Converted {
    let Some(parent) = parent else {
        return missing_parent("error");
    };
    let Ok(content) = serde_json::from_value::<jupyter_protocol::ErrorOutput>(msg.content.clone())
    else {
        return malformed("error");
    };
    Converted::Event(KernelEvent::Iopub(IopubMsg::Error {
        parent,
        ename: content.ename,
        evalue: content.evalue,
        traceback: content.traceback,
    }))
}

/// Convert a clear_output message.
fn convert_clear_output(msg: &WireMessage, parent: Option<MsgId>) -> Converted {
    let Some(parent) = parent else {
        return missing_parent("clear_output");
    };
    let Ok(content) = serde_json::from_value::<jupyter_protocol::ClearOutput>(msg.content.clone())
    else {
        return malformed("clear_output");
    };
    Converted::Event(KernelEvent::Iopub(IopubMsg::ClearOutput {
        parent,
        wait: content.wait,
    }))
}

/// Convert an execute_input message.
fn convert_execute_input(msg: &WireMessage, parent: Option<MsgId>) -> Converted {
    let Some(parent) = parent else {
        return missing_parent("execute_input");
    };
    let Ok(content) = serde_json::from_value::<jupyter_protocol::ExecuteInput>(msg.content.clone())
    else {
        return malformed("execute_input");
    };
    Converted::Event(KernelEvent::Iopub(IopubMsg::ExecuteInput {
        parent,
        execution_count: usize::from(content.execution_count) as i64,
    }))
}

/// Convert an execute_reply message.
fn convert_execute_reply(msg: &WireMessage, parent: Option<MsgId>) -> Converted {
    let Some(parent) = parent else {
        return missing_parent("execute_reply");
    };
    let status_text = msg.content.get("status").and_then(|value| value.as_str());
    let status = match status_text {
        Some("ok") => ReplyStatus::Ok,
        Some("error") => ReplyStatus::Error,
        Some("aborted") => ReplyStatus::Aborted,
        Some(_) => {
            return Converted::Event(KernelEvent::Warning {
                reason: WarningReason::Malformed,
                msg_type: Some("execute_reply".to_owned()),
                detail: "unknown status".to_owned(),
            });
        }
        None => return malformed("execute_reply"),
    };
    let Ok(_) = serde_json::from_value::<jupyter_protocol::ExecuteReply>(msg.content.clone())
    else {
        return malformed("execute_reply");
    };
    let execution_count = msg
        .content
        .get("execution_count")
        .and_then(|value| value.as_i64());
    Converted::Event(KernelEvent::ExecuteReply {
        parent,
        status,
        execution_count,
    })
}

/// Convert a kernel_info_reply message.
fn convert_kernel_info_reply(msg: &WireMessage, parent: Option<MsgId>) -> Converted {
    if msg
        .content
        .get("status")
        .is_some_and(|value| value == "error")
    {
        let detail = msg
            .content
            .get("evalue")
            .and_then(|value| value.as_str())
            .unwrap_or("kernel info failed")
            .to_owned();
        return Converted::KernelInfoReply {
            parent,
            result: Err(detail),
        };
    }
    let Ok(content) =
        serde_json::from_value::<jupyter_protocol::KernelInfoReply>(msg.content.clone())
    else {
        return Converted::KernelInfoReply {
            parent,
            result: Err("invalid content".to_owned()),
        };
    };
    if content.status == jupyter_protocol::ReplyStatus::Error {
        let detail = content
            .error
            .as_ref()
            .map(|error| error.evalue.clone())
            .unwrap_or_else(|| "kernel info failed".to_owned());
        return Converted::KernelInfoReply {
            parent,
            result: Err(detail),
        };
    }
    Converted::KernelInfoReply {
        parent,
        result: Ok(KernelInfo {
            protocol_version: content.protocol_version,
            implementation: content.implementation,
            implementation_version: content.implementation_version,
            language: LanguageInfo {
                name: content.language_info.name,
                version: content.language_info.version,
                file_extension: content.language_info.file_extension,
            },
            banner: content.banner,
        }),
    }
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
