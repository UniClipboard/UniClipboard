//! End-to-end smoke test for `WaylandClipboard::{read_snapshot,write_snapshot}`.
//!
//! Exercises:
//!
//! 1. **Write**: pushes a synthetic snapshot into the wayland clipboard. A
//!    paster (`wl-paste`) running in another process should now see our
//!    content.
//! 2. **Read**: asks the worker to return the cached selection — should
//!    match either what we just wrote (if the compositor reflected the
//!    selection back to us) or whatever is currently on the clipboard.
//! 3. **Read after external copy**: prompts the user to run `wl-copy
//!    "external"` and waits a moment so the next read sees that content.
//!
//! Run with:
//!
//! ```sh
//! RUST_LOG="info,uc_platform=debug" cargo run --example wayland_clipboard_test -p uc-platform
//! ```

use std::sync::Arc;
use std::time::Duration;

use uc_core::clipboard::{MimeType, ObservedClipboardRepresentation, SystemClipboardSnapshot};
use uc_core::ids::RepresentationId;
use uc_core::ports::SystemClipboardPort;
use uc_platform::clipboard::LocalClipboard;

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,uc_platform=debug")),
        )
        .with_target(true)
        .init();

    eprintln!(
        "(WAYLAND_DISPLAY={:?})",
        std::env::var_os("WAYLAND_DISPLAY")
    );

    let clipboard: Arc<dyn SystemClipboardPort> = Arc::new(LocalClipboard::new()?);

    // ---- Write ----
    let payload = format!(
        "phase2b verification {}",
        chrono::Local::now().format("%H:%M:%S%.3f")
    );
    let write_snap = SystemClipboardSnapshot {
        ts_ms: chrono::Utc::now().timestamp_millis(),
        representations: vec![ObservedClipboardRepresentation::new(
            RepresentationId::new(),
            "text".into(),
            Some(MimeType("text/plain;charset=utf-8".into())),
            payload.clone().into_bytes(),
        )],
        file_content_digests: Vec::new(),
    };
    eprintln!("[1/3] writing via WaylandClipboard: {payload:?}");
    clipboard.write_snapshot(write_snap)?;
    eprintln!("    write OK");

    // Give the compositor a moment to reflect Selection back to our worker.
    std::thread::sleep(Duration::from_millis(120));

    // ---- wl-paste verification ----
    eprintln!("[2/3] asking wl-paste to read what we just wrote...");
    let out = std::process::Command::new("wl-paste")
        .arg("--no-newline")
        .output();
    match out {
        Ok(o) if o.status.success() => {
            let s = String::from_utf8_lossy(&o.stdout);
            if s == payload {
                eprintln!("    wl-paste sees expected payload ✓");
            } else {
                eprintln!("    wl-paste mismatch! expected={payload:?} got={s:?}",);
            }
        }
        Ok(o) => {
            eprintln!("    wl-paste exited non-zero: {o:?}");
        }
        Err(e) => {
            eprintln!("    failed to invoke wl-paste: {e}");
        }
    }

    // ---- Read via WaylandClipboard ----
    eprintln!("[3/3] reading back via WaylandClipboard.read_snapshot...");
    let snap = clipboard.read_snapshot()?;
    eprintln!("    snapshot: {} reps", snap.representations.len());
    for rep in &snap.representations {
        let bytes = rep.inline_bytes().unwrap_or(&[]);
        let preview: String = String::from_utf8_lossy(bytes).chars().take(80).collect();
        eprintln!(
            "      - format={} mime={:?} bytes={} preview={:?}",
            rep.format_id,
            rep.mime.as_ref().map(|m| m.0.as_str()),
            bytes.len(),
            preview
        );
    }

    eprintln!("\ndone — try `wl-paste` in another shell to see {payload:?}");
    Ok(())
}
