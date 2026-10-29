//! Applying iopub messages to notebook outputs.
//!
//! [`apply_iopub`] folds `stream`, `clear_output`, `display_data`,
//! `update_display_data`, `execute_result`, `error` and `execute_input`
//! messages into the outputs and execution counts of code cells. It has no IO,
//! so the same input always yields the same result.
//!
//! Design decisions:
//!
//! - A deferred `clear_output(wait=true)` is keyed by the parent message id
//!   and stored in [`ExecState`], so it is dropped together with the
//!   correlation entry and never leaks into a later run of the same cell.
//! - A clear fires only when the next output of the same run arrives. A run
//!   that ends while a clear is still pending keeps its outputs: the clear is
//!   dropped with the correlation entry and never fires.
//! - `\r` rebuilds only the last line. A trailing `\r` stays in the text until
//!   the next chunk shows whether it ends the line or overwrites it, which
//!   keeps the result independent of how the text is chunked.
//! - The notebook is marked as changed, and the cell is listed in
//!   [`IopubApplied::changed`], only when an output is added, extended with
//!   stream text, removed or replaced, or the execution count takes a
//!   different value. Content is never compared.
//! - [`apply_iopub`] returns the cells that changed, because a message can
//!   change cells other than the one its parent id maps to.
//! - A display id is bound to the output it was shown with and not kept in
//!   [`ExecState`], so the binding disappears with the output however that is
//!   removed, even when a caller empties `outputs` directly. It is never
//!   saved, so a notebook read from a file has nothing to replace.
//! - A `display_data` whose display id is already bound replaces the earlier
//!   outputs, in any cell, before it adds its own, as IPython does.
//! - A message whose parent has no correlation entry, which includes a run
//!   that has ended, replaces nothing. Outputs bound before an
//!   [`ExecEvent::Reset`] are not replaced afterwards, because a fixed id such
//!   as `"progress"` may be reused for something else once the kernel is gone.
//!   Both differ from Jupyter on purpose.
//! - An `execute_result` carries no display id, because kernels do not send one
//!   and frontends treat it differently.
//! - The execution count of a cell changes on `execute_input` only. The count
//!   on an `execute_result` stays on its output and may differ.
//! - Restart a kernel by sending [`ExecEvent::Reset`] and do not build a new
//!   [`ExecState`]. A new state starts the generation over, and outputs that
//!   the notebook still holds from before the restart would match their
//!   display ids again.

use crate::exec::{ExecEvent, ExecState, MsgId};
use crate::json::JsonMap;
use crate::notebook::{Cell, CodeCell, DisplayBinding, Notebook, Output, OutputOrigin};

