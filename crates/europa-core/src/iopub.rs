//! Applying iopub messages to notebook outputs.
//!
//! [`apply_iopub`] folds `stream` and `clear_output` messages into a code
//! cell's outputs. It has no IO, so the same input always yields the same
//! result.

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
        /// Defer clearing until the next output arrives.
        wait: bool,
    },
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
///    or pushes a new stream output.
/// 6. Only the non-empty stream of step 5 and a `clear_output(wait=false)` that
///    removed at least one output mark the notebook as changed.
/// 7. Return the cell id for every message that passed step 1.
///
/// Has no IO and returns the same result for the same input. Returns the id
/// of the cell that was updated, or `None` when the parent id is unknown or
/// no matching code cell exists.
pub fn apply_iopub(
    exec: &mut ExecState,
    notebook: &mut Notebook,
    msg: &IopubMsg,
) -> Option<String> {
    let (IopubMsg::Stream { parent, .. } | IopubMsg::ClearOutput { parent, .. }) = msg;
    let cell_id = exec.cell_for(parent)?.to_owned();
    let code = notebook.cells.iter_mut().find_map(|cell| match cell {
        Cell::Code(code) if code.mem_id == cell_id => Some(code),
        _ => None,
    })?;
    match msg {
        IopubMsg::Stream { name, text, .. } => {
            if text.is_empty() {
                return Some(cell_id);
            }
            append_output(&mut code.outputs, name, text);
            let _ = exec.apply(ExecEvent::Output {
                parent: parent.clone(),
            });
        }
        IopubMsg::ClearOutput { .. } => todo!(),
    }
    Some(cell_id)
}

/// Fold a stream chunk into the last output, or start a new stream output.
fn append_output(outputs: &mut Vec<Output>, name: &str, text: &str) {
    if let Some(Output::Stream(last)) = outputs.last_mut() {
        if last.name == name
            && last.origin == OutputOrigin::New
            && last.text.edit_tail(|body| append_stream(body, text))
        {
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
/// not grow with the length of the whole text. A `\r` run before `\n` belongs
/// to the newline; elsewhere `\r` discards what the line held before it. A
/// trailing `\r` is kept until the next chunk shows whether it ends the line
/// or overwrites it, which makes the result independent of how the text is
/// split into chunks.
pub(crate) fn append_stream(existing: &mut String, chunk: &str) {
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
