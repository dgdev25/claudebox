use std::path::PathBuf;

pub enum VmStatusDisplay {
    Running { pid: u32, ssh_port: u16, mcp_port: u16 },
    Stopped,
    NotFound,
}

pub struct ProjectStatus {
    pub project_name: String,
    pub rvf_path: PathBuf,
    pub rvf_size_mb: u64,
    pub vm_status: VmStatusDisplay,
    pub language: String,
    pub kernel_age_days: i64,
    pub kernel_stale: bool,
    pub witness_hot_entries: u64,
    pub witness_archived_months: u64,
    pub vec_chunks: u64,
    pub vec_files: u64,
    pub vec_tombstoned: u64,
    pub schema_version: u8,
}

/// Format a u64 with thousands comma separators.
fn fmt_comma(n: u64) -> String {
    let s = n.to_string();
    let mut result = String::new();
    let chars: Vec<char> = s.chars().collect();
    let len = chars.len();
    for (i, ch) in chars.iter().enumerate() {
        if i > 0 && (len - i) % 3 == 0 {
            result.push(',');
        }
        result.push(*ch);
    }
    result
}

pub fn format_status(info: &ProjectStatus) -> String {
    let mut lines = Vec::new();

    lines.push(format!("Project:  {}", info.project_name));
    lines.push(format!("RVF:      {} ({} MB)", info.rvf_path.display(), info.rvf_size_mb));
    lines.push(format!("Language: {}", info.language));
    lines.push(format!("Schema:   v{}", info.schema_version));

    match &info.vm_status {
        VmStatusDisplay::Running { pid, ssh_port, mcp_port } => {
            lines.push(format!("VM:       running (pid {}, ssh :{}, mcp :{})", pid, ssh_port, mcp_port));
        }
        VmStatusDisplay::Stopped => {
            lines.push("VM:       stopped".to_string());
        }
        VmStatusDisplay::NotFound => {
            lines.push("VM:       not found".to_string());
        }
    }

    lines.push(format!(
        "Kernel:   {} days old{}",
        info.kernel_age_days,
        if info.kernel_stale { " — stale ⚠ run `claudebox upgrade-kernel`" } else { "" }
    ));

    lines.push(format!(
        "Witness:  {} hot entries, {} archived month(s)",
        fmt_comma(info.witness_hot_entries),
        info.witness_archived_months,
    ));

    lines.push(format!(
        "VEC:      {} chunks, {} files ({} tombstoned)",
        fmt_comma(info.vec_chunks),
        info.vec_files,
        info.vec_tombstoned,
    ));

    if info.kernel_stale {
        lines.push(format!(
            "WARNING:  kernel is {} days old — run `claudebox upgrade-kernel` to update",
            info.kernel_age_days
        ));
    }

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_status() -> ProjectStatus {
        ProjectStatus {
            project_name: "test".into(),
            rvf_path: PathBuf::from("test.rvf"),
            rvf_size_mb: 100,
            vm_status: VmStatusDisplay::Stopped,
            language: "node@22".into(),
            kernel_age_days: 0,
            kernel_stale: false,
            witness_hot_entries: 0,
            witness_archived_months: 0,
            vec_chunks: 0,
            vec_files: 0,
            vec_tombstoned: 0,
            schema_version: 1,
        }
    }

    #[test]
    fn test_status_output_contains_required_fields() {
        let info = ProjectStatus {
            project_name: "myproject".into(),
            rvf_path: PathBuf::from("myproject.rvf"),
            rvf_size_mb: 245,
            vm_status: VmStatusDisplay::Running { pid: 18432, ssh_port: 2222, mcp_port: 7878 },
            language: "node@22".into(),
            kernel_age_days: 12,
            kernel_stale: false,
            witness_hot_entries: 1247,
            witness_archived_months: 2,
            vec_chunks: 4821,
            vec_files: 312,
            vec_tombstoned: 3,
            schema_version: 1,
        };
        let output = format_status(&info);
        assert!(output.contains("myproject"));
        assert!(output.contains("245 MB"));
        assert!(output.contains("18432"));
        assert!(output.contains("1,247"));
        assert!(output.contains("312 files"));
        assert!(output.contains("tombstoned"));
    }

    #[test]
    fn test_status_shows_kernel_staleness_warning() {
        let info = ProjectStatus { kernel_stale: true, kernel_age_days: 94, ..default_status() };
        let output = format_status(&info);
        assert!(output.contains("⚠") || output.contains("warning") || output.contains("stale"));
        assert!(output.contains("upgrade-kernel"));
    }
}
