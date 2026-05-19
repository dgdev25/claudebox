/// A structured log entry forwarded from inside the Firecracker VM to the host
/// via a vsock connection.
#[derive(serde::Serialize, serde::Deserialize, Debug)]
pub struct LogEntry {
    pub ts: String,
    #[serde(rename = "type")]
    pub kind: LogEntryKind,
    pub data: serde_json::Value,
}

/// The type of activity captured in a log entry.
#[derive(serde::Serialize, serde::Deserialize, Debug)]
pub enum LogEntryKind {
    /// Shell command executed (via PROMPT_COMMAND hook).
    #[serde(rename = "CMD")]
    Cmd,
    /// File-system write/delete event (via inotify on /workspace).
    #[serde(rename = "FILE")]
    File,
    /// Stdout/stderr output line from a running command.
    #[serde(rename = "STDOUT")]
    Stdout,
}

/// Entry point for the in-VM log daemon.
///
/// Full vsock implementation (connect to CID 2 port 9999, write inotify/PROMPT_COMMAND
/// events as JSON-lines) requires Linux VM context — stubbed here so host-side
/// tools can be built and tested without a running VM.
fn main() {
    println!("claudebox-logd starting");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cmd_log_entry_is_valid_json() {
        let entry = LogEntry {
            ts: "2026-05-19T09:01:14Z".into(),
            kind: LogEntryKind::Cmd,
            data: serde_json::json!({ "cmd": "cargo build --release", "pid": 1234 }),
        };
        let json = serde_json::to_string(&entry).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["type"].as_str().unwrap(), "CMD");
        assert!(v["data"]["cmd"].as_str().is_some());
    }

    #[test]
    fn test_file_log_entry_is_valid_json() {
        let entry = LogEntry {
            ts: "2026-05-19T09:01:22Z".into(),
            kind: LogEntryKind::File,
            data: serde_json::json!({ "path": "src/auth.rs", "op": "write", "bytes": 1248 }),
        };
        let json = serde_json::to_string(&entry).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["type"].as_str().unwrap(), "FILE");
    }
}