/// An iopub message that changes cell outputs or the execution count.
///
/// The kernel layer converts wire messages into this type before handing them
/// to [`apply_iopub`]; this crate never sees the wire format. The `name` of a
/// stream is not restricted to `stdout` and `stderr`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "msg", rename_all = "snake_case")]
pub enum IopubMsg {
    /// A `stream` message.
    Stream {
        /// Parent message id of the originating `execute_request`.
        parent: MsgId,
        /// Stream name such as `stdout` or `stderr`.
        name: String,
        /// Text chunk to append.
        text: String,
    },
    /// A `clear_output` message.
    ClearOutput {
        /// Parent message id of the originating `execute_request`.
        parent: MsgId,
        /// Defer clearing until the next output of the same run arrives.
        wait: bool,
    },
    /// A `display_data` message.
    DisplayData {
        /// Parent message id of the originating `execute_request`.
        parent: MsgId,
        /// MIME bundle to show. It may be empty.
        data: JsonMap,
        /// Per-MIME metadata.
        #[serde(default)]
        metadata: JsonMap,
        /// The `transient.display_id` of the message.
        ///
        /// An empty string counts as no display id. With an id, the message
        /// first replaces the content of every output already bound to the id,
        /// in any cell, and then binds the output it adds. Later messages with
        /// the id replace that content while the output exists, as long as no
        /// [`ExecEvent::Reset`] came in between and their own parent is still
        /// known; see [`apply_iopub`].
        #[serde(default, skip_serializing_if = "Option::is_none")]
        display_id: Option<String>,
    },
    /// An `update_display_data` message.
    UpdateDisplayData {
        /// Parent message id of the originating `execute_request`.
        parent: MsgId,
        /// MIME bundle that replaces the content of the matching outputs.
        data: JsonMap,
        /// Per-MIME metadata that replaces the metadata of the matching outputs.
        #[serde(default)]
        metadata: JsonMap,
        /// The `transient.display_id` of the message. An empty string matches
        /// no output.
        display_id: String,
    },
    /// An `execute_result` message.
    ///
    /// It carries no display id, so it is only appended and never replaced by
    /// a later `update_display_data`.
    ExecuteResult {
        /// Parent message id of the originating `execute_request`.
        parent: MsgId,
        /// MIME bundle of the result.
        data: JsonMap,
        /// Per-MIME metadata.
        #[serde(default)]
        metadata: JsonMap,
        /// Execution count the kernel reports with the result. It is kept as
        /// sent and does not change the count of the cell.
        execution_count: Option<i64>,
    },
    /// An `error` message.
    Error {
        /// Parent message id of the originating `execute_request`.
        parent: MsgId,
        /// Exception name.
        ename: String,
        /// Exception value.
        evalue: String,
        /// Traceback lines, escape sequences included.
        traceback: Vec<String>,
    },
    /// An `execute_input` message.
    ///
    /// It has no `code` field, because it never changes the `source` of the
    /// cell; it only sets the execution count.
    ExecuteInput {
        /// Parent message id of the originating `execute_request`.
        parent: MsgId,
        /// Execution count the kernel assigned to the run.
        execution_count: i64,
    },
}

impl IopubMsg {
    fn parent(&self) -> &MsgId {
        match self {
            Self::Stream { parent, .. }
            | Self::ClearOutput { parent, .. }
            | Self::DisplayData { parent, .. }
            | Self::UpdateDisplayData { parent, .. }
            | Self::ExecuteResult { parent, .. }
            | Self::Error { parent, .. }
            | Self::ExecuteInput { parent, .. } => parent,
        }
    }
}

/// The cell an iopub message belongs to and the cells it changed.
///
/// Returned by [`apply_iopub`]. A caller should redraw the cells in `changed`,
/// which may leave out `cell` and include other cells.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[must_use]
pub struct IopubApplied {
    /// The cell the parent message id maps to. It is listed in `changed` only
    /// if it changed.
    pub cell: String,
    /// The cells whose outputs changed because an output was added, extended
    /// with stream text, removed or replaced, and the cells that took a
    /// different execution count, in notebook order and without duplicates.
    ///
    /// Content is never compared, so rewriting an output with the same content
    /// still lists its cell.
    pub changed: Vec<String>,
}

