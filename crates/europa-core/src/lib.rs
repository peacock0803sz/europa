//! Shared data types for Europa core models.
//!
//! Parse nbformat v4 notebooks that pass validation, preserving every field
//! value including unknown fields, and write them back deterministically
//! with sorted keys.
//!
//! Numbers keep the spelling they were read with, so `1E5` and `1.50` come
//! back unchanged. A notebook that Jupyter wrote is a fixed point of
//! [`parse`] followed by [`serialize`]: the saved bytes equal the input.
//!
//! ```
//! let bytes = br#"{"nbformat":4,"nbformat_minor":5,"metadata":{},"cells":[]}"#;
//! let notebook = europa_core::parse(bytes).expect("loads");
//! let text = europa_core::serialize(&notebook);
//! assert!(text.ends_with('\n'));
//! ```
//!
//! Execution state lives in [`exec`] as a pure state machine. Advance it with
//! [`ExecState::apply`]; the caller performs the returned [`ExecEffect`] and
//! feeds observed message ids back as [`ExecEvent::Sent`].
//!
//! ```
//! use europa_core::{CellExecState, ExecEvent, ExecState, MsgId, ReplyStatus};
//!
//! let mut state = ExecState::default();
//! let queued = state.apply(ExecEvent::Enqueue {
//!     cell_id: "c1".to_owned(),
//! });
//! assert!(queued.effect.is_some());
//! let _ = state.apply(ExecEvent::Sent {
//!     cell_id: "c1".to_owned(),
//!     msg_id: MsgId::new("m1"),
//! });
//! let done = state.apply(ExecEvent::Reply {
//!     parent: MsgId::new("m1"),
//!     status: ReplyStatus::Ok,
//! });
//! assert_eq!(done.cell.as_deref(), Some("c1"));
//! assert_eq!(state.state("c1"), CellExecState::Done);
//! ```
//!
//! Output messages from the kernel are folded into the cell with
//! [`apply_iopub`]. A `\r` overwrites the current line, so a progress bar
//! leaves only its latest state.
//!
//! ```
//! use europa_core::{apply_iopub, ExecEvent, ExecState, IopubMsg, MsgId};
//!
//! let bytes = br#"{"nbformat":4,"nbformat_minor":5,"metadata":{},"cells":[{"cell_type":"code","id":"c1","metadata":{},"source":"","outputs":[],"execution_count":null}]}"#;
//! let mut notebook = europa_core::parse(bytes).expect("loads");
//! let mut state = ExecState::default();
//! let _ = state.apply(ExecEvent::Enqueue {
//!     cell_id: "c1".to_owned(),
//! });
//! let _ = state.apply(ExecEvent::Sent {
//!     cell_id: "c1".to_owned(),
//!     msg_id: MsgId::new("m1"),
//! });
//! for text in ["\r10%", "\r20%"] {
//!     let msg = IopubMsg::Stream {
//!         parent: MsgId::new("m1"),
//!         name: "stdout".to_owned(),
//!         text: text.to_owned(),
//!     };
//!     let applied = apply_iopub(&mut state, &mut notebook, &msg);
//!     assert_eq!(applied.map(|applied| applied.cell), Some("c1".to_owned()));
//! }
//! assert!(europa_core::serialize(&notebook).contains(r#""20%""#));
//! assert!(!europa_core::serialize(&notebook).contains("10%"));
//! ```
//!
//! A `display_data` message with a display id can be updated in place, even
//! from another cell: the matching output is replaced where it was shown, and
//! [`IopubApplied::changed`] lists the cells that need a redraw.
//!
//! ```
//! use europa_core::{
//!     apply_iopub, ExecEvent, ExecState, IopubMsg, JsonMap, MsgId, ReplyStatus,
//! };
//!
//! let bytes = br#"{"nbformat":4,"nbformat_minor":5,"metadata":{},"cells":[{"cell_type":"code","id":"c1","metadata":{},"source":"","outputs":[],"execution_count":null},{"cell_type":"code","id":"c2","metadata":{},"source":"","outputs":[],"execution_count":null}]}"#;
//! let mut notebook = europa_core::parse(bytes).expect("loads");
//! let mut state = ExecState::default();
//! let bundle = |text: &str| JsonMap::from([("text/plain".to_owned(), text.into())]);
//!
//! let _ = state.apply(ExecEvent::Enqueue {
//!     cell_id: "c1".to_owned(),
//! });
//! let _ = state.apply(ExecEvent::Sent {
//!     cell_id: "c1".to_owned(),
//!     msg_id: MsgId::new("m1"),
//! });
//! let shown = IopubMsg::DisplayData {
//!     parent: MsgId::new("m1"),
//!     data: bundle("50%"),
//!     metadata: JsonMap::new(),
//!     display_id: Some("p".to_owned()),
//! };
//! let _ = apply_iopub(&mut state, &mut notebook, &shown);
//!
//! // The run of c1 ends and c2 runs next.
//! let _ = state.apply(ExecEvent::Reply {
//!     parent: MsgId::new("m1"),
//!     status: ReplyStatus::Ok,
//! });
//! let _ = state.apply(ExecEvent::Idle {
//!     parent: MsgId::new("m1"),
//! });
//! let _ = state.apply(ExecEvent::Enqueue {
//!     cell_id: "c2".to_owned(),
//! });
//! let _ = state.apply(ExecEvent::Sent {
//!     cell_id: "c2".to_owned(),
//!     msg_id: MsgId::new("m2"),
//! });
//!
//! let update = IopubMsg::UpdateDisplayData {
//!     parent: MsgId::new("m2"),
//!     data: bundle("100%"),
//!     metadata: JsonMap::new(),
//!     display_id: "p".to_owned(),
//! };
//! let applied = apply_iopub(&mut state, &mut notebook, &update).expect("known run");
//! assert_eq!(applied.cell, "c2");
//! assert_eq!(applied.changed, ["c1"]);
//! let saved = europa_core::serialize(&notebook);
//! assert!(saved.contains("100%"));
//! assert!(!saved.contains("50%"));
//! ```
#![deny(missing_docs)]

mod canonical;
pub mod error;
pub mod exec;
pub mod io;
pub mod iopub;
pub mod json;
pub mod normalize;
pub mod notebook;
pub mod pos;

pub use error::ParseError;
pub use exec::{Applied, CellExecState, ExecEffect, ExecEvent, ExecState, MsgId, ReplyStatus};
pub use io::{parse, serialize};
pub use iopub::{apply_iopub, IopubApplied, IopubMsg};
pub use json::{JsonMap, JsonNumber, JsonValue};
pub use normalize::MultilineText;
pub use notebook::{Cell, Notebook, Output};
pub use pos::{CellPos, ResolveError};
