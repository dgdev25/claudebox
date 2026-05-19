use std::path::PathBuf;

#[derive(Debug)]
pub struct LogEntry {
    pub ts: String,
    pub kind: String,
    pub data: serde_json::Value,
}

impl LogEntry {
    /// Format the entry for human display.
    ///
    /// Example output: `[09:01:14] CMD {"cmd":"cargo build"}`
    pub fn pretty_print(&self) -> String {
        // ts format is "2026-05-19T09:01:14Z" — skip 11 chars, take 8 for HH:MM:SS.
        let time: String = self.ts.chars().skip(11).take(8).collect();
        let data_compact = self.data.to_string();
        format!("[{time}] {} {data_compact}", self.kind)
    }
}

/// Reads real-time logs forwarded by `claudebox-logd` via a vsock UDS proxy.
pub struct VsockLogReader {
    pub uds_path: PathBuf,
}

impl VsockLogReader {
    /// Stream JSON-lines from the vsock UDS socket, pretty-printing each line
    /// to `writer` until the connection closes or an error occurs.
    pub async fn follow(
        &self,
        _writer: &mut (impl tokio::io::AsyncWrite + Unpin),
    ) -> anyhow::Result<()> {
        anyhow::bail!(
            "not yet implemented: requires vsock UDS at {:?} — available when VM is running",
            self.uds_path
        )
    }

    /// Read all log entries since `since`, returning them as a vec.
    pub async fn read_since(
        &self,
        _since: chrono::DateTime<chrono::Utc>,
    ) -> anyhow::Result<Vec<LogEntry>> {
        anyhow::bail!(
            "not yet implemented: requires vsock UDS at {:?} — available when VM is running",
            self.uds_path
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_log_entry_pretty_print() {
        let entry = LogEntry {
            ts: "2026-05-19T09:01:14Z".into(),
            kind: "CMD".into(),
            data: serde_json::json!({ "cmd": "cargo build" }),
        };
        let line = entry.pretty_print();
        assert!(line.contains("09:01:14"));
        assert!(line.contains("CMD"));
        assert!(line.contains("cargo build"));
    }
}
