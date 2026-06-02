//! `uniclipd` — standalone UniClipboard daemon binary.
//!
//! Thin entry point that initializes platform prerequisites and delegates to
//! [`uc_daemon::daemon::host::run_standalone_from_env`].

/// Bootstrap AppKit for headless macOS processes.
///
/// `clipboard-rs` eagerly calls `+[NSPasteboard generalPasteboard]` during
/// `wire_dependencies`, which panics without AppKit loaded. `NSApplicationLoad`
/// is the documented way to bootstrap AppKit in non-`.app` processes.
#[cfg(target_os = "macos")]
fn init_macos_appkit() {
    extern "C" {
        fn NSApplicationLoad() -> bool;
    }
    unsafe {
        let _ = NSApplicationLoad();
    }
}
#[cfg(not(target_os = "macos"))]
fn init_macos_appkit() {}

fn main() -> anyhow::Result<()> {
    init_macos_appkit();

    // rustls 0.23+ requires a process-wide CryptoProvider. Install ring as
    // the default before any TLS handshake (same as the CLI binary).
    let _ = rustls::crypto::ring::default_provider().install_default();

    // Mark this process as a daemon for observability (Sentry device.role).
    std::env::set_var("UC_HOST_ROLE", "daemon");

    uc_daemon::daemon::host::run_standalone_from_env()
}
