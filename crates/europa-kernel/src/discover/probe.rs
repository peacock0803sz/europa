//! One probe: does an interpreter import `ipykernel`?

use std::path::Path;
use std::time::Duration;

use europa_core::ProbeOutcome;

use crate::registry::Registry;

/// Start `python` once and report whether it imports `ipykernel`.
///
/// The child is listed in `registry` while it runs, and it is stopped when it
/// outlives `timeout` or when the future is dropped.
pub(crate) async fn probe(python: &Path, timeout: Duration, registry: &Registry) -> ProbeOutcome {
    let _ = (python, timeout, registry);
    todo!()
}

/// [`probe`] with a hook between the start of the child and its registration.
///
/// Tests use the hook to run an emergency stop while the child is not listed
/// yet.
pub(crate) async fn probe_with(
    python: &Path,
    timeout: Duration,
    registry: &Registry,
    before_register: impl FnOnce(),
) -> ProbeOutcome {
    let _ = (python, timeout, registry, before_register);
    todo!()
}
