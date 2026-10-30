//! Execution state for one notebook.
//!
//! Tracks queue, running cell, terminal results, message correlation, pending
//! clears, a count of resets that retires display ids, and the dirty flag as a
//! pure state machine. Advance it with [`ExecState::apply`]; the caller
//! performs the returned [`ExecEffect`] and feeds observed message ids back as
//! [`ExecEvent::Sent`]. This keeps id generation and IO outside the
//! transition, so the same event sequence always yields the same state.
//! Pending clears are the exception: [`crate::apply_iopub`] records and ends
//! them, and events only drop them together with their correlation entries.
//!
//! Cell status is derived from queue, running slot, and results rather than
//! stored per cell. A correlation entry is kept until both `execute_reply` and
//! `status: idle` have been observed for its message id, in either order, so
//! late outputs still map to their original cell. [`ExecEvent::Reset`] and
//! [`ExecEvent::RemoveCell`] drop entries early, so outputs arriving after
//! them are not mapped. Send failures and lost kernels are recovered by the
//! caller with [`ExecEvent::Reset`].

use std::collections::{BTreeMap, BTreeSet, VecDeque};

/// Parent message id of an `execute_request`.
///
/// Created by the kernel layer; this module only compares and records it.
/// Serializes as a plain string and orders as one for map keys.
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(transparent)]
pub struct MsgId(String);

impl MsgId {
    /// Wrap a message id.
    #[must_use]
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Borrow the id as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<String> for MsgId {
    fn from(id: String) -> Self {
        Self(id)
    }
}

impl From<&str> for MsgId {
    fn from(id: &str) -> Self {
        Self(id.to_owned())
    }
}

/// Execution stage of a single cell, derived by [`ExecState::state`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum CellExecState {
    /// Not scheduled: no queue entry, running slot, or terminal result.
    Idle,
    /// Waiting in the queue for its turn.
    Queued,
    /// Currently executing.
    Running,
    /// Finished successfully.
    Done,
    /// Finished with an error or was aborted.
    Error,
}

/// Outcome of an `execute_reply`.
///
/// [`ReplyStatus::Ok`] maps to [`CellExecState::Done`], while
/// [`ReplyStatus::Error`] and [`ReplyStatus::Aborted`] map to
/// [`CellExecState::Error`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ReplyStatus {
    /// Execution succeeded.
    Ok,
    /// Execution failed.
    Error,
    /// Execution was aborted.
    Aborted,
}

/// Last terminal result of a cell.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) enum Outcome {
    /// Execution succeeded; derives [`CellExecState::Done`].
    Done,
    /// Execution failed or was aborted; derives [`CellExecState::Error`].
    Error,
}

/// Running slot of the state machine.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Running {
    /// Id of the running cell.
    pub(crate) cell_id: String,
    /// Message id once `Sent` arrives; `None` between promotion and `Sent`.
    pub(crate) msg_id: Option<MsgId>,
    /// Whether `status: idle` arrived before `execute_reply`.
    pub(crate) idle_seen: bool,
}

/// Execution state of one notebook.
///
/// `Default` is the state right after opening a notebook: every cell is idle
/// and the dirty flag is false. Per-cell status is not stored; [`ExecState::state`]
/// derives it from queue, running slot, and results. The hand-written
/// `Deserialize` impl validates the invariants below and returns `Err` on
/// violation.
///
/// The state counts resets to tell the display ids bound before one from those
/// bound after it. A state rebuilt with `Default` starts the count over, so
/// outputs that the notebook still holds from before can match their display
/// ids again. Send [`ExecEvent::Reset`] when the kernel restarts instead.
///
/// # Invariants
///
/// These hold after every [`ExecState::apply`]:
///
/// 1. The queue never holds the same cell twice.
/// 2. If the running cell has a message id, the correlation map has an entry
///    for that id pointing to the running cell.
/// 3. If the running cell has seen `status: idle`, it has a message id.
/// 4. If the queue is non-empty, a cell is running.
/// 5. Every pending clear is keyed by a message id that has a correlation
///    entry.
///
/// ```
/// use europa_core::{CellExecState, ExecEvent, ExecState, MsgId, ReplyStatus};
///
/// let mut state = ExecState::default();
/// let _ = state.apply(ExecEvent::Enqueue {
///     cell_id: "c1".to_owned(),
/// });
/// let _ = state.apply(ExecEvent::Sent {
///     cell_id: "c1".to_owned(),
///     msg_id: MsgId::new("m1"),
/// });
/// let _ = state.apply(ExecEvent::Reply {
///     parent: MsgId::new("m1"),
///     status: ReplyStatus::Ok,
/// });
/// assert_eq!(state.state("c1"), CellExecState::Done);
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub struct ExecState {
    pub(crate) queue: VecDeque<String>,
    pub(crate) running: Option<Running>,
    pub(crate) results: BTreeMap<String, Outcome>,
    pub(crate) correlation: BTreeMap<MsgId, String>,
    pub(crate) dirty: bool,
    // Skipped when empty so states without a pending clear serialize as before.
    #[serde(skip_serializing_if = "BTreeSet::is_empty")]
    pub(crate) pending_clear: BTreeSet<MsgId>,
    // Skipped when zero so states that never saw a reset serialize as before.
    #[serde(skip_serializing_if = "is_zero")]
    pub(crate) display_generation: u64,
}

