//! Applying iopub messages to notebook outputs.
//!
//! [`apply_iopub`] folds `stream` and `clear_output` messages into a code
//! cell's outputs. It has no IO, so the same input always yields the same
//! result.
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
//! - The notebook is marked as changed only when the list of outputs changes.

use crate::exec::{ExecEvent, ExecState, MsgId};
use crate::notebook::{Cell, Notebook, Output, OutputOrigin};

/// An iopub message that changes cell outputs.
///
/// The kernel layer converts wire messages into this type before handing them
/// to [`apply_iopub`]; this crate never sees the wire format. The `name` of a
/// stream is not restricted to `stdout` and `stderr`. Messages such as
/// `display_data` are not covered yet; the specs that handle them add the
/// variants.
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
}

/// The cells an iopub message belongs to and changed.
///
/// Returned by [`apply_iopub`] so that a caller redraws only the cells that
/// need it, which are not limited to the cell the message belongs to.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[must_use]
pub struct IopubApplied {
    /// The cell the parent message id maps to.
    pub cell: String,
    /// The cells that gained or lost outputs, had outputs replaced, or took a
    /// different execution count, in notebook order and without duplicates.
    pub changed: Vec<String>,
}

/// Apply one iopub message to the outputs of the cell it belongs to.
///
/// Steps:
///
/// 1. Look the cell up by the parent id; return `None` and change nothing when
///    the id is unknown or the notebook has no code cell with that id.
/// 2. `clear_output(wait=true)` records the parent as pending and leaves the
///    outputs alone.
/// 3. `clear_output(wait=false)` drops any pending record and empties the
///    outputs.
/// 4. A `stream` with empty text changes nothing.
/// 5. A `stream` with text first empties the outputs if the parent is pending,
///    then appends to the last output when it is a stream of the same name,
///    or pushes a new stream output. Appending to a loaded output rebuilds it
///    as a new one.
/// 6. Only the non-empty stream of step 5 and a `clear_output(wait=false)` that
///    removed at least one output mark the notebook as changed and list the
///    cell in [`IopubApplied::changed`].
/// 7. Return the cell id and the changed cells for every message that passed
///    step 1.
///
/// Returns the cell the message belongs to with the cells that changed, even
/// when nothing did, or `None` when the parent id is unknown or no matching
/// code cell exists.
#[must_use]
pub fn apply_iopub(
    exec: &mut ExecState,
    notebook: &mut Notebook,
    msg: &IopubMsg,
) -> Option<IopubApplied> {
    let (IopubMsg::Stream { parent, .. } | IopubMsg::ClearOutput { parent, .. }) = msg;
    let cell_id = exec.cell_for(parent)?.to_owned();
    let code = notebook.cells.iter_mut().find_map(|cell| match cell {
        Cell::Code(code) if code.mem_id == cell_id => Some(code),
        _ => None,
    })?;
    let mut touched = false;
    match msg {
        IopubMsg::Stream { name, text, .. } => {
            if !text.is_empty() {
                if exec.pending_clear.remove(parent) {
                    code.outputs.clear();
                }
                append_output(&mut code.outputs, name, text);
                touched = true;
            }
        }
        IopubMsg::ClearOutput { wait: true, .. } => {
            exec.pending_clear.insert(parent.clone());
        }
        IopubMsg::ClearOutput { wait: false, .. } => {
            exec.pending_clear.remove(parent);
            if !code.outputs.is_empty() {
                code.outputs.clear();
                touched = true;
            }
        }
    }
    let changed = if touched {
        let _ = exec.apply(ExecEvent::Output {
            parent: parent.clone(),
        });
        vec![cell_id.clone()]
    } else {
        Vec::new()
    };
    Some(IopubApplied {
        cell: cell_id,
        changed,
    })
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
    use super::append_stream;

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
