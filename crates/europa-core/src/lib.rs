//! Shared data types for Europa.
//!
//! The workspace skeleton exposes no types yet.

/// Placeholder to keep the skeleton testable.
#[must_use]
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_crate_version() {
        assert!(!version().is_empty());
    }
}
