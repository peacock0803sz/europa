//! Local kernel behind the client window.
//!
//! One spawned task owns the sockets, the child watcher, and the
//! [`KernelState`]. Every command passes [`admit`] first and moves state
//! only through [`next_state`].

use europa_core::{ProcessExit, SpawnErrorKind};

/// Tell a missing `ipykernel` apart from any other early exit.
pub(crate) fn classify(early: ProcessExit, stderr_tail: &str) -> SpawnErrorKind {
    if stderr_tail.contains("No module named ipykernel_launcher")
        || stderr_tail.contains("No module named 'ipykernel'")
    {
        SpawnErrorKind::IpykernelMissing
    } else {
        SpawnErrorKind::EarlyExit { exit: early }
    }
}

/// Whether a failure is worth one more spawn with fresh ports.
pub(crate) fn should_retry(kind: &SpawnErrorKind, stderr_tail: &str) -> bool {
    match kind {
        SpawnErrorKind::EarlyExit { .. } => stderr_tail.contains("Address already in use"),
        SpawnErrorKind::HandshakeTimeout { .. } => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tells_missing_ipykernel_apart() {
        assert_eq!(
            classify(
                ProcessExit::Code(1),
                "Traceback: No module named ipykernel_launcher\n"
            ),
            SpawnErrorKind::IpykernelMissing
        );
        assert_eq!(
            classify(ProcessExit::Code(1), "No module named 'ipykernel'\n"),
            SpawnErrorKind::IpykernelMissing
        );
        assert_eq!(
            classify(ProcessExit::Code(1), "SyntaxError: bad\n"),
            SpawnErrorKind::EarlyExit {
                exit: ProcessExit::Code(1),
            }
        );
        assert_eq!(
            classify(ProcessExit::Signal(9), ""),
            SpawnErrorKind::EarlyExit {
                exit: ProcessExit::Signal(9),
            }
        );
    }

    #[test]
    fn retries_only_port_conflicts_and_timeouts() {
        assert!(should_retry(
            &SpawnErrorKind::EarlyExit {
                exit: ProcessExit::Code(1),
            },
            "Address already in use\n",
        ));
        assert!(should_retry(
            &SpawnErrorKind::HandshakeTimeout {
                after: std::time::Duration::from_secs(30),
            },
            "",
        ));
        assert!(!should_retry(&SpawnErrorKind::InterpreterNotFound, "",));
        assert!(!should_retry(&SpawnErrorKind::IpykernelMissing, "",));
        assert!(!should_retry(
            &SpawnErrorKind::EarlyExit {
                exit: ProcessExit::Code(1),
            },
            "SyntaxError: bad\n",
        ));
        assert!(!should_retry(
            &SpawnErrorKind::ConnectFailed {
                message: "refused".to_owned(),
            },
            "",
        ));
    }
}
