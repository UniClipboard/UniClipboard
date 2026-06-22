//! End-to-end smoke test for `X11Clipboard::{read_snapshot,write_snapshot}`.
//!
//! Forces the X11 backend by unsetting `WAYLAND_DISPLAY`, so even on a
//! Wayland session this exercises x11rb against the local XWayland server.
//!
//! Exercises:
//!
//! 1. **Write**: pushes a synthetic snapshot into the X11 CLIPBOARD selection.
//! 2. **xclip verification**: shells out to `xclip` to confirm an external
//!    paster can read what we wrote.
//! 3. **Read**: asks the worker to return the snapshot it just installed
//!    (cached path) and prints it.
//!
//! Run with:
//!
//! ```sh
//! RUST_LOG="info,uc_platform=debug" cargo run --example x11_clipboard_test -p uc-platform
//! ```

use std::sync::Arc;

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

    // Force the native X11 backend even on a Wayland session.
    // SAFETY: single-threaded main, no concurrent env access.
    unsafe {
        std::env::remove_var("WAYLAND_DISPLAY");
    }

    eprintln!("(DISPLAY={:?})", std::env::var_os("DISPLAY"));

    let clipboard: Arc<dyn SystemClipboardPort> = Arc::new(LocalClipboard::new()?);

    let payload = format!(
        "phase3 x11 verification {}",
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
    eprintln!("[1/3] writing via X11Clipboard: {payload:?}");
    clipboard.write_snapshot(write_snap)?;
    eprintln!("    write OK");

    // Give the worker a moment to register with the server before xclip races.
    std::thread::sleep(std::time::Duration::from_millis(120));

    eprintln!("[2/3] asking xclip / xsel to read what we just wrote...");
    let attempts: &[(&str, &[&str])] = &[
        ("xclip", &["-selection", "clipboard", "-o"]),
        ("xsel", &["--clipboard", "--output"]),
    ];
    let mut external_verified = false;
    for (cmd, args) in attempts {
        match std::process::Command::new(cmd)
            .args(args.iter().copied())
            .output()
        {
            Ok(o) if o.status.success() => {
                let s = String::from_utf8_lossy(&o.stdout)
                    .trim_end_matches('\n')
                    .to_string();
                if s == payload {
                    eprintln!("    {cmd} sees expected payload ✓");
                    external_verified = true;
                    break;
                } else {
                    eprintln!("    {cmd} mismatch! expected={payload:?} got={s:?}");
                }
            }
            Ok(o) => eprintln!("    {cmd} exited non-zero: {o:?}"),
            Err(e) => eprintln!("    failed to invoke {cmd}: {e}"),
        }
    }
    if !external_verified {
        eprintln!("    (install xclip or xsel to verify the write end externally)");
    }

    eprintln!("[3/3] reading back via X11Clipboard.read_snapshot...");
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

    eprintln!("\ndone — try `xclip -selection clipboard -o` in another shell to see {payload:?}");
    Ok(())
}
