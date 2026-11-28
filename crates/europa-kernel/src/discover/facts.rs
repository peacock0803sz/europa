//! Facts about the file system and the environment, read without running any
//! Python.

use std::ffi::OsStr;
use std::io::Read;
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path, PathBuf};

use europa_core::{AncestorScope, DiscoveryRequest};

use super::plan::{EnvPython, Facts, FileState, LevelFact, OwnerFact, VenvFact};

/// Bytes of `pyvenv.cfg` that are read; real files hold a few lines.
const VENV_CONFIG_LIMIT: u64 = 64 * 1024;

/// Collect what [`plan`](super::plan::plan) needs to know about `request`.
///
/// With a `kernel.python` setting, only that interpreter is looked at.
pub(crate) fn gather(request: &DiscoveryRequest) -> Facts {
    // This fails only for an empty path or without a current directory, and
    // then no level is searched.
    let notebook =
        std::path::absolute(&request.notebook).unwrap_or_else(|_| request.notebook.clone());
    let notebook_dir = resolve(notebook.parent().unwrap_or(&notebook));
    let mut facts = Facts {
        notebook,
        scope: AncestorScope {
            notebook_dir,
            repo_root: None,
        },
        levels: Vec::new(),
        config: EnvPython::NotSet,
        virtual_env: EnvPython::NotSet,
        conda_prefix: EnvPython::NotSet,
        path_python3: None,
        path_python: None,
        euid: account::euid(),
    };
    if let Some(python) = &request.config_python {
        facts.config = configured(python);
        return facts;
    }
    // A relative directory would be searched from wherever Europa runs.
    if facts.scope.notebook_dir.is_absolute() {
        facts.scope.repo_root = repo_root(&facts.scope.notebook_dir);
        facts.levels = levels(&facts.scope);
    }
    facts.virtual_env = activated(request.env.virtual_env.as_deref());
    facts.conda_prefix = activated(request.env.conda_prefix.as_deref());
    let dirs = request.env.path.as_deref().map(path_dirs);
    let dirs = dirs.unwrap_or_default();
    facts.path_python3 = first_executable(&dirs, "python3");
    facts.path_python = first_executable(&dirs, "python");
    facts
}

/// Split a `PATH` value, leaving out empty and relative entries.
///
/// Both kinds name the current directory, so searching them would run whatever
/// `python3` lies where Europa was started.
pub(crate) fn path_dirs(path: &OsStr) -> Vec<PathBuf> {
    std::env::split_paths(path)
        .filter(|dir| dir.is_absolute())
        .collect()
}

/// The real directory behind `dir`, or `dir` with its dots folded away when it
/// cannot be resolved.
fn resolve(dir: &Path) -> PathBuf {
    std::fs::canonicalize(dir).unwrap_or_else(|_| {
        let mut folded = PathBuf::new();
        for part in dir.components() {
            match part {
                Component::CurDir => {}
                Component::ParentDir => {
                    folded.pop();
                }
                part => folded.push(part),
            }
        }
        folded
    })
}

/// Nearest directory at or above `dir` that holds a `.git` file or directory.
fn repo_root(dir: &Path) -> Option<PathBuf> {
    dir.ancestors()
        .find(|level| {
            std::fs::metadata(level.join(".git")).is_ok_and(|git| git.is_dir() || git.is_file())
        })
        .map(Path::to_path_buf)
}

/// The directories from the notebook's up to the repository root, or the
/// notebook's alone outside a repository.
fn levels(scope: &AncestorScope) -> Vec<LevelFact> {
    let top = scope.repo_root.as_ref().unwrap_or(&scope.notebook_dir);
    scope
        .notebook_dir
        .ancestors()
        .take_while(|dir| dir.starts_with(top))
        .map(|dir| LevelFact {
            dir: dir.to_path_buf(),
            dot_venv: venv(&dir.join(".venv")),
            venv: venv(&dir.join("venv")),
        })
        .collect()
}

