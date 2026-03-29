//! UniClipboard daemon binary entry point.
//!
//! Delegates to `uc_daemon::entrypoint::run()` which contains the full
//! composition root. The same function is also called by
//! `uniclipboard-cli daemon` for single-binary distribution.

fn main() -> anyhow::Result<()> {
    let gui_managed = std::env::args().any(|arg| arg == "--gui-managed");
    uc_daemon::entrypoint::run(gui_managed)
}
