#![allow(dead_code)]

use europa_core::{Cell, ExecEvent, ExecState, IopubMsg, MsgId, Notebook, Output};

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
