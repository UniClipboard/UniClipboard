//! Manual smoke test for the Linux platform clipboard event loop.
//!
//! Run with:
//!
//! ```sh
//! RUST_LOG=uc_platform=debug cargo run --example wayland_watch -p uc-platform
//! ```
//!
//! Then in a second terminal try:
//! - `wl-copy "hello wayland"` (native Wayland) — expect a snapshot to print.
//! - `echo png | xclip -selection clipboard -t image/png -i some.png` (X11)
//!   — when running under XWayland in a niri/sway/KDE Wayland session, the
//!   compositor mirrors X11 selections into the Wayland clipboard, so the
//!   wayland watcher should still see it.
//! - Ctrl+C in this terminal to stop. The watcher releases its wayland
//!   connection cleanly.
//!
//! On a session without `WAYLAND_DISPLAY`, the factory falls back to the
//! `clipboard_rs`-backed X11 adapter and you should still see snapshots
//! when X11 selection changes.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc;
use uc_platform::clipboard::{
    build_event_loop, shutdown_channel, watcher::ClipboardWatcher, watcher::PlatformEvent,
    LocalClipboard,
};

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,uc_platform=debug")),
        )
        .with_target(true)
        .init();

    eprintln!("watching the clipboard — copy something and watch this terminal");
    eprintln!(
        "(WAYLAND_DISPLAY={:?})",
        std::env::var_os("WAYLAND_DISPLAY")
    );

    let clipboard: Arc<dyn uc_core::ports::SystemClipboardPort> = Arc::new(LocalClipboard::new()?);
    let (tx, mut rx) = mpsc::channel::<PlatformEvent>(64);
    let handler = ClipboardWatcher::new(clipboard, tx);

    let event_loop = build_event_loop()?;
    let (shutdown_tx, shutdown_rx) = shutdown_channel();

    let join = tokio::task::spawn_blocking(move || {
        if let Err(err) = event_loop.run(handler, shutdown_rx) {
            eprintln!("event loop returned with error: {err:?}");
        }
    });

    let mut stop = std::pin::pin!(tokio::signal::ctrl_c());

    loop {
        tokio::select! {
            biased;
            _ = &mut stop => {
                eprintln!("\nctrl+c — shutting down");
                shutdown_tx.signal();
                break;
            }
            event = rx.recv() => {
                match event {
                    Some(PlatformEvent::ClipboardChanged { snapshot }) => {
                        println!(
                            "\n[{}] snapshot: {} reps",
                            chrono::Local::now().format("%H:%M:%S%.3f"),
                            snapshot.representations.len()
                        );
                        for rep in &snapshot.representations {
                            let preview = preview_bytes(&rep.bytes);
                            println!(
                                "  - format={} mime={:?} bytes={} preview={:?}",
                                rep.format_id,
                                rep.mime.as_ref().map(|m| m.0.as_str()),
                                rep.bytes.len(),
                                preview
                            );
                        }
                    }
                    None => {
                        eprintln!("event channel closed");
                        break;
                    }
                }
            }
        }
    }

    // Give the event loop a moment to clean up before we exit, but don't
    // hang the example if it doesn't.
    let _ = tokio::time::timeout(Duration::from_secs(2), join).await;
    Ok(())
}

fn preview_bytes(b: &[u8]) -> String {
    if let Ok(s) = std::str::from_utf8(b) {
        let trimmed: String = s.chars().take(60).collect();
        if s.len() > trimmed.len() {
            format!("{trimmed}…")
        } else {
            trimmed
        }
    } else {
        format!("<{} binary bytes>", b.len())
    }
}
