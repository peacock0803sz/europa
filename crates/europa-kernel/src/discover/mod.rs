//! Search for a Python that can import `ipykernel`.
//!
//! The work is split in three parts. `facts` reads the file system and the
//! environment without starting any Python. `plan` turns those facts into the
//! ordered candidates, without IO. `probe` starts one interpreter and reports
//! whether it imports `ipykernel`. The core [`DiscoveryFold`] joins them: it
//! walks the candidates and asks for one probe at a time.

use europa_core::{Discovery, DiscoveryError, DiscoveryFold, DiscoveryRequest, FoldNext};

use crate::registry::GLOBAL;

mod facts;
mod plan;
mod probe;

/// Find the Python that runs the kernel of a notebook.
///
/// A set `kernel.python` is the only candidate. Otherwise the search tries the
/// virtual environments beside the notebook and up to its repository root,
/// then `VIRTUAL_ENV`, `CONDA_PREFIX`, and `PATH`, in the order of
/// [`europa_core::PythonSource`]. The first interpreter that imports
/// `ipykernel` wins. Nothing is installed or created, and `request` is only
/// read.
///
/// # When to call
///
/// Call it only while no search has succeeded yet, and only when the user asks
/// to run a cell or to start a kernel: opening a notebook must not run
/// anything that lies beside it. Keep a successful result, also across
/// restarts of the kernel. After a failure, search again on the next such
/// request.
///
/// A UI passes [`europa_core::DiscoveryEnv::from_vars`] of
/// `std::env::vars_os()` and the absolute path of the notebook.
///
/// # Stopping
///
/// Call it inside a tokio runtime. Dropping the future stops the probe that is
/// running. After [`kill_all_kernels`](crate::kill_all_kernels) no further
/// probe starts.
///
/// # Errors
///
/// Returns why no interpreter can be used; the
/// [`Display`](std::fmt::Display) of the error is meant for the user.
// A failure carries the report of every candidate and is built once per search,
// so its size costs nothing that boxing the public type would be worth.
#[allow(clippy::result_large_err)]
pub async fn discover_python(request: &DiscoveryRequest) -> Result<Discovery, DiscoveryError> {
    let mut fold = DiscoveryFold::new(plan::plan(facts::gather(request)));
    loop {
        match fold.advance() {
            FoldNext::Probe(python) => {
                let outcome = probe::probe(&python, request.probe_timeout, &GLOBAL).await;
                fold.record(outcome);
            }
            FoldNext::Done(result) => return result,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use europa_core::DiscoveryEnv;

    #[test]
    fn search_future_is_send() {
        fn assert_send<T: Send>(_: T) {}
        let request = DiscoveryRequest::new("/a.ipynb", None, DiscoveryEnv::default());
        assert_send(discover_python(&request));
    }
}
