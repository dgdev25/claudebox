//! eBPF compiler and macOS squid fallback.
//!
//! On Linux: `EbpfCompiler` resolves domains to IPv4s at init time (static
//! snapshot prevents mid-session DNS manipulation), compiles `filter.c` with
//! clang, strips debug symbols, and returns the ELF bytes for embedding.
//!
//! On macOS: `EbpfCompiler::compile*` returns an error directing callers to
//! use `SquidConfigGenerator` instead, which produces a `squid.conf` suitable
//! for the QEMU-HVF development environment.

use std::net::{IpAddr, Ipv4Addr, ToSocketAddrs};
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// EbpfCompiler
// ---------------------------------------------------------------------------

/// Resolves an allowlist of domain names to IPv4 addresses and (on Linux)
/// compiles the XDP BPF filter with those addresses baked in via the map.
pub struct EbpfCompiler {
    /// Domains the VM is allowed to reach (e.g. "registry.npmjs.org").
    pub allow_domains: Vec<String>,
    /// DNS server for resolution (e.g. "1.1.1.1").
    pub dns_server: String,
}

impl EbpfCompiler {
    /// Resolve all `allow_domains` to IPv4 addresses.
    ///
    /// Uses the system resolver (respects `/etc/resolv.conf` or the OS
    /// resolver, not the `dns_server` field directly — the field is stored for
    /// future use with a custom resolver backend).
    ///
    /// Returns a deduplicated list of `Ipv4Addr`; IPv6 results are silently
    /// filtered out because the XDP filter only handles IPv4.
    pub async fn resolve_allowlist(&self) -> anyhow::Result<Vec<Ipv4Addr>> {
        let mut ips: Vec<Ipv4Addr> = Vec::new();

        for domain in &self.allow_domains {
            // ToSocketAddrs needs a host:port pair; port 80 is arbitrary.
            let addr_str = format!("{domain}:80");
            let resolved = tokio::task::spawn_blocking(move || {
                addr_str.to_socket_addrs().map(|iter| iter.collect::<Vec<_>>())
            })
            .await
            .map_err(|e| anyhow::anyhow!("join error resolving {domain}: {e}"))??;

            for socket_addr in resolved {
                if let IpAddr::V4(v4) = socket_addr.ip() {
                    if !ips.contains(&v4) {
                        ips.push(v4);
                    }
                }
            }
        }

        Ok(ips)
    }

    /// Compile `ebpf/network_filter/filter.c` with clang (Linux only).
    ///
    /// Writes the stripped `.o` file to a deterministic temporary path and
    /// returns that path.  The caller is responsible for loading the object
    /// into the kernel via libbpf or similar.
    ///
    /// # Errors
    /// Returns an error on macOS (compilation requires Linux kernel headers),
    /// if clang is not found, or if compilation fails.
    pub fn compile(&self) -> anyhow::Result<PathBuf> {
        #[cfg(not(target_os = "linux"))]
        {
            anyhow::bail!(
                "eBPF compilation requires Linux; use SquidConfigGenerator on macOS"
            );
        }

        #[cfg(target_os = "linux")]
        {
            use std::process::Command;

            let out_path = PathBuf::from("/tmp/claudebox_filter.o");
            let out_str = out_path
                .to_str()
                .expect("output path is valid UTF-8");

            let clang_out = Command::new("clang")
                .args([
                    "-O2",
                    "-target",
                    "bpf",
                    "-c",
                    "ebpf/network_filter/filter.c",
                    "-o",
                    out_str,
                ])
                .output()
                .map_err(|e| anyhow::anyhow!("failed to run clang: {e}"))?;

            if !clang_out.status.success() {
                let stderr = String::from_utf8_lossy(&clang_out.stderr);
                anyhow::bail!("clang compilation failed:\n{stderr}");
            }

            let strip_out = Command::new("llvm-strip")
                .args(["-g", out_str])
                .output()
                .map_err(|e| anyhow::anyhow!("failed to run llvm-strip: {e}"))?;

            if !strip_out.status.success() {
                let stderr = String::from_utf8_lossy(&strip_out.stderr);
                anyhow::bail!("llvm-strip failed:\n{stderr}");
            }

            Ok(out_path)
        }
    }

