//! Scripted kernel driving execution state without a real kernel.

use std::collections::VecDeque;
use std::future::Future;
use std::sync::{mpsc, Mutex};
use std::time::Duration;

use europa_core::{
    apply_iopub, parse, serialize, CellExecState, ExecEffect, ExecEvent, ExecState, IopubMsg,
    KernelError, KernelEvent, KernelInfo, KernelStatus, LanguageInfo, MsgId, Notebook, ReplyStatus,
};
use europa_kernel::KernelClient;

/// Kernel double driven only by the public window.
struct ScriptedKernel {
    /// Description returned by `kernel_info` and `restart`.
    info: KernelInfo,
    /// Queued scripts keyed by the id `execute` will return.
    scripts: Mutex<VecDeque<(MsgId, Vec<KernelEvent>)>>,
    /// Channel sending scripted events to the test.
    sender: mpsc::Sender<KernelEvent>,
    /// Channel handed out once by `messages`.
    receiver: Mutex<Option<mpsc::Receiver<KernelEvent>>>,
}

fn assert_send_sync<T: Send + Sync>() {}

impl ScriptedKernel {
    /// Build a double with queued scripts and a fresh channel.
    fn new(info: KernelInfo, scripts: VecDeque<(MsgId, Vec<KernelEvent>)>) -> Self {
        let (sender, receiver) = mpsc::channel();
        assert_send_sync::<Self>();
        Self {
            info,
            scripts: Mutex::new(scripts),
            sender,
            receiver: Mutex::new(Some(receiver)),
        }
    }
}

impl KernelClient for ScriptedKernel {
    fn kernel_info(&self) -> impl Future<Output = Result<KernelInfo, KernelError>> + Send {
        let info = self.info.clone();
        async move { Ok(info) }
    }

    fn execute(&self, code: &str) -> impl Future<Output = Result<MsgId, KernelError>> + Send {
        let _ = code;
        let (id, events) = self
            .scripts
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .pop_front()
            .expect("scripted execute has a script");
        for event in events {
            self.sender.send(event).expect("receiver lives");
        }
        async move { Ok(id) }
    }

    async fn interrupt(&self) -> Result<(), KernelError> {
        Ok(())
    }

    fn restart(&self) -> impl Future<Output = Result<KernelInfo, KernelError>> + Send {
        let info = self.info.clone();
        async move { Ok(info) }
    }

    async fn shutdown(&self) -> Result<(), KernelError> {
        Ok(())
    }

    fn messages(&mut self) -> Option<mpsc::Receiver<KernelEvent>> {
        self.receiver
            .get_mut()
            .unwrap_or_else(|error| error.into_inner())
            .take()
    }
}

/// Drive one cell through queueing, sending, and collecting its run.
async fn run_cell<K: KernelClient>(
    client: &K,
    events: &mpsc::Receiver<KernelEvent>,
    exec: &mut ExecState,
    notebook: &mut Notebook,
    cell_id: &str,
) {
    let applied = exec.apply(ExecEvent::Enqueue {
        cell_id: cell_id.to_owned(),
    });
    let ExecEffect::Execute { .. } = applied.effect.expect("enqueue promotes");
    let msg_id = client.execute("").await.expect("execute sends");
    let _ = exec.apply(ExecEvent::Sent {
        cell_id: cell_id.to_owned(),
        msg_id: msg_id.clone(),
    });
    let mut reply_seen = false;
    let mut idle_seen = false;
    while !(reply_seen && idle_seen) {
        let event = events
            .recv_timeout(Duration::from_secs(1))
            .expect("scripted events arrive");
        match &event {
            KernelEvent::Iopub(msg) => {
                let _ = apply_iopub(exec, notebook, msg);
            }
            KernelEvent::Status { parent, state } => {
                if parent.as_ref() == Some(&msg_id) && *state == KernelStatus::Idle {
                    idle_seen = true;
                }
                if let Some(exec_event) = event.exec_event() {
                    let _ = exec.apply(exec_event);
                }
            }
            KernelEvent::ExecuteReply { parent, .. } => {
                if parent == &msg_id {
                    reply_seen = true;
                }
                if let Some(exec_event) = event.exec_event() {
                    let _ = exec.apply(exec_event);
                }
            }
            _ => {
                if let Some(exec_event) = event.exec_event() {
                    let _ = exec.apply(exec_event);
                }
            }
        }
    }
}

/// Parse a notebook with one empty code cell.
fn sample_notebook() -> Notebook {
    let bytes = br#"{"nbformat":4,"nbformat_minor":5,"metadata":{},"cells":[{"cell_type":"code","id":"c1","metadata":{},"source":"","outputs":[],"execution_count":null}]}"#;
    parse(bytes).expect("loads")
}

/// Sample kernel description shared by the tests.
fn info() -> KernelInfo {
    KernelInfo {
        protocol_version: "5.3".to_owned(),
        implementation: "ipython".to_owned(),
        implementation_version: "9.7.0".to_owned(),
        language: LanguageInfo {
            name: "python".to_owned(),
            version: "3.12.0".to_owned(),
            file_extension: Some("py".to_owned()),
        },
        banner: String::new(),
    }
}

/// Outputs of the first code cell as compact JSON.
fn outputs_json(notebook: &Notebook) -> String {
    let saved = serialize(notebook);
    let value: serde_json::Value = serde_json::from_str(&saved).expect("json");
    serde_json::to_string(&value["cells"][0]["outputs"]).expect("json")
}

