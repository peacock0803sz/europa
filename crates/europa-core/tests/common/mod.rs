#![allow(dead_code)]

use europa_core::notebook::CodeCell;
use europa_core::{
    Cell, ExecEvent, ExecState, IopubApplied, IopubMsg, JsonMap, MsgId, Notebook, Output,
    ReplyStatus,
};

/// Notebook holding only empty code cells with the given ids.
pub fn notebook(cells: &[&str]) -> Notebook {
    let cells: Vec<String> = cells
        .iter()
        .map(|id| {
            format!(
                r#"{{"cell_type":"code","id":"{id}","metadata":{{}},"source":"","outputs":[],"execution_count":null}}"#
            )
        })
        .collect();
    let text = format!(
        r#"{{"nbformat":4,"nbformat_minor":5,"metadata":{{}},"cells":[{}]}}"#,
        cells.join(",")
    );
    europa_core::parse(text.as_bytes()).expect("fixture notebook loads")
}

/// State where `cell` is running with message id `msg`.
pub fn running(cell: &str, msg: &str) -> ExecState {
    let mut state = ExecState::default();
    let _ = state.apply(ExecEvent::Enqueue {
        cell_id: cell.to_owned(),
    });
    let _ = state.apply(ExecEvent::Sent {
        cell_id: cell.to_owned(),
        msg_id: MsgId::new(msg),
    });
    state
}

pub fn stream(msg: &str, name: &str, text: &str) -> IopubMsg {
    IopubMsg::Stream {
        parent: MsgId::new(msg),
        name: name.to_owned(),
        text: text.to_owned(),
    }
}

pub fn clear(msg: &str, wait: bool) -> IopubMsg {
    IopubMsg::ClearOutput {
        parent: MsgId::new(msg),
        wait,
    }
}

