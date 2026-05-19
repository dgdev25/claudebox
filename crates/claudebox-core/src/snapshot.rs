use std::path::{Path, PathBuf};
use anyhow::Result;

/// Supported snapshot operations (maps to `qemu-img snapshot` subcommands).
#[derive(Debug, Clone)]
pub enum SnapshotOp {
    Create(String),
    List,
    Restore(String),
    Export { name: String, output: PathBuf },
}

/// Run `qemu-img snapshot` against the overlay disk.
///
/// All operations target the per-instance overlay at `overlay_path`.
pub fn run_snapshot(overlay_path: &Path, op: SnapshotOp) -> Result<()> {
    match op {
        SnapshotOp::Create(name) => {
            run_qemu_img(&[
                "snapshot",
                "-c",
                &name,
                &overlay_path.to_string_lossy(),
            ])?;
            eprintln!("Snapshot '{name}' created.");
        }
        SnapshotOp::List => {
            let out = run_qemu_img_output(&[
                "snapshot",
                "-l",
                &overlay_path.to_string_lossy(),
            ])?;
            let names = parse_snapshot_list(&out);
            if names.is_empty() {
                println!("No snapshots.");
            } else {
                for name in &names {
                    println!("{name}");
                }
            }
        }
        SnapshotOp::Restore(name) => {
            run_qemu_img(&[
                "snapshot",
                "-a",
                &name,
                &overlay_path.to_string_lossy(),
            ])?;
            eprintln!("Restored snapshot '{name}'.");
        }
        SnapshotOp::Export { name, output } => {
            // Convert the snapshot to a standalone qcow2 at `output`.
            run_qemu_img(&[
                "convert",
                "-f",
                "qcow2",
                "-O",
                "qcow2",
                "-l",
                &format!("snapshot.name={name}"),
                &overlay_path.to_string_lossy(),
                &output.to_string_lossy(),
            ])?;
            eprintln!("Snapshot '{name}' exported to {}.", output.display());
        }
    }
    Ok(())
}

/// Parse the output of `qemu-img snapshot -l <image>` into a list of
/// snapshot names (the TAG column — second whitespace-delimited field on
/// each data row, skipping the two header lines).
pub fn parse_snapshot_list(output: &str) -> Vec<String> {
    output
        .lines()
        .skip_while(|l| !l.starts_with("ID"))
        .skip(1) // skip header line itself
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| {
            // Row format: <id>  <tag>  <vm_size> <date> ...
            let mut cols = l.split_whitespace();
            cols.next(); // id
            cols.next().map(|s| s.to_string()) // tag
        })
        .collect()
}

/// Create a named branch snapshot (equivalent to `snapshot create`).
pub fn create_branch(overlay_path: &Path, name: &str) -> Result<()> {
    run_snapshot(overlay_path, SnapshotOp::Create(name.to_string()))
}

/// Roll back to a named branch snapshot (equivalent to `snapshot restore`).
pub fn rollback_to_branch(overlay_path: &Path, branch: &str) -> Result<()> {
    run_snapshot(overlay_path, SnapshotOp::Restore(branch.to_string()))
}

/// Compact the overlay by converting it to a new compressed qcow2 and
/// atomically replacing the original (write to temp, then rename).
///
/// This reclaims space from deleted files inside the VM. The operation is
/// disk-level only — no witness or VEC compaction.
pub fn compact_overlay(overlay_path: &Path) -> Result<()> {
    let tmp = overlay_path.with_extension("qcow2.tmp");
    run_qemu_img(&[
        "convert",
        "-c",
        "-f",
        "qcow2",
        "-O",
        "qcow2",
        &overlay_path.to_string_lossy(),
        &tmp.to_string_lossy(),
    ])?;
    std::fs::rename(&tmp, overlay_path)
        .map_err(|e| anyhow::anyhow!("atomic rename failed: {e}"))?;
    eprintln!("Overlay compacted: {}", overlay_path.display());
    Ok(())
}

fn run_qemu_img(args: &[&str]) -> Result<()> {
    let status = std::process::Command::new("qemu-img")
        .args(args)
        .status()
        .map_err(|e| anyhow::anyhow!("qemu-img not found: {e}"))?;
    if !status.success() {
        anyhow::bail!("qemu-img {} failed", args.first().unwrap_or(&""));
    }
    Ok(())
}

fn run_qemu_img_output(args: &[&str]) -> Result<String> {
    let out = std::process::Command::new("qemu-img")
        .args(args)
        .output()
        .map_err(|e| anyhow::anyhow!("qemu-img not found: {e}"))?;
    if !out.status.success() {
        anyhow::bail!("qemu-img {} failed", args.first().unwrap_or(&""));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_snapshot_list_empty() {
        let output = "Snapshot list:\nID        TAG               VM SIZE                DATE     VM CLOCK     ICOUNT\n";
        assert_eq!(parse_snapshot_list(output), Vec::<String>::new());
    }

    #[test]
    fn test_parse_snapshot_list_one_entry() {
        let output = "Snapshot list:\nID        TAG               VM SIZE                DATE     VM CLOCK     ICOUNT\n1         mysnap            0 B 2026-05-19 10:00:00     00:00:00.000         0\n";
        let names = parse_snapshot_list(output);
        assert_eq!(names, vec!["mysnap"]);
    }

    #[test]
    fn test_parse_snapshot_list_multiple_entries() {
        let output = concat!(
            "Snapshot list:\n",
            "ID        TAG               VM SIZE                DATE     VM CLOCK     ICOUNT\n",
            "1         snap-a            0 B 2026-05-01 10:00:00     00:00:00.000         0\n",
            "2         snap-b            0 B 2026-05-02 11:00:00     00:00:00.000         0\n",
        );
        let names = parse_snapshot_list(output);
        assert_eq!(names, vec!["snap-a", "snap-b"]);
    }
}
