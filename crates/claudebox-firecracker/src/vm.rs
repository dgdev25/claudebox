use std::path::PathBuf;

use claudebox_core::manifest::ClaudeBoxManifest;

#[derive(serde::Serialize, serde::Deserialize, Debug)]
pub struct BootSourceConfig {
    pub kernel_image_path: String,
    pub boot_args: String,
}

/// All configuration needed to launch a Firecracker VM.
pub struct FirecrackerVm {
    pub project_id: String,
    pub rvf_path: PathBuf,
    pub workspace_path: PathBuf,
    pub manifest: ClaudeBoxManifest,
    /// Path to the Firecracker Unix domain socket.
    pub socket_path: PathBuf,
}

/// Handle returned after a VM has been successfully started.
pub struct VmHandle {
    pub pid: u32,
    pub ssh_port: u16,
    pub mcp_port: u16,
    pub started_at: String,
}

#[derive(Debug, PartialEq)]
pub enum VmStatus {
    /// Firecracker socket exists — VM is (probably) running.
    Running,
    /// Socket path does not exist.
    Stopped,
    /// The configured socket path parent directory does not exist.
    NotFound,
}

impl FirecrackerVm {
    /// Configure the VM via 6 PUT requests to the Firecracker HTTP API:
    ///
    /// 1. PUT /boot-source           — kernel image + boot args
    /// 2. PUT /drives/rootfs         — root block device from .rvf
    /// 3. PUT /machine-config        — vCPU count + memory
    /// 4. PUT /network-interfaces/eth0 — TAP device + MAC
    /// 5. PUT /vsock                 — guest CID + UDS path for vsock
    /// 6. PUT /actions { InstanceStart } — fire!
    ///
    /// Actual HTTP-over-UDS requests require Firecracker to be running.
    pub async fn configure(&self) -> anyhow::Result<()> {
        anyhow::bail!(
            "requires Firecracker runtime — would send 6 PUT requests to {:?}: \
             /boot-source, /drives/rootfs, /machine-config, \
             /network-interfaces/eth0, /vsock, /actions",
            self.socket_path
        )
    }

    /// Start the VM (PUT /actions InstanceStart) and return a handle.
    pub async fn start(&self) -> anyhow::Result<VmHandle> {
        anyhow::bail!("requires Firecracker runtime — would PUT /actions InstanceStart to {:?}", self.socket_path)
    }

    /// Gracefully stop the VM (PUT /actions SendCtrlAltDel).
    pub async fn stop(&self) -> anyhow::Result<()> {
        anyhow::bail!("requires Firecracker runtime — would PUT /actions SendCtrlAltDel to {:?}", self.socket_path)
    }

    /// Check the current VM status by inspecting the socket path.
    pub fn status(&self) -> VmStatus {
        if self.socket_path.exists() {
            VmStatus::Running
        } else if self
            .socket_path
            .parent()
            .map(|p| p.exists())
            .unwrap_or(false)
        {
            VmStatus::Stopped
        } else {
            VmStatus::NotFound
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_firecracker_config_json_shape() {
        // Verify the boot-source config serialises to the correct Firecracker API shape.
        let boot_config = BootSourceConfig {
            kernel_image_path: "/tmp/vmlinux".into(),
            boot_args: "console=ttyS0 reboot=k panic=1 pci=off".into(),
        };
        let json = serde_json::to_value(&boot_config).unwrap();
        assert!(json["kernel_image_path"].as_str().is_some());
        assert!(json["boot_args"].as_str().unwrap().contains("console=ttyS0"));
    }

    #[test]
    fn test_vm_status_stopped_when_socket_absent() {
        use claudebox_core::manifest::{
            ClaudeBoxManifest, KernelConfig, LanguageProfile, NetworkPolicy, ResourceLimits,
            SingleProfile, Lang, WitnessPolicy,
        };
        use tempfile::tempdir;
        let dir = tempdir().unwrap();
        let manifest = ClaudeBoxManifest {
            version: 1,
            project_id: "proj-1".into(),
            project_name: "test".into(),
            language: LanguageProfile::Single(SingleProfile {
                lang: Lang::Rust,
                version: "1.87".into(),
            }),
            created_at: "2026-05-19T00:00:00Z".into(),
            kernel_built_at: "2026-05-19T00:00:00Z".into(),
            network: NetworkPolicy {
                allow_domains: vec![],
                allow_localhost: true,
                dns_server: "1.1.1.1".into(),
            },
            resources: ResourceLimits::default(),
            kernel: KernelConfig {
                arch: "x86_64".into(),
                ssh_port: 2222,
                mcp_port: 7878,
            },
            witness: WitnessPolicy::default(),
        };
        let vm = FirecrackerVm {
            project_id: "proj-1".into(),
            rvf_path: dir.path().join("test.rvf"),
            workspace_path: dir.path().to_path_buf(),
            manifest,
            socket_path: dir.path().join("firecracker.sock"),
        };
        // The parent dir exists (tempdir), but socket file doesn't.
        assert_eq!(vm.status(), VmStatus::Stopped);
    }
}
