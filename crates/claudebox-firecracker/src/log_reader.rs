use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

#[derive(Debug, Serialize, Deserialize)]
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
///
/// The host-side bridge exposes a Unix domain socket at `uds_path` that
/// forwards newline-delimited JSON records from the in-VM daemon. Records
/// must deserialise to [`LogEntry`]; lines that fail to parse are skipped
/// with a `tracing::warn` so a single malformed line does not abort the
/// stream.
pub struct VsockLogReader {
    pub uds_path: PathBuf,
}

impl VsockLogReader {
    /// Stream entries from the UDS socket and pretty-print each one to
    /// `writer` until the connection closes.
    pub async fn follow(
        &self,
        writer: &mut (impl AsyncWrite + Unpin),
    ) -> anyhow::Result<()> {
        let stream = UnixStream::connect(&self.uds_path).await.map_err(|e| {
            anyhow::anyhow!(
                "failed to connect to vsock UDS at {}: {e}",
                self.uds_path.display()
            )
        })?;
        let mut lines = BufReader::new(stream).lines();
        while let Some(line) = lines
            .next_line()
            .await
            .map_err(|e| anyhow::anyhow!("vsock read error: {e}"))?
        {
            let Some(entry) = parse_entry_or_warn(&line) else { continue };
            writer
                .write_all(entry.pretty_print().as_bytes())
                .await
                .map_err(|e| anyhow::anyhow!("write failed: {e}"))?;
            writer
                .write_all(b"\n")
                .await
                .map_err(|e| anyhow::anyhow!("write failed: {e}"))?;
            writer
                .flush()
                .await
                .map_err(|e| anyhow::anyhow!("flush failed: {e}"))?;
        }
        Ok(())
    }

    /// Read everything currently available on the UDS socket, parse it
    /// into `LogEntry`s, and return only entries whose RFC3339 `ts` is
    /// later than `since`.
    ///
    /// The host bridge is responsible for replaying its in-memory buffer
    /// on connect; this method drains until EOF and then returns.
    pub async fn read_since(
        &self,
        since: chrono::DateTime<chrono::Utc>,
    ) -> anyhow::Result<Vec<LogEntry>> {
        let stream = UnixStream::connect(&self.uds_path).await.map_err(|e| {
            anyhow::anyhow!(
                "failed to connect to vsock UDS at {}: {e}",
                self.uds_path.display()
            )
        })?;
        let mut lines = BufReader::new(stream).lines();
        let mut out = Vec::new();
        while let Some(line) = lines
            .next_line()
            .await
            .map_err(|e| anyhow::anyhow!("vsock read error: {e}"))?
        {
            let Some(entry) = parse_entry_or_warn(&line) else { continue };
            match chrono::DateTime::parse_from_rfc3339(&entry.ts) {
                Ok(ts) if ts.with_timezone(&chrono::Utc) > since => out.push(entry),
                Ok(_) => {}
                Err(e) => {
                    tracing::warn!(
                        ts = %entry.ts,
                        error = %e,
                        "log entry has malformed timestamp; skipping"
                    );
                }
            }
        }
        Ok(out)
    }
}

fn parse_entry_or_warn(line: &str) -> Option<LogEntry> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    match serde_json::from_str::<LogEntry>(line) {
        Ok(e) => Some(e),
        Err(err) => {
            tracing::warn!(error = %err, "skipping malformed log line");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncWriteExt;
    use tokio::net::UnixListener;

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

    /// Spawn a one-shot UDS server that writes `payload` to the first
    /// client and then closes the write half so the client sees EOF.
    async fn spawn_uds_server(socket: PathBuf, payload: &'static str) {
        let listener = UnixListener::bind(&socket).unwrap();
        tokio::spawn(async move {
            if let Ok((mut sock, _)) = listener.accept().await {
                let _ = sock.write_all(payload.as_bytes()).await;
                let _ = sock.shutdown().await;
            }
        });
        tokio::task::yield_now().await;
    }

    #[tokio::test]
    async fn test_follow_pretty_prints_each_line() {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("logs.sock");
        spawn_uds_server(
            socket.clone(),
            "{\"ts\":\"2026-05-19T09:01:14Z\",\"kind\":\"CMD\",\"data\":{\"cmd\":\"ls\"}}\n\
             {\"ts\":\"2026-05-19T09:01:15Z\",\"kind\":\"NET\",\"data\":{\"host\":\"x\"}}\n",
        )
        .await;

        let reader = VsockLogReader { uds_path: socket };
        let mut out: Vec<u8> = Vec::new();
        reader.follow(&mut out).await.unwrap();
        let s = String::from_utf8(out).unwrap();
        assert!(s.contains("CMD"));
        assert!(s.contains("NET"));
        assert!(s.contains("09:01:14"));
        assert!(s.contains("09:01:15"));
    }

    #[tokio::test]
    async fn test_follow_skips_malformed_lines() {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("logs.sock");
        spawn_uds_server(
            socket.clone(),
            "not-json\n\
             {\"ts\":\"2026-05-19T09:01:14Z\",\"kind\":\"CMD\",\"data\":{\"x\":1}}\n",
        )
        .await;

        let reader = VsockLogReader { uds_path: socket };
        let mut out: Vec<u8> = Vec::new();
        reader.follow(&mut out).await.unwrap();
        let s = String::from_utf8(out).unwrap();
        assert!(s.contains("CMD"));
        assert_eq!(s.lines().count(), 1, "malformed line must be dropped");
    }

    #[tokio::test]
    async fn test_read_since_filters_by_timestamp() {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("logs.sock");
        spawn_uds_server(
            socket.clone(),
            "{\"ts\":\"2026-05-19T09:00:00Z\",\"kind\":\"OLD\",\"data\":{}}\n\
             {\"ts\":\"2026-05-19T10:00:00Z\",\"kind\":\"NEW\",\"data\":{}}\n",
        )
        .await;

        let reader = VsockLogReader { uds_path: socket };
        let cutoff: chrono::DateTime<chrono::Utc> =
            chrono::DateTime::parse_from_rfc3339("2026-05-19T09:30:00Z")
                .unwrap()
                .into();
        let entries = reader.read_since(cutoff).await.unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].kind, "NEW");
    }

    #[tokio::test]
    async fn test_follow_returns_error_when_socket_missing() {
        let dir = tempfile::tempdir().unwrap();
        let reader = VsockLogReader { uds_path: dir.path().join("never-bound.sock") };
        let mut out: Vec<u8> = Vec::new();
        assert!(reader.follow(&mut out).await.is_err());
    }
}
