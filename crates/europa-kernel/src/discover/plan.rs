//! Ordered candidates built from facts, without IO.

use std::path::PathBuf;

use europa_core::{
    AncestorScope, CandidateReport, DiscoveryPlan, DiscoveryStep, InstallHint, PythonSource,
    Rejection, VenvRecipe,
};

/// What the file system and the environment showed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Facts {
    /// Notebook path, made absolute.
    pub(crate) notebook: PathBuf,
    /// Directories that were searched.
    pub(crate) scope: AncestorScope,
    /// Each level from the notebook directory up to the repository root,
    /// nearest first.
    pub(crate) levels: Vec<LevelFact>,
    /// State of the `kernel.python` setting.
    pub(crate) config: EnvPython,
    /// State of `VIRTUAL_ENV`.
    pub(crate) virtual_env: EnvPython,
    /// State of `CONDA_PREFIX`.
    pub(crate) conda_prefix: EnvPython,
    /// `python3` found on `PATH`.
    pub(crate) path_python3: Option<PathBuf>,
    /// `python` found on `PATH`.
    pub(crate) path_python: Option<PathBuf>,
    /// User that runs the search.
    pub(crate) euid: u32,
}

/// Virtual environments in one directory level.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LevelFact {
    /// The directory.
    pub(crate) dir: PathBuf,
    /// Its `.venv`, when that is a directory.
    pub(crate) dot_venv: Option<VenvFact>,
    /// Its `venv`, when that is a directory.
    pub(crate) venv: Option<VenvFact>,
}

/// A virtual environment directory in an ancestor level.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct VenvFact {
    /// Directory with every symbolic link resolved.
    pub(crate) root: PathBuf,
    /// Owner and permissions of `root`.
    pub(crate) owner: OwnerFact,
    /// `root/bin/python`, whose own link is not resolved.
    pub(crate) python: PathBuf,
    /// State of `python`.
    pub(crate) file: FileState,
    /// Whether `pyvenv.cfg` records that uv created it.
    pub(crate) uv: bool,
}

/// Owner and permissions of a directory, as seen through symbolic links.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OwnerFact {
    /// Owning user.
    pub(crate) uid: u32,
    /// File mode bits.
    pub(crate) mode: u32,
    /// Whether the owning group has the user as its only member.
    pub(crate) group_private: bool,
}

/// An interpreter named by the setting or an environment variable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum EnvPython {
    /// Nothing names one.
    NotSet,
    /// The value is a relative path, kept as given.
    NotAbsolute(PathBuf),
    /// An absolute interpreter path.
    Python {
        /// The interpreter.
        python: PathBuf,
        /// State of the file.
        file: FileState,
        /// Whether the environment records that uv created it.
        uv: bool,
    },
}

/// State of an interpreter file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FileState {
    /// Nothing is there.
    Missing,
    /// A symbolic link that leads nowhere.
    BrokenLink,
    /// A file without an execute bit, or not a file.
    NotExecutable,
    /// An executable file.
    Present,
}

/// Whether only this user can change the directory.
///
/// All three must hold: the owner is `euid`, others cannot write, and the
/// group cannot write unless the user is its only member. A virtual
/// environment that fails this is never run, because whoever can write it
/// chooses what runs.
pub(crate) fn owned(owner: &OwnerFact, euid: u32) -> bool {
    owner.uid == euid && owner.mode & 0o002 == 0 && (owner.mode & 0o020 == 0 || owner.group_private)
}

/// Put the candidates in the order the search tries them.
///
/// A set `kernel.python` is the only candidate. Otherwise the steps are the
/// unset setting, then `.venv` and `venv` of each level from the nearest one,
/// then `VIRTUAL_ENV`, `CONDA_PREFIX`, and `python3` and `python` from
/// `PATH`. A virtual environment of a level that is not [`owned`] is rejected,
/// as is any interpreter file that cannot be run.
pub(crate) fn plan(facts: Facts) -> DiscoveryPlan {
    if facts.config != EnvPython::NotSet {
        todo!()
    }
    let recipe = recipe(&facts);
    let mut steps = vec![reject(PythonSource::Config, None, Rejection::NotSet)];
    for level in facts.levels {
        for (source, venv) in [
            (PythonSource::DotVenv, level.dot_venv),
            (PythonSource::Venv, level.venv),
        ] {
            if let Some(venv) = venv {
                steps.push(ancestor(source, venv, facts.euid));
            }
        }
    }
    steps.push(variable(PythonSource::VirtualEnv, facts.virtual_env));
    steps.push(variable(PythonSource::CondaPrefix, facts.conda_prefix));
    steps.push(on_path(PythonSource::PathPython3, facts.path_python3));
    steps.push(on_path(PythonSource::PathPython, facts.path_python));
    DiscoveryPlan::Search {
        notebook: facts.notebook,
        scope: facts.scope,
        steps,
        recipe,
    }
}

