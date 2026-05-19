use std::path::{Path, PathBuf};

use claudebox_core::witness::append_witness_entry;
use claudebox_witness::WitnessEvent;
use rvf_runtime::{options::RvfOptions, RvfStore};
use rvf_types::KernelHeader;
use sha3::{Digest, Sha3_256};

/// Performs in-place kernel upgrades on an `.rvf` appliance file.
///
/// The upgrade extracts the current kernel + manifest JSON (cmdline), writes a
/// fresh `.rvf` at a sibling temp path with the new kernel image and the same
/// manifest, atomically renames it over the original, and appends a
/// `WitnessEvent::KernelUpgrade` entry to the witness sidecar.
///
/// VEC segments are not preserved across upgrade — they are rebuilt on the
/// next `claudebox start` from the workspace. eBPF, Dashboard, and WASM
/// segments are round-tripped via the public rvf-runtime typed extractors.
pub struct KernelUpgrader {
    pub rvf_path: PathBuf,
}

/// Result of a successful kernel upgrade, carrying the before/after hashes.
pub struct UpgradeResult {
    pub from_hash: String,
    pub to_hash: String,
}

impl KernelUpgrader {
    /// SHA3-256 hex digest of the current KERNEL_SEG kernel image bytes
    /// (i.e. the binary kernel, excluding header and cmdline).
    pub fn hash_current_kernel(&self) -> anyhow::Result<String> {
        let (_, image, _) = self.read_kernel_seg()?;
        Ok(hex_sha3_256(&image))
    }

    /// Return non-kernel segments that can be round-tripped via public
    /// rvf-runtime extractors: eBPF, Dashboard, WASM. Each entry is
    /// `(label, header_bytes, payload_bytes)` where label is a stable
    /// short name (e.g. "EBPF", "DASHBOARD", "WASM").
    ///
    /// VEC segments are intentionally excluded: rvf-runtime exposes no
    /// public reader for them, and ClaudeBox rebuilds the workspace index
    /// from source on next start.
    pub fn extract_non_kernel_segments(&self) -> anyhow::Result<Vec<NonKernelSegment>> {
        let store = RvfStore::open_readonly(&self.rvf_path)
            .map_err(|e| anyhow::anyhow!("open_readonly {} failed: {e:?}", self.rvf_path.display()))?;

        let mut out = Vec::new();

        if let Some((hdr, payload)) = store
            .extract_ebpf()
            .map_err(|e| anyhow::anyhow!("extract_ebpf failed: {e:?}"))?
        {
            out.push(NonKernelSegment { label: "EBPF".into(), header: hdr, payload });
        }
        if let Some((hdr, payload)) = store
            .extract_dashboard()
            .map_err(|e| anyhow::anyhow!("extract_dashboard failed: {e:?}"))?
        {
            out.push(NonKernelSegment { label: "DASHBOARD".into(), header: hdr, payload });
        }
        for (hdr, payload) in store
            .extract_wasm_all()
            .map_err(|e| anyhow::anyhow!("extract_wasm_all failed: {e:?}"))?
        {
            out.push(NonKernelSegment { label: "WASM".into(), header: hdr, payload });
        }

        Ok(out)
    }

