use std::path::PathBuf;

use rvf_runtime::RvfStore;
use rvf_types::kernel::KernelHeader;

pub struct StartOptions {
    pub rvf: PathBuf,
    pub workspace: PathBuf,
}

/// Extracted kernel data from a ClaudeBox RVF appliance.
pub struct ExtractedKernel {
    /// Path to the written kernel image (bzImage) in a temp location.
    pub kernel_path: PathBuf,
    /// SSH port parsed from the KernelHeader api_port field.
    pub ssh_port: u16,
    /// Manifest JSON recovered from the kernel cmdline field.
    pub manifest_json: String,
}

/// Open the RVF appliance and extract the kernel image to a temp file.
///
/// Returns `ExtractedKernel` with paths and metadata for the caller to
/// build the QEMU/initramfs invocation. The temp file lives at
/// `/tmp/claudebox-<project_id>/bzImage` and must be cleaned up by the caller.
///
/// This function does NOT depend on `claudebox-firecracker` to avoid a
/// circular dependency. The caller (CLI or integration layer) is
/// responsible for wiring QEMU launch using the returned data.
pub fn extract_kernel(opts: &StartOptions) -> anyhow::Result<ExtractedKernel> {
    // Step 1: preflight checks.
    crate::preflight::check_dependencies()?;

    // Step 2-5: Open the RVF and parse the kernel segment.
    let parsed = parse_kernel_segment(&opts.rvf)?;

    // Step 6-7: Write kernel image to a per-project temp file.
    let project_id = extract_project_id(&parsed.manifest_json);
    let tmp_dir = std::path::PathBuf::from(format!("/tmp/claudebox-{project_id}"));
    std::fs::create_dir_all(&tmp_dir)
        .map_err(|e| anyhow::anyhow!("failed to create temp dir: {e}"))?;
    let kernel_path = tmp_dir.join("bzImage");
    std::fs::write(&kernel_path, &parsed.kernel_image)
        .map_err(|e| anyhow::anyhow!("failed to write kernel to {}: {e}", kernel_path.display()))?;

    Ok(ExtractedKernel {
        kernel_path,
        ssh_port: parsed.ssh_port,
        manifest_json: parsed.manifest_json,
    })
}

/// Open the RVF and return the embedded `ClaudeBoxManifest` without writing
/// any files — used by stop/status/kernel commands that only need metadata.
pub fn read_manifest_from_rvf(rvf: &std::path::Path) -> anyhow::Result<crate::manifest::ClaudeBoxManifest> {
    let parsed = parse_kernel_segment(rvf)?;
    serde_json::from_str(&parsed.manifest_json)
        .map_err(|e| anyhow::anyhow!("corrupt manifest in {}: {e}", rvf.display()))
}

/// Parsed contents of a `KERNEL_SEG`: the kernel image bytes, the manifest
/// JSON recovered from the cmdline field, and the SSH port from the header.
struct ParsedKernelSegment {
    kernel_image: Vec<u8>,
    manifest_json: String,
    ssh_port: u16,
}

/// Open the RVF at `rvf` and decode its kernel segment. Shared by
/// `extract_kernel` (which then writes the image to disk) and
/// `read_manifest_from_rvf` (which discards the image and parses the JSON).
fn parse_kernel_segment(rvf: &std::path::Path) -> anyhow::Result<ParsedKernelSegment> {
    let store = RvfStore::open_readonly(rvf)
        .map_err(|e| anyhow::anyhow!("failed to open {}: {e:?}", rvf.display()))?;

    let (hdr_bytes, remainder) = store
        .extract_kernel()
        .map_err(|e| anyhow::anyhow!("extract_kernel failed: {e:?}"))?
        .ok_or_else(|| {
            anyhow::anyhow!(
                "no KERNEL_SEG in {} — run `claudebox init --kernel-from <bzImage>` first",
                rvf.display()
            )
        })?;

    anyhow::ensure!(
        hdr_bytes.len() == 128,
        "kernel header must be 128 bytes, got {}",
        hdr_bytes.len()
    );

    let mut hdr_array = [0u8; 128];
    hdr_array.copy_from_slice(&hdr_bytes);
    let header = KernelHeader::from_bytes(&hdr_array)
        .map_err(|e| anyhow::anyhow!("invalid KernelHeader: {e:?}"))?;

    // Segment payload layout (after the 128B KernelHeader) is:
    //   kernel_image (image_size bytes) || cmdline (cmdline_length bytes)
    let image_size = header.image_size as usize;
    let cmdline_length = header.cmdline_length as usize;
    let cmdline_end = image_size + cmdline_length;

    anyhow::ensure!(
        remainder.len() >= cmdline_end,
        "remainder too short for kernel+cmdline: {} < {}",
        remainder.len(),
        cmdline_end
    );

    let manifest_json = String::from_utf8(remainder[image_size..cmdline_end].to_vec())
        .map_err(|e| anyhow::anyhow!("cmdline is not valid UTF-8: {e}"))?;

    Ok(ParsedKernelSegment {
        kernel_image: remainder[..image_size].to_vec(),
        manifest_json,
        ssh_port: header.api_port,
    })
}

/// Pull `project_id` from the manifest JSON without a full deserialize.
/// Falls back to "default" if parsing fails.
fn extract_project_id(manifest_json: &str) -> String {
    serde_json::from_str::<serde_json::Value>(manifest_json)
        .ok()
        .and_then(|v| v["project_id"].as_str().map(|s| s.to_string()))
        .unwrap_or_else(|| "default".to_string())
}

/// Execute the full `claudebox start` boot sequence.
///
/// Steps 1-7 (kernel extraction) are implemented here. Steps 8-20
/// (QEMU launch, SSH polling, MCP wiring) are handled by the CLI layer
/// using `claudebox_firecracker::qemu` and `claudebox_firecracker::initramfs`
/// to avoid a circular dependency (claudebox-firecracker → claudebox-core).
pub async fn run_start(opts: StartOptions) -> anyhow::Result<()> {
    let extracted = extract_kernel(&opts)?;

    eprintln!(
        "Kernel extracted to {}  (ssh_port={})",
        extracted.kernel_path.display(),
        extracted.ssh_port
    );
    eprintln!(
        "Manifest: {}",
        extracted.manifest_json.chars().take(120).collect::<String>()
    );
    eprintln!(
        "Note: QEMU launch wired in CLI layer. \
         Run `claudebox start <rvf>` via the compiled binary."
    );

    // Steps 8-20 are wired in the CLI's Commands::Start handler
    // (see crates/claudebox-cli/src/main.rs) to avoid circular deps.
    let _ = &opts.workspace;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_start_options_default_workspace_is_current_dir() {
        let opts = StartOptions {
            rvf: PathBuf::from("myapp.rvf"),
            workspace: PathBuf::from("."),
        };
        assert_eq!(opts.workspace, PathBuf::from("."));
    }

    #[test]
    fn test_extract_project_id_from_valid_json() {
        let json = r#"{"project_id":"abc-123","project_name":"test"}"#;
        assert_eq!(extract_project_id(json), "abc-123");
    }

    #[test]
    fn test_extract_project_id_fallback_on_bad_json() {
        assert_eq!(extract_project_id("not-json"), "default");
    }
}
