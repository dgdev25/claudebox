use std::path::{Path, PathBuf};
use std::process::{Child, Command};

pub trait KernelResolver {
    fn resolve_default_kernel(&self) -> Option<PathBuf>;
}

pub trait VmAdapter {
    fn build_qemu_command(
        &self,
        kernel_path: &Path,
        initramfs_path: Option<&Path>,
        ssh_port: u16,
        memory_mb: u32,
        rootfs_path: Option<&Path>,
        workspace_path: Option<&Path>,
        guest_arch: &str,
    ) -> anyhow::Result<Command>;

    fn create_overlay(&self, base_image: &Path, overlay_path: &Path) -> anyhow::Result<()>;
}

pub struct DefaultKernelResolver;

impl KernelResolver for DefaultKernelResolver {
    fn resolve_default_kernel(&self) -> Option<PathBuf> {
        let default = claudebox_core::setup::default_kernel_path();
        if default.exists() { Some(default) } else { None }
    }
}

pub struct DefaultVmAdapter;

impl VmAdapter for DefaultVmAdapter {
    fn build_qemu_command(
        &self,
        kernel_path: &Path,
        initramfs_path: Option<&Path>,
        ssh_port: u16,
        memory_mb: u32,
        rootfs_path: Option<&Path>,
        workspace_path: Option<&Path>,
        guest_arch: &str,
    ) -> anyhow::Result<Command> {
        claudebox_firecracker::qemu::build_qemu_command(
            kernel_path,
            initramfs_path,
            ssh_port,
            memory_mb,
            rootfs_path,
            workspace_path,
            guest_arch,
        )
    }

    fn create_overlay(&self, base_image: &Path, overlay_path: &Path) -> anyhow::Result<()> {
        claudebox_firecracker::qemu::create_instance_overlay(base_image, overlay_path)
    }
}

pub fn spawn_vm(cmd: &mut Command) -> anyhow::Result<Child> {
    cmd.spawn()
        .map_err(|e| anyhow::anyhow!("failed to spawn QEMU: {e}"))
}
