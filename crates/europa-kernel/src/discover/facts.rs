//! Facts about the file system and the environment, read without running any
//! Python.

use std::ffi::OsStr;
use std::path::PathBuf;

use europa_core::DiscoveryRequest;

use super::plan::Facts;

/// Collect what [`plan`](super::plan::plan) needs to know about `request`.
///
/// With a `kernel.python` setting, only that interpreter is looked at.
pub(crate) fn gather(request: &DiscoveryRequest) -> Facts {
    let _ = request;
    todo!()
}

/// Split a `PATH` value, leaving out empty and relative entries.
///
/// Both kinds name the current directory, so searching them would run whatever
/// `python3` lies where Europa was started.
pub(crate) fn path_dirs(path: &OsStr) -> Vec<PathBuf> {
    let _ = path;
    todo!()
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
    let _ = (dir_gid, group, user);
    todo!()
}

/// The few calls into the account database.
mod account {
    use super::{GroupInfo, UserInfo};

    /// The user this process acts as.
    pub(super) fn euid() -> u32 {
        todo!()
    }

    /// Account entry of the effective user.
    pub(super) fn current_user() -> Option<UserInfo> {
        todo!()
    }

    /// Account entry of a group.
    pub(super) fn group(gid: u32) -> Option<GroupInfo> {
        let _ = gid;
        todo!()
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
    fn agrees_with_id_about_a_private_primary_group() {
        let user = account::current_user().expect("an account entry");
        let own = account::group(user.primary_gid);
        if !group_is_private(user.primary_gid, own.as_ref(), &user) {
            return;
        }
        let id = |flag: &str| std::process::Command::new("id").arg(flag).output();
        let (Ok(group), Ok(name)) = (id("-gn"), id("-un")) else {
            eprintln!("skipped: no id command");
            return;
        };
        assert_eq!(group.stdout, name.stdout);
    }
}