/// The virtual environment at `path`, when that leads to a directory.
fn venv(path: &Path) -> Option<VenvFact> {
    if !std::fs::metadata(path).ok()?.is_dir() {
        return None;
    }
    // Everything after this uses the resolved directory, so swapping a link
    // after the owner was checked does not change what runs.
    let root = std::fs::canonicalize(path).ok()?;
    let directory = std::fs::metadata(&root).ok()?;
    let python = root.join("bin/python");
    Some(VenvFact {
        owner: OwnerFact {
            uid: directory.uid(),
            mode: directory.mode(),
            group_private: private_to_user(directory.gid()),
        },
        file: file_state(&python),
        uv: created_by_uv(&root),
        root,
        python,
    })
}

/// The interpreter that the `kernel.python` value names.
fn configured(python: &Path) -> EnvPython {
    if !python.is_absolute() {
        return EnvPython::NotAbsolute(python.to_path_buf());
    }
    // An interpreter of a virtual environment sits two levels below its root.
    let root = python.parent().and_then(Path::parent);
    EnvPython::Python {
        python: python.to_path_buf(),
        file: file_state(python),
        uv: root.is_some_and(created_by_uv),
    }
}

/// The interpreter of the environment that a variable names.
fn activated(value: Option<&Path>) -> EnvPython {
    match value {
        None => EnvPython::NotSet,
        Some(root) if root.as_os_str().is_empty() => EnvPython::NotSet,
        Some(root) if !root.is_absolute() => EnvPython::NotAbsolute(root.to_path_buf()),
        Some(root) => {
            let python = root.join("bin/python");
            EnvPython::Python {
                file: file_state(&python),
                uv: created_by_uv(root),
                python,
            }
        }
    }
}

/// The first executable called `name` in `dirs`, as a shell would find it.
fn first_executable(dirs: &[PathBuf], name: &str) -> Option<PathBuf> {
    dirs.iter()
        .map(|dir| dir.join(name))
        .find(|command| file_state(command) == FileState::Present)
}

/// Whether the file at `python` can be run, following symbolic links.
fn file_state(python: &Path) -> FileState {
    match std::fs::metadata(python) {
        Ok(file) if file.is_file() && file.mode() & 0o111 != 0 => FileState::Present,
        Ok(_) => FileState::NotExecutable,
        Err(_) if std::fs::symlink_metadata(python).is_ok() => FileState::BrokenLink,
        Err(_) => FileState::Missing,
    }
}

/// Whether `pyvenv.cfg` in `root` has a `uv` key.
fn created_by_uv(root: &Path) -> bool {
    let config = root.join("pyvenv.cfg");
    // The directory may be anyone's: opening a pipe would hang, and a link to
    // an endless device would never finish.
    if !std::fs::metadata(&config).is_ok_and(|file| file.is_file()) {
        return false;
    }
    let mut text = String::new();
    let read = std::fs::File::open(config)
        .and_then(|file| file.take(VENV_CONFIG_LIMIT).read_to_string(&mut text));
    read.is_ok()
        && text.lines().any(|line| {
            line.split_once('=')
                .is_some_and(|(key, _)| key.trim() == "uv")
        })
}

/// Whether the group `gid` is the effective user's private group.
fn private_to_user(gid: u32) -> bool {
    account::current_user()
        .is_some_and(|user| group_is_private(gid, account::group(gid).as_ref(), &user))
}

/// A group, as the account database describes it.
struct GroupInfo {
    /// Group name.
    name: String,
    /// Users listed as members besides those whose primary group it is.
    members: Vec<String>,
}

/// A user, as the account database describes it.
struct UserInfo {
    /// User name.
    name: String,
    /// Primary group.
    primary_gid: u32,
}

/// Whether only `user` belongs to the group `dir_gid`.
///
/// That holds for a user private group: the primary group, named after the
/// user, with no other member. A group that cannot be looked up does not count.
fn group_is_private(dir_gid: u32, group: Option<&GroupInfo>, user: &UserInfo) -> bool {
    group.is_some_and(|group| {
        dir_gid == user.primary_gid
            && group.name == user.name
            && group.members.iter().all(|member| *member == user.name)
    })
}

