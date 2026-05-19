use std::path::{Path, PathBuf};

/// Utilities for seeding the local kernel cache without running a full
/// Docker build — essential for air-gap / offline environments.
pub struct KernelImporter;

impl KernelImporter {
    /// Extracts the `KERNEL_SEG` from an existing `.rvf` appliance file and
    /// seeds the local kernel cache at the path derived from `cache_key`.
    ///
    /// TODO(Phase-5): implement once rvf-runtime segment extraction is wired in.
    pub async fn import_from_rvf(
        _source_rvf: &Path,
        _cache_key: &str,
    ) -> anyhow::Result<PathBuf> {
        anyhow::bail!(
            "not yet implemented — requires rvf-runtime KERNEL_SEG extraction (deferred to Phase 5)"
        )
    }

    /// Copies a raw `bzImage` file into the given `cache_dir`, creating it if
    /// needed.  Returns the path of the newly-placed `cache_dir/bzImage`.
    ///
    /// This is the primary entry-point for air-gap environments where a
    /// kernel binary is transferred out-of-band rather than built locally.
    pub async fn import_from_file_to(
        kernel_path: &Path,
        cache_dir: &Path,
    ) -> anyhow::Result<PathBuf> {
        tokio::fs::create_dir_all(cache_dir).await.map_err(|e| {
            anyhow::anyhow!(
                "failed to create cache directory {}: {}",
                cache_dir.display(),
                e
            )
        })?;

        let dest = cache_dir.join("bzImage");
        tokio::fs::copy(kernel_path, &dest).await.map_err(|e| {
            anyhow::anyhow!(
                "failed to copy kernel from {} to {}: {}",
                kernel_path.display(),
                dest.display(),
                e
            )
        })?;

        Ok(dest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_import_from_file_seeds_cache() {
        let dir = tempfile::tempdir().unwrap();
        let fake_kernel = dir.path().join("bzImage");
        std::fs::write(&fake_kernel, b"fake kernel content").unwrap();
        let cache_dir = dir.path().join("cache");
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(KernelImporter::import_from_file_to(&fake_kernel, &cache_dir))
            .unwrap();
        assert!(cache_dir.join("bzImage").exists());
        let content = std::fs::read(cache_dir.join("bzImage")).unwrap();
        assert_eq!(content, b"fake kernel content");
    }
}