/// Step of a virtual environment found in an ancestor directory.
fn ancestor(source: PythonSource, venv: VenvFact, euid: u32) -> DiscoveryStep {
    if !owned(&venv.owner, euid) {
        return reject(source, Some(venv.python), Rejection::NotOwned);
    }
    interpreter(source, venv.python, venv.file, venv.uv)
}

/// Step of an interpreter named by a variable.
///
/// The user chose it, so its owner is not checked.
fn variable(source: PythonSource, named: EnvPython) -> DiscoveryStep {
    match named {
        EnvPython::NotSet => reject(source, None, Rejection::NotSet),
        EnvPython::NotAbsolute(value) => reject(source, Some(value), Rejection::NotAbsolute),
        EnvPython::Python { python, file, uv } => interpreter(source, python, file, uv),
    }
}

/// Step of an interpreter file, probed only when it can run.
fn interpreter(source: PythonSource, python: PathBuf, file: FileState, uv: bool) -> DiscoveryStep {
    let not_executable = |detail: &str| Rejection::NotExecutable {
        detail: detail.to_owned(),
    };
    let rejection = match file {
        FileState::Missing => Rejection::NotFound,
        FileState::BrokenLink => not_executable("broken symbolic link"),
        FileState::NotExecutable => not_executable("not executable"),
        FileState::Present => {
            let install = if uv {
                InstallHint::Uv
            } else {
                InstallHint::Pip
            };
            return DiscoveryStep::Probe {
                source,
                python,
                install: Some(install),
            };
        }
    };
    reject(source, Some(python), rejection)
}

/// Step of a command looked up on `PATH`.
///
/// It gets no install hint: a Python found there is often managed by the
/// system, which refuses direct installs.
fn on_path(source: PythonSource, python: Option<PathBuf>) -> DiscoveryStep {
    match python {
        Some(python) => DiscoveryStep::Probe {
            source,
            python,
            install: None,
        },
        None => reject(source, None, Rejection::NotFound),
    }
}

/// Step that is already known to be unusable.
fn reject(source: PythonSource, python: Option<PathBuf>, rejection: Rejection) -> DiscoveryStep {
    DiscoveryStep::Rejected {
        report: CandidateReport {
            source,
            python,
            rejection,
            install: None,
        },
    }
}

