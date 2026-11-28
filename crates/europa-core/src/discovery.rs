//! Python discovery inputs, results, and errors.
//!
//! The kernel adapter searches for an interpreter that can import
//! `ipykernel`. This module owns what crosses the crate boundary: the request,
//! the ordered candidates, the probe results, and the failure shown to the
//! user. [`DiscoveryFold`] chooses which candidate to probe next without doing
//! any IO.

mod fold;

use std::ffi::OsString;
use std::fmt;
use std::path::PathBuf;
use std::time::Duration;

use crate::ProcessExit;

pub use fold::{DiscoveryFold, FoldNext};

/// Input of one search.
///
/// The search reads this value and never changes it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DiscoveryRequest {
    /// Path of the opened notebook; the file need not exist yet.
    ///
    /// A relative path is made absolute once, at the start of the search.
    pub notebook: PathBuf,
    /// Value of the `kernel.python` setting, `None` when it is not set.
    pub config_python: Option<PathBuf>,
    /// Environment variables the search uses.
    pub env: DiscoveryEnv,
    /// Longest wait for the probe of one candidate.
    pub probe_timeout: Duration,
}

impl DiscoveryRequest {
    /// Build a request whose `probe_timeout` is 10 seconds.
    #[must_use]
    pub fn new(
        notebook: impl Into<PathBuf>,
        config_python: Option<PathBuf>,
        env: DiscoveryEnv,
    ) -> Self {
        Self {
            notebook: notebook.into(),
            config_python,
            env,
            probe_timeout: Duration::from_secs(10),
        }
    }
}

/// Environment variables the search uses.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DiscoveryEnv {
    /// Value of `VIRTUAL_ENV`.
    pub virtual_env: Option<PathBuf>,
    /// Value of `CONDA_PREFIX`.
    pub conda_prefix: Option<PathBuf>,
    /// Value of `PATH`, which the search splits.
    pub path: Option<OsString>,
}

impl DiscoveryEnv {
    /// Take `VIRTUAL_ENV`, `CONDA_PREFIX`, and `PATH` from variable pairs.
    ///
    /// Other names are ignored. An empty value becomes `None`, and a relative
    /// path is kept as given for the search to judge. A caller that wants the
    /// process environment passes `std::env::vars_os()`.
    #[must_use]
    pub fn from_vars<I>(vars: I) -> Self
    where
        I: IntoIterator<Item = (OsString, OsString)>,
    {
        let mut env = Self::default();
        for (name, value) in vars {
            if value.is_empty() {
                continue;
            }
            if name == "VIRTUAL_ENV" {
                env.virtual_env = Some(value.into());
            } else if name == "CONDA_PREFIX" {
                env.conda_prefix = Some(value.into());
            } else if name == "PATH" {
                env.path = Some(value);
            }
        }
        env
    }
}

/// Where a candidate interpreter came from.
///
/// The variants are declared in the order the search tries them.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PythonSource {
    /// The `kernel.python` setting.
    Config,
    /// `.venv` in an ancestor directory of the notebook.
    DotVenv,
    /// `venv` in an ancestor directory of the notebook.
    Venv,
    /// The environment named by `VIRTUAL_ENV`.
    VirtualEnv,
    /// The environment named by `CONDA_PREFIX`.
    CondaPrefix,
    /// The first `python3` on `PATH`.
    PathPython3,
    /// The first `python` on `PATH`.
    PathPython,
}

impl fmt::Display for PythonSource {
    /// Write the heading that error messages use for this source.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // `pad` lets error messages align the headings with a width.
        f.pad(match self {
            Self::Config => "kernel.python",
            Self::DotVenv => ".venv",
            Self::Venv => "venv",
            Self::VirtualEnv => "$VIRTUAL_ENV",
            Self::CondaPrefix => "$CONDA_PREFIX",
            Self::PathPython3 => "python3 on PATH",
            Self::PathPython => "python on PATH",
        })
    }
}

/// An inspected candidate and why it cannot be used.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CandidateReport {
    /// Where the candidate came from.
    pub source: PythonSource,
    /// Interpreter that was inspected.
    ///
    /// `None` when the setting or variable is not set, or when `PATH` has no
    /// such command. A value that is not absolute is kept as given.
    pub python: Option<PathBuf>,
    /// Why the candidate cannot be used.
    pub rejection: Rejection,
    /// How to install `ipykernel` into it.
    ///
    /// `Some` only when `rejection` is `CannotImport` and the candidate did not
    /// come from `PATH`.
    pub install: Option<InstallHint>,
}

/// Why a candidate cannot be used.
///
/// A probe process runs only before `CannotImport`, `TimedOut`, and a
/// `NotExecutable` that reports a failed start. Everything else is decided
/// without starting one.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "reason", rename_all = "snake_case")]
pub enum Rejection {
    /// The setting or variable is not set.
    NotSet,
    /// There is no interpreter file.
    NotFound,
    /// The setting or variable holds a relative path.
    NotAbsolute,
    /// An ancestor virtual environment belongs to, or is writable by, someone
    /// else.
    NotOwned,
    /// The interpreter cannot be run.
    NotExecutable {
        /// `not executable` or `broken symbolic link` when seen before the
        /// probe, otherwise why starting failed, or `Europa is stopping`.
        detail: String,
    },
    /// The probe ran and could not import `ipykernel`.
    CannotImport {
        /// How the probe ended.
        exit: ProcessExit,
        /// Last 5 lines of standard error joined by `\n`, without a trailing
        /// newline.
        stderr_tail: String,
    },
    /// The probe did not finish in time.
    TimedOut {
        /// How long the probe was given.
        after: Duration,
    },
    /// The same path was already probed earlier in this search.
    AlreadyChecked,
}