/// Apply one iopub message to the notebook.
///
/// Steps:
///
/// 1. Look the cell up by the parent id; return `None` and change nothing, not
///    even outputs in other cells, when the parent id has no correlation entry
///    or the notebook has no code cell with the id it maps to.
/// 2. `clear_output(wait=true)` records the parent as pending and leaves the
///    outputs alone. `clear_output(wait=false)` drops any pending record and
///    empties the outputs. A `stream` with empty text changes nothing.
/// 3. A `stream` with text, `display_data`, `execute_result` and `error` first
///    empty the outputs of the cell if the parent is pending, which ends the
///    pending state.
/// 4. A `display_data` with a non-empty display id replaces the data and
///    metadata of every output bound to that id, in any code cell. Outputs
///    bound before the last [`ExecEvent::Reset`] are left alone.
/// 5. A `stream` with text appends to the last output when it is a stream of
///    the same name, or pushes a new stream output; appending to a loaded
///    output rebuilds it as a new one. `display_data`, `execute_result` and
///    `error` push a new output onto the cell. A `display_data` with a
///    non-empty display id binds it to that id.
/// 6. `update_display_data` only does the replacement of step 4. It adds no
///    output, so, as in JupyterLab, it neither fires nor ends a pending clear.
/// 7. `execute_input` replaces the execution count of the cell, but only when
///    the value differs.
/// 8. The cells that gained, extended, lost or had replaced an output, or took
///    a different execution count, are listed in [`IopubApplied::changed`]. If
///    any is listed, the notebook is marked as changed.
/// 9. Return the cell and the changed cells.
///
/// Returns `Some` with the cell the message belongs to and the cells that
/// changed, which is empty when nothing changed, or `None` when the parent id
/// is unknown or no matching code cell exists.
#[must_use]
pub fn apply_iopub(
    exec: &mut ExecState,
    notebook: &mut Notebook,
    msg: &IopubMsg,
) -> Option<IopubApplied> {
    let parent = msg.parent();
    let cell_id = exec.cell_for(parent)?.to_owned();
    let at = notebook
        .cells
        .iter()
        .position(|cell| matches!(cell, Cell::Code(code) if code.mem_id == cell_id))?;
    // Positions rather than borrows, because a replacement also reaches cells
    // other than this one.
    let mut touched = Vec::new();
    match msg {
        IopubMsg::Stream { name, text, .. } => {
            if !text.is_empty() {
                let code = code_at(notebook, at);
                fire_pending_clear(exec, parent, code);
                append_output(&mut code.outputs, name, text);
                touched.push(at);
            }
        }
        IopubMsg::ClearOutput { wait: true, .. } => {
            exec.pending_clear.insert(parent.clone());
        }
        IopubMsg::ClearOutput { wait: false, .. } => {
            exec.pending_clear.remove(parent);
            let code = code_at(notebook, at);
            if !code.outputs.is_empty() {
                code.outputs.clear();
                touched.push(at);
            }
        }
        IopubMsg::DisplayData {
            data,
            metadata,
            display_id,
            ..
        } => {
            fire_pending_clear(exec, parent, code_at(notebook, at));
            let binding = binding_for(exec, display_id.as_deref());
            if let Some(binding) = &binding {
                touched.extend(replace_bound(notebook, binding, data, metadata));
            }
            let output = display_output(data, metadata, binding);
            code_at(notebook, at).outputs.push(output);
            touched.push(at);
        }
        IopubMsg::UpdateDisplayData {
            data,
            metadata,
            display_id,
            ..
        } => {
            if let Some(binding) = binding_for(exec, Some(display_id)) {
                touched.extend(replace_bound(notebook, &binding, data, metadata));
            }
        }
        IopubMsg::ExecuteResult {
            data,
            metadata,
            execution_count,
            ..
        } => {
            let code = code_at(notebook, at);
            fire_pending_clear(exec, parent, code);
            code.outputs.push(Output::new_execute_result(
                data.clone(),
                metadata.clone(),
                *execution_count,
            ));
            touched.push(at);
        }
        IopubMsg::Error {
            ename,
            evalue,
            traceback,
            ..
        } => {
            let code = code_at(notebook, at);
            fire_pending_clear(exec, parent, code);
            code.outputs.push(Output::new_error(
                ename.clone(),
                evalue.clone(),
                traceback.clone(),
            ));
            touched.push(at);
        }
        IopubMsg::ExecuteInput {
            execution_count, ..
        } => {
            let code = code_at(notebook, at);
            if code.execution_count != Some(*execution_count) {
                code.execution_count = Some(*execution_count);
                touched.push(at);
            }
        }
    }
    touched.sort_unstable();
    touched.dedup();
    let changed: Vec<String> = touched
        .iter()
        .map(|&at| notebook.cells[at].id().to_owned())
        .collect();
    if !changed.is_empty() {
        let _ = exec.apply(ExecEvent::Output {
            parent: parent.clone(),
        });
    }
    Some(IopubApplied {
        cell: cell_id,
        changed,
    })
}

fn code_at(notebook: &mut Notebook, at: usize) -> &mut CodeCell {
    match &mut notebook.cells[at] {
        Cell::Code(code) => code,
        _ => unreachable!("the position was found by matching a code cell"),
    }
}