/// Outputs of `cell` as `name:"text"` joined by `, `; other kinds show as `<output_type>`.
pub fn outputs(notebook: &Notebook, cell: &str) -> String {
    let Some(Cell::Code(code)) = notebook
        .cells
        .iter()
        .find(|candidate| candidate.id() == cell)
    else {
        panic!("no code cell {cell}");
    };
    code.outputs
        .iter()
        .map(|output| match output {
            Output::Stream(stream) => format!("{}:{:?}", stream.name, stream.text.text()),
            Output::DisplayData(_) => "<display_data>".to_owned(),
            Output::ExecuteResult(_) => "<execute_result>".to_owned(),
            Output::Error(_) => "<error>".to_owned(),
            Output::Unknown(_) => "<unknown>".to_owned(),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// MIME bundle holding only `text/plain`.
pub fn plain(text: &str) -> JsonMap {
    JsonMap::from([("text/plain".to_owned(), text.into())])
}

pub fn display(msg: &str, id: Option<&str>, text: &str) -> IopubMsg {
    IopubMsg::DisplayData {
        parent: MsgId::new(msg),
        data: plain(text),
        metadata: JsonMap::new(),
        display_id: id.map(str::to_owned),
    }
}

pub fn update(msg: &str, id: &str, text: &str) -> IopubMsg {
    IopubMsg::UpdateDisplayData {
        parent: MsgId::new(msg),
        data: plain(text),
        metadata: JsonMap::new(),
        display_id: id.to_owned(),
    }
}

pub fn result(msg: &str, count: Option<i64>, text: &str) -> IopubMsg {
    IopubMsg::ExecuteResult {
        parent: MsgId::new(msg),
        data: plain(text),
        metadata: JsonMap::new(),
        execution_count: count,
    }
}

pub fn error(msg: &str, ename: &str, evalue: &str, traceback: &[&str]) -> IopubMsg {
    IopubMsg::Error {
        parent: MsgId::new(msg),
        ename: ename.to_owned(),
        evalue: evalue.to_owned(),
        traceback: traceback.iter().map(|line| (*line).to_owned()).collect(),
    }
}

pub fn input(msg: &str, count: i64) -> IopubMsg {
    IopubMsg::ExecuteInput {
        parent: MsgId::new(msg),
        execution_count: count,
    }
}

/// Run `cell` as `msg` on a state that may already know other runs.
pub fn start(exec: &mut ExecState, cell: &str, msg: &str) {
    let _ = exec.apply(ExecEvent::Enqueue {
        cell_id: cell.to_owned(),
    });
    let _ = exec.apply(ExecEvent::Sent {
        cell_id: cell.to_owned(),
        msg_id: MsgId::new(msg),
    });
}

/// End the run of `msg` the way a kernel does, which drops its correlation.
pub fn finish(exec: &mut ExecState, msg: &str) {
    let _ = exec.apply(ExecEvent::Reply {
        parent: MsgId::new(msg),
        status: ReplyStatus::Ok,
    });
    let _ = exec.apply(ExecEvent::Idle {
        parent: MsgId::new(msg),
    });
}

fn json(map: &JsonMap) -> String {
    serde_json::to_string(map).expect("serializes")
}

fn meta(metadata: &JsonMap) -> String {
    if metadata.is_empty() {
        String::new()
    } else {
        format!(" meta:{}", json(metadata))
    }
}

fn count(count: Option<i64>) -> String {
    count.map_or_else(|| "null".to_owned(), |count| count.to_string())
}

/// One output as text, so that expectations read as a line.
///
/// A stream is `name:"text"`, a display is `display:{data}` and a result is
/// `result[count]:{data}`, both followed by ` meta:{metadata}` unless the
/// metadata is empty. An error is `error:ename:"evalue":["traceback"]`.
pub fn describe(output: &Output) -> String {
    match output {
        Output::Stream(stream) => format!("{}:{:?}", stream.name, stream.text.text()),
        Output::DisplayData(display) => {
            format!("display:{}{}", json(&display.data), meta(&display.metadata))
        }
        Output::ExecuteResult(result) => format!(
            "result[{}]:{}{}",
            count(result.execution_count),
            json(&result.data),
            meta(&result.metadata)
        ),
        Output::Error(error) => format!(
            "error:{}:{:?}:{:?}",
            error.ename, error.evalue, error.traceback
        ),
        Output::Unknown(_) => "<unknown>".to_owned(),
    }
}

/// The outputs of one code cell, described and joined by `, `.
pub fn shown(notebook: &Notebook, cell: &str) -> String {
    code(notebook, cell)
        .outputs
        .iter()
        .map(describe)
        .collect::<Vec<_>>()
        .join(", ")
}

/// Every code cell as `id[count]: outputs`, joined by ` | `.
pub fn cells(notebook: &Notebook) -> String {
    notebook
        .cells
        .iter()
        .filter_map(|cell| match cell {
            Cell::Code(code) => Some(format!(
                "{}[{}]: {}",
                cell.id(),
                count(code.execution_count),
                shown(notebook, cell.id())
            )),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join(" | ")
}

/// What `apply_iopub` returns for a message of `cell` that changed `changed`.
pub fn applied(cell: &str, changed: &[&str]) -> Option<IopubApplied> {
    Some(IopubApplied {
        cell: cell.to_owned(),
        changed: changed.iter().map(|id| (*id).to_owned()).collect(),
    })
}

/// The code cell with this id, or a panic when there is none.
pub fn code<'a>(notebook: &'a Notebook, cell: &str) -> &'a CodeCell {
    notebook
        .cells
        .iter()
        .find_map(|candidate| match candidate {
            Cell::Code(code) if candidate.id() == cell => Some(code),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no code cell {cell}"))
}

/// Like [`code`], for editing the cell.
pub fn code_mut<'a>(notebook: &'a mut Notebook, cell: &str) -> &'a mut CodeCell {
    for candidate in &mut notebook.cells {
        if candidate.id() != cell {
            continue;
        }
        if let Cell::Code(code) = candidate {
            return code;
        }
    }
    panic!("no code cell {cell}");
}
