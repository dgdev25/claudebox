use std::path::{Path, PathBuf};

use crate::commands::context::AppContext;
use crate::SnapshotAction;

pub fn handle_branch_command(rvf: &Path, name: &str) -> anyhow::Result<()> {
    let cx = AppContext::for_rvf(rvf)?;
    anyhow::ensure!(
        cx.paths.overlay_path.exists(),
        "no overlay found — is the VM initialised?"
    );
    claudebox_core::snapshot::create_branch(&cx.paths.overlay_path, name)?;
    Ok(())
}

pub fn handle_rollback_command(rvf: &Path, branch: &str) -> anyhow::Result<()> {
    let cx = AppContext::for_rvf(rvf)?;
    anyhow::ensure!(
        cx.paths.overlay_path.exists(),
        "no overlay found — is the VM initialised?"
    );
    claudebox_core::snapshot::rollback_to_branch(&cx.paths.overlay_path, branch)?;
    Ok(())
}

pub fn handle_snapshot_command(rvf: &Path, action: SnapshotAction) -> anyhow::Result<()> {
    let cx = AppContext::for_rvf(rvf)?;
    anyhow::ensure!(
        cx.paths.overlay_path.exists(),
        "no overlay found — is the VM initialised?"
    );
    let op = match action {
        SnapshotAction::Create { name } => claudebox_core::snapshot::SnapshotOp::Create(name),
        SnapshotAction::List => claudebox_core::snapshot::SnapshotOp::List,
        SnapshotAction::Restore { name } => claudebox_core::snapshot::SnapshotOp::Restore(name),
        SnapshotAction::Export { name, output } => {
            claudebox_core::snapshot::SnapshotOp::Export { name, output }
        }
    };
    claudebox_core::snapshot::run_snapshot(&cx.paths.overlay_path, op)?;
    Ok(())
}

pub fn handle_compact_command(rvf: &Path) -> anyhow::Result<()> {
    let cx = AppContext::for_rvf(rvf)?;
    let overlay: PathBuf = cx.paths.overlay_path;
    anyhow::ensure!(overlay.exists(), "no overlay found — is the VM initialised?");
    claudebox_core::snapshot::compact_overlay(&overlay)?;
    Ok(())
}