/// The few calls into the account database.
mod account {
    use std::ffi::{c_char, c_int, CStr};
    use std::mem::MaybeUninit;
    use std::ptr;

    use super::{GroupInfo, UserInfo};

    /// Largest buffer offered for one account entry.
    const BUFFER_LIMIT: usize = 1 << 20;

    /// The user this process acts as.
    pub(super) fn euid() -> u32 {
        // SAFETY: `geteuid` takes nothing, cannot fail, and touches no memory.
        unsafe { libc::geteuid() }
    }

    /// Account entry of the effective user.
    pub(super) fn current_user() -> Option<UserInfo> {
        let uid = euid();
        with_buffer(|buffer| {
            let mut entry = MaybeUninit::<libc::passwd>::uninit();
            let mut found = ptr::null_mut();
            // SAFETY: the pointers refer to values that live through the call,
            // and the length passed is that of `buffer`.
            let code = unsafe {
                libc::getpwuid_r(
                    uid,
                    entry.as_mut_ptr(),
                    buffer.as_mut_ptr(),
                    buffer.len(),
                    &mut found,
                )
            };
            if code != 0 {
                return Err(code);
            }
            if found.is_null() {
                return Ok(None);
            }
            // SAFETY: a non-null result says that `entry` was filled in.
            let entry = unsafe { entry.assume_init() };
            // SAFETY: the name is a C string inside `buffer`, which is not
            // touched before the copy is taken.
            let name = unsafe { CStr::from_ptr(entry.pw_name) };
            Ok(name.to_str().ok().map(|name| UserInfo {
                name: name.to_owned(),
                primary_gid: entry.pw_gid,
            }))
        })
    }

    /// Account entry of a group.
    ///
    /// A name that is not UTF-8 gives `None`, so no member goes unnoticed.
    pub(super) fn group(gid: u32) -> Option<GroupInfo> {
        with_buffer(|buffer| {
            let mut entry = MaybeUninit::<libc::group>::uninit();
            let mut found = ptr::null_mut();
            // SAFETY: the pointers refer to values that live through the call,
            // and the length passed is that of `buffer`.
            let code = unsafe {
                libc::getgrgid_r(
                    gid,
                    entry.as_mut_ptr(),
                    buffer.as_mut_ptr(),
                    buffer.len(),
                    &mut found,
                )
            };
            if code != 0 {
                return Err(code);
            }
            if found.is_null() {
                return Ok(None);
            }
            // SAFETY: a non-null result says that `entry` was filled in.
            let entry = unsafe { entry.assume_init() };
            // SAFETY: the name is a C string inside `buffer`, which is not
            // touched before the copy is taken.
            let name = unsafe { CStr::from_ptr(entry.gr_name) };
            let Ok(name) = name.to_str() else {
                return Ok(None);
            };
            let mut members = Vec::new();
            let mut next: *const *mut c_char = entry.gr_mem;
            while !next.is_null() {
                // SAFETY: `gr_mem` is an array inside `buffer` that ends with a
                // null pointer; macOS may leave it unaligned.
                let member = unsafe { next.read_unaligned() };
                if member.is_null() {
                    break;
                }
                // SAFETY: each member is a C string inside `buffer`.
                let Ok(member) = unsafe { CStr::from_ptr(member) }.to_str() else {
                    return Ok(None);
                };
                members.push(member.to_owned());
                // SAFETY: this element was not the terminating null, so another
                // one follows.
                next = unsafe { next.add(1) };
            }
            Ok(Some(GroupInfo {
                name: name.to_owned(),
                members,
            }))
        })
    }