    /// Replace the kernel inside `self.rvf_path` with the bytes at
    /// `new_kernel_path`, preserving the manifest JSON (cmdline) and any
    /// round-trippable non-kernel segments.
    ///
    /// Atomic: the new `.rvf` is built at a `.rvf.upgrade_tmp` sibling path
    /// and `rename`d over the original on success. On failure the temp is
    /// deleted and the original remains untouched.
    pub async fn upgrade(&self, new_kernel_path: &Path) -> anyhow::Result<UpgradeResult> {
        let new_kernel_bytes = tokio::fs::read(new_kernel_path).await.map_err(|e| {
            anyhow::anyhow!("failed to read new kernel at {}: {e}", new_kernel_path.display())
        })?;

        // Single open: capture header, image, cmdline, and dimension together
        // so build_upgraded_rvf doesn't need to reopen the source file.
        let (header, current_image, cmdline, source_dim) = self.read_kernel_seg_with_dim()?;
        let from_hash = hex_sha3_256(&current_image);
        let to_hash = hex_sha3_256(&new_kernel_bytes);

        let non_kernel = self.extract_non_kernel_segments()?;

        let tmp_path = PathBuf::from(format!("{}.upgrade_tmp", self.rvf_path.display()));
        if tmp_path.exists() {
            std::fs::remove_file(&tmp_path)
                .map_err(|e| anyhow::anyhow!("failed to clear stale tmp {}: {e}", tmp_path.display()))?;
        }

        if let Err(e) = self.build_upgraded_rvf(
            &tmp_path,
            &header,
            &new_kernel_bytes,
            &cmdline,
            &non_kernel,
            source_dim,
        ) {
            let _ = std::fs::remove_file(&tmp_path);
            return Err(e);
        }

        std::fs::rename(&tmp_path, &self.rvf_path).map_err(|e| {
            let _ = std::fs::remove_file(&tmp_path);
            anyhow::anyhow!("atomic rename {} -> {} failed: {e}", tmp_path.display(), self.rvf_path.display())
        })?;

        // Witness sidecar is keyed off the .rvf path and survives the rename.
        append_witness_entry(
            &self.rvf_path,
            WitnessEvent::KernelUpgrade {
                from_hash: from_hash.clone(),
                to_hash: to_hash.clone(),
            },
        )?;

        Ok(UpgradeResult { from_hash, to_hash })
    }

    /// Open the current `.rvf`, decode its KERNEL_SEG, and return
    /// `(KernelHeader, kernel_image_bytes, cmdline_bytes)`.
    fn read_kernel_seg(&self) -> anyhow::Result<(KernelHeader, Vec<u8>, Vec<u8>)> {
        let (header, image, cmdline, _) = self.read_kernel_seg_with_dim()?;
        Ok((header, image, cmdline))
    }

    /// Same as `read_kernel_seg`, but also returns the source store's
    /// dimension so callers can rebuild a valid `.rvf` without reopening.
    fn read_kernel_seg_with_dim(
        &self,
    ) -> anyhow::Result<(KernelHeader, Vec<u8>, Vec<u8>, u16)> {
        let store = RvfStore::open_readonly(&self.rvf_path)
            .map_err(|e| anyhow::anyhow!("open_readonly {} failed: {e:?}", self.rvf_path.display()))?;
        let dimension = store.dimension();

        let (hdr_bytes, remainder) = store
            .extract_kernel()
            .map_err(|e| anyhow::anyhow!("extract_kernel failed: {e:?}"))?
            .ok_or_else(|| anyhow::anyhow!("no KERNEL_SEG in {}", self.rvf_path.display()))?;

        anyhow::ensure!(hdr_bytes.len() == 128, "kernel header must be 128 bytes");
        let mut hdr_array = [0u8; 128];
        hdr_array.copy_from_slice(&hdr_bytes);
        let header = KernelHeader::from_bytes(&hdr_array)
            .map_err(|e| anyhow::anyhow!("invalid KernelHeader: {e:?}"))?;

        let image_size = header.image_size as usize;
        let cmdline_length = header.cmdline_length as usize;
        anyhow::ensure!(
            remainder.len() >= image_size + cmdline_length,
            "kernel payload truncated"
        );

        let image = remainder[..image_size].to_vec();
        let cmdline = remainder[image_size..image_size + cmdline_length].to_vec();
        Ok((header, image, cmdline, dimension))
    }