/// Empty the outputs of a cell whose run asked for `clear_output(wait=true)`.
///
/// This happens when the next output arrives and not earlier, so that output
/// redrawn in place does not flicker.
fn fire_pending_clear(exec: &mut ExecState, parent: &MsgId, code: &mut CodeCell) {
    if exec.pending_clear.remove(parent) {
        code.outputs.clear();
    }
}

/// The binding a display id gets in the current generation.
///
/// An empty id counts as none, as the frontends treat it, so a kernel layer
/// that forgets to drop it cannot change the outcome.
fn binding_for(exec: &ExecState, display_id: Option<&str>) -> Option<DisplayBinding> {
    display_id
        .filter(|id| !id.is_empty())
        .map(|id| DisplayBinding {
            id: id.to_owned(),
            generation: exec.display_generation,
        })
}

/// Replace the content of every display output bound to `binding`.
///
/// Returns the positions of the cells that held one, in ascending order. The
/// unknown fields and the origin of an output stay as they were.
fn replace_bound(
    notebook: &mut Notebook,
    binding: &DisplayBinding,
    data: &JsonMap,
    metadata: &JsonMap,
) -> Vec<usize> {
    let mut replaced = Vec::new();
    for (at, cell) in notebook.cells.iter_mut().enumerate() {
        let Cell::Code(code) = cell else {
            continue;
        };
        let mut found = false;
        for output in &mut code.outputs {
            let Output::DisplayData(display) = output else {
                continue;
            };
            if display.binding.as_ref() == Some(binding) {
                display.data = data.clone();
                display.metadata = metadata.clone();
                found = true;
            }
        }
        if found {
            replaced.push(at);
        }
    }
    replaced
}

fn display_output(data: &JsonMap, metadata: &JsonMap, binding: Option<DisplayBinding>) -> Output {
    let mut output = Output::new_display_data(data.clone(), metadata.clone());
    if let Output::DisplayData(display) = &mut output {
        display.binding = binding;
    }
    output
}

/// Fold a stream chunk into the last output, or start a new stream output.
fn append_output(outputs: &mut Vec<Output>, name: &str, text: &str) {
    if let Some(Output::Stream(last)) = outputs.last_mut() {
        if last.name == name {
            // A long line printed piece by piece would otherwise be copied
            // and renormalized on every chunk.
            let edited = last.origin == OutputOrigin::New
                && match last.text.as_single() {
                    Some(body) if appends_verbatim(body, text) => last.text.push_tail(text),
                    _ => last.text.edit_tail(|body| append_stream(body, text)),
                };
            if !edited {
                last.rebuild_text(|body| append_stream(body, text));
            }
            return;
        }
    }
    let mut body = String::new();
    append_stream(&mut body, text);
    outputs.push(Output::new_stream(name, body));
}

/// Append a stream chunk to the text of the last output, resolving `\r`.
///
/// Only the last line of `existing` is rebuilt, so the cost of one call does
/// not grow with the length of the whole text, and a chunk that
/// [`appends_verbatim`] is pushed without copying even that line. A `\r` run
/// before `\n` belongs to the newline; elsewhere `\r` discards what the line
/// held before it. A trailing `\r` is kept until the next chunk shows whether
/// it ends the line or overwrites it, which makes the result independent of
/// how the text is split into chunks.
pub(crate) fn append_stream(existing: &mut String, chunk: &str) {
    if appends_verbatim(existing, chunk) {
        existing.push_str(chunk);
        return;
    }
    let cut = existing.rfind('\n').map_or(0, |at| at + 1);
    let mut tail = existing.split_off(cut);
    tail.push_str(chunk);

    let mut lines = tail.split('\n').peekable();
    while let Some(line) = lines.next() {
        let line = line.trim_end_matches('\r');
        if lines.peek().is_some() {
            existing.push_str(after_last_cr(line));
            existing.push('\n');
        }
    }
    let last = tail.rsplit('\n').next().unwrap_or_default();
    let body = last.trim_end_matches('\r');
    existing.push_str(after_last_cr(body));
    if body.len() < last.len() {
        existing.push('\r');
    }
}

