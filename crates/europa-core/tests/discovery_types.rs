//! Round-trip, default, and heading tests for Python discovery types.

use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Duration;

use europa_core::{
    AncestorScope, CandidateReport, Discovery, DiscoveryEnv, DiscoveryError, DiscoveryPlan,
    DiscoveryRequest, DiscoveryStep, InstallHint, ProbeOutcome, ProcessExit, PythonSource,
    Rejection, VenvRecipe,
};

fn round_trip<T>(value: &T)
where
    T: serde::Serialize + serde::de::DeserializeOwned + PartialEq + std::fmt::Debug,
{
    let text = serde_json::to_string(value).expect("serializes");
    assert_eq!(
        &serde_json::from_str::<T>(&text).expect("loads"),
        value,
        "{text}"
    );
}

fn report() -> CandidateReport {
    CandidateReport {
        source: PythonSource::DotVenv,
        python: Some("/p/.venv/bin/python".into()),
        rejection: Rejection::CannotImport {
            exit: ProcessExit::Code(1),
            stderr_tail: "ModuleNotFoundError".to_owned(),
        },
        install: Some(InstallHint::Uv),
    }
}

fn scope() -> AncestorScope {
    AncestorScope {
        notebook_dir: "/p/nb".into(),
        repo_root: Some("/p".into()),
    }
}

fn recipe() -> VenvRecipe {
    VenvRecipe {
        python: "/usr/bin/python3".into(),
        venv_dir: "/p/.venv".into(),
    }
}

fn vars(pairs: &[(&str, &str)]) -> Vec<(OsString, OsString)> {
    pairs
        .iter()
        .map(|(name, value)| (OsString::from(name), OsString::from(value)))
        .collect()
}

#[test]
fn round_trips_rejections() {
    for rejection in [
        Rejection::NotSet,
        Rejection::NotFound,
        Rejection::NotAbsolute,
        Rejection::NotOwned,
        Rejection::NotExecutable {
            detail: "not executable".to_owned(),
        },
        Rejection::CannotImport {
            exit: ProcessExit::Signal(9),
            stderr_tail: "a\nb".to_owned(),
        },
        Rejection::TimedOut {
            after: Duration::from_millis(500),
        },
        Rejection::AlreadyChecked,
    ] {
        round_trip(&rejection);
    }
}

#[test]
fn round_trips_errors_and_success() {
    round_trip(&DiscoveryError::ConfigNotAbsolute {
        path: "py/bin/python".into(),
    });
    round_trip(&DiscoveryError::ConfigUnusable { report: report() });
    round_trip(&DiscoveryError::NoneUsable {
        notebook: "/p/nb/a.ipynb".into(),
        scope: scope(),
        reports: vec![report()],
        recipe: recipe(),
    });
    round_trip(&Discovery {
        python: "/usr/bin/python3".into(),
        source: PythonSource::PathPython3,
        skipped: vec![report()],
    });
}

#[test]
fn round_trips_requests_plans_and_outcomes() {
    let env = DiscoveryEnv {
        virtual_env: Some("/v".into()),
        conda_prefix: None,
        path: Some("/usr/bin".into()),
    };
    round_trip(&DiscoveryRequest::new(
        "/p/a.ipynb",
        Some("/py/bin/python".into()),
        env,
    ));
    let rejected = DiscoveryStep::Rejected { report: report() };
    let probe = DiscoveryStep::Probe {
        source: PythonSource::Venv,
        python: "/p/venv/bin/python".into(),
        install: Some(InstallHint::Pip),
    };
    round_trip(&rejected);
    round_trip(&probe);
    round_trip(&DiscoveryPlan::ConfigNotAbsolute {
        path: "py/bin/python".into(),
    });
    round_trip(&DiscoveryPlan::Config {
        step: probe.clone(),
    });
    round_trip(&DiscoveryPlan::Search {
        notebook: "/p/nb/a.ipynb".into(),
        scope: scope(),
        steps: vec![rejected, probe],
        recipe: recipe(),
    });
    for outcome in [
        ProbeOutcome::Usable,
        ProbeOutcome::NotExecutable {
            detail: "Text file busy".to_owned(),
        },
        ProbeOutcome::CannotImport {
            exit: ProcessExit::Unknown,
            stderr_tail: String::new(),
        },
        ProbeOutcome::TimedOut {
            after: Duration::from_secs(10),
        },
    ] {
        round_trip(&outcome);
    }
}

#[test]
fn waits_ten_seconds_by_default() {
    let request = DiscoveryRequest::new("/p/a.ipynb", None, DiscoveryEnv::default());
    assert_eq!(request.probe_timeout, Duration::from_secs(10));
}

#[test]
fn takes_only_the_three_variables() {
    let env = DiscoveryEnv::from_vars(vars(&[
        ("VIRTUAL_ENV", "/v"),
        ("CONDA_PREFIX", ""),
        ("PATH", "/usr/bin:rel"),
        ("HOME", "/h"),
    ]));
    assert_eq!(
        env,
        DiscoveryEnv {
            virtual_env: Some(PathBuf::from("/v")),
            conda_prefix: None,
            path: Some(OsString::from("/usr/bin:rel")),
        }
    );
}

#[test]
fn keeps_relative_values() {
    let env = DiscoveryEnv::from_vars(vars(&[("VIRTUAL_ENV", "rel/v")]));
    assert_eq!(env.virtual_env, Some(PathBuf::from("rel/v")));
}

#[test]
fn names_every_source() {
    for (source, heading) in [
        (PythonSource::Config, "kernel.python"),
        (PythonSource::DotVenv, ".venv"),
        (PythonSource::Venv, "venv"),
        (PythonSource::VirtualEnv, "$VIRTUAL_ENV"),
        (PythonSource::CondaPrefix, "$CONDA_PREFIX"),
        (PythonSource::PathPython3, "python3 on PATH"),
        (PythonSource::PathPython, "python on PATH"),
    ] {
        assert_eq!(source.to_string(), heading);
    }
}
