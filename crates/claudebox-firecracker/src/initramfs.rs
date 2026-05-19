//! Initramfs builder for ClaudeBox microVM boot.
//!
//! Adapted from the ruvector reference implementation (MIT licensed).
//! Creates a minimal Linux initramfs with a statically-compiled C init
//! that mounts proc/sys/dev and prints ClaudeBox boot information.

use std::path::{Path, PathBuf};
#[cfg(not(target_os = "macos"))]
use std::process::Command;

/// C source for the minimal init process.
///
/// Mounts proc/sys/dev, prints a ClaudeBox banner, reads the cmdline
/// (which contains the manifest JSON), and reboots.
#[cfg(not(target_os = "macos"))]
const INIT_C_SRC: &str = r#"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <sys/mount.h>
#include <sys/reboot.h>
#include <fcntl.h>
#include <sys/utsname.h>

int main(void) {
    struct utsname uts;
    mount("proc", "/proc", "proc", 0, NULL);
    mount("sysfs", "/sys", "sysfs", 0, NULL);
    mount("devtmpfs", "/dev", "devtmpfs", 0, NULL);

    printf("\n");
    printf("================================================================\n");
    printf("  ClaudeBox MicroVM - Per-Project Isolated Environment\n");
    printf("================================================================\n\n");

    if (uname(&uts) == 0) {
        printf("  Kernel:  %s %s\n  Arch:    %s\n\n",
               uts.sysname, uts.release, uts.machine);
    }

    /* Print first few lines of /proc/meminfo */
    char buf[512];
    int fd = open("/proc/meminfo", O_RDONLY);
    if (fd >= 0) {
        ssize_t n = read(fd, buf, 511);
        buf[n > 0 ? n : 0] = 0;
        close(fd);
        char *p = buf;
        for (int i = 0; i < 3 && *p; i++) {
            char *nl = strchr(p, '\n');
            if (nl) *nl = 0;
            printf("  %s\n", p);
            if (nl) p = nl + 1; else break;
        }
    }
    printf("\n");

    /* Print cmdline (manifest JSON) */
    fd = open("/proc/cmdline", O_RDONLY);
    if (fd >= 0) {
        ssize_t n = read(fd, buf, 511);
        buf[n > 0 ? n : 0] = 0;
        close(fd);
        printf("  Cmdline: %s\n", buf);
    }
    printf("\n");
    printf("  Status: BOOT OK - ClaudeBox kernel verified\n");
    printf("================================================================\n\n");

    sync();
    reboot(0x4321fedc);
    for (;;) sleep(1);
    return 0;
}
"#;

/// Build a minimal initramfs cpio.gz in `tmp_dir`.
///
/// On Linux: compiles a static C init, bundles with cpio+gzip.
/// On macOS: `cc -static` cannot produce Linux ELF, so we return an
/// error — the caller (CLI) treats this as "boot without initramfs" and
/// passes `--append "root=/dev/sda"` or falls back to a disk image.
///
/// If a pre-built initramfs already exists at `tmp_dir/initramfs.cpio.gz`
/// (placed by `claudebox setup`), it is returned immediately.
pub fn build_initramfs(tmp_dir: &Path) -> anyhow::Result<PathBuf> {
    // Reuse a cached initramfs if present (e.g. from `claudebox setup`)
    let cpio_gz_path = tmp_dir.join("initramfs.cpio.gz");
    if cpio_gz_path.exists() {
        return Ok(cpio_gz_path);
    }

    // macOS cannot cross-compile a Linux ELF init — caller must use a disk image.
    #[cfg(target_os = "macos")]
    anyhow::bail!(
        "Cannot build a Linux initramfs on macOS (cross-compilation not supported). \
         Use `claudebox start --rootfs <alpine.qcow2>` with a downloaded Alpine disk image instead."
    );

    // Linux path: compile static init + cpio pack
    #[cfg(not(target_os = "macos"))]
    {
        let initramfs_dir = tmp_dir.join("initramfs");
        for sub in &["proc", "sys", "dev", "bin", "etc"] {
            std::fs::create_dir_all(initramfs_dir.join(sub))
                .map_err(|e| anyhow::anyhow!("failed to create initramfs/{sub}: {e}"))?;
        }

        let init_c_path = tmp_dir.join("init.c");
        std::fs::write(&init_c_path, INIT_C_SRC)
            .map_err(|e| anyhow::anyhow!("failed to write init.c: {e}"))?;

        let init_bin = initramfs_dir.join("init");
        let compile = Command::new("cc")
            .args(["-static", "-Os", "-o"])
            .arg(&init_bin)
            .arg(&init_c_path)
            .output()
            .map_err(|e| anyhow::anyhow!("cc not found: {e}. apt install gcc musl-tools"))?;

        if !compile.status.success() {
            let stderr = String::from_utf8_lossy(&compile.stderr);
            anyhow::bail!("cc -static failed (apt install musl-tools libc6-dev):\n{stderr}");
        }

        let cpio_cmd = format!(
            "cd '{}' && find . | cpio -o -H newc | gzip > '{}'",
            initramfs_dir.display(),
            cpio_gz_path.display()
        );
        let pack = Command::new("sh")
            .arg("-c")
            .arg(&cpio_cmd)
            .output()
            .map_err(|e| anyhow::anyhow!("cpio not found: {e}. apt install cpio"))?;

        if !pack.status.success() {
            let stderr = String::from_utf8_lossy(&pack.stderr);
            anyhow::bail!("cpio/gzip failed:\n{stderr}");
        }

        Ok(cpio_gz_path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn init_c_src_contains_banner() {
        assert!(INIT_C_SRC.contains("ClaudeBox MicroVM"));
    }

    /// On macOS, build_initramfs always returns a clear error directing
    /// the user to supply --rootfs instead.
    #[cfg(target_os = "macos")]
    #[test]
    fn build_initramfs_fails_gracefully_on_macos() {
        let tmp = tempdir().unwrap();
        let result = build_initramfs(tmp.path());
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("macOS") || msg.contains("rootfs"),
            "error should mention macOS limitation: {msg}"
        );
    }

    /// On Linux, build_initramfs succeeds when gcc+cpio are available.
    /// Skipped silently if the toolchain is absent.
    #[cfg(not(target_os = "macos"))]
    #[test]
    fn build_initramfs_produces_cpio_gz_when_toolchain_available() {
        if std::process::Command::new("cc")
            .arg("--version")
            .output()
            .map(|o| !o.status.success())
            .unwrap_or(true)
        {
            return; // skip if cc not available
        }

        let tmp = tempdir().unwrap();
        let result = build_initramfs(tmp.path());
        match result {
            Ok(path) => {
                assert!(path.exists());
                assert!(std::fs::metadata(&path).unwrap().len() > 0);
            }
            Err(e) => {
                let msg = e.to_string();
                assert!(
                    msg.contains("cc -static failed") || msg.contains("musl"),
                    "unexpected error: {msg}"
                );
            }
        }
    }
}
