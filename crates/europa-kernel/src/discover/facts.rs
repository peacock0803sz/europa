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
