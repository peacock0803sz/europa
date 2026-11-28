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
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::kernel::exit_text;
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
/// The variants are declared in the order the search tries them, except that
/// `DotVenv` and `Venv` take turns for each directory level, nearest first.
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
/// Only `CannotImport`, `TimedOut`, and a `NotExecutable` that reports why
/// starting failed follow an attempted probe. The others are decided without
/// one.
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
        match self {
            Self::ConfigNotAbsolute { path } => write!(
                f,
                "kernel.python must be an absolute path, but it is set to {}.\n\
                 Set it to the absolute path of a Python that has ipykernel.",
                path.display()
            ),
            Self::ConfigUnusable { report } => write_config_unusable(f, report),
            Self::NoneUsable {
                notebook,
                scope,
                reports,
                recipe,
            } => {
                writeln!(
                    f,
                    "No Python with ipykernel was found for {}.\n",
                    notebook.display()
                )?;
                write_candidates(f, scope, reports)?;
                write_next_steps(f, reports, recipe)
            }
        }
    }
}

impl std::error::Error for DiscoveryError {}

/// Message for a `kernel.python` interpreter that cannot be used.
fn write_config_unusable(f: &mut fmt::Formatter<'_>, report: &CandidateReport) -> fmt::Result {
    let python = report.python.as_deref().unwrap_or(Path::new(""));
    writeln!(
        f,
        "kernel.python is set to {}, but it cannot be used: {}",
        python.display(),
        reason(report)
    )?;
    for line in stderr_lines(&report.rejection) {
        writeln!(f, "  {line}")?;
    }
    writeln!(f, "\nNext steps:")?;
    if let Some(command) = install_command(report) {
        writeln!(f, "  - Install ipykernel: {command}")?;
        write!(f, "  - Or change")?;
    } else {
        write!(f, "  - Change")?;
    }
    write!(
        f,
        " kernel.python to the absolute path of a Python that has ipykernel.\n\n\
         Europa checked only kernel.python because it is set, and does not install packages or create environments."
    )
}

/// Every inspected candidate, in the order it was inspected.
fn write_candidates(
    f: &mut fmt::Formatter<'_>,
    scope: &AncestorScope,
    reports: &[CandidateReport],
) -> fmt::Result {
    let has_venv = reports
        .iter()
        .any(|report| matches!(report.source, PythonSource::DotVenv | PythonSource::Venv));
    writeln!(f, "Checked in this order:")?;
    for report in reports {
        match &report.python {
            Some(python) => {
                writeln!(f, "  {:<19}{}", report.source, python.display())?;
                writeln!(f, "{:21}{}", "", reason(report))?;
                for line in stderr_lines(&report.rejection) {
                    writeln!(f, "{:23}{line}", "")?;
                }
                if let Some(command) = install_command(report) {
                    writeln!(f, "{:21}install it: {command}", "")?;
                }
            }
            None => writeln!(f, "  {:<19}{}", report.source, reason(report))?,
        }
        // One line stands for the levels that had no environment to report.
        if !has_venv && report.source == PythonSource::Config {
            write!(
                f,
                "  {:<19}none found from {}",
                ".venv, venv",
                scope.notebook_dir.display()
            )?;
            match &scope.repo_root {
                Some(root) => writeln!(f, " up to {}", root.display())?,
                None => writeln!(f, " (outside a Git repository)")?,
            }
        }
    }
    Ok(())
}

/// What the user can do when no candidate works.
fn write_next_steps(
    f: &mut fmt::Formatter<'_>,
    reports: &[CandidateReport],
    recipe: &VenvRecipe,
) -> fmt::Result {
    writeln!(f, "\nNext steps:")?;
    if reports.iter().any(|report| report.install.is_some()) {
        writeln!(f, "  - Install ipykernel with one of the commands above.")?;
    } else {
        writeln!(
            f,
            "  - Create a virtual environment where Europa looks for one, then install ipykernel into it:"
        )?;
        writeln!(
            f,
            "      {} -m venv {}",
            recipe.python.display(),
            recipe.venv_dir.display()
        )?;
        writeln!(
            f,
            "      {} -m pip install ipykernel",
            recipe.venv_dir.join("bin/python").display()
        )?;
    }
    write!(
        f,
        "  - Or set kernel.python in the Europa config to the absolute path of a Python that has ipykernel.\n\n\
         Europa does not install packages or create environments."
    )
}

/// The line saying why a candidate cannot be used.
fn reason(report: &CandidateReport) -> String {
    match &report.rejection {
        Rejection::NotSet => "not set".to_owned(),
        Rejection::NotFound => {
            let on_path = matches!(
                report.source,
                PythonSource::PathPython3 | PythonSource::PathPython
            );
            if on_path && report.python.is_none() {
                "not found on PATH".to_owned()
            } else {
                "not found".to_owned()
            }
        }
        Rejection::NotAbsolute => "not an absolute path".to_owned(),
        Rejection::NotOwned => "skipped: owned by another user or writable by others".to_owned(),
        Rejection::NotExecutable { detail } => format!("cannot be executed: {detail}"),
        Rejection::CannotImport { exit, .. } => {
            format!("ipykernel cannot be imported ({})", exit_text(*exit))
        }
        Rejection::TimedOut { after } if after.as_secs() > 0 => {
            format!("timed out after {}s", after.as_secs())
        }
        Rejection::TimedOut { after } => format!("timed out after {}ms", after.as_millis()),
        Rejection::AlreadyChecked => "already checked above".to_owned(),
    }
}

/// Lines of standard error to show under the reason.
fn stderr_lines(rejection: &Rejection) -> std::str::Lines<'_> {
    match rejection {
        Rejection::CannotImport { stderr_tail, .. } => stderr_tail.lines(),
        _ => "".lines(),
    }
}

/// The command that installs `ipykernel` into the candidate, if one is
/// suggested.
fn install_command(report: &CandidateReport) -> Option<String> {
    let python = report.python.as_ref()?.display();
    Some(match report.install.as_ref()? {
        InstallHint::Uv => format!("uv pip install --python {python} ipykernel"),
        InstallHint::Pip => format!("{python} -m pip install ipykernel"),
    })
}

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
        /// Candidates in the order [`PythonSource`] describes, starting with
        /// `Config` rejected as `NotSet`.
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