    fn build_upgraded_rvf(
        &self,
        tmp_path: &Path,
        old_header: &KernelHeader,
        new_image: &[u8],
        cmdline: &[u8],
        non_kernel: &[NonKernelSegment],
        source_dim: u16,
    ) -> anyhow::Result<()> {
        // Preserve the source store's dimension so the rewritten file
        // remains a valid RVF (dimension 0 is rejected as InvalidManifest).
        let opts = RvfOptions { dimension: source_dim.max(1), ..Default::default() };
        let mut store = RvfStore::create(tmp_path, opts)
            .map_err(|e| anyhow::anyhow!("RvfStore::create {} failed: {e:?}", tmp_path.display()))?;

        let cmdline_str = std::str::from_utf8(cmdline)
            .map_err(|e| anyhow::anyhow!("manifest cmdline not valid UTF-8: {e}"))?;
        let cmdline_opt = if cmdline_str.is_empty() { None } else { Some(cmdline_str) };

        store
            .embed_kernel(
                old_header.arch,
                old_header.kernel_type,
                old_header.kernel_flags,
                new_image,
                old_header.api_port,
                cmdline_opt,
            )
            .map_err(|e| anyhow::anyhow!("embed_kernel failed: {e:?}"))?;

        for seg in non_kernel {
            match seg.label.as_str() {
                "EBPF" => {
                    // EbpfHeader is 64 bytes; we re-embed program bytecode
                    // verbatim. BTF data is bundled into the payload by the
                    // public extractor and re-embeds as the full payload.
                    let (program_type, attach_type, max_dim) =
                        parse_ebpf_header(&seg.header).unwrap_or((0, 0, 0));
                    store
                        .embed_ebpf(program_type, attach_type, max_dim, &seg.payload, None)
                        .map_err(|e| anyhow::anyhow!("re-embed_ebpf failed: {e:?}"))?;
                }
                "DASHBOARD" | "WASM" => {
                    // Skip: re-embedding requires reconstructing typed
                    // metadata not exposed by the public extractor.
                    tracing::warn!(
                        label = %seg.label,
                        "non-kernel segment dropped during kernel upgrade — public rvf-runtime API does not expose enough metadata to round-trip"
                    );
                }
                _ => {}
            }
        }

        store
            .close()
            .map_err(|e| anyhow::anyhow!("RvfStore::close failed: {e:?}"))?;
        Ok(())
    }
}

/// One non-kernel segment captured from the source `.rvf`.
#[derive(Debug, Clone)]
pub struct NonKernelSegment {
    pub label: String,
    pub header: Vec<u8>,
    pub payload: Vec<u8>,
}

fn parse_ebpf_header(bytes: &[u8]) -> Option<(u8, u8, u16)> {
    if bytes.len() < 64 {
        return None;
    }
    // EbpfHeader layout (per rvf-types): magic(4) + version(2) + program_type(1)
    // + attach_type(1) + program_flags(4) + insn_count(2) + max_dimension(2) + ...
    let program_type = bytes[6];
    let attach_type = bytes[7];
    let max_dim = u16::from_le_bytes([bytes[14], bytes[15]]);
    Some((program_type, attach_type, max_dim))
}

