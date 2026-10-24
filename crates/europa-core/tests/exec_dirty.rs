use europa_core::{Applied, CellExecState, ExecEvent, ExecState, MsgId, ReplyStatus};

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

fn check_schedule(state: &ExecState) {
    if state.queue().count() > 0 {
        assert!(state.running().is_some());
    }
}

fn apply(state: &mut ExecState, event: ExecEvent) -> Applied {
    let applied = state.apply(event);
    check_schedule(state);
    applied
}

#[test]
fn linked_output_marks_dirty() {
    let mut state = ExecState::default();
    let _ = apply(&mut state, enqueue("c1"));
    let _ = apply(&mut state, sent("c1", "m1"));
    assert!(!state.is_dirty());
    let applied = apply(&mut state, output("m1"));
    assert_eq!(applied.cell, Some("c1".to_owned()));
    assert!(state.is_dirty());
}

#[test]
fn unknown_output_keeps_clean() {
    let mut state = ExecState::default();
    assert_eq!(apply(&mut state, output("mx0")), Applied::default());
    assert!(!state.is_dirty());
}

#[test]
fn save_clears_dirty() {
    let mut state = ExecState::default();
    let _ = apply(&mut state, enqueue("c1"));
    let _ = apply(&mut state, sent("c1", "m1"));
    let _ = apply(&mut state, output("m1"));
    assert!(state.is_dirty());
    let applied = apply(&mut state, ExecEvent::Saved);
    assert_eq!(applied, Applied::default());
    assert!(!state.is_dirty());
}

#[test]
fn other_events_keep_clean() {
    let mut state = ExecState::default();
    assert!(!state.is_dirty());
    let _ = apply(&mut state, enqueue("c1"));
    assert!(!state.is_dirty());
    let _ = apply(&mut state, sent("c1", "m1"));
    assert!(!state.is_dirty());
    let _ = apply(&mut state, reply("m1", ReplyStatus::Ok));
    assert!(!state.is_dirty());
    let _ = apply(&mut state, idle("m1"));
    assert!(!state.is_dirty());
    let _ = apply(
        &mut state,
        ExecEvent::RemoveCell {
            cell_id: "c1".to_owned(),
        },
    );
    assert!(!state.is_dirty());
    let _ = apply(&mut state, ExecEvent::Reset);
    assert!(!state.is_dirty());
}

#[test]
fn other_events_keep_dirty() {
    let mut state = ExecState::default();
    let _ = apply(&mut state, enqueue("c1"));
    let _ = apply(&mut state, sent("c1", "m1"));
    let _ = apply(&mut state, output("m1"));
    assert!(state.is_dirty());
    let _ = apply(&mut state, enqueue("c2"));
    assert!(state.is_dirty());
    let _ = apply(&mut state, sent("c2", "m2"));
    assert!(state.is_dirty());
    let _ = apply(&mut state, reply("m2", ReplyStatus::Ok));
    assert!(state.is_dirty());
    let _ = apply(&mut state, idle("m2"));
    assert!(state.is_dirty());
    let _ = apply(&mut state, reply("mx0", ReplyStatus::Ok));
    assert!(state.is_dirty());
    let _ = apply(&mut state, output("mx0"));
    assert!(state.is_dirty());
    let _ = apply(&mut state, idle("mx0"));
    assert!(state.is_dirty());
    let _ = apply(
        &mut state,
        ExecEvent::RemoveCell {
            cell_id: "c9".to_owned(),
        },
    );
    assert!(state.is_dirty());
    let _ = apply(&mut state, ExecEvent::Reset);
    assert!(state.is_dirty());
}

#[test]
fn dirty_follows_output_then_save() {
    let mut state = ExecState::default();
    let mut seen = Vec::new();
    let _ = apply(&mut state, enqueue("c1"));
    seen.push(state.is_dirty());
    let _ = apply(&mut state, sent("c1", "m1"));
    seen.push(state.is_dirty());
    let _ = apply(&mut state, output("m1"));
    seen.push(state.is_dirty());
    let _ = apply(&mut state, reply("m1", ReplyStatus::Ok));
    seen.push(state.is_dirty());
    let _ = apply(&mut state, idle("m1"));
    seen.push(state.is_dirty());
    let _ = apply(&mut state, ExecEvent::Saved);
    seen.push(state.is_dirty());
    assert_eq!(seen, vec![false, false, true, true, true, false]);
}

#[test]
fn reset_keeps_finished_and_drops_live_cells() {
    let mut state = ExecState::default();
    let _ = apply(&mut state, enqueue("c1"));
    let _ = apply(&mut state, sent("c1", "m1"));
    let _ = apply(&mut state, reply("m1", ReplyStatus::Ok));
    let _ = apply(&mut state, enqueue("c2"));
    let _ = apply(&mut state, sent("c2", "m2"));
    let _ = apply(&mut state, enqueue("c3"));
    let _ = apply(&mut state, output("m2"));
    assert!(state.is_dirty());
    let applied = apply(&mut state, ExecEvent::Reset);
    assert_eq!(applied, Applied::default());
    assert_eq!(state.state("c1"), CellExecState::Done);
    assert_eq!(state.state("c2"), CellExecState::Idle);
    assert_eq!(state.state("c3"), CellExecState::Idle);
    assert_eq!(state.queue().count(), 0);
    assert_eq!(state.running(), None);
    assert_eq!(state.cell_for(&MsgId::new("m1")), None);
    assert_eq!(state.cell_for(&MsgId::new("m2")), None);
    assert!(state.is_dirty());
}

#[test]
fn reset_clears_a_rerequested_cell() {
    let mut state = ExecState::default();
    let _ = apply(&mut state, enqueue("c1"));
    let _ = apply(&mut state, sent("c1", "m1"));
    let _ = apply(&mut state, enqueue("c1"));
    let _ = apply(&mut state, reply("m1", ReplyStatus::Ok));
    assert_ne!(state.state("c1"), CellExecState::Done);
    let _ = apply(&mut state, ExecEvent::Reset);
    assert_eq!(state.state("c1"), CellExecState::Idle);
}

#[test]
fn output_after_reset_changes_nothing() {
    let mut state = ExecState::default();
    let _ = apply(&mut state, enqueue("c1"));
    let _ = apply(&mut state, enqueue("c2"));
    let _ = apply(&mut state, sent("c1", "m1"));
    let _ = apply(&mut state, reply("m1", ReplyStatus::Ok));
    let _ = apply(&mut state, sent("c2", "m2"));
    let _ = apply(&mut state, output("m2"));
    let _ = apply(&mut state, ExecEvent::Reset);
    let before = state.clone();
    assert_eq!(apply(&mut state, output("m2")), Applied::default());
    assert_eq!(state, before);
}
