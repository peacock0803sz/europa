use europa_core::{Applied, ExecEvent, ExecState, MsgId, ReplyStatus};

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
fn output_maps_to_its_sender() {
    let mut state = ExecState::default();
    let _ = apply(&mut state, enqueue("c1"));
    let _ = apply(&mut state, sent("c1", "m1"));
    assert_eq!(state.cell_for(&MsgId::new("m1")), Some("c1"));
    let applied = apply(&mut state, output("m1"));
    assert_eq!(applied.cell, Some("c1".to_owned()));
    assert_eq!(applied.effect, None);
}

#[test]
fn outputs_follow_their_own_cells() {
    let mut state = ExecState::default();
    let _ = apply(&mut state, enqueue("c1"));
    let _ = apply(&mut state, enqueue("c2"));
    let _ = apply(&mut state, sent("c1", "m1"));
    let _ = apply(&mut state, reply("m1", ReplyStatus::Ok));
    let _ = apply(&mut state, sent("c2", "m2"));
    assert_eq!(apply(&mut state, output("m1")).cell, Some("c1".to_owned()));
    assert_eq!(apply(&mut state, output("m2")).cell, Some("c2".to_owned()));
}

#[test]
fn waiting_cells_have_no_link_yet() {
    let mut state = ExecState::default();
    let _ = apply(&mut state, enqueue("c1"));
    let _ = apply(&mut state, enqueue("c2"));
    let _ = apply(&mut state, sent("c1", "m1"));
    assert_eq!(state.cell_for(&MsgId::new("m1")), Some("c1"));
    assert!(state.cell_for(&MsgId::new("m2")).is_none());
}

#[test]
fn links_survive_until_idle_and_then_drop() {
    use europa_core::CellExecState;
    let mut state = ExecState::default();
    let _ = apply(&mut state, enqueue("c1"));
    let _ = apply(&mut state, enqueue("c2"));
    let _ = apply(&mut state, sent("c1", "m1"));
    let _ = apply(&mut state, reply("m1", ReplyStatus::Ok));
    assert_eq!(state.state("c1"), CellExecState::Done);
    // Late output still maps while idle is pending.
    assert_eq!(apply(&mut state, output("m1")).cell, Some("c1".to_owned()));
    assert_eq!(state.state("c1"), CellExecState::Done);
    // Next cell runs while the old link waits for idle.
    assert_eq!(state.running(), Some("c2"));
    let _ = apply(&mut state, sent("c2", "m2"));
    assert_eq!(state.cell_for(&MsgId::new("m1")), Some("c1"));
    assert_eq!(state.cell_for(&MsgId::new("m2")), Some("c2"));
    let _ = apply(&mut state, idle("m1"));
    assert_eq!(apply(&mut state, output("m1")), Applied::default());
}

#[test]
fn repeated_reply_changes_nothing() {
    let mut state = ExecState::default();
    let _ = apply(&mut state, enqueue("c1"));
    let _ = apply(&mut state, sent("c1", "m1"));
    let first = apply(&mut state, reply("m1", ReplyStatus::Ok));
    assert_eq!(first.cell, Some("c1".to_owned()));
    let before = state.clone();
    assert_eq!(
        apply(&mut state, reply("m1", ReplyStatus::Ok)),
        Applied::default()
    );
    assert_eq!(state, before);
}

#[test]
fn idle_before_reply_keeps_the_link() {
    use europa_core::CellExecState;
    let mut state = ExecState::default();
    let _ = apply(&mut state, enqueue("c1"));
    let _ = apply(&mut state, enqueue("c2"));
    let _ = apply(&mut state, sent("c1", "m1"));
    let round_trip = |state: &ExecState| {
        let text = serde_json::to_string(state).expect("serializes");
        serde_json::from_str::<ExecState>(&text).expect("validates");
    };
    assert_eq!(apply(&mut state, idle("m1")), Applied::default());
    assert_eq!(state.state("c1"), CellExecState::Running);
    assert_eq!(state.cell_for(&MsgId::new("m1")), Some("c1"));
    round_trip(&state);
    assert_eq!(apply(&mut state, output("m1")).cell, Some("c1".to_owned()));
    round_trip(&state);
    let applied = apply(&mut state, reply("m1", ReplyStatus::Ok));
    assert_eq!(state.state("c1"), CellExecState::Done);
    assert!(applied.effect.is_some());
    assert_eq!(state.cell_for(&MsgId::new("m1")), None);
    round_trip(&state);
}

#[test]
fn unknown_ids_leave_state_untouched() {
    let mut state = ExecState::default();
    let _ = apply(&mut state, enqueue("c1"));
    let _ = apply(&mut state, sent("c1", "m1"));
    let before = state.clone();
    for index in 0..33 {
        let unknown = format!("mx{index}");
        assert_eq!(
            apply(&mut state, reply(&unknown, ReplyStatus::Ok)),
            Applied::default()
        );
        assert_eq!(apply(&mut state, output(&unknown)), Applied::default());
        assert_eq!(apply(&mut state, idle(&unknown)), Applied::default());
        assert_eq!(state, before);
    }
    assert_eq!(
        apply(&mut state, reply("mx33", ReplyStatus::Ok)),
        Applied::default()
    );
    assert_eq!(state, before);
    // A genuine reply still behaves as if the unknown ids never arrived.
    let mut expected = before.clone();
    let wanted = expected.apply(reply("m1", ReplyStatus::Ok));
    let got = apply(&mut state, reply("m1", ReplyStatus::Ok));
    assert_eq!(got, wanted);
    assert_eq!(state, expected);
}
