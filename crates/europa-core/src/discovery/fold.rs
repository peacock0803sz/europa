//! Synchronous state machine that walks a [`DiscoveryPlan`].

use std::collections::{BTreeSet, VecDeque};
use std::path::PathBuf;

use super::{
    AncestorScope, CandidateReport, Discovery, DiscoveryError, DiscoveryPlan, DiscoveryStep,
    InstallHint, ProbeOutcome, PythonSource, Rejection, VenvRecipe,
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
        let (kind, steps) = match plan {
            DiscoveryPlan::Search {
                notebook,
                scope,
                steps,
                recipe,
            } => (
                Kind::Search {
                    notebook,
                    scope,
                    recipe,
                },
                steps,
            ),
            DiscoveryPlan::Config { step } => (Kind::Config, vec![step]),
            DiscoveryPlan::ConfigNotAbsolute { path } => {
                (Kind::ConfigNotAbsolute { path }, Vec::new())
            }
        };
        Self {
            kind,
            steps: steps.into(),
            reports: Vec::new(),
            probed: BTreeSet::new(),
            phase: Phase::Walking,
        }
    }

    /// Take steps until a probe is needed or the search ends.
    pub fn advance(&mut self) -> FoldNext {
        match std::mem::replace(&mut self.phase, Phase::Finished) {
            Phase::Walking => {}
            Phase::Passed(candidate) => {
                return FoldNext::Done(Ok(Discovery {
                    python: candidate.python,
                    source: candidate.source,
                    skipped: std::mem::take(&mut self.reports),
                }));
            }
            Phase::Waiting(_) => panic!("advance called before record"),
            Phase::Finished => panic!("advance called after Done"),
        }
        while let Some(step) = self.steps.pop_front() {
            match step {
                DiscoveryStep::Rejected { report } => self.reports.push(report),
                DiscoveryStep::Probe { source, python, .. } if self.probed.contains(&python) => {
                    self.reports.push(CandidateReport {
                        source,
                        python: Some(python),
                        rejection: Rejection::AlreadyChecked,
                        install: None,
                    });
                }
                DiscoveryStep::Probe {
                    source,
                    python,
                    install,
                } => {
                    self.phase = Phase::Waiting(Candidate {
                        source,
                        python: python.clone(),
                        install,
                    });
                    return FoldNext::Probe(python);
                }
            }
        }
        FoldNext::Done(Err(self.failure()))
    }

    /// Give the result of the probe that `advance` asked for.
    pub fn record(&mut self, outcome: ProbeOutcome) {
        let Phase::Waiting(candidate) = std::mem::replace(&mut self.phase, Phase::Walking) else {
            panic!("record called without a pending probe");
        };
        let (rejection, install, ran) = match outcome {
            ProbeOutcome::Usable => {
                self.phase = Phase::Passed(candidate);
                return;
            }
            ProbeOutcome::NotExecutable { detail } => {
                (Rejection::NotExecutable { detail }, None, false)
            }
            ProbeOutcome::CannotImport { exit, stderr_tail } => (
                Rejection::CannotImport { exit, stderr_tail },
                candidate.install,
                true,
            ),
            ProbeOutcome::TimedOut { after } => (Rejection::TimedOut { after }, None, true),
        };
        // Without a process, a later candidate of the same path still deserves
        // its probe.
        if ran {
            self.probed.insert(candidate.python.clone());
        }
        self.reports.push(CandidateReport {
            source: candidate.source,
            python: Some(candidate.python),
            rejection,
            install,
        });
    }

    /// Error of a search that ran out of candidates.
    fn failure(&mut self) -> DiscoveryError {
        let mut reports = std::mem::take(&mut self.reports);
        match &self.kind {
            Kind::Search {
                notebook,
                scope,
                recipe,
            } => DiscoveryError::NoneUsable {
                notebook: notebook.clone(),
                scope: scope.clone(),
                reports,
                recipe: recipe.clone(),
            },
            Kind::Config => DiscoveryError::ConfigUnusable {
                report: reports.pop().expect("the setting was reported"),
            },
            Kind::ConfigNotAbsolute { path } => {
                DiscoveryError::ConfigNotAbsolute { path: path.clone() }
            }
        }
    }
}