/// Which command to suggest for installing `ipykernel`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallHint {
    /// `uv pip install --python <python> ipykernel`, for environments that uv
    /// created.
    Uv,
    /// `<python> -m pip install ipykernel`.
    Pip,
}

/// Ancestor directories that were searched for virtual environments.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AncestorScope {
    /// Resolved directory of the notebook.
    pub notebook_dir: PathBuf,
    /// Nearest directory at or above `notebook_dir` that holds `.git`, `None`
    /// outside a repository.
    pub repo_root: Option<PathBuf>,
}

/// What an error message needs to suggest a new virtual environment.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct VenvRecipe {
    /// Interpreter that creates it: `python3` or else `python` from `PATH`, or
    /// the bare name `python3` when `PATH` has neither.
    pub python: PathBuf,
    /// Absolute path of the `.venv` to create, in the repository root or else
    /// in the notebook directory.
    pub venv_dir: PathBuf,
}

/// A successful search.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Discovery {
    /// Absolute path of the chosen interpreter.
    pub python: PathBuf,
    /// Where the interpreter came from.
    pub source: PythonSource,
    /// Candidates inspected before the chosen one, in order.
    ///
    /// Without a `kernel.python` setting, the first entry reports `Config` as
    /// `NotSet`.
    pub skipped: Vec<CandidateReport>,
}

/// Why a search failed.
///
/// [`Display`](fmt::Display) gives the message shown to the user.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DiscoveryError {
    /// `kernel.python` is a relative path, so nothing was probed.
    ConfigNotAbsolute {
        /// The configured value.
        path: PathBuf,
    },
    /// The `kernel.python` interpreter cannot be used, and no other candidate
    /// was inspected.
    ConfigUnusable {
        /// Report of the configured interpreter.
        report: CandidateReport,
    },
    /// `kernel.python` is not set and no candidate can be used.
    NoneUsable {
        /// Notebook the search was for.
        notebook: PathBuf,
        /// Ancestor directories that were searched.
        scope: AncestorScope,
        /// Every inspected candidate, in order.
        ///
        /// Starts with `Config` as `NotSet` and holds exactly one entry each
        /// for `VirtualEnv`, `CondaPrefix`, `PathPython3`, and `PathPython`.
        /// `DotVenv` and `Venv` appear once for each directory level that has
        /// them, and not at all when no level does.
        reports: Vec<CandidateReport>,
        /// Material for suggesting a new virtual environment.
        recipe: VenvRecipe,
    },
}

impl fmt::Display for DiscoveryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let _ = f;
        todo!()
    }
}

impl std::error::Error for DiscoveryError {}

/// One entry of the ordered candidate list.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "step", rename_all = "snake_case")]
pub enum DiscoveryStep {
    /// A candidate rejected without a probe.
    Rejected {
        /// Why it was rejected.
        report: CandidateReport,
    },
    /// A candidate that waits for its probe.
    Probe {
        /// Where it came from.
        source: PythonSource,
        /// Interpreter to probe.
        python: PathBuf,
        /// Moves into the report if the probe ends in `CannotImport`; `None`
        /// for `PATH` candidates.
        install: Option<InstallHint>,
    },
}

/// The candidates of one search, decided before anything is probed.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "plan", rename_all = "snake_case")]
pub enum DiscoveryPlan {
    /// `kernel.python` is relative, which fails the search at once.
    ConfigNotAbsolute {
        /// The configured value.
        path: PathBuf,
    },
    /// `kernel.python` is set, so it is the only candidate.
    Config {
        /// The configured candidate.
        step: DiscoveryStep,
    },
    /// `kernel.python` is not set, so every source is searched.
    Search {
        /// Notebook the search is for.
        notebook: PathBuf,
        /// Ancestor directories that were searched.
        scope: AncestorScope,
        /// Candidates in the order of [`PythonSource`], starting with `Config`
        /// rejected as `NotSet`.
        steps: Vec<DiscoveryStep>,
        /// Material for suggesting a new virtual environment.
        recipe: VenvRecipe,
    },
}

/// Result of probing one interpreter.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum ProbeOutcome {
    /// `ipykernel` imports.
    Usable,
    /// No probe process ran.
    NotExecutable {
        /// Why it did not run.
        detail: String,
    },
    /// The probe ended without importing `ipykernel`.
    CannotImport {
        /// How the probe ended.
        exit: ProcessExit,
        /// Last 5 lines of standard error joined by `\n`, without a trailing
        /// newline.
        stderr_tail: String,
    },
    /// The probe did not finish in time.
    TimedOut {
        /// How long the probe was given.
        after: Duration,
    },
}