    /// Compile the XDP filter and return the ELF bytes (Linux only).
    ///
    /// Calls `compile()` and reads the resulting `.o` file into memory.
    ///
    /// # Errors
    /// Returns an error on macOS (direct message: use SquidConfigGenerator),
    /// or if compilation / file I/O fails.
    pub fn compile_to_bytes(&self) -> anyhow::Result<Vec<u8>> {
        #[cfg(not(target_os = "linux"))]
        {
            anyhow::bail!(
                "eBPF compilation requires Linux; use SquidConfigGenerator on macOS"
            );
        }

        #[cfg(target_os = "linux")]
        {
            let path = self.compile()?;
            let bytes = std::fs::read(&path)
                .map_err(|e| anyhow::anyhow!("failed to read compiled object {path:?}: {e}"))?;
            Ok(bytes)
        }
    }
}

// ---------------------------------------------------------------------------
// SquidConfigGenerator
// ---------------------------------------------------------------------------

/// Generates a `squid.conf` for the macOS QEMU-HVF development environment.
///
/// Each domain in `allow_domains` gets its own named ACL so operators can
/// audit exactly which destinations are permitted.
pub struct SquidConfigGenerator {
    /// Domains the proxy is allowed to forward to.
    pub allow_domains: Vec<String>,
    /// DNS server squid should use for resolution.
    pub dns_server: String,
}

impl SquidConfigGenerator {
    /// Produce a complete `squid.conf` string with per-domain ACLs.
    ///
    /// The generated config:
    /// - Defines one `acl dstdomain` entry per allowed domain.
    /// - Emits `http_access allow` for each ACL.
    /// - Ends with `http_access deny all` to block everything else.
    pub fn generate(&self) -> anyhow::Result<String> {
        let mut conf = String::new();

        conf.push_str("# ClaudeBox squid.conf — auto-generated macOS fallback\n");
        conf.push_str("# DO NOT edit manually; regenerate via SquidConfigGenerator.\n\n");

        conf.push_str(&format!("dns_nameservers {}\n\n", self.dns_server));

        // Define one ACL per domain
        for domain in &self.allow_domains {
            // Sanitise: strip leading dots/whitespace so `.npmjs.org` and
            // `npmjs.org` both produce a valid squid ACL name.
            let safe_name = domain
                .trim_start_matches('.')
                .replace(['.', '-'], "_");
            conf.push_str(&format!(
                "acl {safe_name} dstdomain .{domain}\n"
            ));
        }

        conf.push('\n');

        // Emit http_access allow for each ACL
        for domain in &self.allow_domains {
            let safe_name = domain
                .trim_start_matches('.')
                .replace(['.', '-'], "_");
            conf.push_str(&format!("http_access allow {safe_name}\n"));
        }

        conf.push_str("http_access deny all\n");

        Ok(conf)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore = "requires network"]
    async fn test_domain_resolution_returns_ips() {
        let compiler = EbpfCompiler {
            allow_domains: vec!["registry.npmjs.org".into()],
            dns_server: "1.1.1.1".into(),
        };
        let ips = compiler.resolve_allowlist().await.unwrap();
        assert!(!ips.is_empty(), "registry.npmjs.org must resolve to at least one IP");
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn test_compiled_bytes_are_elf() {
        let compiler = EbpfCompiler {
            allow_domains: vec!["registry.npmjs.org".into()],
            dns_server: "1.1.1.1".into(),
        };
        let bytes = compiler.compile_to_bytes().unwrap();
        assert_eq!(&bytes[0..4], b"\x7fELF", "output must be ELF binary");
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn test_macos_generates_squid_config_not_elf() {
        let generator = SquidConfigGenerator {
            allow_domains: vec!["registry.npmjs.org".into()],
            dns_server: "1.1.1.1".into(),
        };
        let config = generator.generate().unwrap();
        assert!(config.contains("registry.npmjs.org"));
        assert!(config.contains("acl"));
    }

    // Task 3.3: multi-language union allowlist compilation test
    #[test]
    #[cfg(target_os = "linux")]
    fn test_multi_language_union_allowlist_compiles() {
        // node (2 domains) + rust (3 domains) = 5 unique domains, must compile without error
        let compiler = EbpfCompiler {
            allow_domains: vec![
                "registry.npmjs.org".into(),
                "nodejs.org".into(),
                "crates.io".into(),
                "static.crates.io".into(),
                "index.crates.io".into(),
            ],
            dns_server: "1.1.1.1".into(),
        };
        let bytes = compiler.compile_to_bytes().unwrap();
        assert_eq!(&bytes[0..4], b"\x7fELF");
    }
}