fn is_zero(value: &u64) -> bool {
    *value == 0
}

impl<'de> serde::Deserialize<'de> for ExecState {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(serde::Deserialize)]
        struct Fields {
            queue: VecDeque<String>,
            running: Option<Running>,
            results: BTreeMap<String, Outcome>,
            correlation: BTreeMap<MsgId, String>,
            dirty: bool,
            #[serde(default)]
            pending_clear: BTreeSet<MsgId>,
            #[serde(default)]
            display_generation: u64,
        }
        let fields = Fields::deserialize(deserializer)?;
        let mut seen = std::collections::BTreeSet::new();
        if fields.queue.iter().any(|cell| !seen.insert(cell)) {
            return Err(serde::de::Error::custom(
                "invariant 1 violated: queue must not hold the same cell twice",
            ));
        }
        if let Some(running) = &fields.running {
            if let Some(msg) = &running.msg_id {
                if fields
                    .correlation
                    .get(msg)
                    .is_none_or(|cell| cell != &running.cell_id)
                {
                    return Err(serde::de::Error::custom(
                        "invariant 2 violated: running msg_id must have a correlation entry pointing to the running cell",
                    ));
                }
            }
            if running.idle_seen && running.msg_id.is_none() {
                return Err(serde::de::Error::custom(
                    "invariant 3 violated: running idle_seen requires msg_id to be set",
                ));
            }
        }
        if !fields.queue.is_empty() && fields.running.is_none() {
            return Err(serde::de::Error::custom(
                "invariant 4 violated: non-empty queue requires a running cell",
            ));
        }
        if fields
            .pending_clear
            .iter()
            .any(|parent| !fields.correlation.contains_key(parent))
        {
            return Err(serde::de::Error::custom(
                "invariant 5 violated: pending_clear must only hold correlated message ids",
            ));
        }
        Ok(Self {
            queue: fields.queue,
            running: fields.running,
            results: fields.results,
            correlation: fields.correlation,
            dirty: fields.dirty,
            pending_clear: fields.pending_clear,
            display_generation: fields.display_generation,
        })
    }
}

