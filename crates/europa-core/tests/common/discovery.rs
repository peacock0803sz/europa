//! One failed search, as a plan with probe results and as the expected error.

use std::path::PathBuf;
use std::time::Duration;

use europa_core::{
    AncestorScope, CandidateReport, DiscoveryError, DiscoveryPlan, DiscoveryStep, InstallHint,
    ProbeOutcome, ProcessExit, PythonSource, Rejection, VenvRecipe,
};

/// Standard error of a Python without `ipykernel`.
pub const TAIL: &str = "Traceback (most recent call last):\n  File \"<string>\", line 1, in <module>\nModuleNotFoundError: No module named 'ipykernel'";

pub const NOTEBOOK: &str = "/home/user/proj/notebooks/a.ipynb";
pub const DOT_VENV: &str = "/home/user/proj/.venv/bin/python";
pub const VENV: &str = "/home/user/proj/venv/bin/python";
pub const CONDA: &str = "/home/user/proj/conda/bin/python";
pub const PYTHON3: &str = "/home/user/proj/tools/bin/python3";

pub fn scope() -> AncestorScope {
    AncestorScope {
        notebook_dir: "/home/user/proj/notebooks".into(),
        repo_root: Some("/home/user/proj".into()),
    }
}

pub fn recipe() -> VenvRecipe {
    VenvRecipe {
        python: PYTHON3.into(),
        venv_dir: "/home/user/proj/.venv".into(),
    }
}

pub fn report(source: PythonSource, python: Option<&str>, rejection: Rejection) -> CandidateReport {
    CandidateReport {
        source,
        python: python.map(PathBuf::from),
        rejection,
        install: None,
    }
}

pub fn missing_ipykernel() -> Rejection {
    Rejection::CannotImport {
        exit: ProcessExit::Code(1),
        stderr_tail: TAIL.to_owned(),
    }
}

fn rejected(source: PythonSource, python: Option<&str>, rejection: Rejection) -> DiscoveryStep {
    DiscoveryStep::Rejected {
        report: report(source, python, rejection),
    }
}

fn probe(source: PythonSource, python: &str, install: Option<InstallHint>) -> DiscoveryStep {
    DiscoveryStep::Probe {
        source,
        python: python.into(),
        install,
    }
}

/// A search that touches every source and finds nothing usable.
pub fn all_sources_plan() -> DiscoveryPlan {
    use PythonSource::*;

    DiscoveryPlan::Search {
        notebook: NOTEBOOK.into(),
        scope: scope(),
        steps: vec![
            rejected(Config, None, Rejection::NotSet),
            probe(DotVenv, DOT_VENV, Some(InstallHint::Uv)),
            rejected(Venv, Some(VENV), Rejection::NotFound),
            rejected(VirtualEnv, None, Rejection::NotSet),
            probe(CondaPrefix, CONDA, Some(InstallHint::Pip)),
            probe(PathPython3, PYTHON3, None),
            rejected(PathPython, None, Rejection::NotFound),
        ],
        recipe: recipe(),
    }
}

/// Probe results for [`all_sources_plan`].
pub fn all_sources_outcomes() -> Vec<(&'static str, ProbeOutcome)> {
    let missing = ProbeOutcome::CannotImport {
        exit: ProcessExit::Code(1),
        stderr_tail: TAIL.to_owned(),
    };
    let slow = ProbeOutcome::TimedOut {
        after: Duration::from_secs(10),
    };
    vec![
        (DOT_VENV, missing.clone()),
        (CONDA, slow),
        (PYTHON3, missing),
    ]
}

/// The failure that [`all_sources_plan`] ends in.
pub fn all_sources_error() -> DiscoveryError {
    use PythonSource::*;

    let mut dot_venv = report(DotVenv, Some(DOT_VENV), missing_ipykernel());
    dot_venv.install = Some(InstallHint::Uv);
    DiscoveryError::NoneUsable {
        notebook: NOTEBOOK.into(),
        scope: scope(),
        reports: vec![
            report(Config, None, Rejection::NotSet),
            dot_venv,
            report(Venv, Some(VENV), Rejection::NotFound),
            report(VirtualEnv, None, Rejection::NotSet),
            report(
                CondaPrefix,
                Some(CONDA),
                Rejection::TimedOut {
                    after: Duration::from_secs(10),
                },
            ),
            report(PathPython3, Some(PYTHON3), missing_ipykernel()),
            report(PathPython, None, Rejection::NotFound),
        ],
        recipe: recipe(),
    }
}
