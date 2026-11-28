//! Synchronous state machine that walks a [`DiscoveryPlan`].

use std::collections::{BTreeSet, VecDeque};
use std::path::PathBuf;

use super::{
    AncestorScope, CandidateReport, Discovery, DiscoveryError, DiscoveryPlan, DiscoveryStep,
    InstallHint, ProbeOutcome, PythonSource, VenvRecipe,
};

/// Chooses which candidate to probe next and builds the result.
///
/// Has no IO: [`advance`](Self::advance) hands each probe out as
/// [`FoldNext::Probe`], and the caller answers with [`record`](Self::record).
///
/// | Situation | Call | Effect |
/// |---|---|---|
/// | The next step is `Rejected` | `advance` | Collects its report and moves on |
/// | The next step probes an already probed path | `advance` | Collects `AlreadyChecked` and moves on |
/// | The next step is `Probe` | `advance` | Returns `Probe` and waits for `record` |
/// | The probe was `Usable` | next `advance` | `Done(Ok)`; the collected reports become `skipped` |
/// | The probe ended in `CannotImport` or `TimedOut` | `record` | Remembers the path and collects the report |
/// | The probe was `NotExecutable` | `record` | Collects the report but not the path, as nothing ran |
/// | No steps remain | `advance` | `Done(Err)`: `NoneUsable` for a search, `ConfigUnusable` for the setting |
/// | The plan is `ConfigNotAbsolute` | first `advance` | `Done(Err(ConfigNotAbsolute))` |
///
/// # Panics
///
/// These are mistakes of the caller:
///
/// - `advance` after it returned `Done`: `advance called after Done`.
/// - `advance` while a probe waits for `record`:
///   `advance called before record`.
/// - `record` while no probe waits: `record called without a pending probe`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DiscoveryFold {
    /// What the plan asks for.
    kind: Kind,
    /// Steps not yet taken.
    steps: VecDeque<DiscoveryStep>,
    /// Reports of the candidates passed over so far.
    reports: Vec<CandidateReport>,
    /// Paths whose probe process actually ran.
    probed: BTreeSet<PathBuf>,
    /// Which call comes next.
    phase: Phase,
}

/// Kind of the plan, with what only a failed search needs.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
enum Kind {
    /// The setting is relative.
    ConfigNotAbsolute { path: PathBuf },
    /// The setting is the only candidate.
    Config,
    /// Every source is searched.
    Search {
        notebook: PathBuf,
        scope: AncestorScope,
        recipe: VenvRecipe,
    },
}

/// Progress between two calls.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
enum Phase {
    /// `advance` takes the next step.
    Walking,
    /// `record` is due for this candidate.
    Waiting(Candidate),
    /// This candidate passed and `advance` has yet to report it.
    Passed(Candidate),
    /// `Done` was returned.
    Finished,
}

/// A candidate handed out for a probe.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct Candidate {
    source: PythonSource,
    python: PathBuf,
    install: Option<InstallHint>,
}

/// What the caller of [`DiscoveryFold::advance`] does next.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FoldNext {
    /// Probe this interpreter and answer with [`DiscoveryFold::record`].
    Probe(PathBuf),
    /// The search ended.
    Done(Result<Discovery, DiscoveryError>),
}

impl DiscoveryFold {
    /// Start before the first candidate of `plan`.
    #[must_use]
    pub fn new(plan: DiscoveryPlan) -> Self {
        let _ = plan;
        todo!()
    }

    /// Take steps until a probe is needed or the search ends.
    pub fn advance(&mut self) -> FoldNext {
        todo!()
    }

    /// Give the result of the probe that `advance` asked for.
    pub fn record(&mut self, outcome: ProbeOutcome) {
        let _ = outcome;
        todo!()
    }
}
