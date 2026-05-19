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
/// This implementation is intentionally minimal and portable: it reads
/// newline-delimited records from stdin and forwards canonical JSON log lines
/// to stdout. Guest-side integration (inotify + shell hooks + vsock bridge)
/// wires into this process by feeding stdin.
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let stdin = tokio::io::stdin();
    let mut lines = BufReader::new(stdin).lines();
    let mut stdout = tokio::io::stdout();

    while let Some(line) = lines.next_line().await? {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let entry = if let Ok(v) = serde_json::from_str::<serde_json::Value>(trimmed) {
            if v.get("ts").is_some() && v.get("type").is_some() && v.get("data").is_some() {
                v
            } else {
                serde_json::json!({
                    "ts": chrono::Utc::now().to_rfc3339(),
                    "type": "STDOUT",
                    "data": { "line": trimmed }
                })
            }
        } else {
            serde_json::json!({
                "ts": chrono::Utc::now().to_rfc3339(),
                "type": "STDOUT",
                "data": { "line": trimmed }
            })
        };

        stdout
            .write_all(entry.to_string().as_bytes())
            .await?;
        stdout.write_all(b"\n").await?;
        stdout.flush().await?;
    }

    Ok(())
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
