//! `uniclip probe` —— 隐藏的剪贴板诊断子命令组。
//!
//! 由独立的 `uc-clipboard-probe` 二进制收编而来：只面向开发与 E2E
//! 调试，用于观察平台剪贴板事件、抓取/检查快照、必要时把快照写回
//! 系统剪贴板。`probe restore` 是 `uniclip` 唯一允许写系统剪贴板的
//! 入口，作为 `uc-cli/AGENTS.md` 中"CLI 不写系统剪贴板"规则的诊断
//! 例外存在 —— 不要把它当作公开命令使用，也不要在生产流程上依赖。
//!
//! Phase 4 switched `watch` from a direct `clipboard_rs::ClipboardWatcherContext`
//! to `uc_platform::clipboard::build_event_loop` so the diagnostic surface
//! exercises the same backend (Wayland data-control / x11rb / clipboard_rs)
//! the daemon picks on each OS.

use std::fs;
use std::sync::Arc;
use std::time::Instant;

use anyhow::{anyhow, Result};
use clap::Subcommand;
use tokio::sync::mpsc;

use uc_core::ports::SystemClipboardPort;
use uc_core::{ObservedClipboardRepresentation, SystemClipboardSnapshot};
use uc_platform::clipboard::{
    build_event_loop, shutdown_channel, watcher::ClipboardWatcher, watcher::PlatformEvent,
    LocalClipboard,
};

use crate::exit_codes;

#[derive(Subcommand)]
pub enum ProbeCommands {
    /// Watch clipboard changes (default mode)
    Watch {
        /// Stop after N events
        #[arg(short, long)]
        max_events: Option<usize>,
    },
    /// Capture current clipboard to file
    Capture {
        /// Output file path
        #[arg(short, long)]
        out: String,
    },
    /// Restore clipboard from file (writes the system clipboard — diagnostic only)
    Restore {
        /// Input file path
        #[arg(short, long)]
        r#in: String,
        /// Select representation by index (0-based)
        #[arg(short, long)]
        select: Option<usize>,
    },
    /// Inspect snapshot file
    Inspect {
        /// Input file path
        #[arg(short, long)]
        r#in: String,
    },
}

pub async fn run(command: ProbeCommands, _verbose: bool) -> i32 {
    // 整个 probe 子命令是同步的 (clipboard-rs 的 watcher 走 std::thread,
    // serde_json + fs 也是同步)。塞进 spawn_blocking 避免阻塞 tokio
    // 的工作线程。
    let join = tokio::task::spawn_blocking(move || dispatch(command)).await;

    let result = match join {
        Ok(inner) => inner,
        Err(err) => Err(anyhow!("probe task panicked: {err}")),
    };

    match result {
        Ok(()) => exit_codes::EXIT_SUCCESS,
        Err(err) => {
            eprintln!("probe failed: {err:#}");
            exit_codes::EXIT_ERROR
        }
    }
}

