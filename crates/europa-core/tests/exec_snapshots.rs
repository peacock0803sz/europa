use europa_core::{ExecEvent, ExecState, MsgId, ReplyStatus};

fn enqueue(cell: &str) -> ExecEvent {
    ExecEvent::Enqueue {
        cell_id: cell.to_owned(),
    }
}

fn sent(cell: &str, msg: &str) -> ExecEvent {
    ExecEvent::Sent {
        cell_id: cell.to_owned(),
        msg_id: MsgId::new(msg),
    }
}

fn reply(msg: &str, status: ReplyStatus) -> ExecEvent {
    ExecEvent::Reply {
        parent: MsgId::new(msg),
        status,
    }
}

fn output(msg: &str) -> ExecEvent {
    ExecEvent::Output {
        parent: MsgId::new(msg),
    }
}

fn idle(msg: &str) -> ExecEvent {
    ExecEvent::Idle {
        parent: MsgId::new(msg),
    }
}

fn remove(cell: &str) -> ExecEvent {
    ExecEvent::RemoveCell {
        cell_id: cell.to_owned(),
    }
}

fn trace(events: &[ExecEvent]) -> String {
    let mut state = ExecState::default();
    let mut lines = Vec::with_capacity(events.len());
    for event in events {
        let applied = state.apply(event.clone());
        lines.push(format!(
            "{} => {} | {}",
            serde_json::to_string(event).expect("serializes"),
            serde_json::to_string(&applied).expect("serializes"),
            serde_json::to_string(&state).expect("serializes")
        ));
    }
    lines.join("\n")
}

#[test]
fn single_cell_runs_to_done() {
    let events = vec![
        enqueue("c1"),
        sent("c1", "m1"),
        output("m1"),
        reply("m1", ReplyStatus::Ok),
        idle("m1"),
    ];
    insta::assert_snapshot!("exec-single-done", trace(&events));
}

#[test]
fn failures_end_cells_as_error() {
    let events = vec![
        enqueue("c1"),
        enqueue("c2"),
        sent("c1", "m1"),
        reply("m1", ReplyStatus::Error),
        sent("c2", "m2"),
        reply("m2", ReplyStatus::Aborted),
    ];
    insta::assert_snapshot!("exec-error-abort", trace(&events));
}

#[test]
fn two_cells_run_in_order() {
    let events = vec![
        enqueue("c1"),
        enqueue("c2"),
        sent("c1", "m1"),
        reply("m1", ReplyStatus::Ok),
        sent("c2", "m2"),
        idle("m1"),
        reply("m2", ReplyStatus::Ok),
        idle("m2"),
    ];
    insta::assert_snapshot!("exec-two-cells", trace(&events));
}

#[test]
fn rerequested_cells_run_again() {
    let events = vec![
        enqueue("c1"),
        enqueue("c2"),
        enqueue("c3"),
        sent("c1", "m1"),
        enqueue("c2"),
        enqueue("c1"),
        reply("m1", ReplyStatus::Ok),
        sent("c3", "m3"),
        reply("m3", ReplyStatus::Ok),
        sent("c2", "m2"),
        reply("m2", ReplyStatus::Ok),
        sent("c1", "m4"),
        reply("m4", ReplyStatus::Ok),
    ];
    insta::assert_snapshot!("exec-rerequest", trace(&events));
}

#[test]
fn unknown_ids_do_not_disturb() {
    let events = vec![
        enqueue("c1"),
        sent("c1", "m1"),
        reply("mx0", ReplyStatus::Ok),
        output("mx0"),
        idle("mx0"),
        reply("m1", ReplyStatus::Ok),
    ];
    insta::assert_snapshot!("exec-unknown-ids", trace(&events));
}

#[test]
fn late_output_still_maps_before_idle() {
    let events = vec![
        enqueue("c1"),
        enqueue("c2"),
        sent("c1", "m1"),
        reply("m1", ReplyStatus::Ok),
        sent("c2", "m2"),
        output("m1"),
        idle("m1"),
        output("m1"),
    ];
    insta::assert_snapshot!("exec-late-output", trace(&events));
}

#[test]
fn reset_drops_live_cells_only() {
    let events = vec![
        enqueue("c1"),
        sent("c1", "m1"),
        output("m1"),
        reply("m1", ReplyStatus::Ok),
        enqueue("c2"),
        sent("c2", "m2"),
        enqueue("c3"),
        output("m2"),
        ExecEvent::Reset,
        output("m2"),
    ];
    insta::assert_snapshot!("exec-reset", trace(&events));
}

#[test]
fn removing_cells_promotes_waiters() {
    let events = vec![
        enqueue("c1"),
        enqueue("c2"),
        enqueue("c3"),
        sent("c1", "m1"),
        remove("c2"),
        remove("c1"),
    ];
    insta::assert_snapshot!("exec-remove", trace(&events));
}

#[test]
fn save_clears_dirty_twice() {
    let events = vec![
        enqueue("c1"),
        sent("c1", "m1"),
        output("m1"),
        ExecEvent::Saved,
        output("m1"),
        ExecEvent::Saved,
    ];
    insta::assert_snapshot!("exec-save", trace(&events));
}
