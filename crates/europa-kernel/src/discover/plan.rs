//! Ordered candidates built from facts, without IO.

use std::path::PathBuf;

use europa_core::{AncestorScope, DiscoveryPlan};

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
    let _ = (owner, euid);
    todo!()
}

/// Put the candidates in the order the search tries them.
///
/// A set `kernel.python` is the only candidate. Otherwise the steps are the
/// unset setting, then `.venv` and `venv` of each level from the nearest one,
/// then `VIRTUAL_ENV`, `CONDA_PREFIX`, and `python3` and `python` from
/// `PATH`. A virtual environment of a level that is not [`owned`] is rejected,
/// as is any interpreter file that cannot be run.
pub(crate) fn plan(facts: Facts) -> DiscoveryPlan {
    let _ = facts;
    todo!()
}
