use std::path::{Path, PathBuf};

use anyhow::Context;
use tokio::process::Command;

/// Thin async wrapper around the `rvf` CLI binary.
pub struct ClaudeboxRvfCli {
    pub binary: PathBuf,
}

/// Result type returned by [`ClaudeboxRvfCli::verify_witness`].
pub struct WitnessVerifyResult {
    pub is_valid: bool,
    pub message: String,
}

/// Result type returned by [`ClaudeboxRvfCli::inspect`].
pub struct RvfInspectResult {
    pub segments: Vec<String>,
}

impl ClaudeboxRvfCli {
    /// Derive a new appliance from a source path to an output path.
    pub async fn derive(&self, source: &Path, output: &Path) -> anyhow::Result<()> {
        let status = Command::new(&self.binary)
            .arg("derive")
            .arg(source)
            .arg(output)
            .status()
            .await
            .with_context(|| format!("failed to run {:?} derive", self.binary))?;
        anyhow::ensure!(status.success(), "rvf derive failed with status {status}");
        Ok(())
    }

    /// Verify the witness chain of an RVF file.
    pub async fn verify_witness(&self, rvf: &Path) -> anyhow::Result<WitnessVerifyResult> {
        let output = Command::new(&self.binary)
            .arg("verify-witness")
            .arg(rvf)
            .output()
            .await
            .with_context(|| format!("failed to run {:?} verify-witness", self.binary))?;
        let message = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok(WitnessVerifyResult {
            is_valid: output.status.success(),
            message,
        })
    }

    /// Inspect the segments of an RVF file.
    pub async fn inspect(&self, rvf: &Path) -> anyhow::Result<RvfInspectResult> {
        let output = Command::new(&self.binary)
            .arg("inspect")
            .arg(rvf)
            .output()
            .await
            .with_context(|| format!("failed to run {:?} inspect", self.binary))?;
        anyhow::ensure!(
            output.status.success(),
            "rvf inspect failed with status {} for {:?}",
            output.status,
            rvf
        );
        let stdout = String::from_utf8_lossy(&output.stdout);
        let segments = stdout
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect();
        Ok(RvfInspectResult { segments })
    }

    /// Compact an RVF file in-place.
    pub async fn compact(&self, rvf: &Path) -> anyhow::Result<()> {
        let status = Command::new(&self.binary)
            .arg("compact")
            .arg(rvf)
            .status()
            .await
            .with_context(|| format!("failed to run {:?} compact", self.binary))?;
        anyhow::ensure!(status.success(), "rvf compact failed with status {status}");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rvf_cli_returns_error_for_nonexistent_file() {
        let cli = ClaudeboxRvfCli {
            binary: "rvf".into(),
        };
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(cli.inspect(Path::new("/tmp/does-not-exist.rvf")));
        assert!(result.is_err());
    }
}
