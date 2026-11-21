//! Free ports and the connection file handed to the kernel.
//!
//! Ports are picked by binding `127.0.0.1:0` five times at once, then the
//! file is written with 0600 permissions and removed after shutdown.

use std::io;
use std::path::Path;
use tempfile::TempPath;

/// Five ports for the kernel sockets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Ports {
    /// Shell socket port.
    pub(crate) shell: u16,
    /// Iopub socket port.
    pub(crate) iopub: u16,
    /// Stdin socket port.
    pub(crate) stdin: u16,
    /// Control socket port.
    pub(crate) control: u16,
    /// Heartbeat socket port.
    pub(crate) hb: u16,
}

/// Pick five free ports on loopback.
pub(crate) fn pick_ports() -> io::Result<Ports> {
    todo!()
}

/// Make a random 64 character lowercase hex key.
pub(crate) fn new_key() -> String {
    todo!()
}

/// Connection file written for one spawn, removed afterwards.
#[derive(Debug)]
pub(crate) struct ConnectionFile {
    /// Temporary file path.
    path: TempPath,
    /// Ports written into the file.
    ports: Ports,
    /// Key written into the file.
    key: String,
}

impl ConnectionFile {
    /// Write the connection file for these ports and key.
    pub(crate) fn create(ports: Ports, key: String, kernel_name: &str) -> io::Result<Self> {
        let _ = (ports, key, kernel_name);
        todo!()
    }

    /// Borrow the file path.
    pub(crate) fn path(&self) -> &Path {
        todo!()
    }

    /// Remove the file.
    pub(crate) fn remove(self) -> io::Result<()> {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn picks_distinct_nonzero_ports() {
        let ports = pick_ports().expect("picks ports");
        let all = [
            ports.shell,
            ports.iopub,
            ports.stdin,
            ports.control,
            ports.hb,
        ];
        assert!(all.iter().all(|port| *port != 0));
        let mut sorted = all.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), 5);
    }

    #[test]
    fn makes_hex_keys() {
        let first = new_key();
        let second = new_key();
        for key in [&first, &second] {
            assert_eq!(key.len(), 64);
            assert!(key.bytes().all(|byte| byte.is_ascii_hexdigit()));
            assert_eq!(key.to_lowercase(), *key);
        }
        assert_ne!(first, second);
    }

    #[test]
    fn writes_and_removes_the_file() {
        let ports = Ports {
            shell: 11111,
            iopub: 22222,
            stdin: 33333,
            control: 44444,
            hb: 55555,
        };
        let key = "k".repeat(64);
        let file = ConnectionFile::create(ports, key.clone(), "python3").expect("writes");
        let path = file.path().to_path_buf();
        assert_eq!(
            path.parent().expect("parent"),
            std::env::temp_dir().as_path()
        );
        let name = path.file_name().expect("name").to_str().expect("utf8");
        assert!(name.starts_with("europa-kernel-"));
        assert!(name.ends_with(".json"));
        #[cfg(unix)]
        {
            let mode = std::fs::metadata(&path)
                .expect("metadata")
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o600);
        }
        let text = std::fs::read_to_string(&path).expect("reads");
        let value: serde_json::Value = serde_json::from_str(&text).expect("json");
        assert_eq!(value["ip"], serde_json::json!("127.0.0.1"));
        assert_eq!(value["transport"], serde_json::json!("tcp"));
        assert_eq!(value["signature_scheme"], serde_json::json!("hmac-sha256"));
        assert_eq!(value["key"], serde_json::json!(key));
        assert_eq!(value["kernel_name"], serde_json::json!("python3"));
        assert_eq!(value["shell_port"], serde_json::json!(11111));
        assert_eq!(value["iopub_port"], serde_json::json!(22222));
        assert_eq!(value["stdin_port"], serde_json::json!(33333));
        assert_eq!(value["control_port"], serde_json::json!(44444));
        assert_eq!(value["hb_port"], serde_json::json!(55555));
        file.remove().expect("removes");
        assert!(!path.exists());
    }
}
