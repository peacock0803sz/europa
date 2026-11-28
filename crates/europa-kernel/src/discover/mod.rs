//! Search for a Python that can import `ipykernel`.
//!
//! The work is split in three parts. `facts` reads the file system and the
//! environment without starting any Python. `plan` turns those facts into the
//! ordered candidates, without IO. `probe` starts one interpreter and reports
//! whether it imports `ipykernel`. The core `DiscoveryFold` joins them: it
//! walks the candidates and asks for one probe at a time.

use europa_core::{Discovery, DiscoveryError, DiscoveryRequest};

mod facts;
mod plan;
mod probe;

/// Find the Python that runs the kernel of a notebook.
// A failure carries the report of every candidate and is built once per search,
// so its size costs nothing that boxing the public type would be worth.
#[allow(clippy::result_large_err)]
pub async fn discover_python(request: &DiscoveryRequest) -> Result<Discovery, DiscoveryError> {
    let _ = request;
    todo!()
}