/// Where and with what to create a virtual environment that the next search
/// finds.
fn recipe(facts: &Facts) -> VenvRecipe {
    let python = facts
        .path_python3
        .clone()
        .or_else(|| facts.path_python.clone())
        .unwrap_or_else(|| "python3".into());
    let root = facts
        .scope
        .repo_root
        .as_ref()
        .unwrap_or(&facts.scope.notebook_dir);
    VenvRecipe {
        python,
        venv_dir: root.join(".venv"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    const NB: &str = "/home/u/proj/nb";
    const PROJ: &str = "/home/u/proj";

    fn facts() -> Facts {
        Facts {
            notebook: "/home/u/proj/nb/a.ipynb".into(),
            scope: AncestorScope {
                notebook_dir: NB.into(),
                repo_root: Some(PROJ.into()),
            },
            levels: Vec::new(),
            config: EnvPython::NotSet,
            virtual_env: EnvPython::NotSet,
            conda_prefix: EnvPython::NotSet,
            path_python3: None,
            path_python: None,
            euid: 1000,
        }
    }

    fn owner(uid: u32, mode: u32, group_private: bool) -> OwnerFact {
        OwnerFact {
            uid,
            mode,
            group_private,
        }
    }

    fn venv(root: &str) -> VenvFact {
        VenvFact {
            root: root.into(),
            owner: owner(1000, 0o40755, false),
            python: Path::new(root).join("bin/python"),
            file: FileState::Present,
            uv: false,
        }
    }

    fn level(dir: &str, dot_venv: Option<VenvFact>, venv: Option<VenvFact>) -> LevelFact {
        LevelFact {
            dir: dir.into(),
            dot_venv,
            venv,
        }
    }

    fn env_python(python: &str, file: FileState, uv: bool) -> EnvPython {
        EnvPython::Python {
            python: python.into(),
            file,
            uv,
        }
    }

    fn steps(facts: Facts) -> Vec<DiscoveryStep> {
        match plan(facts) {
            DiscoveryPlan::Search { steps, .. } => steps,
            other => panic!("not a search: {other:?}"),
        }
    }

    /// The step of a `.venv` that is the only one in the notebook directory.
    fn dot_venv_step(dot_venv: VenvFact) -> DiscoveryStep {
        let mut facts = facts();
        facts.levels = vec![level(NB, Some(dot_venv), None)];
        steps(facts).swap_remove(1)
    }

    fn rejected(source: PythonSource, python: Option<&str>, rejection: Rejection) -> DiscoveryStep {
        DiscoveryStep::Rejected {
            report: CandidateReport {
                source,
                python: python.map(PathBuf::from),
                rejection,
                install: None,
            },
        }
    }

    fn probe(source: PythonSource, python: &str, install: Option<InstallHint>) -> DiscoveryStep {
        DiscoveryStep::Probe {
            source,
            python: python.into(),
            install,
        }
    }

    fn config_not_set() -> DiscoveryStep {
        rejected(PythonSource::Config, None, Rejection::NotSet)
    }

    #[test]
    fn orders_every_source() {
        use PythonSource::*;

        let mut facts = facts();
        facts.levels = vec![
            level(
                NB,
                Some(venv("/home/u/proj/nb/.venv")),
                Some(venv("/home/u/proj/nb/venv")),
            ),
            level(PROJ, Some(venv("/home/u/proj/.venv")), None),
        ];
        facts.virtual_env = env_python("/v/bin/python", FileState::Present, false);
        facts.conda_prefix = env_python("/c/bin/python", FileState::Present, false);
        facts.path_python3 = Some("/usr/bin/python3".into());
        facts.path_python = Some("/usr/bin/python".into());
        let pip = Some(InstallHint::Pip);
        assert_eq!(
            steps(facts),
            [
                config_not_set(),
                probe(DotVenv, "/home/u/proj/nb/.venv/bin/python", pip.clone()),
                probe(Venv, "/home/u/proj/nb/venv/bin/python", pip.clone()),
                probe(DotVenv, "/home/u/proj/.venv/bin/python", pip.clone()),
                probe(VirtualEnv, "/v/bin/python", pip.clone()),
                probe(CondaPrefix, "/c/bin/python", pip),
                probe(PathPython3, "/usr/bin/python3", None),
                probe(PathPython, "/usr/bin/python", None),
            ]
        );
    }

    #[test]
    fn rejects_environments_others_can_change() {
        for unsafe_owner in [
            owner(1001, 0o40755, false),
            owner(1001, 0o40775, true),
            owner(1000, 0o40757, false),
            owner(1000, 0o40777, true),
            owner(1000, 0o40775, false),
        ] {
            assert!(!owned(&unsafe_owner, 1000), "{unsafe_owner:?}");
            let mut dot_venv = venv("/home/u/proj/nb/.venv");
            dot_venv.owner = unsafe_owner.clone();
            assert_eq!(
                dot_venv_step(dot_venv),
                rejected(
                    PythonSource::DotVenv,
                    Some("/home/u/proj/nb/.venv/bin/python"),
                    Rejection::NotOwned
                ),
                "{unsafe_owner:?}"
            );
        }
    }

    #[test]
    fn probes_environments_only_the_user_can_change() {
        for safe_owner in [owner(1000, 0o40775, true), owner(1000, 0o40755, false)] {
            assert!(owned(&safe_owner, 1000), "{safe_owner:?}");
            let mut dot_venv = venv("/home/u/proj/nb/.venv");
            dot_venv.owner = safe_owner.clone();
            assert_eq!(
                dot_venv_step(dot_venv),
                probe(
                    PythonSource::DotVenv,
                    "/home/u/proj/nb/.venv/bin/python",
                    Some(InstallHint::Pip)
                ),
                "{safe_owner:?}"
            );
        }
    }

    #[test]
    fn judges_ownership_before_the_interpreter_file() {
        let mut dot_venv = venv("/home/u/proj/nb/.venv");
        dot_venv.owner = owner(1001, 0o40755, false);
        dot_venv.file = FileState::Missing;
        assert_eq!(
            dot_venv_step(dot_venv),
            rejected(
                PythonSource::DotVenv,
                Some("/home/u/proj/nb/.venv/bin/python"),
                Rejection::NotOwned
            )
        );
    }

    #[test]
    fn probes_environment_and_path_candidates_whoever_runs() {
        use PythonSource::*;

        let mut facts = facts();
        facts.euid = 4242;
        facts.virtual_env = env_python("/v/bin/python", FileState::Present, false);
        facts.conda_prefix = env_python("/c/bin/python", FileState::Present, false);
        facts.path_python3 = Some("/usr/bin/python3".into());
        facts.path_python = Some("/usr/bin/python".into());
        let pip = Some(InstallHint::Pip);
        assert_eq!(
            steps(facts),
            [
                config_not_set(),
                probe(VirtualEnv, "/v/bin/python", pip.clone()),
                probe(CondaPrefix, "/c/bin/python", pip),
                probe(PathPython3, "/usr/bin/python3", None),
                probe(PathPython, "/usr/bin/python", None),
            ]
        );
    }

    #[test]
    fn rejects_interpreters_that_cannot_run() {
        use PythonSource::*;

        let not_executable = |detail: &str| Rejection::NotExecutable {
            detail: detail.to_owned(),
        };
        for (file, rejection) in [
            (FileState::Missing, Rejection::NotFound),
            (
                FileState::BrokenLink,
                not_executable("broken symbolic link"),
            ),
            (FileState::NotExecutable, not_executable("not executable")),
        ] {
            let mut facts = facts();
            let mut dot_venv = venv("/home/u/proj/nb/.venv");
            dot_venv.file = file.clone();
            facts.levels = vec![level(NB, Some(dot_venv), None)];
            facts.virtual_env = env_python("/v/bin/python", file.clone(), false);
            facts.conda_prefix = env_python("/c/bin/python", file, false);
            assert_eq!(
                steps(facts)[1..4],
                [
                    rejected(
                        DotVenv,
                        Some("/home/u/proj/nb/.venv/bin/python"),
                        rejection.clone()
                    ),
                    rejected(VirtualEnv, Some("/v/bin/python"), rejection.clone()),
                    rejected(CondaPrefix, Some("/c/bin/python"), rejection),
                ]
            );
        }
    }

    #[test]
    fn reports_what_is_not_set_or_not_found() {
        use PythonSource::*;

        assert_eq!(
            steps(facts()),
            [
                config_not_set(),
                rejected(VirtualEnv, None, Rejection::NotSet),
                rejected(CondaPrefix, None, Rejection::NotSet),
                rejected(PathPython3, None, Rejection::NotFound),
                rejected(PathPython, None, Rejection::NotFound),
            ]
        );
    }

    #[test]
    fn rejects_relative_variables() {
        use PythonSource::*;

        let mut facts = facts();
        facts.virtual_env = EnvPython::NotAbsolute("rel/v".into());
        facts.conda_prefix = EnvPython::NotAbsolute("rel/c".into());
        assert_eq!(
            steps(facts)[1..3],
            [
                rejected(VirtualEnv, Some("rel/v"), Rejection::NotAbsolute),
                rejected(CondaPrefix, Some("rel/c"), Rejection::NotAbsolute),
            ]
        );
    }

    #[test]
    fn suggests_uv_only_for_uv_environments() {
        let mut facts = facts();
        let mut dot_venv = venv("/home/u/proj/nb/.venv");
        dot_venv.uv = true;
        facts.levels = vec![level(
            NB,
            Some(dot_venv),
            Some(venv("/home/u/proj/nb/venv")),
        )];
        facts.virtual_env = env_python("/v/bin/python", FileState::Present, true);
        facts.conda_prefix = env_python("/c/bin/python", FileState::Present, false);
        facts.path_python3 = Some("/usr/bin/python3".into());
        facts.path_python = Some("/usr/bin/python".into());
        let installs: Vec<Option<InstallHint>> = steps(facts)
            .into_iter()
            .filter_map(|step| match step {
                DiscoveryStep::Probe { install, .. } => Some(install),
                DiscoveryStep::Rejected { .. } => None,
            })
            .collect();
        let (uv, pip) = (Some(InstallHint::Uv), Some(InstallHint::Pip));
        assert_eq!(installs, [uv.clone(), pip.clone(), uv, pip, None, None]);
    }

    #[test]
    fn creates_the_new_environment_with_a_path_python() {
        for (python3, python, expected) in [
            (
                Some("/usr/bin/python3"),
                Some("/opt/python"),
                "/usr/bin/python3",
            ),
            (None, Some("/opt/python"), "/opt/python"),
            (None, None, "python3"),
        ] {
            let mut facts = facts();
            facts.path_python3 = python3.map(PathBuf::from);
            facts.path_python = python.map(PathBuf::from);
            let DiscoveryPlan::Search { recipe, .. } = plan(facts) else {
                panic!("not a search");
            };
            assert_eq!(
                recipe,
                VenvRecipe {
                    python: expected.into(),
                    venv_dir: "/home/u/proj/.venv".into(),
                }
            );
        }
    }

    #[test]
    fn creates_the_new_environment_by_the_notebook_outside_a_repository() {
        let mut facts = facts();
        facts.scope.repo_root = None;
        let expected_scope = facts.scope.clone();
        let DiscoveryPlan::Search {
            notebook,
            scope,
            recipe,
            ..
        } = plan(facts)
        else {
            panic!("not a search");
        };
        assert_eq!(notebook, PathBuf::from("/home/u/proj/nb/a.ipynb"));
        assert_eq!(scope, expected_scope);
        assert_eq!(recipe.venv_dir, PathBuf::from("/home/u/proj/nb/.venv"));
    }
}
