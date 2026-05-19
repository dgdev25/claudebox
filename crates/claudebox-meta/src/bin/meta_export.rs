fn main() -> anyhow::Result<()> {
    let rvf_path = std::env::args().nth(1)
        .ok_or_else(|| anyhow::anyhow!("usage: claudebox-meta-export <rvf-path>"))?;
    // stub: deferred to Phase 11 — would call BootHook::run()
    eprintln!("claudebox-meta-export: deferred to Phase 11 (rvf-runtime integration)");
    let _ = rvf_path;
    Ok(())
}
