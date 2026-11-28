//! Search for a Python that can import `ipykernel`.
//!
//! The work is split in three parts. `facts` reads the file system and the
//! environment without starting any Python. `plan` turns those facts into the
//! ordered candidates, without IO. `probe` starts one interpreter and reports
//! whether it imports `ipykernel`. The core `DiscoveryFold` joins them: it
//! walks the candidates and asks for one probe at a time.

mod plan;