/// Event that advances [`ExecState`].
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum ExecEvent {
    /// The user ran a cell; bulk runs send one event per cell in request order.
    ///
    /// - A cell that is already queued moves to the back of the queue.
    /// - The running cell is appended to the queue, scheduling a rerun after
    ///   the current run finishes; the running slot is left untouched.
    /// - Any other cell has its previous terminal result cleared, is appended
    ///   to the queue, and is promoted if nothing is running.
    Enqueue {
        /// Target cell id.
        cell_id: String,
    },
    /// `Execute` was handled and `execute_request` returned this message id.
    ///
    /// Ignored when `cell_id` is not the running cell or the running cell
    /// already has a message id.
    Sent {
        /// Target cell id.
        cell_id: String,
        /// Message id assigned by the kernel layer.
        msg_id: MsgId,
    },
    /// An `execute_reply` arrived on the shell channel.
    ///
    /// Records the terminal result of the running cell and promotes the next
    /// queued cell. Ignored when `parent` is not the running cell's message
    /// id; [`Applied::cell`] is then `None`, so callers can log the unknown id.
    ///
    /// When it drops the correlation entry, it drops the pending clear for the
    /// same id too.
    Reply {
        /// Parent message id of the reply.
        parent: MsgId,
        /// Reply outcome.
        status: ReplyStatus,
    },
    /// An iopub message arrived that updates outputs or the execution count.
    /// Do not send for `status`-like messages that change nothing.
    ///
    /// Sets the dirty flag. Ignored when `parent` has no correlation entry;
    /// [`Applied::cell`] is then `None`, so callers can log the unknown id.
    ///
    /// [`crate::apply_iopub`] applies it itself whenever it lists a cell in
    /// [`crate::IopubApplied::changed`], so do not send it for messages handed
    /// to `apply_iopub`. Otherwise messages that change nothing, such as an
    /// update with no bound output or a repeated execution count, would set
    /// the dirty flag.
    Output {
        /// Parent message id of the output.
        parent: MsgId,
    },
    /// `status: idle` arrived on iopub.
    ///
    /// For the running cell's message id this only marks idle as seen and
    /// keeps the correlation entry until `Reply`; for a finished cell it drops
    /// the entry and its pending clear. Ignored when `parent` has no
    /// correlation entry.
    Idle {
        /// Parent message id of the idle status.
        parent: MsgId,
    },
    /// An interrupt or restart happened. The caller also sends this after a
    /// send failure or a lost kernel to return queued and running cells to idle.
    ///
    /// Clears the terminal results of queued and running cells, empties the
    /// queue and running slot, and drops all correlation entries and pending
    /// clears, so later replies and outputs for earlier message ids are
    /// ignored. Results of other cells and the dirty flag are unchanged.
    ///
    /// Outputs bound to a display id before the reset are not replaced
    /// afterwards. That holds after an interrupt too, although the kernel and
    /// its display handles survive it, because the same event stands for a
    /// restart, after which a fixed id such as `"progress"` may be reused for
    /// something else.
    Reset,
    /// A cell was deleted.
    ///
    /// Removes the cell from the queue, its terminal result, and every
    /// correlation entry pointing to it, along with the pending clears of those
    /// entries. If the cell was running, the slot is released immediately and
    /// the next queued cell is promoted without waiting for the kernel; the
    /// removed run's later reply and outputs are ignored.
    RemoveCell {
        /// Deleted cell id.
        cell_id: String,
    },
    /// A save succeeded. Do not send on failure.
    Saved,
}

/// Work the caller must perform after [`ExecState::apply`].
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "effect", rename_all = "snake_case")]
pub enum ExecEffect {
    /// Send `execute_request` for this cell, then apply `Sent` with the
    /// returned id before any other event.
    Execute {
        /// Cell to execute.
        cell_id: String,
    },
}

/// Result of applying one [`ExecEvent`].
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[must_use]
pub struct Applied {
    /// Work the caller must perform, if any.
    pub effect: Option<ExecEffect>,
    /// Cell a reply or output mapped to, if any.
    pub cell: Option<String>,
}

