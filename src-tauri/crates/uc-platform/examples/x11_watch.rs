//! Manual smoke test for the native X11 clipboard event loop (Phase 3).
//!
//! Forces the X11 backend by **unsetting** `WAYLAND_DISPLAY` before
//! constructing the clipboard, so this example exercises the new x11rb
//! path even on a Wayland session (where the box's XWayland server is what
//! we'll be talking to).
//!
//! Run with:
//!
//! ```sh
//! RUST_LOG=info,uc_platform=debug cargo run --example x11_watch -p uc-platform
//! ```
//!
//! Then in a second terminal try:
//! - `xclip -selection clipboard -i <<< "x11 hello"` (or just copy from any
//!   X11 / XWayland app) — expect a snapshot to print.
//! - Ctrl+C in this terminal to stop. The watcher releases its connection.

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

    // Force the native X11 backend even on a Wayland session.
    // SAFETY: single-threaded tokio runtime, no concurrent env access.
    unsafe {
        std::env::remove_var("WAYLAND_DISPLAY");
    }

    eprintln!("watching X11 CLIPBOARD via x11rb — copy from an X11/XWayland app");
    eprintln!("(DISPLAY={:?})", std::env::var_os("DISPLAY"));

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
                            let bytes = rep.inline_bytes().unwrap_or(&[]);
                            let preview = preview_bytes(bytes);
                            println!(
                                "  - format={} mime={:?} bytes={} preview={:?}",
                                rep.format_id,
                                rep.mime.as_ref().map(|m| m.0.as_str()),
                                bytes.len(),
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
