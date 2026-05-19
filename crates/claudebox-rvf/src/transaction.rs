use std::path::{Path, PathBuf};

use anyhow::Result;

/// RAII guard for atomic appliance initialisation.
///
/// On construction, reserves a `.rvf.tmp` path. On [`commit`](InitTransaction::commit),
/// atomically renames it to the final `.rvf` path. If dropped without committing
/// (including during a panic), all temporary paths are removed best-effort.
pub struct InitTransaction {
    tmp_path: PathBuf,
    final_path: PathBuf,
    cleanup_paths: Vec<PathBuf>,
    committed: bool,
}

impl InitTransaction {
    /// Create a new transaction for `<dir>/<name>.rvf`, using `<dir>/<name>.rvf.tmp`
    /// as the staging path.
    pub fn new(name: &str, dir: &Path) -> Result<Self> {
        let tmp_path = dir.join(format!("{name}.rvf.tmp"));
        let final_path = dir.join(format!("{name}.rvf"));
        Ok(InitTransaction {
            tmp_path,
            final_path,
            cleanup_paths: Vec::new(),
            committed: false,
        })
    }

    /// Returns the temporary staging path. Write all content here before committing.
    pub fn tmp_path(&self) -> &Path {
        &self.tmp_path
    }

    /// Register an additional path that should be cleaned up on drop if the
    /// transaction is not committed.
    pub fn register_cleanup(&mut self, path: PathBuf) {
        self.cleanup_paths.push(path);
    }

    /// Atomically move the staging file to its final location.
    ///
    /// Uses `rename(2)` for same-filesystem atomicity. If `rename` fails with
    /// `EXDEV` (source and destination on different filesystems), falls back to
    /// `copy` + `remove` — this is not atomic but is the only option across
    /// filesystem boundaries.
    ///
    /// After this call succeeds, the `Drop` impl will not attempt cleanup.
    pub fn commit(mut self) -> Result<()> {
        if let Err(e) = std::fs::rename(&self.tmp_path, &self.final_path) {
            // EXDEV = 18 on Linux and macOS (POSIX): cross-device rename.
            if e.raw_os_error() == Some(18) {
                std::fs::copy(&self.tmp_path, &self.final_path)
                    .map_err(|ce| anyhow::anyhow!("cross-filesystem copy failed: {ce}"))?;
                std::fs::remove_file(&self.tmp_path)
                    .map_err(|re| anyhow::anyhow!("failed to remove tmp after cross-fs copy: {re}"))?;
            } else {
                return Err(e.into());
            }
        }
        self.committed = true;
        Ok(())
    }
}

impl Drop for InitTransaction {
    fn drop(&mut self) {
        if !self.committed {
            let _ = std::fs::remove_file(&self.tmp_path);
            for path in &self.cleanup_paths {
                let _ = std::fs::remove_file(path);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_commit_renames_tmp_to_final() {
        let dir = tempdir().unwrap();
        let tx = InitTransaction::new("myapp", dir.path()).unwrap();
        std::fs::write(tx.tmp_path(), b"content").unwrap();
        tx.commit().unwrap();
        assert!(dir.path().join("myapp.rvf").exists());
        assert!(!dir.path().join("myapp.rvf.tmp").exists());
    }

    #[test]
    fn test_drop_removes_tmp_on_failure() {
        let dir = tempdir().unwrap();
        let tmp_path;
        {
            let tx = InitTransaction::new("myapp", dir.path()).unwrap();
            tmp_path = tx.tmp_path().to_owned();
            std::fs::write(&tmp_path, b"partial").unwrap();
            // tx dropped here without commit
        }
        assert!(!tmp_path.exists());
    }

    #[test]
    fn test_commit_produces_final_file_with_correct_content() {
        let dir = tempdir().unwrap();
        let tx = InitTransaction::new("cross", dir.path()).unwrap();
        std::fs::write(tx.tmp_path(), b"payload").unwrap();
        tx.commit().unwrap();
        let final_path = dir.path().join("cross.rvf");
        assert!(final_path.exists());
        assert_eq!(std::fs::read(&final_path).unwrap(), b"payload");
    }

    #[test]
    fn test_drop_on_panic_cleans_up() {
        let dir = tempdir().unwrap();
        let tmp_path;
        {
            let tx = InitTransaction::new("myapp", dir.path()).unwrap();
            tmp_path = tx.tmp_path().to_owned();
            std::fs::write(&tmp_path, b"partial").unwrap();
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _tx = tx;
                panic!("simulated panic");
            }));
        }
        assert!(!tmp_path.exists());
    }
}