impl ExecState {
    /// Advance the state with one event.
    ///
    /// Has no IO and returns the same result for the same input. At most one
    /// promotion happens per call, so the returned effect holds at most one
    /// `Execute`.
    pub fn apply(&mut self, event: ExecEvent) -> Applied {
        match event {
            ExecEvent::Enqueue { cell_id } => {
                if let Some(pos) = self.queue.iter().position(|queued| *queued == cell_id) {
                    self.queue.remove(pos);
                    self.queue.push_back(cell_id);
                    return Applied::default();
                }
                if self
                    .running
                    .as_ref()
                    .is_some_and(|running| running.cell_id == cell_id)
                {
                    self.queue.push_back(cell_id);
                    return Applied::default();
                }
                self.results.remove(&cell_id);
                self.queue.push_back(cell_id);
                Applied {
                    effect: self.promote(),
                    cell: None,
                }
            }
            ExecEvent::Sent { cell_id, msg_id } => {
                let matches = self
                    .running
                    .as_ref()
                    .is_some_and(|running| running.cell_id == cell_id && running.msg_id.is_none());
                if !matches {
                    return Applied::default();
                }
                if let Some(running) = self.running.as_mut() {
                    running.msg_id = Some(msg_id.clone());
                }
                self.correlation.insert(msg_id, cell_id);
                Applied::default()
            }
            ExecEvent::Reply { parent, status } => {
                let matches = self
                    .running
                    .as_ref()
                    .is_some_and(|running| running.msg_id.as_ref() == Some(&parent));
                if !matches {
                    return Applied::default();
                }
                let running = self.running.take().expect("checked above");
                let outcome = match status {
                    ReplyStatus::Ok => Outcome::Done,
                    ReplyStatus::Error | ReplyStatus::Aborted => Outcome::Error,
                };
                self.results.insert(running.cell_id.clone(), outcome);
                if running.idle_seen {
                    self.correlation.remove(&parent);
                    self.pending_clear.remove(&parent);
                }
                Applied {
                    effect: self.promote(),
                    cell: Some(running.cell_id),
                }
            }
            ExecEvent::RemoveCell { cell_id } => {
                self.queue.retain(|queued| *queued != cell_id);
                self.results.remove(&cell_id);
                self.correlation.retain(|_, linked| *linked != cell_id);
                self.pending_clear
                    .retain(|parent| self.correlation.contains_key(parent));
                let was_running = self
                    .running
                    .as_ref()
                    .is_some_and(|running| running.cell_id == cell_id);
                if !was_running {
                    return Applied::default();
                }
                self.running = None;
                Applied {
                    effect: self.promote(),
                    cell: None,
                }
            }
            ExecEvent::Output { parent } => match self.correlation.get(&parent) {
                Some(cell_id) => {
                    let cell_id = cell_id.clone();
                    self.dirty = true;
                    Applied {
                        effect: None,
                        cell: Some(cell_id),
                    }
                }
                None => Applied::default(),
            },
            ExecEvent::Idle { parent } => {
                let is_running = self
                    .running
                    .as_ref()
                    .is_some_and(|running| running.msg_id.as_ref() == Some(&parent));
                if is_running {
                    if let Some(running) = self.running.as_mut() {
                        running.idle_seen = true;
                    }
                    return Applied::default();
                }
                self.correlation.remove(&parent);
                self.pending_clear.remove(&parent);
                Applied::default()
            }
            ExecEvent::Reset => {
                if let Some(running) = &self.running {
                    self.results.remove(&running.cell_id);
                }
                for queued in &self.queue {
                    self.results.remove(queued);
                }
                self.queue.clear();
                self.running = None;
                self.correlation.clear();
                self.pending_clear.clear();
                // A counter that panics on overflow would be worse than one that
                // wraps, and 2^64 resets never happen in practice.
                self.display_generation = self.display_generation.wrapping_add(1);
                Applied::default()
            }
            ExecEvent::Saved => {
                self.dirty = false;
                Applied::default()
            }
        }
    }

    fn promote(&mut self) -> Option<ExecEffect> {
        if self.running.is_some() || self.queue.is_empty() {
            return None;
        }
        let cell_id = self.queue.pop_front().expect("checked above");
        self.running = Some(Running {
            cell_id: cell_id.clone(),
            msg_id: None,
            idle_seen: false,
        });
        Some(ExecEffect::Execute { cell_id })
    }

    /// Derive the execution stage of a cell.
    ///
    /// Running wins over queued, queued wins over the last terminal result,
    /// and anything else is idle.
    #[must_use]
    pub fn state(&self, cell_id: &str) -> CellExecState {
        if self
            .running
            .as_ref()
            .is_some_and(|running| running.cell_id == cell_id)
        {
            return CellExecState::Running;
        }
        if self.queue.iter().any(|queued| queued == cell_id) {
            return CellExecState::Queued;
        }
        match self.results.get(cell_id) {
            Some(Outcome::Done) => CellExecState::Done,
            Some(Outcome::Error) => CellExecState::Error,
            None => CellExecState::Idle,
        }
    }

