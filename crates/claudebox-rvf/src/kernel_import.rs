use std::path::{Path, PathBuf};

use rvf_runtime::RvfStore;
use rvf_types::KernelHeader;

/// Utilities for seeding the local kernel cache without running a full
/// Docker build — essential for air-gap / offline environments.
pub struct KernelImporter;

impl KernelImporter {
    /// Extract the KERNEL_SEG image bytes from `source_rvf` and seed the
    /// local kernel cache under `~/.claudebox/kernels/<cache_key>/kernel`.
    ///
    /// `cache_key` is typically an architecture string (e.g. `"x86_64"`,
    /// `"aarch64"`) so multiple kernels can coexist on a dev machine.
    /// Returns the path of the seeded `kernel` file.
    pub async fn import_from_rvf(
        source_rvf: &Path,
        cache_key: &str,
    ) -> anyhow::Result<PathBuf> {
        let kernel_image = extract_kernel_image_bytes(source_rvf)?;

        let cache_dir = claudebox_core::setup::data_dir()
            .join("kernels")
            .join(cache_key);
        Self::write_to_cache(&cache_dir, &kernel_image).await
    }

    /// Like `import_from_rvf`, but writes the cached `kernel` file under
    /// the caller-supplied `cache_dir` instead of `~/.claudebox/kernels/...`.
    ///
    /// Provided so tests can target a tempdir without touching the user's
    /// home directory.
    pub async fn import_from_rvf_to(
        source_rvf: &Path,
        cache_dir: &Path,
    ) -> anyhow::Result<PathBuf> {
        let kernel_image = extract_kernel_image_bytes(source_rvf)?;
        Self::write_to_cache(cache_dir, &kernel_image).await
    }

    async fn write_to_cache(cache_dir: &Path, kernel_image: &[u8]) -> anyhow::Result<PathBuf> {
        tokio::fs::create_dir_all(cache_dir).await.map_err(|e| {
            anyhow::anyhow!("failed to create cache dir {}: {e}", cache_dir.display())
        })?;
        let dest = cache_dir.join("kernel");
        tokio::fs::write(&dest, kernel_image).await.map_err(|e| {
            anyhow::anyhow!("failed to write kernel to {}: {e}", dest.display())
        })?;
        Ok(dest)
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

/// Open `source_rvf` and decode its KERNEL_SEG, returning just the
/// kernel image bytes (no header, no cmdline).
fn extract_kernel_image_bytes(source_rvf: &Path) -> anyhow::Result<Vec<u8>> {
    let store = RvfStore::open_readonly(source_rvf)
        .map_err(|e| anyhow::anyhow!("open_readonly {} failed: {e:?}", source_rvf.display()))?;

    let (hdr_bytes, remainder) = store
        .extract_kernel()
        .map_err(|e| anyhow::anyhow!("extract_kernel failed: {e:?}"))?
        .ok_or_else(|| anyhow::anyhow!("no KERNEL_SEG in {}", source_rvf.display()))?;

    anyhow::ensure!(hdr_bytes.len() == 128, "kernel header must be 128 bytes");
    let mut hdr_array = [0u8; 128];
    hdr_array.copy_from_slice(&hdr_bytes);
    let header = KernelHeader::from_bytes(&hdr_array)
        .map_err(|e| anyhow::anyhow!("invalid KernelHeader: {e:?}"))?;

    let image_size = header.image_size as usize;
    anyhow::ensure!(
        remainder.len() >= image_size,
        "kernel payload truncated: {} < {}",
        remainder.len(),
        image_size
    );
    Ok(remainder[..image_size].to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rvf_runtime::options::RvfOptions;

    fn make_rvf_with_kernel(path: &Path, kernel_bytes: &[u8]) {
        let opts = RvfOptions { dimension: 1, ..Default::default() };
        let mut store = RvfStore::create(path, opts).unwrap();
        store
            .embed_kernel(0x01, 0x01, 0, kernel_bytes, 2222, Some("{}"))
            .unwrap();
        store.close().unwrap();
    }

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

    #[tokio::test]
    async fn test_import_from_rvf_to_extracts_kernel_image_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let rvf = dir.path().join("src.rvf");
        let original = b"the-real-kernel-payload";
        make_rvf_with_kernel(&rvf, original);

        let cache_dir = dir.path().join("cache");
        let dest = KernelImporter::import_from_rvf_to(&rvf, &cache_dir).await.unwrap();
        assert_eq!(dest, cache_dir.join("kernel"));
        assert_eq!(std::fs::read(&dest).unwrap(), original);
    }

    #[tokio::test]
    async fn test_import_from_rvf_to_missing_kernel_seg_errors() {
        let dir = tempfile::tempdir().unwrap();
        let rvf = dir.path().join("empty.rvf");
        let opts = RvfOptions { dimension: 1, ..Default::default() };
        let store = RvfStore::create(&rvf, opts).unwrap();
        store.close().unwrap();

        let result = KernelImporter::import_from_rvf_to(&rvf, &dir.path().join("cache")).await;
        let err = result.unwrap_err().to_string();
        assert!(err.contains("no KERNEL_SEG"), "got error: {err}");
    }

    #[tokio::test]
    async fn test_import_from_rvf_to_missing_source_errors() {
        let dir = tempfile::tempdir().unwrap();
        let result = KernelImporter::import_from_rvf_to(
            &dir.path().join("does-not-exist.rvf"),
            &dir.path().join("cache"),
        )
        .await;
        assert!(result.is_err());
    }
}