    /// Run a reentrant lookup, growing its buffer while that is too small.
    ///
    /// Any other error counts as no entry.
    fn with_buffer<T>(
        mut lookup: impl FnMut(&mut [c_char]) -> Result<Option<T>, c_int>,
    ) -> Option<T> {
        let mut buffer = vec![0; 1024];
        loop {
            match lookup(&mut buffer) {
                Ok(found) => return found,
                Err(libc::ERANGE) if buffer.len() < BUFFER_LIMIT => {
                    buffer.resize(buffer.len() * 2, 0);
                }
                Err(_) => return None,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discover::plan::{EnvPython, FileState, LevelFact};
    use europa_core::{AncestorScope, DiscoveryEnv};
    use std::ffi::OsString;
    use std::os::unix::fs::{symlink, PermissionsExt};
    use std::path::Path;
    use tempfile::TempDir;

    /// A canonical temporary directory that is in no repository.
    fn tree() -> (TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = std::fs::canonicalize(dir.path()).expect("resolves");
        for ancestor in root.ancestors() {
            assert!(
                !ancestor.join(".git").exists(),
                "{} holds .git; point TMPDIR outside a repository",
                ancestor.display()
            );
        }
        (dir, root)
    }

    fn mkdir(path: &Path) {
        std::fs::create_dir_all(path).expect("creates");
    }

    fn set_mode(path: &Path, mode: u32) {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).expect("chmod");
    }

    /// A file with `content` and `mode`, in a directory made when missing.
    fn put(path: &Path, content: &str, mode: u32) {
        mkdir(path.parent().expect("a parent"));
        std::fs::write(path, content).expect("writes");
        set_mode(path, mode);
    }

    fn executable(path: &Path) {
        put(path, "#!/bin/sh\n", 0o755);
    }

    /// Restores the permissions of a directory, even when the test fails.
    struct Locked(PathBuf);

    impl Locked {
        fn new(path: PathBuf) -> Self {
            set_mode(&path, 0o600);
            Self(path)
        }
    }

    impl Drop for Locked {
        fn drop(&mut self) {
            set_mode(&self.0, 0o755);
        }
    }

    /// Whether to skip a test that permissions cannot stop, as for root.
    fn ignores_permissions() -> bool {
        let root = account::euid() == 0;
        if root {
            eprintln!("skipped: root ignores permissions");
        }
        root
    }

    fn gather_with(
        notebook: impl Into<PathBuf>,
        config: Option<PathBuf>,
        env: DiscoveryEnv,
    ) -> Facts {
        gather(&DiscoveryRequest::new(notebook, config, env))
    }

    fn facts_for(notebook: impl Into<PathBuf>) -> Facts {
        gather_with(notebook, None, DiscoveryEnv::default())
    }

    fn level_dirs(facts: &Facts) -> Vec<PathBuf> {
        facts.levels.iter().map(|level| level.dir.clone()).collect()
    }

    /// The level of a notebook that sits in `dir`.
    fn nearest(dir: &Path) -> LevelFact {
        facts_for(dir.join("a.ipynb")).levels.swap_remove(0)
    }

    #[test]
    fn searches_from_the_notebook_up_to_the_repository_root() {
        for git_is_file in [false, true] {
            let (_dir, root) = tree();
            let repo = root.join("repo");
            mkdir(&repo.join("nb"));
            if git_is_file {
                put(&repo.join(".git"), "gitdir: elsewhere\n", 0o644);
            } else {
                mkdir(&repo.join(".git"));
            }
            mkdir(&root.join(".venv"));
            let facts = facts_for(repo.join("nb/a.ipynb"));
            assert_eq!(facts.notebook, repo.join("nb/a.ipynb"));
            assert_eq!(
                facts.scope,
                AncestorScope {
                    notebook_dir: repo.join("nb"),
                    repo_root: Some(repo.clone()),
                }
            );
            assert_eq!(level_dirs(&facts), [repo.join("nb"), repo]);
            assert!(facts.levels.iter().all(|level| level.dot_venv.is_none()));
        }
    }

    #[test]
    fn stops_at_the_nearest_repository() {
        let (_dir, root) = tree();
        let sub = root.join("repo/sub");
        mkdir(&root.join("repo/.git"));
        put(&sub.join(".git"), "gitdir: elsewhere\n", 0o644);
        mkdir(&sub.join("nb"));
        let facts = facts_for(sub.join("nb/a.ipynb"));
        assert_eq!(facts.scope.repo_root, Some(sub.clone()));
        assert_eq!(level_dirs(&facts), [sub.join("nb"), sub]);
    }

    #[test]
    fn looks_only_beside_the_notebook_outside_a_repository() {
        let (_dir, root) = tree();
        let notebooks = root.join("proj/nb");
        mkdir(&notebooks);
        mkdir(&root.join("proj/.venv"));
        let facts = facts_for(notebooks.join("a.ipynb"));
        assert_eq!(facts.scope.repo_root, None);
        assert_eq!(level_dirs(&facts), [notebooks]);
        assert!(facts.levels[0].dot_venv.is_none());
    }

    #[test]
    fn passes_a_level_it_cannot_inspect() {
        if ignores_permissions() {
            return;
        }
        let (_dir, root) = tree();
        let repo = root.join("repo");
        mkdir(&repo.join(".git"));
        mkdir(&repo.join("nb/sub"));
        let locked = Locked::new(repo.join("nb"));
        let facts = facts_for(repo.join("nb/sub/a.ipynb"));
        drop(locked);
        assert_eq!(
            level_dirs(&facts),
            [repo.join("nb/sub"), repo.join("nb"), repo.clone()]
        );
        assert_eq!(facts.scope.repo_root, Some(repo));
    }

    #[test]
    fn resolves_the_notebook_directory() {
        let (_dir, root) = tree();
        let repo = root.join("repo");
        mkdir(&repo.join(".git"));
        mkdir(&repo.join("nb"));
        symlink(repo.join("nb"), root.join("link")).expect("links");
        for notebook in [repo.join("nb/../nb/a.ipynb"), root.join("link/a.ipynb")] {
            let scope = facts_for(&notebook).scope;
            assert_eq!(scope.notebook_dir, repo.join("nb"), "{notebook:?}");
            assert_eq!(scope.repo_root, Some(repo.clone()), "{notebook:?}");
        }
        let missing = facts_for(repo.join("new/./x/../a.ipynb"));
        assert_eq!(missing.scope.notebook_dir, repo.join("new"));
    }

    #[test]
    fn resolves_a_relative_notebook_against_the_current_directory() {
        let facts = facts_for("a.ipynb");
        let here = std::env::current_dir().expect("current dir");
        assert_eq!(
            facts.scope.notebook_dir,
            std::fs::canonicalize(here).expect("resolves")
        );
        assert!(facts.notebook.is_absolute());
    }

    #[test]
    fn finds_both_environment_names() {
        let (_dir, root) = tree();
        executable(&root.join(".venv/bin/python"));
        executable(&root.join("venv/bin/python"));
        let level = nearest(&root);
        assert_eq!(
            level.dot_venv.expect(".venv").python,
            root.join(".venv/bin/python")
        );
        assert_eq!(
            level.venv.expect("venv").python,
            root.join("venv/bin/python")
        );
    }

    #[test]
    fn ignores_an_environment_that_is_no_directory() {
        let (_dir, root) = tree();
        put(&root.join(".venv"), "", 0o755);
        assert_eq!(nearest(&root).dot_venv, None);
    }

    #[test]
    fn judges_a_linked_environment_by_its_target() {
        let (_dir, root) = tree();
        let real = root.join("real");
        mkdir(&real);
        set_mode(&real, 0o700);
        mkdir(&root.join("nb"));
        symlink(&real, root.join("nb/.venv")).expect("links");
        let venv = nearest(&root.join("nb")).dot_venv.expect(".venv");
        assert_eq!(venv.owner.mode & 0o777, 0o700);
        assert_eq!(venv.owner.uid, account::euid());
        assert_eq!(venv.root, real);
        assert_eq!(venv.python, real.join("bin/python"));
    }

    #[test]
    fn records_the_state_of_the_interpreter() {
        let (_dir, root) = tree();
        let python = |case: &str| root.join(case).join(".venv/bin/python");
        mkdir(&root.join("missing/.venv/bin"));
        mkdir(python("broken").parent().expect("bin"));
        symlink("/nonexistent-europa-python", python("broken")).expect("links");
        put(&python("plain"), "", 0o644);
        mkdir(&python("directory"));
        executable(&python("ready"));
        for (case, expected) in [
            ("missing", FileState::Missing),
            ("broken", FileState::BrokenLink),
            ("plain", FileState::NotExecutable),
            ("directory", FileState::NotExecutable),
            ("ready", FileState::Present),
        ] {
            let venv = nearest(&root.join(case)).dot_venv.expect(".venv");
            assert_eq!(venv.file, expected, "{case}");
        }
    }

    #[test]
    fn calls_an_unsearchable_interpreter_missing() {
        if ignores_permissions() {
            return;
        }
        let (_dir, root) = tree();
        executable(&root.join(".venv/bin/python"));
        let locked = Locked::new(root.join(".venv"));
        let venv = nearest(&root).dot_venv.expect(".venv");
        drop(locked);
        assert_eq!(venv.file, FileState::Missing);
    }

    #[test]
    fn reads_whether_uv_created_an_environment() {
        for (config, uv) in [
            (Some("home = /x\nuv = 0.11.8\n"), true),
            (Some("  uv=0.1\n"), true),
            (Some("home = /x\n"), false),
            (Some("uvx = 1\n"), false),
            (None, false),
        ] {
            let (_dir, root) = tree();
            executable(&root.join(".venv/bin/python"));
            if let Some(config) = config {
                put(&root.join(".venv/pyvenv.cfg"), config, 0o644);
            }
            assert_eq!(nearest(&root).dot_venv.expect(".venv").uv, uv, "{config:?}");
        }
    }

    #[test]
    fn drops_empty_and_relative_path_entries() {
        assert_eq!(
            path_dirs(OsStr::new("/a::rel:/b")),
            [PathBuf::from("/a"), PathBuf::from("/b")]
        );
    }

    /// `PATH` made of the directories `a` and `b` of `root`.
    fn path_of(root: &Path) -> DiscoveryEnv {
        DiscoveryEnv {
            path: Some(std::env::join_paths([root.join("a"), root.join("b")]).expect("joins")),
            ..DiscoveryEnv::default()
        }
    }

    #[test]
    fn takes_the_first_executable_of_each_command() {
        let (_dir, root) = tree();
        put(&root.join("a/python3"), "", 0o644);
        executable(&root.join("b/python3"));
        let notebook = root.join("a.ipynb");
        let facts = gather_with(&notebook, None, path_of(&root));
        assert_eq!(facts.path_python3, Some(root.join("b/python3")));
        assert_eq!(facts.path_python, None);

        executable(&root.join("a/python3"));
        executable(&root.join("b/python"));
        let facts = gather_with(&notebook, None, path_of(&root));
        assert_eq!(facts.path_python3, Some(root.join("a/python3")));
        assert_eq!(facts.path_python, Some(root.join("b/python")));
    }

    #[test]
    fn finds_no_command_without_path() {
        let (_dir, root) = tree();
        let facts = facts_for(root.join("a.ipynb"));
        assert_eq!((facts.path_python3, facts.path_python), (None, None));
    }

    #[test]
    fn inspects_the_environments_named_by_variables() {
        let (_dir, root) = tree();
        executable(&root.join("env/bin/python"));
        put(&root.join("env/pyvenv.cfg"), "uv = 0.11.8\n", 0o644);
        let env = DiscoveryEnv {
            virtual_env: Some("rel/v".into()),
            conda_prefix: Some(root.join("env")),
            path: None,
        };
        let facts = gather_with(root.join("a.ipynb"), None, env);
        assert_eq!(facts.virtual_env, EnvPython::NotAbsolute("rel/v".into()));
        assert_eq!(
            facts.conda_prefix,
            EnvPython::Python {
                python: root.join("env/bin/python"),
                file: FileState::Present,
                uv: true,
            }
        );
        assert_eq!(facts.config, EnvPython::NotSet);
        assert_eq!(facts.euid, account::euid());

        let env = DiscoveryEnv {
            virtual_env: Some(root.join("gone")),
            ..DiscoveryEnv::default()
        };
        let facts = gather_with(root.join("a.ipynb"), None, env);
        assert_eq!(
            facts.virtual_env,
            EnvPython::Python {
                python: root.join("gone/bin/python"),
                file: FileState::Missing,
                uv: false,
            }
        );
        assert_eq!(facts.conda_prefix, EnvPython::NotSet);
    }

    #[test]
    fn treats_an_empty_variable_as_unset() {
        let (_dir, root) = tree();
        let env = DiscoveryEnv {
            virtual_env: Some(PathBuf::new()),
            conda_prefix: Some(PathBuf::new()),
            path: Some(OsString::new()),
        };
        let facts = gather_with(root.join("a.ipynb"), None, env);
        assert_eq!(facts.virtual_env, EnvPython::NotSet);
        assert_eq!(facts.conda_prefix, EnvPython::NotSet);
        assert_eq!((facts.path_python3, facts.path_python), (None, None));
    }

    #[test]
    fn looks_only_at_a_set_config() {
        let (_dir, root) = tree();
        mkdir(&root.join("repo/.git"));
        executable(&root.join("repo/nb/.venv/bin/python"));
        executable(&root.join("env/bin/python"));
        put(&root.join("env/pyvenv.cfg"), "uv = 0.11.8\n", 0o644);
        let notebook = root.join("repo/nb/a.ipynb");

        let relative = Some(PathBuf::from("py/bin/python"));
        let facts = gather_with(&notebook, relative, DiscoveryEnv::default());
        assert_eq!(facts.config, EnvPython::NotAbsolute("py/bin/python".into()));
        assert!(facts.levels.is_empty());

        let absolute = Some(root.join("env/bin/python"));
        let facts = gather_with(&notebook, absolute, DiscoveryEnv::default());
        assert_eq!(
            facts.config,
            EnvPython::Python {
                python: root.join("env/bin/python"),
                file: FileState::Present,
                uv: true,
            }
        );
        assert!(facts.levels.is_empty());
    }

    #[test]
    fn calls_only_the_users_own_group_private() {
        let user = UserInfo {
            name: "u".to_owned(),
            primary_gid: 1000,
        };
        let group = |name: &str, members: &[&str]| GroupInfo {
            name: name.to_owned(),
            members: members.iter().map(|member| (*member).to_owned()).collect(),
        };
        assert!(!group_is_private(1000, None, &user));
        assert!(!group_is_private(1000, Some(&group("users", &[])), &user));
        assert!(!group_is_private(1001, Some(&group("u", &[])), &user));
        assert!(!group_is_private(
            1000,
            Some(&group("u", &["u", "v"])),
            &user
        ));
        assert!(group_is_private(1000, Some(&group("u", &[])), &user));
        assert!(group_is_private(1000, Some(&group("u", &["u"])), &user));
    }

    #[test]
    fn reads_the_account_database_as_id_does() {
        let user = account::current_user().expect("an account entry");
        let own = account::group(user.primary_gid).expect("the primary group");
        let id = |flag: &str| std::process::Command::new("id").arg(flag).output();
        let (Ok(group), Ok(name)) = (id("-gn"), id("-un")) else {
            eprintln!("skipped: no id command");
            return;
        };
        let line = |text: &str| format!("{text}\n").into_bytes();
        assert_eq!(name.stdout, line(&user.name));
        assert_eq!(group.stdout, line(&own.name));
        if group_is_private(user.primary_gid, Some(&own), &user) {
            assert_eq!(group.stdout, name.stdout);
        }
    }

    #[test]
    fn never_reads_a_pyvenv_cfg_that_is_no_file() {
        let (_dir, root) = tree();
        executable(&root.join(".venv/bin/python"));
        symlink("/dev/zero", root.join(".venv/pyvenv.cfg")).expect("links");
        assert!(!nearest(&root).dot_venv.expect(".venv").uv);
    }
}
