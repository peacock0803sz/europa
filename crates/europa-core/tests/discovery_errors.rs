//! Snapshot tests for the messages of a failed Python search.

use std::time::Duration;

use europa_core::{
    CandidateReport, DiscoveryError, InstallHint, PythonSource, Rejection, VenvRecipe,
};

mod common;

use common::discovery::{
    all_sources_error, missing_ipykernel, recipe, report, scope, CONDA, DOT_VENV, NOTEBOOK,
    PYTHON3, VENV,
};

const LAST_LINE: &str = "Europa does not install packages or create environments.";
const SET_CONFIG: &str = "  - Or set kernel.python in the Europa config to the absolute path of a Python that has ipykernel.";

/// A failure in a repository whose reports are `reports`.
fn none_usable(reports: Vec<CandidateReport>) -> DiscoveryError {
    DiscoveryError::NoneUsable {
        notebook: NOTEBOOK.into(),
        scope: scope(),
        reports,
        recipe: recipe(),
    }
}

/// Reports of the setting, the variables, and `PATH` with `python3` found.
fn without_ancestors() -> Vec<CandidateReport> {
    use PythonSource::*;

    vec![
        report(Config, None, Rejection::NotSet),
        report(VirtualEnv, None, Rejection::NotSet),
        report(CondaPrefix, None, Rejection::NotSet),
        report(PathPython3, Some(PYTHON3), missing_ipykernel()),
        report(PathPython, None, Rejection::NotFound),
    ]
}

fn pip_venv() -> DiscoveryError {
    let mut venv = report(PythonSource::Venv, Some(VENV), missing_ipykernel());
    venv.install = Some(InstallHint::Pip);
    let mut reports = without_ancestors();
    reports.insert(1, venv);
    none_usable(reports)
}

fn mixed_reasons() -> DiscoveryError {
    use PythonSource::*;

    let waited = Rejection::TimedOut {
        after: Duration::from_millis(500),
    };
    let failed_start = Rejection::NotExecutable {
        detail: "Exec format error (os error 8)".to_owned(),
    };
    DiscoveryError::NoneUsable {
        notebook: "/home/user/proj/a.ipynb".into(),
        scope: europa_core::AncestorScope {
            notebook_dir: "/home/user/proj".into(),
            repo_root: None,
        },
        reports: vec![
            report(Config, None, Rejection::NotSet),
            report(DotVenv, Some(DOT_VENV), Rejection::NotOwned),
            report(Venv, Some(VENV), waited),
            report(VirtualEnv, Some("rel/v"), Rejection::NotAbsolute),
            report(CondaPrefix, Some(VENV), Rejection::AlreadyChecked),
            report(PathPython3, Some(PYTHON3), failed_start),
            report(PathPython, None, Rejection::NotFound),
        ],
        recipe: VenvRecipe {
            python: PYTHON3.into(),
            venv_dir: "/home/user/proj/.venv".into(),
        },
    }
}

fn config_unusable(rejection: Rejection, install: Option<InstallHint>) -> DiscoveryError {
    let mut report = report(PythonSource::Config, Some(CONDA), rejection);
    report.install = install;
    DiscoveryError::ConfigUnusable { report }
}

/// Lines from the one starting with `heading` up to the next candidate.
fn section(text: &str, heading: &str) -> String {
    let mut lines = text.lines().skip_while(|line| !line.starts_with(heading));
    let first = lines.next().expect("heading is shown");
    std::iter::once(first)
        .chain(lines.take_while(|line| line.starts_with("   ")))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn snapshots_every_source() {
    let text = all_sources_error().to_string();
    insta::assert_snapshot!("none_usable_all_sources", text);
    for heading in [
        "kernel.python",
        ".venv",
        "venv",
        "$VIRTUAL_ENV",
        "$CONDA_PREFIX",
        "python3 on PATH",
        "python on PATH",
    ] {
        assert!(text.contains(&format!("\n  {heading} ")), "{heading}");
    }
    assert!(text.contains(
        "install it: uv pip install --python /home/user/proj/.venv/bin/python ipykernel"
    ));
    assert!(text.contains("Next steps:"));
    assert!(text.contains(SET_CONFIG));
    assert!(!section(&text, "  python3 on PATH").contains("install it:"));
    assert_eq!(text.lines().last(), Some(LAST_LINE));
}

#[test]
fn snapshots_a_pip_environment() {
    let text = pip_venv().to_string();
    insta::assert_snapshot!("none_usable_pip_venv", text);
    assert!(text.contains("install it: /home/user/proj/venv/bin/python -m pip install ipykernel"));
    assert_eq!(text.lines().last(), Some(LAST_LINE));
}

#[test]
fn snapshots_no_environment() {
    let text = none_usable(without_ancestors()).to_string();
    insta::assert_snapshot!("none_usable_no_venv", text);
    assert_eq!(
        text.lines()
            .filter(|line| line.starts_with("  .venv, venv "))
            .count(),
        1
    );
    assert!(text.contains("-m venv /home/user/proj/.venv"));
    assert!(text.contains("/home/user/proj/.venv/bin/python -m pip install ipykernel"));
    assert!(!text.contains("install it:"));
    assert_eq!(text.lines().last(), Some(LAST_LINE));
}

#[test]
fn snapshots_mixed_reasons() {
    let text = mixed_reasons().to_string();
    insta::assert_snapshot!("none_usable_mixed_reasons", text);
    assert!(text.contains("timed out after 500ms"));
    assert_eq!(text.lines().last(), Some(LAST_LINE));
}

#[test]
fn says_when_the_notebook_is_outside_a_repository() {
    let DiscoveryError::NoneUsable {
        notebook,
        mut scope,
        reports,
        recipe,
    } = none_usable(without_ancestors())
    else {
        panic!("not a failed search");
    };
    scope.repo_root = None;
    let text = DiscoveryError::NoneUsable {
        notebook,
        scope,
        reports,
        recipe,
    }
    .to_string();
    assert!(text.contains("none found from /home/user/proj/notebooks (outside a Git repository)"));
}

#[test]
fn snapshots_an_unusable_setting() {
    let text = config_unusable(missing_ipykernel(), Some(InstallHint::Pip)).to_string();
    insta::assert_snapshot!("config_unusable", text);
    assert!(text.starts_with("kernel.python is set to "));
    assert!(!text.contains(".venv"));
    assert!(!text.contains("Checked in this order:"));
}

#[test]
fn offers_only_a_change_when_nothing_can_be_installed() {
    let text = config_unusable(Rejection::NotFound, None).to_string();
    assert!(!text.contains("Install ipykernel"));
    assert!(text
        .lines()
        .any(|line| line.trim_start().starts_with("- Change kernel.python")));
}

#[test]
fn snapshots_a_relative_setting() {
    let error = DiscoveryError::ConfigNotAbsolute {
        path: "py/bin/python".into(),
    };
    insta::assert_snapshot!("config_not_absolute", error.to_string());
}