    /// Iterate the waiting queue from front to back.
    pub fn queue(&self) -> impl Iterator<Item = &str> + '_ {
        self.queue.iter().map(String::as_str)
    }

    /// Borrow the running cell id, if any.
    #[must_use]
    pub fn running(&self) -> Option<&str> {
        self.running
            .as_ref()
            .map(|running| running.cell_id.as_str())
    }

    /// Look up which cell a parent message id belongs to.
    #[must_use]
    pub fn cell_for(&self, parent: &MsgId) -> Option<&str> {
        self.correlation.get(parent).map(String::as_str)
    }

    /// Whether the run with this parent id received `clear_output(wait=true)`
    /// and has not cleared its outputs yet.
    ///
    /// The pending state also ends, without clearing, when the run's
    /// correlation entry is dropped.
    #[must_use]
    pub fn is_clear_pending(&self, parent: &MsgId) -> bool {
        self.pending_clear.contains(parent)
    }

    /// Whether the notebook has unsaved output changes.
    #[must_use]
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state_with(
        queue: &[&str],
        running: Option<Running>,
        results: &[(&str, Outcome)],
        correlation: &[(&str, &str)],
        dirty: bool,
    ) -> ExecState {
        ExecState {
            queue: queue.iter().map(|cell| (*cell).to_owned()).collect(),
            running,
            results: results
                .iter()
                .map(|(cell, outcome)| ((*cell).to_owned(), outcome.clone()))
                .collect(),
            correlation: correlation
                .iter()
                .map(|(id, cell)| (MsgId::new(*id), (*cell).to_owned()))
                .collect(),
            dirty,
            pending_clear: BTreeSet::new(),
            display_generation: 0,
        }
    }

    fn running(cell: &str, msg: Option<&str>, idle_seen: bool) -> Running {
        Running {
            cell_id: cell.to_owned(),
            msg_id: msg.map(MsgId::new),
            idle_seen,
        }
    }

    #[test]
    fn derives_state_with_running_over_queue_over_result() {
        let state = state_with(
            &["c1"],
            Some(running("c1", Some("m1"), false)),
            &[("c1", Outcome::Done), ("c2", Outcome::Done)],
            &[("m1", "c1")],
            false,
        );
        // Same cell in every slot still reports running.
        assert_eq!(state.state("c1"), CellExecState::Running);
        // Queued waiter with a terminal result still reports queued.
        let queued = state_with(
            &["c2"],
            Some(running("c1", Some("m1"), false)),
            &[("c2", Outcome::Done)],
            &[("m1", "c1")],
            false,
        );
        assert_eq!(queued.state("c2"), CellExecState::Queued);
        assert_eq!(queued.state("c1"), CellExecState::Running);
    }

    #[test]
    fn derives_done_error_and_idle() {
        let state = state_with(
            &[],
            None,
            &[("c1", Outcome::Done), ("c2", Outcome::Error)],
            &[],
            false,
        );
        assert_eq!(state.state("c1"), CellExecState::Done);
        assert_eq!(state.state("c2"), CellExecState::Error);
        assert_eq!(state.state("missing"), CellExecState::Idle);
    }

    #[test]
    fn reads_queue_running_correlation_and_dirty() {
        let state = state_with(
            &["c2", "c3"],
            Some(running("c1", Some("m1"), false)),
            &[],
            &[("m1", "c1")],
            true,
        );
        assert_eq!(
            state.queue().collect::<Vec<_>>(),
            vec!["c2", "c3"],
            "queue reads front to back"
        );
        assert_eq!(state.running(), Some("c1"));
        assert_eq!(state.cell_for(&MsgId::new("m1")), Some("c1"));
        assert_eq!(state.cell_for(&MsgId::new("unknown")), None);
        assert!(state.is_dirty());
    }

    #[test]
    fn default_reports_idle_and_clean() {
        let state = ExecState::default();
        assert_eq!(state.state("c1"), CellExecState::Idle);
        assert_eq!(state.state("anything"), CellExecState::Idle);
        assert!(!state.is_dirty());
        assert_eq!(state.running(), None);
        assert_eq!(state.queue().count(), 0);
    }

    #[test]
    fn round_trips_valid_states() {
        let fresh = ExecState::default();
        let text = serde_json::to_string(&fresh).expect("serializes");
        assert_eq!(
            serde_json::from_str::<ExecState>(&text).expect("loads"),
            fresh
        );

        let full = state_with(
            &["c2", "c3"],
            Some(running("c1", Some("m1"), false)),
            &[("c0", Outcome::Done)],
            &[("m1", "c1")],
            true,
        );
        let text = serde_json::to_string(&full).expect("serializes");
        assert_eq!(
            serde_json::from_str::<ExecState>(&text).expect("loads"),
            full
        );
    }

    fn assert_violates(text: &str, invariant: u8) {
        let err = serde_json::from_str::<ExecState>(text).expect_err("violation is rejected");
        let wanted = format!("invariant {invariant} violated");
        assert!(
            err.to_string().contains(&wanted),
            "expected {wanted:?} in {err}"
        );
    }

    #[test]
    fn rejects_each_invariant_violation() {
        // Invariant 1: queue holds the same cell twice.
        let duplicate = r#"{"queue":["c1","c1"],"running":{"cell_id":"c1","msg_id":null,"idle_seen":false},"results":{},"correlation":{},"dirty":false}"#;
        assert_violates(duplicate, 1);
        // Invariant 2: running id has no correlation entry.
        let dangling = r#"{"queue":[],"running":{"cell_id":"c1","msg_id":"m1","idle_seen":false},"results":{},"correlation":{},"dirty":false}"#;
        assert_violates(dangling, 2);
        // Invariant 2: running id has an entry, but it points to another cell.
        let mislinked = r#"{"queue":[],"running":{"cell_id":"c1","msg_id":"m1","idle_seen":false},"results":{},"correlation":{"m1":"c2"},"dirty":false}"#;
        assert_violates(mislinked, 2);
        // Invariant 3: idle flag without a message id.
        let idle_without_id = r#"{"queue":[],"running":{"cell_id":"c1","msg_id":null,"idle_seen":true},"results":{},"correlation":{},"dirty":false}"#;
        assert_violates(idle_without_id, 3);
        // Invariant 4: waiters without a running cell.
        let waiter_without_running =
            r#"{"queue":["c1"],"running":null,"results":{},"correlation":{},"dirty":false}"#;
        assert_violates(waiter_without_running, 4);
        // Invariant 5: pending clear without a correlation entry.
        let orphan_clear = r#"{"queue":[],"running":null,"results":{},"correlation":{},"dirty":false,"pending_clear":["m1"]}"#;
        assert_violates(orphan_clear, 5);
    }

    #[test]
    fn round_trips_pending_clear() {
        let mut state = state_with(
            &[],
            Some(running("c1", Some("m1"), false)),
            &[],
            &[("m1", "c1")],
            false,
        );
        state.pending_clear.insert(MsgId::new("m1"));
        let text = serde_json::to_string(&state).expect("serializes");
        assert!(text.contains(r#""pending_clear":["m1"]"#));
        assert_eq!(
            serde_json::from_str::<ExecState>(&text).expect("loads"),
            state
        );
        assert!(state.is_clear_pending(&MsgId::new("m1")));
        assert!(!state.is_clear_pending(&MsgId::new("m2")));
    }

    #[test]
    fn omits_empty_pending_clear_from_json() {
        let text = serde_json::to_string(&ExecState::default()).expect("serializes");
        assert!(!text.contains("pending_clear"), "{text}");
    }

    #[test]
    fn round_trips_a_display_generation() {
        let state = ExecState {
            display_generation: 3,
            ..ExecState::default()
        };
        let text = serde_json::to_string(&state).expect("serializes");
        assert!(text.contains(r#""display_generation":3"#), "{text}");
        assert_eq!(
            serde_json::from_str::<ExecState>(&text).expect("loads"),
            state
        );
    }

    #[test]
    fn omits_a_zero_display_generation_from_json() {
        let text = serde_json::to_string(&ExecState::default()).expect("serializes");
        assert!(!text.contains("display_generation"), "{text}");
    }

    #[test]
    fn reset_advances_the_display_generation() {
        let mut state = ExecState::default();
        let _ = state.apply(ExecEvent::Reset);
        let _ = state.apply(ExecEvent::Reset);
        assert_eq!(state.display_generation, 2);
    }

    #[test]
    fn reset_wraps_the_display_generation_instead_of_panicking() {
        let mut state = ExecState {
            display_generation: u64::MAX,
            ..ExecState::default()
        };
        let _ = state.apply(ExecEvent::Reset);
        assert_eq!(state.display_generation, 0);
    }
}