/// Script for `print("hi")` ending in the given order.
fn print_script(id: &str, reply_first: bool) -> (MsgId, Vec<KernelEvent>) {
    let parent = MsgId::new(id);
    let mut events = vec![
        KernelEvent::Status {
            parent: Some(parent.clone()),
            state: KernelStatus::Busy,
        },
        KernelEvent::Iopub(IopubMsg::ExecuteInput {
            parent: parent.clone(),
            execution_count: 1,
        }),
        KernelEvent::Iopub(IopubMsg::Stream {
            parent: parent.clone(),
            name: "stdout".to_owned(),
            text: "hi\n".to_owned(),
        }),
    ];
    let reply = KernelEvent::ExecuteReply {
        parent: parent.clone(),
        status: ReplyStatus::Ok,
        execution_count: Some(1),
    };
    let idle = KernelEvent::Status {
        parent: Some(parent.clone()),
        state: KernelStatus::Idle,
    };
    if reply_first {
        events.push(reply);
        events.push(idle);
    } else {
        events.push(idle);
        events.push(reply);
    }
    (parent, events)
}

/// Script for `1/0` failing with an error.
fn error_script() -> (MsgId, Vec<KernelEvent>) {
    let parent = MsgId::new("m-err");
    let events = vec![
        KernelEvent::Status {
            parent: Some(parent.clone()),
            state: KernelStatus::Busy,
        },
        KernelEvent::Iopub(IopubMsg::Error {
            parent: parent.clone(),
            ename: "ZeroDivisionError".to_owned(),
            evalue: "division by zero".to_owned(),
            traceback: vec!["Traceback (most recent call last):".to_owned()],
        }),
        KernelEvent::ExecuteReply {
            parent: parent.clone(),
            status: ReplyStatus::Error,
            execution_count: Some(1),
        },
        KernelEvent::Status {
            parent: Some(parent.clone()),
            state: KernelStatus::Idle,
        },
    ];
    (parent, events)
}

#[tokio::test(flavor = "current_thread")]
async fn print_hi() {
    let mut kernel = ScriptedKernel::new(info(), VecDeque::from([print_script("m1", true)]));
    let events = kernel.messages().expect("first takes the channel");
    let mut exec = ExecState::default();
    let mut notebook = sample_notebook();
    let cell_id = notebook.cells[0].id().to_owned();
    run_cell(&kernel, &events, &mut exec, &mut notebook, &cell_id).await;
    assert_eq!(exec.state(&cell_id), CellExecState::Done);
    insta::assert_snapshot!("print_hi", outputs_json(&notebook));
}

#[tokio::test(flavor = "current_thread")]
async fn zero_division() {
    let mut kernel = ScriptedKernel::new(info(), VecDeque::from([error_script()]));
    let events = kernel.messages().expect("first takes the channel");
    let mut exec = ExecState::default();
    let mut notebook = sample_notebook();
    let cell_id = notebook.cells[0].id().to_owned();
    run_cell(&kernel, &events, &mut exec, &mut notebook, &cell_id).await;
    assert_eq!(exec.state(&cell_id), CellExecState::Error);
    insta::assert_snapshot!("zero_division", outputs_json(&notebook));
}

#[tokio::test(flavor = "current_thread")]
async fn reply_before_idle() {
    let mut kernel = ScriptedKernel::new(info(), VecDeque::from([print_script("m1", true)]));
    let events = kernel.messages().expect("first takes the channel");
    let mut exec = ExecState::default();
    let mut notebook = sample_notebook();
    let cell_id = notebook.cells[0].id().to_owned();
    run_cell(&kernel, &events, &mut exec, &mut notebook, &cell_id).await;
    assert_eq!(exec.state(&cell_id), CellExecState::Done);
    let first = outputs_json(&notebook);

    let mut kernel = ScriptedKernel::new(info(), VecDeque::from([print_script("m1", false)]));
    let events = kernel.messages().expect("first takes the channel");
    let mut exec = ExecState::default();
    let mut notebook = sample_notebook();
    let cell_id = notebook.cells[0].id().to_owned();
    run_cell(&kernel, &events, &mut exec, &mut notebook, &cell_id).await;
    assert_eq!(exec.state(&cell_id), CellExecState::Done);
    assert_eq!(outputs_json(&notebook), first);
}

#[tokio::test(flavor = "current_thread")]
async fn idle_before_reply() {
    let mut kernel = ScriptedKernel::new(info(), VecDeque::from([print_script("m1", false)]));
    let events = kernel.messages().expect("first takes the channel");
    let mut exec = ExecState::default();
    let mut notebook = sample_notebook();
    let cell_id = notebook.cells[0].id().to_owned();
    run_cell(&kernel, &events, &mut exec, &mut notebook, &cell_id).await;
    assert_eq!(exec.state(&cell_id), CellExecState::Done);
    let first = outputs_json(&notebook);

    let mut kernel = ScriptedKernel::new(info(), VecDeque::from([print_script("m1", true)]));
    let events = kernel.messages().expect("first takes the channel");
    let mut exec = ExecState::default();
    let mut notebook = sample_notebook();
    let cell_id = notebook.cells[0].id().to_owned();
    run_cell(&kernel, &events, &mut exec, &mut notebook, &cell_id).await;
    assert_eq!(exec.state(&cell_id), CellExecState::Done);
    assert_eq!(outputs_json(&notebook), first);
}

#[tokio::test(flavor = "current_thread")]
async fn messages_once() {
    let mut kernel = ScriptedKernel::new(info(), VecDeque::new());
    assert!(kernel.messages().is_some());
    assert!(kernel.messages().is_none());
}
