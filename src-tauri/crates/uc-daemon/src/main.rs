//! `uniclipd` — standalone UniClipboard daemon binary.
//!
//! Thin entry point that initializes platform prerequisites and delegates to
//! [`uc_daemon::daemon::host::run_standalone_from_env`].

/// Bootstrap AppKit for headless macOS processes.
///
/// `clipboard-rs` eagerly calls `+[NSPasteboard generalPasteboard]` during
/// `wire_dependencies`, which panics without AppKit loaded. `NSApplicationLoad`
/// is the documented way to bootstrap AppKit in non-`.app` processes.
///
/// Loading AppKit promotes this headless process to a foreground application,
/// so macOS hands it a Dock tile. Because the daemon never runs an
/// `NSApplication` event loop, that tile would bounce forever in a perpetual
/// "launching" state (and duplicates the GUI's icon, since both binaries share
/// one bundle). Immediately demoting the activation policy to `Prohibited`
/// keeps AppKit available — pasteboard access still works — while removing the
/// Dock tile, menu bar, and activation eligibility.
#[cfg(target_os = "macos")]
fn init_macos_appkit() {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};

    extern "C" {
        fn NSApplicationLoad() -> bool;
    }
    unsafe {
        let _ = NSApplicationLoad();
    }

    // `main()` runs on the process's main thread, so this marker is always
    // available here. `setActivationPolicy` must run on the main thread.
    if let Some(mtm) = MainThreadMarker::new() {
        let app = NSApplication::sharedApplication(mtm);
        app.setActivationPolicy(NSApplicationActivationPolicy::Prohibited);
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
