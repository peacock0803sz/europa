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
