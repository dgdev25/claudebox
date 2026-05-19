//! Hypervisor detection for ClaudeBox VM launch.
//!
//! On a Linux host with `/dev/kvm` present: use Firecracker.
//! On macOS (including CI runners): use QEMU with HVF acceleration.
//! Anywhere else: warn and fall back to QEMU-HVF (will fail at VM boot time
//! if HVF is not available, rather than at detection time).

/// The hypervisor backend that will be used to run the ClaudeBox VM.
#[derive(Debug, PartialEq)]
pub enum Hypervisor {
    /// Firecracker — lightweight VMM for Linux/KVM.
    Firecracker,
    /// QEMU with Apple Hypervisor Framework (HVF) — for macOS development.
    QemuHvf,
}

/// Detect which hypervisor is available on the current host.
///
/// The detection is intentionally conservative: it never panics, and an
/// ambiguous result falls back to `QemuHvf` with a warning log entry. Hard
/// failures are deferred to VM boot time so callers can still run preflight
/// checks or generate configs even when the full hypervisor stack isn't ready.
pub fn detect_hypervisor() -> Hypervisor {
    if std::path::Path::new("/dev/kvm").exists() {
        Hypervisor::Firecracker
    } else if cfg!(target_os = "macos") {
        Hypervisor::QemuHvf
    } else {
        tracing::warn!(
            "No KVM found and not macOS — defaulting to QemuHvf (may fail at runtime)"
        );
        Hypervisor::QemuHvf
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_hypervisor_does_not_panic() {
        // On any platform, detect_hypervisor() must return a value, never panic.
        let h = detect_hypervisor();
        let _ = h;
    }
}