/// Whether [`append_stream`] would just push `chunk` onto `existing`.
///
/// That holds when neither `chunk` nor the last line of `existing` has a
/// `\r` to resolve. Text built by [`append_stream`] keeps at most a trailing
/// `\r` in its last line, but text given to [`Output::new_stream`] or read
/// from a file may hold one anywhere. The check scans only the last line.
fn appends_verbatim(existing: &str, chunk: &str) -> bool {
    let cut = existing.rfind('\n').map_or(0, |at| at + 1);
    !chunk.contains('\r') && !existing[cut..].contains('\r')
}

/// The part of a line that survives carriage-return overwrites.
fn after_last_cr(line: &str) -> &str {
    line.rsplit('\r').next().unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::{append_stream, IopubMsg};
    use crate::exec::MsgId;
    use crate::json::{JsonMap, JsonValue};

    fn plain(text: &str) -> JsonMap {
        JsonMap::from([("text/plain".to_owned(), JsonValue::from(text))])
    }

    #[test]
    fn recorded_messages_may_leave_out_metadata_and_the_display_id() {
        let display = r#"{"msg":"display_data","parent":"m1","data":{"text/plain":"x"}}"#;
        let msg: IopubMsg = serde_json::from_str(display).expect("reads");
        assert_eq!(
            msg,
            IopubMsg::DisplayData {
                parent: MsgId::new("m1"),
                data: plain("x"),
                metadata: JsonMap::new(),
                display_id: None,
            }
        );
        // A recorded line and the line written back would otherwise differ.
        let written = serde_json::to_string(&msg).expect("writes");
        assert!(!written.contains("display_id"), "{written}");

        let result = r#"{"msg":"execute_result","parent":"m1","data":{"text/plain":"42"},"execution_count":1}"#;
        let msg: IopubMsg = serde_json::from_str(result).expect("reads");
        assert_eq!(
            msg,
            IopubMsg::ExecuteResult {
                parent: MsgId::new("m1"),
                data: plain("42"),
                metadata: JsonMap::new(),
                execution_count: Some(1),
            }
        );
    }

    fn append(existing: &str, chunk: &str) -> String {
        let mut text = existing.to_owned();
        append_stream(&mut text, chunk);
        text
    }

    #[test]
    fn appends_plain_text() {
        assert_eq!(append("", "10%"), "10%");
    }

    #[test]
    fn plain_chunks_extend_a_long_line() {
        let mut text = "a\n".to_owned();
        for _ in 0..3 {
            append_stream(&mut text, "..");
        }
        assert_eq!(text, "a\n......");
    }

    #[test]
    fn carriage_return_left_inside_the_last_line_is_still_resolved() {
        assert_eq!(append("a\nx\ry", "z"), "a\nyz");
    }

    #[test]
    fn carriage_return_overwrites_the_line() {
        assert_eq!(append("10%", "\r20%"), "20%");
    }

    #[test]
    fn carriage_return_replaces_the_whole_line() {
        assert_eq!(append("abcdef", "\rXY"), "XY");
    }

    #[test]
    fn newline_after_overwrite_ends_the_line() {
        assert_eq!(append("30%", "\r100%\n"), "100%\n");
    }

    #[test]
    fn trailing_carriage_return_becomes_newline_when_lf_follows() {
        assert_eq!(append("abc\r", "\ndef"), "abc\ndef");
    }

    #[test]
    fn trailing_carriage_return_is_overwritten_by_text() {
        assert_eq!(append("abc\r", "d"), "d");
    }

    #[test]
    fn earlier_lines_stay_untouched() {
        assert_eq!(append("a\nb", "\rc"), "a\nc");
        assert_eq!(append("a\nb\nc", "\rX"), "a\nb\nX");
    }

    #[test]
    fn repeated_trailing_carriage_returns_collapse_to_one() {
        assert_eq!(append("", "abc\rde\r\r"), "de\r");
    }

    #[test]
    fn carriage_return_runs_before_newline_are_part_of_the_newline() {
        assert_eq!(append("", "a\r\r\nb"), "a\nb");
    }
}
