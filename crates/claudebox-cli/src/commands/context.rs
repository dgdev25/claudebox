use std::path::{Path, PathBuf};

use claudebox_core::manifest::ClaudeBoxManifest;

#[derive(Clone, Debug)]
pub struct ProjectPaths {
    pub rvf: PathBuf,
    pub pid_path: PathBuf,
    pub overlay_path: PathBuf,
    pub logs_sock_path: PathBuf,
    pub witness_archive_dir: PathBuf,
}

#[derive(Clone, Debug)]
pub struct AppContext {
    pub manifest: ClaudeBoxManifest,
    pub paths: ProjectPaths,
}

impl AppContext {
    pub fn for_rvf(rvf: &Path) -> anyhow::Result<Self> {
        claudebox_migrate::check_and_migrate(rvf, true)?;
        let manifest = claudebox_core::start::read_manifest_from_rvf(rvf)?;
        let paths = ProjectPaths {
            rvf: rvf.to_path_buf(),
            pid_path: claudebox_core::setup::instance_pid_path(&manifest.project_id),
            overlay_path: claudebox_core::setup::instance_overlay_path(&manifest.project_id),
            logs_sock_path: claudebox_core::setup::instance_vsock_path(&manifest.project_id),
            witness_archive_dir: claudebox_core::witness::witness_archive_dir(rvf),
        };
        Ok(Self { manifest, paths })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use claudebox_core::manifest::{
        ClaudeBoxManifest, KernelConfig, Lang, LanguageProfile, NetworkPolicy, ResourceLimits,
        SingleProfile, WitnessPolicy,
    };
    use claudebox_rvf::builder::ApplianceBuilder;

    fn test_manifest(project_id: &str) -> ClaudeBoxManifest {
        ClaudeBoxManifest {
            version: 2,
            project_id: project_id.to_string(),
            project_name: "ctx-test".into(),
            language: LanguageProfile::Single(SingleProfile {
                lang: Lang::Node,
                version: "22".into(),
            }),
            created_at: "2026-05-19T00:00:00Z".into(),
            kernel_built_at: "2026-05-19T00:00:00Z".into(),
            network: NetworkPolicy {
                allow_domains: vec!["registry.npmjs.org".into()],
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
        }
    }

    #[test]
    fn app_context_derives_expected_paths_from_manifest() {
        let dir = tempfile::tempdir().unwrap();
        let rvf = dir.path().join("ctx.rvf");
        let project_id = "ctx-project-123";
        let builder = ApplianceBuilder::new(test_manifest(project_id)).unwrap();
        builder.build_skeleton(&rvf, None).unwrap();

        let cx = AppContext::for_rvf(&rvf).unwrap();
        assert_eq!(cx.manifest.project_id, project_id);
        assert_eq!(cx.paths.rvf, rvf);
        assert!(cx.paths.pid_path.to_string_lossy().contains(project_id));
        assert!(cx.paths.overlay_path.to_string_lossy().contains(project_id));
        assert!(cx.paths.logs_sock_path.to_string_lossy().contains(project_id));
        assert!(cx
            .paths
            .witness_archive_dir
            .to_string_lossy()
            .contains(".witness-archives"));
    }

    #[test]
    fn app_context_errors_for_missing_rvf() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("missing.rvf");
        assert!(AppContext::for_rvf(&missing).is_err());
    }
}