fn dispatch(command: ProbeCommands) -> Result<()> {
    match command {
        ProbeCommands::Watch { max_events } => run_watch(max_events),
        ProbeCommands::Capture { out } => run_capture(out),
        ProbeCommands::Restore { r#in, select } => run_restore(r#in, select),
        ProbeCommands::Inspect { r#in } => run_inspect(r#in),
    }
}

fn run_watch(max_events: Option<usize>) -> Result<()> {
    println!("probe: watch mode");
    println!(
        "- max_events: {}",
        max_events.map_or("none".into(), |v| v.to_string())
    );
    println!("- stop: Ctrl+C");

    let clipboard: Arc<dyn SystemClipboardPort> = Arc::new(LocalClipboard::new()?);
    match clipboard.read_snapshot() {
        Ok(snapshot) => {
            println!("\ninitial snapshot");
            print_snapshot(&snapshot);
        }
        Err(err) => {
            println!("\ninitial snapshot error: {err}");
        }
    }

    let (tx, mut rx) = mpsc::channel::<PlatformEvent>(64);
    let handler = ClipboardWatcher::new(clipboard, tx);
    let event_loop = build_event_loop()?;
    let (shutdown_tx, shutdown_rx) = shutdown_channel();

    // Run the (blocking) event loop on a dedicated OS thread. The diagnostic
    // surface intentionally goes through the same `build_event_loop` factory
    // the daemon uses, so what `probe watch` shows on each OS matches the
    // backend the daemon picks (Wayland data-control / x11rb / clipboard_rs).
    let event_loop_thread = std::thread::Builder::new()
        .name("probe-clipboard-watcher".into())
        .spawn(move || {
            println!("\nclipboard watcher: started");
            if let Err(err) = event_loop.run(handler, shutdown_rx) {
                eprintln!("clipboard watcher exited with error: {err:?}");
            }
            println!("clipboard watcher: stopped");
        })
        .map_err(|e| anyhow!("failed to spawn watcher thread: {e}"))?;

    let mut last_event_instant: Option<Instant> = None;
    let mut last_fingerprint: Option<u64> = None;
    let mut same_streak: usize = 0;
    let mut event_count: usize = 0;

    // `dispatch` is invoked from `tokio::task::spawn_blocking`, so we can
    // bridge between the blocking world and the tokio mpsc with
    // `blocking_recv`.
    while let Some(PlatformEvent::ClipboardChanged { snapshot }) = rx.blocking_recv() {
        event_count += 1;
        let observed_ms = chrono::Utc::now().timestamp_millis();
        let observed_instant = Instant::now();

        let delta_ms =
            last_event_instant.map(|instant| observed_instant.duration_since(instant).as_millis());
        last_event_instant = Some(observed_instant);

        println!("\nevent #{event_count}");
        println!("- observed: {}", format_ms(observed_ms));
        println!(
            "- delta_ms: {}",
            delta_ms.map_or("n/a".into(), |v| v.to_string())
        );

        let fingerprint = snapshot_fingerprint(&snapshot);
        if last_fingerprint == Some(fingerprint) {
            same_streak += 1;
        } else {
            same_streak = 0;
        }
        last_fingerprint = Some(fingerprint);

        println!("- same_content_streak: {same_streak}");
        print_snapshot(&snapshot);

        if let Some(limit) = max_events {
            if event_count >= limit {
                println!("\nmax_events reached, exiting");
                break;
            }
        }
    }

    shutdown_tx.signal();
    let _ = event_loop_thread.join();
    Ok(())
}

fn run_capture(out: String) -> Result<()> {
    println!("probe: capture mode");
    println!("- output: {out}");

    let clipboard = LocalClipboard::new()?;
    let snapshot = clipboard.read_snapshot()?;

    println!("\ncaptured snapshot:");
    print_snapshot(&snapshot);

    let json = serde_json::to_string_pretty(&snapshot)?;
    fs::write(&out, json)?;

    println!("\nsnapshot written to: {out}");

    Ok(())
}

fn run_restore(input: String, select: Option<usize>) -> Result<()> {
    println!("probe: restore mode");
    println!("- input: {input}");

    let json = fs::read_to_string(&input)?;
    let mut snapshot: SystemClipboardSnapshot = serde_json::from_str(&json)?;

    println!("\nrestoring snapshot:");
    print_snapshot(&snapshot);

    if snapshot.representations.is_empty() {
        println!("error: snapshot has no representations");
        return Ok(());
    }

    if snapshot.representations.len() > 1 {
        println!(
            "\nwarning: snapshot has {} representations, \
            current implementation only supports single representation restore",
            snapshot.representations.len()
        );

        let index = select.unwrap_or(0);
        if index >= snapshot.representations.len() {
            println!("error: selected index {index} out of range");
            return Ok(());
        }

        println!("using representation at index {index}");
        for (idx, rep) in snapshot.representations.iter().enumerate() {
            let marker = if idx == index { "->[SELECTED]" } else { "  " };
            println!(
                "{}  rep[{}]: format_id={}, mime={:?}, size={}",
                marker,
                idx,
                rep.format_id,
                rep.mime,
                rep.size_bytes()
            );
        }

        snapshot.representations = vec![snapshot.representations.remove(index)];
    }

    let clipboard = LocalClipboard::new()?;
    clipboard.write_snapshot(snapshot)?;

    println!("\nsnapshot restored to clipboard");

    Ok(())
}

fn run_inspect(input: String) -> Result<()> {
    println!("probe: inspect mode");
    println!("- input: {input}");

    let json = fs::read_to_string(&input)?;
    let snapshot: SystemClipboardSnapshot = serde_json::from_str(&json)?;

    println!("\ninspected snapshot:");
    print_snapshot(&snapshot);

    Ok(())
}

fn print_snapshot(snapshot: &SystemClipboardSnapshot) {
    println!(
        "- snapshot.ts_ms: {} ({})",
        snapshot.ts_ms,
        format_ms(snapshot.ts_ms)
    );
    println!("- representations: {}", snapshot.representations.len());

    for (idx, rep) in snapshot.representations.iter().enumerate() {
        let desc = describe_representation(rep);
        println!("  rep[{idx}]: {desc}");
    }
}

fn describe_representation(rep: &ObservedClipboardRepresentation) -> String {
    let mime = rep.mime.as_ref().map(|m| m.as_str()).unwrap_or("-");
    // probe 只处理 JSON 反序列化产物 + 本机 watcher 当前 snapshot,均为 Inline source。
    // LocalFile source 在 probe 路径上无法出现(JSON serialize 会失败、watcher 还没有
    // 启用 LocalFile 模式)。退一步,即便 LocalFile 出现,展示空预览即可。
    let bytes_view: &[u8] = rep.inline_bytes().unwrap_or(&[]);
    let preview = if is_text_representation(mime, &rep.format_id) {
        format!("\"{}\"", text_preview(bytes_view, 160))
    } else {
        format!("hex:{}", hex_preview(bytes_view, 24))
    };

    format!(
        "format_id={} mime={} bytes={} preview={}",
        rep.format_id,
        mime,
        rep.size_bytes(),
        preview
    )
}

fn is_text_representation(mime: &str, format_id: &str) -> bool {
    if mime.starts_with("text/") {
        return true;
    }

    matches!(format_id, "text" | "rtf" | "html" | "files")
}

fn text_preview(bytes: &[u8], max_len: usize) -> String {
    let clipped_len = bytes.len().min(max_len);
    let text = String::from_utf8_lossy(&bytes[..clipped_len]);
    let mut escaped = text.escape_default().to_string();

    if bytes.len() > max_len {
        escaped.push_str("...");
    }

    escaped
}

fn hex_preview(bytes: &[u8], max_len: usize) -> String {
    if bytes.is_empty() {
        return "(empty)".to_string();
    }

    let mut out = String::new();
    for (idx, byte) in bytes.iter().take(max_len).enumerate() {
        if idx > 0 {
            out.push(' ');
        }
        out.push_str(&format!("{byte:02x}"));
    }

    if bytes.len() > max_len {
        out.push_str(" ...");
    }

    out
}

fn snapshot_fingerprint(snapshot: &SystemClipboardSnapshot) -> u64 {
    use std::hash::{Hash, Hasher};

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    snapshot.representations.len().hash(&mut hasher);

    for rep in &snapshot.representations {
        rep.format_id.hash(&mut hasher);
        rep.mime.hash(&mut hasher);
        rep.inline_bytes().unwrap_or(&[]).hash(&mut hasher);
    }

    hasher.finish()
}

fn format_ms(ms: i64) -> String {
    use chrono::TimeZone;

    chrono::Local
        .timestamp_millis_opt(ms)
        .single()
        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S%.3f").to_string())
        .unwrap_or_else(|| ms.to_string())
}
