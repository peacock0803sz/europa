//! Tests of the order and choice that `DiscoveryFold` makes, without Python.

use std::path::{Path, PathBuf};
use std::time::Duration;

use europa_core::{
    AncestorScope, CandidateReport, Discovery, DiscoveryError, DiscoveryFold, DiscoveryPlan,
    DiscoveryStep, FoldNext, InstallHint, ProbeOutcome, ProcessExit, PythonSource, Rejection,
    VenvRecipe,
};

mod common;

use common::discovery::{all_sources_error, all_sources_outcomes, all_sources_plan};

/// One probed candidate for each source that is searched without a setting.
const SEARCHED: [(PythonSource, &str); 6] = [
    (PythonSource::DotVenv, "/p/.venv/bin/python"),
    (PythonSource::Venv, "/p/venv/bin/python"),
    (PythonSource::VirtualEnv, "/v/bin/python"),
    (PythonSource::CondaPrefix, "/c/bin/python"),
    (PythonSource::PathPython3, "/usr/bin/python3"),
    (PythonSource::PathPython, "/usr/bin/python"),
];

/// Drive a fold with a table that answers each probe, and list the probes.
fn run(
    plan: DiscoveryPlan,
    table: &[(&str, ProbeOutcome)],
) -> (Result<Discovery, DiscoveryError>, Vec<PathBuf>) {
    let mut fold = DiscoveryFold::new(plan);
    let mut probed = Vec::new();
    loop {
        match fold.advance() {
            FoldNext::Probe(path) => {
                let (_, outcome) = table
                    .iter()
                    .find(|(known, _)| Path::new(known) == path)
                    .unwrap_or_else(|| panic!("unexpected probe of {path:?}"));
                probed.push(path);
                fold.record(outcome.clone());
            }
            FoldNext::Done(result) => return (result, probed),
        }
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

fn report(source: PythonSource, python: Option<&str>, rejection: Rejection) -> CandidateReport {
    CandidateReport {
        source,
        python: python.map(PathBuf::from),
        rejection,
        install: None,
    }
}

fn not_set() -> CandidateReport {
    report(PythonSource::Config, None, Rejection::NotSet)
}

fn rejected(source: PythonSource, python: &str, rejection: Rejection) -> DiscoveryStep {
    DiscoveryStep::Rejected {
        report: report(source, Some(python), rejection),
    }
}

fn probe(source: PythonSource, python: &str, install: Option<InstallHint>) -> DiscoveryStep {
    DiscoveryStep::Probe {
        source,
        python: python.into(),
        install,
    }
}

/// A search whose steps follow the unset setting.
fn search(steps: Vec<DiscoveryStep>) -> DiscoveryPlan {
    let mut all = vec![DiscoveryStep::Rejected { report: not_set() }];
    all.extend(steps);
    DiscoveryPlan::Search {
        notebook: "/p/nb/a.ipynb".into(),
        scope: scope(),
        steps: all,
        recipe: recipe(),
    }
}

/// A search that probes every entry of [`SEARCHED`].
fn search_everything() -> DiscoveryPlan {
    search(
        SEARCHED
            .iter()
            .map(|(source, python)| probe(source.clone(), python, Some(InstallHint::Pip)))
            .collect(),
    )
}

fn missing_module() -> ProbeOutcome {
    ProbeOutcome::CannotImport {
        exit: ProcessExit::Code(1),
        stderr_tail: "ModuleNotFoundError".to_owned(),
    }
}

fn cannot_import() -> Rejection {
    Rejection::CannotImport {
        exit: ProcessExit::Code(1),
        stderr_tail: "ModuleNotFoundError".to_owned(),
    }
}

fn timed_out() -> ProbeOutcome {
    ProbeOutcome::TimedOut {
        after: Duration::from_secs(10),
    }
}

fn reports(result: Result<Discovery, DiscoveryError>) -> Vec<CandidateReport> {
    match result {
        Err(DiscoveryError::NoneUsable { reports, .. }) => reports,
        other => panic!("not a failed search: {other:?}"),
    }
}

#[test]
fn chooses_the_first_usable_candidate_of_every_source() {
    for (index, (source, python)) in SEARCHED.iter().enumerate() {
        let table: Vec<(&str, ProbeOutcome)> = SEARCHED
            .iter()
            .enumerate()
            .map(|(other, (_, path))| {
                let outcome = if other < index {
                    missing_module()
                } else {
                    ProbeOutcome::Usable
                };
                (*path, outcome)
            })
            .collect();
        let (result, probed) = run(search_everything(), &table);
        let found = result.unwrap_or_else(|error| panic!("{source:?}: {error:?}"));
        assert_eq!(&found.source, source);
        assert_eq!(found.python, PathBuf::from(python), "{source:?}");
        let expected: Vec<PathBuf> = SEARCHED[..=index]
            .iter()
            .map(|(_, path)| PathBuf::from(path))
            .collect();
        assert_eq!(probed, expected, "{source:?}");
    }
}

#[test]
fn keeps_the_candidates_passed_over_in_order() {
    let table = [
        (SEARCHED[0].1, missing_module()),
        (SEARCHED[1].1, timed_out()),
        (SEARCHED[2].1, ProbeOutcome::Usable),
    ];
    let (result, _) = run(search_everything(), &table);
    let mut dot_venv = report(PythonSource::DotVenv, Some(SEARCHED[0].1), cannot_import());
    dot_venv.install = Some(InstallHint::Pip);
    let venv = report(
        PythonSource::Venv,
        Some(SEARCHED[1].1),
        Rejection::TimedOut {
            after: Duration::from_secs(10),
        },
    );
    assert_eq!(
        result.expect("finds one").skipped,
        [not_set(), dot_venv, venv]
    );
}

#[test]
fn skips_only_the_unset_setting_when_the_first_probe_passes() {
    let (result, _) = run(
        search_everything(),
        &[(SEARCHED[0].1, ProbeOutcome::Usable)],
    );
    assert_eq!(result.expect("finds one").skipped, [not_set()]);
}

#[test]
fn never_probes_a_candidate_rejected_for_its_owner() {
    let plan = search(vec![
        rejected(
            PythonSource::DotVenv,
            "/p/.venv/bin/python",
            Rejection::NotOwned,
        ),
        probe(PythonSource::VirtualEnv, "/v/bin/python", None),
    ]);
    let (result, probed) = run(plan, &[("/v/bin/python", ProbeOutcome::Usable)]);
    assert!(result.is_ok());
    assert_eq!(probed, [PathBuf::from("/v/bin/python")]);
}

#[test]
fn probes_a_path_once_after_its_process_ran() {
    for outcome in [missing_module(), timed_out()] {
        let plan = search(vec![
            probe(PythonSource::DotVenv, "/a//b/bin/python", None),
            probe(
                PythonSource::VirtualEnv,
                "/a/b/bin/python",
                Some(InstallHint::Pip),
            ),
        ]);
        let (result, probed) = run(plan, &[("/a/b/bin/python", outcome.clone())]);
        assert_eq!(probed, [PathBuf::from("/a//b/bin/python")], "{outcome:?}");
        assert_eq!(
            reports(result)[2],
            report(
                PythonSource::VirtualEnv,
                Some("/a/b/bin/python"),
                Rejection::AlreadyChecked
            ),
            "{outcome:?}"
        );
    }
}

#[test]
fn probes_a_path_that_was_rejected_without_a_probe() {
    let plan = search(vec![
        rejected(
            PythonSource::DotVenv,
            "/a/b/bin/python",
            Rejection::NotOwned,
        ),
        probe(PythonSource::VirtualEnv, "/a/b/bin/python", None),
    ]);
    let (result, probed) = run(plan, &[("/a/b/bin/python", ProbeOutcome::Usable)]);
    assert_eq!(result.expect("finds one").source, PythonSource::VirtualEnv);
    assert_eq!(probed, [PathBuf::from("/a/b/bin/python")]);
}

#[test]
fn probes_a_path_again_when_no_process_ran() {
    let plan = search(vec![
        probe(PythonSource::DotVenv, "/a/b/bin/python", None),
        probe(PythonSource::VirtualEnv, "/a/b/bin/python", None),
    ]);
    let failed_start = ProbeOutcome::NotExecutable {
        detail: "Text file busy".to_owned(),
    };
    let (_, probed) = run(plan, &[("/a/b/bin/python", failed_start)]);
    assert_eq!(probed.len(), 2);
}

#[test]
fn reports_every_candidate_when_none_works() {
    let uv = Some(InstallHint::Uv);
    let pip = Some(InstallHint::Pip);
    let plan = search(vec![
        probe(PythonSource::DotVenv, "/p/.venv/bin/python", uv.clone()),
        probe(PythonSource::Venv, "/p/venv/bin/python", pip.clone()),
        probe(PythonSource::VirtualEnv, "/v/bin/python", pip),
        probe(PythonSource::PathPython3, "/usr/bin/python3", None),
    ]);
    let failed_start = ProbeOutcome::NotExecutable {
        detail: "Permission denied".to_owned(),
    };
    let table = [
        ("/p/.venv/bin/python", missing_module()),
        ("/p/venv/bin/python", timed_out()),
        ("/v/bin/python", failed_start),
        ("/usr/bin/python3", missing_module()),
    ];
    let (result, _) = run(plan, &table);
    let mut dot_venv = report(
        PythonSource::DotVenv,
        Some("/p/.venv/bin/python"),
        cannot_import(),
    );
    dot_venv.install = uv;
    assert_eq!(
        result,
        Err(DiscoveryError::NoneUsable {
            notebook: "/p/nb/a.ipynb".into(),
            scope: scope(),
            reports: vec![
                not_set(),
                dot_venv,
                report(
                    PythonSource::Venv,
                    Some("/p/venv/bin/python"),
                    Rejection::TimedOut {
                        after: Duration::from_secs(10),
                    }
                ),
                report(
                    PythonSource::VirtualEnv,
                    Some("/v/bin/python"),
                    Rejection::NotExecutable {
                        detail: "Permission denied".to_owned(),
                    }
                ),
                report(
                    PythonSource::PathPython3,
                    Some("/usr/bin/python3"),
                    cannot_import()
                ),
            ],
            recipe: recipe(),
        })
    );
}

/// A plan whose only candidate is the setting, waiting for a probe.
fn configured() -> DiscoveryPlan {
    DiscoveryPlan::Config {
        step: probe(
            PythonSource::Config,
            "/env/bin/python",
            Some(InstallHint::Pip),
        ),
    }
}

#[test]
fn chooses_a_usable_setting() {
    let (result, probed) = run(configured(), &[("/env/bin/python", ProbeOutcome::Usable)]);
    assert_eq!(
        result,
        Ok(Discovery {
            python: "/env/bin/python".into(),
            source: PythonSource::Config,
            skipped: Vec::new(),
        })
    );
    assert_eq!(probed, [PathBuf::from("/env/bin/python")]);
}

#[test]
fn stops_at_a_setting_that_cannot_import() {
    let (result, _) = run(configured(), &[("/env/bin/python", missing_module())]);
    let mut config = report(
        PythonSource::Config,
        Some("/env/bin/python"),
        cannot_import(),
    );
    config.install = Some(InstallHint::Pip);
    assert_eq!(
        result,
        Err(DiscoveryError::ConfigUnusable { report: config })
    );
}

#[test]
fn stops_at_a_rejected_setting_without_a_probe() {
    let plan = DiscoveryPlan::Config {
        step: rejected(PythonSource::Config, "/env/bin/python", Rejection::NotFound),
    };
    let (result, probed) = run(plan, &[]);
    assert_eq!(
        result,
        Err(DiscoveryError::ConfigUnusable {
            report: report(
                PythonSource::Config,
                Some("/env/bin/python"),
                Rejection::NotFound
            ),
        })
    );
    assert!(probed.is_empty());
}

#[test]
fn fails_at_once_on_a_relative_setting() {
    let plan = DiscoveryPlan::ConfigNotAbsolute {
        path: "py/bin/python".into(),
    };
    let (result, probed) = run(plan, &[]);
    assert_eq!(
        result,
        Err(DiscoveryError::ConfigNotAbsolute {
            path: "py/bin/python".into(),
        })
    );
    assert!(probed.is_empty());
}

#[test]
fn explains_a_failed_search_from_the_unset_setting_on() {
    let (result, _) = run(all_sources_plan(), &all_sources_outcomes());
    let error = result.expect_err("nothing is usable");
    let text = error.to_string();
    let first = text
        .lines()
        .skip_while(|line| *line != "Checked in this order:")
        .nth(1)
        .expect("a first candidate");
    assert!(first.trim_start().starts_with("kernel.python "), "{first}");
    assert!(first.ends_with(" not set"), "{first}");
    assert_eq!(error, all_sources_error());
}

#[test]
#[should_panic(expected = "advance called after Done")]
fn panics_when_advanced_after_done() {
    let mut fold = DiscoveryFold::new(search(Vec::new()));
    let _ = fold.advance();
    let _ = fold.advance();
}

#[test]
#[should_panic(expected = "advance called before record")]
fn panics_when_a_probe_is_left_unanswered() {
    let mut fold = DiscoveryFold::new(search_everything());
    let _ = fold.advance();
    let _ = fold.advance();
}

#[test]
#[should_panic(expected = "record called without a pending probe")]
fn panics_when_recording_without_a_probe() {
    let mut fold = DiscoveryFold::new(search_everything());
    fold.record(ProbeOutcome::Usable);
}
