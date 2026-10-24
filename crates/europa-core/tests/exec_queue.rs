use europa_core::{Applied, ExecEffect, ExecEvent, ExecState, MsgId};

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

fn reply(msg: &str, status: europa_core::ReplyStatus) -> ExecEvent {
    ExecEvent::Reply {
        parent: MsgId::new(msg),
        status,
    }
}

fn remove(cell: &str) -> ExecEvent {
    ExecEvent::RemoveCell {
        cell_id: cell.to_owned(),
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
fn first_request_starts_running() {
    let mut state = ExecState::default();
    let applied = apply(&mut state, enqueue("c1"));
    assert_eq!(
        applied.effect,
        Some(ExecEffect::Execute {
            cell_id: "c1".to_owned()
        })
    );
    assert_eq!(state.state("c1"), europa_core::CellExecState::Running);
    assert_eq!(state.running(), Some("c1"));
}

#[test]
fn second_request_waits_while_first_runs() {
    let mut state = ExecState::default();
    let _ = apply(&mut state, enqueue("c1"));
    let applied = apply(&mut state, enqueue("c2"));
    assert_eq!(applied, Applied::default());
    assert_eq!(state.state("c2"), europa_core::CellExecState::Queued);
    assert_eq!(state.running(), Some("c1"));
}

#[test]
fn reply_completes_and_promotes_next() {
    use europa_core::ReplyStatus;
    let mut state = ExecState::default();
    let _ = apply(&mut state, enqueue("c1"));
    let _ = apply(&mut state, enqueue("c2"));
    let _ = apply(&mut state, sent("c1", "m1"));
    let applied = apply(&mut state, reply("m1", ReplyStatus::Ok));
    assert_eq!(state.state("c1"), europa_core::CellExecState::Done);
    assert_eq!(
        applied.effect,
        Some(ExecEffect::Execute {
            cell_id: "c2".to_owned()
        })
    );
    assert_eq!(applied.cell, Some("c1".to_owned()));
    assert_eq!(state.state("c2"), europa_core::CellExecState::Running);
}

#[test]
fn failed_replies_mark_error_and_promote() {
    use europa_core::{CellExecState, ReplyStatus};
    for status in [ReplyStatus::Error, ReplyStatus::Aborted] {
        let mut state = ExecState::default();
        let _ = apply(&mut state, enqueue("c1"));
        let _ = apply(&mut state, enqueue("c2"));
        let _ = apply(&mut state, sent("c1", "m1"));
        let applied = apply(&mut state, reply("m1", status));
        assert_eq!(state.state("c1"), CellExecState::Error);
        assert_eq!(
            applied.effect,
            Some(ExecEffect::Execute {
                cell_id: "c2".to_owned()
            })
        );
        assert_eq!(state.state("c2"), CellExecState::Running);
    }
}

#[test]
fn reply_without_runners_returns_nothing() {
    use europa_core::ReplyStatus;
    let mut state = ExecState::default();
    let applied = apply(&mut state, reply("m1", ReplyStatus::Ok));
    assert_eq!(applied, Applied::default());
    assert_eq!(state.running(), None);
}

#[test]
fn waiters_keep_request_order() {
    let mut state = ExecState::default();
    let _ = apply(&mut state, enqueue("c1"));
    let _ = apply(&mut state, enqueue("c2"));
    let _ = apply(&mut state, enqueue("c3"));
    assert_eq!(
        state.queue().collect::<Vec<_>>(),
        vec!["c2".to_owned(), "c3".to_owned()]
    );
}

#[test]
fn repeated_request_moves_waiter_to_tail() {
    let mut state = ExecState::default();
    let _ = apply(&mut state, enqueue("c1"));
    let _ = apply(&mut state, enqueue("c2"));
    let _ = apply(&mut state, enqueue("c3"));
    let _ = apply(&mut state, sent("c1", "m1"));
    let applied = apply(&mut state, enqueue("c2"));
    assert_eq!(applied, Applied::default());
    assert_eq!(
        state.queue().collect::<Vec<_>>(),
        vec!["c3".to_owned(), "c2".to_owned()]
    );
}

#[test]
fn running_cell_can_queue_again_and_stays_queued_after_done() {
    use europa_core::{CellExecState, ReplyStatus};
    let mut state = ExecState::default();
    let _ = apply(&mut state, enqueue("c1"));
    let _ = apply(&mut state, enqueue("c2"));
    let _ = apply(&mut state, sent("c1", "m1"));
    let _ = apply(&mut state, enqueue("c1"));
    assert_eq!(state.state("c1"), CellExecState::Running);
    assert!(state.queue().any(|cell| cell == "c1"));
    let applied = apply(&mut state, reply("m1", ReplyStatus::Ok));
    assert_eq!(
        applied.effect,
        Some(ExecEffect::Execute {
            cell_id: "c2".to_owned()
        })
    );
    assert_eq!(state.state("c2"), CellExecState::Running);
    assert_eq!(state.state("c1"), CellExecState::Queued);
}

#[test]
fn finished_cell_runs_again_with_a_fresh_id() {
    use europa_core::{CellExecState, ReplyStatus};
    let mut state = ExecState::default();
    let _ = apply(&mut state, enqueue("c1"));
    let _ = apply(&mut state, sent("c1", "m1"));
    let _ = apply(&mut state, reply("m1", ReplyStatus::Ok));
    assert_eq!(state.state("c1"), CellExecState::Done);
    let applied = apply(&mut state, enqueue("c1"));
    assert_eq!(
        applied.effect,
        Some(ExecEffect::Execute {
            cell_id: "c1".to_owned()
        })
    );
    assert_eq!(state.state("c1"), CellExecState::Running);
    let _ = apply(&mut state, sent("c1", "m2"));
    assert_eq!(state.cell_for(&MsgId::new("m2")), Some("c1"));
}

#[test]
fn stray_send_requests_change_nothing() {
    let mut state = ExecState::default();
    let _ = apply(&mut state, enqueue("c1"));
    let _ = apply(&mut state, sent("c1", "m1"));
    let before = state.clone();
    // Cell never ran, so its send is stray.
    assert_eq!(apply(&mut state, sent("c2", "m2")), Applied::default());
    assert_eq!(state, before);
    // Running cell already holds an id, so a second send is stray.
    assert_eq!(apply(&mut state, sent("c1", "m9")), Applied::default());
    assert_eq!(state, before);
}

#[test]
fn deleting_cells_clears_queue_results_and_links() {
    use europa_core::{CellExecState, ReplyStatus};
    // Waiting cell disappears from the queue.
    let mut state = ExecState::default();
    let _ = apply(&mut state, enqueue("c1"));
    let _ = apply(&mut state, enqueue("c2"));
    let _ = apply(&mut state, sent("c1", "m1"));
    assert_eq!(apply(&mut state, remove("c2")), Applied::default());
    assert!(!state.queue().any(|cell| cell == "c2"));
    assert_eq!(state.state("c2"), CellExecState::Idle);

    // Deleting the running cell promotes the head waiter.
    let mut state = ExecState::default();
    let _ = apply(&mut state, enqueue("c1"));
    let _ = apply(&mut state, enqueue("c2"));
    let _ = apply(&mut state, sent("c1", "m1"));
    let applied = apply(&mut state, remove("c1"));
    assert_eq!(
        applied.effect,
        Some(ExecEffect::Execute {
            cell_id: "c2".to_owned()
        })
    );
    assert_eq!(state.cell_for(&MsgId::new("m1")), None);
    assert_eq!(state.state("c2"), CellExecState::Running);

    // Deleting a finished cell clears its result.
    let mut state = ExecState::default();
    let _ = apply(&mut state, enqueue("c1"));
    let _ = apply(&mut state, sent("c1", "m1"));
    let _ = apply(&mut state, reply("m1", ReplyStatus::Ok));
    assert_eq!(state.state("c1"), CellExecState::Done);
    assert_eq!(apply(&mut state, remove("c1")), Applied::default());
    assert_eq!(state.state("c1"), CellExecState::Idle);

    // Deleting a finished cell also drops its idle-wait link.
    let mut state = ExecState::default();
    let _ = apply(&mut state, enqueue("c1"));
    let _ = apply(&mut state, sent("c1", "m1"));
    let _ = apply(&mut state, reply("m1", ReplyStatus::Ok));
    assert_eq!(state.cell_for(&MsgId::new("m1")), Some("c1"));
    let _ = apply(&mut state, remove("c1"));
    assert_eq!(state.cell_for(&MsgId::new("m1")), None);
}