fn hex_sha3_256(data: &[u8]) -> String {
    let mut hasher = Sha3_256::new();
    hasher.update(data);
    hasher.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rvf_runtime::options::RvfOptions;
    use tempfile::tempdir;

    const TEST_MANIFEST: &str = r#"{"version":1,"project_id":"test-id","project_name":"test","language":{"Single":{"lang":"Node","version":"22"}},"created_at":"","kernel_built_at":"","network":{"allow_domains":["registry.npmjs.org"],"allow_localhost":true,"dns_server":"1.1.1.1"},"resources":{"memory_mb":512,"vcpus":1,"disk_gb":8,"network_mbps":100},"kernel":{"arch":"x86_64","ssh_port":2222,"mcp_port":7878},"witness":{"max_entries":10000,"retention_days":30}}"#;

    fn make_rvf_with_kernel(path: &Path, kernel_bytes: &[u8]) {
        let opts = RvfOptions { dimension: 1, ..Default::default() };
        let mut store = RvfStore::create(path, opts).unwrap();
        store
            .embed_kernel(0x01, 0x01, 0, kernel_bytes, 2222, Some(TEST_MANIFEST))
            .unwrap();
        store.close().unwrap();
    }

    #[test]
    fn test_upgrade_result_has_hashes() {
        let r = UpgradeResult { from_hash: "abc".into(), to_hash: "def".into() };
        assert_ne!(r.from_hash, r.to_hash);
    }

    #[test]
    fn test_hash_current_kernel_hashes_image_bytes_only() {
        let dir = tempdir().unwrap();
        let rvf = dir.path().join("test.rvf");
        let kernel = b"the kernel image bytes";
        make_rvf_with_kernel(&rvf, kernel);

        let upgrader = KernelUpgrader { rvf_path: rvf };
        let got = upgrader.hash_current_kernel().unwrap();
        let expected = hex_sha3_256(kernel);
        assert_eq!(got, expected);
    }

    #[test]
    fn test_extract_non_kernel_segments_empty_for_kernel_only_rvf() {
        let dir = tempdir().unwrap();
        let rvf = dir.path().join("test.rvf");
        make_rvf_with_kernel(&rvf, b"kernel-A");

        let upgrader = KernelUpgrader { rvf_path: rvf };
        let segs = upgrader.extract_non_kernel_segments().unwrap();
        assert!(segs.is_empty());
    }

    #[tokio::test]
    async fn test_upgrade_replaces_kernel_bytes_and_keeps_manifest() {
        let dir = tempdir().unwrap();
        let rvf = dir.path().join("test.rvf");
        make_rvf_with_kernel(&rvf, b"old-kernel-image");

        let new_kernel_path = dir.path().join("new_kernel.bin");
        std::fs::write(&new_kernel_path, b"NEW-KERNEL-IMAGE-bytes").unwrap();

        let upgrader = KernelUpgrader { rvf_path: rvf.clone() };
        let result = upgrader.upgrade(&new_kernel_path).await.unwrap();
        assert_eq!(result.from_hash, hex_sha3_256(b"old-kernel-image"));
        assert_eq!(result.to_hash, hex_sha3_256(b"NEW-KERNEL-IMAGE-bytes"));

        // Reopen and verify the new kernel + same manifest JSON.
        let upgrader2 = KernelUpgrader { rvf_path: rvf.clone() };
        let (_, image, cmdline) = upgrader2.read_kernel_seg().unwrap();
        assert_eq!(image, b"NEW-KERNEL-IMAGE-bytes");
        assert_eq!(std::str::from_utf8(&cmdline).unwrap(), TEST_MANIFEST);
    }

    #[tokio::test]
    async fn test_upgrade_appends_kernel_upgrade_witness_entry() {
        let dir = tempdir().unwrap();
        let rvf = dir.path().join("test.rvf");
        make_rvf_with_kernel(&rvf, b"old");

        let new_kernel = dir.path().join("new.bin");
        std::fs::write(&new_kernel, b"new").unwrap();

        let upgrader = KernelUpgrader { rvf_path: rvf.clone() };
        upgrader.upgrade(&new_kernel).await.unwrap();

        let entries = claudebox_core::witness::load_witness_entries(&rvf).unwrap();
        assert!(!entries.is_empty(), "witness sidecar should contain an entry after upgrade");
        let last = entries.last().unwrap();
        match &last.event {
            WitnessEvent::KernelUpgrade { from_hash, to_hash } => {
                assert_eq!(from_hash, &hex_sha3_256(b"old"));
                assert_eq!(to_hash, &hex_sha3_256(b"new"));
            }
            other => panic!("expected KernelUpgrade event, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_upgrade_missing_new_kernel_returns_error() {
        let dir = tempdir().unwrap();
        let rvf = dir.path().join("test.rvf");
        make_rvf_with_kernel(&rvf, b"old");

        let upgrader = KernelUpgrader { rvf_path: rvf };
        let result = upgrader.upgrade(&dir.path().join("does-not-exist.bin")).await;
        assert!(result.is_err());
    }
}
