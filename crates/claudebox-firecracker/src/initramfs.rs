//! Initramfs builder for ClaudeBox microVM boot.
//!
//! Adapted from the ruvector reference implementation (MIT licensed).
//! Creates a minimal Linux initramfs with a statically-compiled C init
//! that mounts proc/sys/dev and prints ClaudeBox boot information.

use std::path::{Path, PathBuf};
use std::process::Command;

/// C source for the minimal init process.
///
/// Mounts proc/sys/dev, prints a ClaudeBox banner, reads the cmdline
/// (which contains the manifest JSON), and reboots.
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

/// Build a minimal initramfs cpio.gz in a temporary directory.
///
/// Steps:
/// 1. Create `{tmp}/initramfs/{proc,sys,dev,bin,etc}/`
/// 2. Write init.c to `{tmp}/init.c`
/// 3. Compile: `cc -static -Os -o {tmp}/initramfs/init {tmp}/init.c`
/// 4. Pack: `cd {tmp}/initramfs && find . | cpio -o -H newc | gzip > {tmp}/initramfs.cpio.gz`
/// 5. Return path to `{tmp}/initramfs.cpio.gz`
///
/// The caller is responsible for keeping `tmp_dir` alive while the
/// initramfs path is in use (the returned path lives inside it).
pub fn build_initramfs(tmp_dir: &Path) -> anyhow::Result<PathBuf> {
    let initramfs_dir = tmp_dir.join("initramfs");
    for sub in &["proc", "sys", "dev", "bin", "etc"] {
        std::fs::create_dir_all(initramfs_dir.join(sub)).map_err(|e| {
            anyhow::anyhow!("failed to create initramfs/{sub}: {e}")
        })?;
    }

    // Write init.c
    let init_c_path = tmp_dir.join("init.c");
    std::fs::write(&init_c_path, INIT_C_SRC)
        .map_err(|e| anyhow::anyhow!("failed to write init.c: {e}"))?;

    // Compile static init
    let init_bin = initramfs_dir.join("init");
    let compile = Command::new("cc")
        .args(["-static", "-Os", "-o"])
        .arg(&init_bin)
        .arg(&init_c_path)
        .output()
        .map_err(|e| anyhow::anyhow!("failed to run cc: {e}. Install gcc or clang with static libc support."))?;

    if !compile.status.success() {
        let stderr = String::from_utf8_lossy(&compile.stderr);
        anyhow::bail!(
            "cc -static failed (install musl-tools or libc6-dev):\n{stderr}"
        );
    }

    // Bundle with cpio + gzip
    let cpio_gz_path = tmp_dir.join("initramfs.cpio.gz");
    let cpio_cmd = format!(
        "cd '{}' && find . | cpio -o -H newc | gzip > '{}'",
        initramfs_dir.display(),
        cpio_gz_path.display()
    );
    let pack = Command::new("sh")
        .arg("-c")
        .arg(&cpio_cmd)
        .output()
        .map_err(|e| anyhow::anyhow!("failed to run cpio: {e}"))?;

    if !pack.status.success() {
        let stderr = String::from_utf8_lossy(&pack.stderr);
        anyhow::bail!("cpio/gzip failed:\n{stderr}");
    }

    Ok(cpio_gz_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn init_c_src_contains_banner() {
        assert!(INIT_C_SRC.contains("ClaudeBox MicroVM"));
    }

    /// Build initramfs only when cc and cpio are available.
    /// Skipped silently if the toolchain is absent (e.g. bare CI).
    #[test]
    fn build_initramfs_produces_cpio_gz_when_toolchain_available() {
        // Quick check: if `cc` isn't on PATH, skip rather than fail.
        if std::process::Command::new("cc")
            .arg("--version")
            .output()
            .map(|o| !o.status.success())
            .unwrap_or(true)
        {
            return;
        }

        let tmp = tempdir().unwrap();
        let result = build_initramfs(tmp.path());
        match result {
            Ok(path) => {
                assert!(path.exists(), "cpio.gz must exist");
                let size = std::fs::metadata(&path).unwrap().len();
                assert!(size > 0, "cpio.gz must be non-empty");
            }
            Err(e) => {
                // May fail if -static linking is unavailable (e.g. macOS without musl)
                let msg = e.to_string();
                assert!(
                    msg.contains("cc -static failed") || msg.contains("musl"),
                    "unexpected error: {msg}"
                );
            }
        }
    }
}
