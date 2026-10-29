mod common;

use europa_core::{apply_iopub, ExecEvent, ExecState, IopubMsg};

#[derive(serde::Deserialize)]
#[serde(untagged)]
enum Step {
    Exec(ExecEvent),
    Iopub(IopubMsg),
}

/// Replay a recorded message sequence, one line per step.
///
/// Two cells are needed because a display can be replaced from another cell.
fn trace(path: &str) -> String {
    let text = std::fs::read_to_string(format!(
        "{}/tests/fixtures/iopub/{path}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("fixture is readable");
    let mut notebook = common::notebook(&["c1", "c2"]);
    let mut exec = ExecState::default();
    let mut lines = Vec::new();
    for line in text.lines() {
        let step: Step = serde_json::from_str(line).expect("fixture line parses");
        let result = match step {
            Step::Exec(event) => serde_json::to_string(&exec.apply(event)),
            Step::Iopub(msg) => serde_json::to_string(&apply_iopub(&mut exec, &mut notebook, &msg)),
        }
        .expect("serializes");
        let state = serde_json::to_value(&exec).expect("serializes");
        let pending = state
            .get("pending_clear")
            .map_or_else(|| "[]".to_owned(), std::string::ToString::to_string);
        lines.push(format!(
            "{line} => {result} | pending={pending} | dirty={} | {}",
            exec.is_dirty(),
            common::cells(&notebook)
        ));
    }
    lines.join("\n")
}

macro_rules! snapshot {
    ($name:ident, $file:literal) => {
        #[test]
        fn $name() {
            insta::assert_snapshot!(concat!("iopub-", $file), trace(concat!($file, ".jsonl")));
        }
    };
}

snapshot!(display_update, "display-update");
snapshot!(display_cross_cell, "display-cross-cell");
snapshot!(display_twice, "display-twice");
snapshot!(display_after_clear, "display-after-clear");
snapshot!(display_reset, "display-reset");
snapshot!(mixed_outputs, "mixed-outputs");
snapshot!(clear_wait_display, "clear-wait-display");
snapshot!(execute_input, "execute-input");
