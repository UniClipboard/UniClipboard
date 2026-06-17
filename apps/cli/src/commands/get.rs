//! `uniclip get` — one-shot reader for already-synced clipboard entries.
//!
//! Unlike `recv` (which subscribes and BLOCKS waiting for the *next* inbound
//! file), `get` reads what is *already* in the daemon's history and returns
//! immediately. It is designed to be called by scripts and agents (e.g. an
//! editor/agent pulling the latest synced image on a headless SSH box, where
//! there is no system clipboard to paste from).
//!
//! Daemon-client mode: connects to a running daemon (or spawns a transient
//! one), holds a control lease while it fetches, materializes the selected
//! entry, and exits. It NEVER writes the system clipboard.
//!
//! ## Output contract (agent-friendly)
//!
//! * **text / link** → content is printed to **stdout** (pipe-friendly). A
//!   trailing newline is appended only when stdout is an interactive terminal,
//!   so piped/redirected output keeps the exact bytes.
//!   `--out` does not apply; redirect with `>` to capture to a file.
//! * **image / file** → bytes are written to the `--out` directory (default
//!   cache dir) and the **absolute path** is printed to stdout. `--out -`
//!   streams the raw bytes to stdout instead.
//!
//! All human-readable status lines go to **stderr** (via `ui`), so stdout
//! stays clean for both the path string and raw binary.
//!
//! ## Exit codes
//!
//! * `0` — entry materialized successfully (or `--list` printed).
//! * `EXIT_NO_MATCH` — no entry matched the selector.
//! * `EXIT_CONTENT_UNAVAILABLE` — entry exists but its payload is `Lost` /
//!   not materialized; the remedy is to re-send it from the source device.

use std::io::{IsTerminal, Write};
use std::path::PathBuf;

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use clap::ValueEnum;
use serde::Serialize;
use uc_daemon_client::DaemonService;
use uc_daemon_contract::api::dto::clipboard::EntryProjectionResponseDto;

use crate::exit_codes;
use crate::ui;

/// Default number of recent entries to scan when selecting / listing.
const DEFAULT_LIMIT: usize = 50;

/// Content kind selector for `--type`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum GetKind {
    Image,
    File,
    Text,
    Link,
}

/// Classified category of an entry, derived from its MIME type. The list
/// projection's `content_type` is the raw representation MIME (e.g.
/// `image/png`, `text/uri-list`, `text/plain`), not a category label.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Category {
    Image,
    File,
    Text,
    Link,
}

impl Category {
    fn as_str(self) -> &'static str {
        match self {
            Category::Image => "image",
            Category::File => "file",
            Category::Text => "text",
            Category::Link => "link",
        }
    }
}

pub struct GetArgs {
    /// Restrict selection to the newest entry of this kind.
    pub kind: Option<GetKind>,
    /// Select a specific entry by id (from `uniclip search query`).
    pub id: Option<String>,
    /// List recent entries instead of materializing one.
    pub list: bool,
    /// Number of recent entries to scan / list.
    pub limit: Option<usize>,
    /// Output target for image/file bytes: a directory, or `-` for stdout.
    pub out: Option<String>,
}

pub async fn run(args: GetArgs, json: bool, verbose: bool) -> i32 {
    let service = match crate::commands::app_session::connect_or_spawn_oneshot_daemon(verbose).await
    {
        Ok(s) => s,
        Err(code) => return code,
    };

    // Hold a control lease so a transient Oneshot daemon does not self-terminate
    // mid-fetch. Bind to a named var (NOT `_`) so it lives to scope end.
    let _lease = match service.hold_control_lease().await {
        Ok(guard) => guard,
        Err(err) => {
            ui::error(&format!("Failed to hold daemon session lease: {err}"));
            return exit_codes::EXIT_ERROR;
        }
    };

    let limit = args.limit.unwrap_or(DEFAULT_LIMIT).max(1);
    let entries = match service.list_entries(limit, 0).await {
        Ok(entries) => entries,
        Err(err) => {
            ui::error(&format!("Failed to list clipboard entries: {err}"));
            return exit_codes::EXIT_ERROR;
        }
    };

    if args.list {
        return print_list(&entries, json);
    }

    let target = match select_target(&entries, &args) {
        Ok(t) => t,
        Err(code) => return code,
    };

    materialize(&*service, target, &args, json).await
}

/// Pick the entry to materialize. For `--id`, find that exact entry. For
/// `--type` / default, take the newest *usable* (non-`Lost`) match.
fn select_target<'a>(
    entries: &'a [EntryProjectionResponseDto],
    args: &GetArgs,
) -> Result<&'a EntryProjectionResponseDto, i32> {
    if let Some(id) = &args.id {
        return match entries.iter().find(|e| &e.id == id) {
            Some(entry) => {
                if is_lost(entry) {
                    ui::error(&format!(
                        "Entry {} exists but its payload is no longer available (Lost). \
                         Re-send it from the source device.",
                        short_hash(id)
                    ));
                    Err(exit_codes::EXIT_CONTENT_UNAVAILABLE)
                } else {
                    Ok(entry)
                }
            }
            None => {
                ui::error(&format!(
                    "No entry with id {} in the latest {} entries. It may be older — \
                     raise --limit, or find it with `uniclip search query`.",
                    short_hash(id),
                    entries.len()
                ));
                Err(exit_codes::EXIT_NO_MATCH)
            }
        };
    }

    // newest-first; skip Lost entries so we return something usable.
    let chosen = entries.iter().find(|e| {
        !is_lost(e)
            && match args.kind {
                Some(kind) => classify(e) == kind_to_category(kind),
                None => true,
            }
    });

    match chosen {
        Some(entry) => Ok(entry),
        None => {
            let what = match args.kind {
                Some(kind) => format!("{} entry", kind_to_category(kind).as_str()),
                None => "usable entry".to_string(),
            };
            ui::error(&format!(
                "No {what} found in the latest {} entries.",
                entries.len()
            ));
            Err(exit_codes::EXIT_NO_MATCH)
        }
    }
}

async fn materialize(
    service: &dyn DaemonService,
    target: &EntryProjectionResponseDto,
    args: &GetArgs,
    json: bool,
) -> i32 {
    let category = classify(target);
    match category {
        Category::Text | Category::Link => emit_text(service, target, category, json).await,
        Category::File => emit_file(service, target, args, json).await,
        Category::Image => emit_image(service, target, args, json).await,
    }
}

async fn emit_text(
    service: &dyn DaemonService,
    target: &EntryProjectionResponseDto,
    category: Category,
    json: bool,
) -> i32 {
    let detail = match service.entry_detail(&target.id).await {
        Ok(Some(detail)) => detail,
        Ok(None) => return content_unavailable(&target.id),
        Err(err) => {
            ui::error(&format!("Failed to read entry text: {err}"));
            return exit_codes::EXIT_ERROR;
        }
    };

    if json {
        print_json(&GetOutcome {
            entry_id: &target.id,
            content_type: category.as_str(),
            mime_type: &target.content_type,
            path: None,
            filename: None,
            text: Some(&detail.content),
            bytes_written: None,
            captured_at: target.captured_at,
            outcome: "exported",
        });
    } else {
        // Raw content to stdout. When stdout is an interactive terminal, append
        // a trailing newline (if absent) so the shell prompt starts on its own
        // line. When piped/redirected, keep the bytes verbatim so callers like
        // `uniclip get | xclip` or `$(uniclip get)` capture the exact content.
        print!("{}", detail.content);
        if std::io::stdout().is_terminal() && !detail.content.ends_with('\n') {
            println!();
        }
        let _ = std::io::stdout().flush();
    }
    exit_codes::EXIT_SUCCESS
}

async fn emit_file(
    service: &dyn DaemonService,
    target: &EntryProjectionResponseDto,
    args: &GetArgs,
    json: bool,
) -> i32 {
    match service.export_entry_file(&target.id).await {
        Ok(Some(export)) => write_bytes_outcome(
            target,
            Category::File,
            &sanitize_filename(&export.filename),
            export.bytes,
            args,
            json,
        ),
        Ok(None) => content_unavailable(&target.id),
        Err(err) => {
            ui::error(&format!("Failed to export file: {err}"));
            exit_codes::EXIT_ERROR
        }
    }
}

async fn emit_image(
    service: &dyn DaemonService,
    target: &EntryProjectionResponseDto,
    args: &GetArgs,
    json: bool,
) -> i32 {
    let resource = match service.entry_resource(&target.id).await {
        Ok(Some(resource)) => resource,
        Ok(None) => return content_unavailable(&target.id),
        Err(err) => {
            ui::error(&format!("Failed to read image resource: {err}"));
            return exit_codes::EXIT_ERROR;
        }
    };

    let mime = resource
        .mime_type
        .clone()
        .unwrap_or_else(|| target.content_type.clone());

    // Small images are stored inline (base64); larger ones live in a blob.
    let bytes = if let Some(b64) = resource.inline_data.as_deref() {
        match STANDARD.decode(b64) {
            Ok(bytes) => bytes,
            Err(err) => {
                ui::error(&format!("Failed to decode inline image data: {err}"));
                return exit_codes::EXIT_ERROR;
            }
        }
    } else if let Some(blob_id) = resource.blob_id.as_deref() {
        match service.fetch_blob(blob_id).await {
            Ok(Some(bytes)) => bytes,
            Ok(None) => return content_unavailable(&target.id),
            Err(err) => {
                ui::error(&format!("Failed to fetch image blob: {err}"));
                return exit_codes::EXIT_ERROR;
            }
        }
    } else {
        return content_unavailable(&target.id);
    };

    let filename = format!("clip-{}.{}", short_hash(&target.id), ext_from_mime(&mime));
    write_bytes_outcome(target, Category::Image, &filename, bytes, args, json)
}

/// Common sink for image/file bytes: stdout (`--out -`) or a file under the
/// resolved output directory.
fn write_bytes_outcome(
    target: &EntryProjectionResponseDto,
    category: Category,
    filename: &str,
    bytes: Vec<u8>,
    args: &GetArgs,
    json: bool,
) -> i32 {
    let bytes_written = bytes.len() as u64;

    if args.out.as_deref() == Some("-") {
        if json {
            ui::error(
                "--json cannot be combined with --out - because the JSON would corrupt the raw byte stream on stdout",
            );
            return exit_codes::EXIT_ERROR;
        }
        if let Err(err) = std::io::stdout().write_all(&bytes) {
            ui::error(&format!("Failed to write bytes to stdout: {err}"));
            return exit_codes::EXIT_ERROR;
        }
        let _ = std::io::stdout().flush();
        return exit_codes::EXIT_SUCCESS;
    }

    let out_dir = match resolve_out_dir(args.out.as_deref()) {
        Ok(dir) => dir,
        Err(msg) => {
            ui::error(&msg);
            return exit_codes::EXIT_ERROR;
        }
    };
    let target_path = out_dir.join(filename);
    if let Err(err) = std::fs::write(&target_path, &bytes) {
        ui::error(&format!("Failed to write file: {err}"));
        return exit_codes::EXIT_ERROR;
    }
    let path_str = target_path.display().to_string();

    if json {
        print_json(&GetOutcome {
            entry_id: &target.id,
            content_type: category.as_str(),
            mime_type: &target.content_type,
            path: Some(&path_str),
            filename: Some(filename),
            text: None,
            bytes_written: Some(bytes_written),
            captured_at: target.captured_at,
            outcome: "exported",
        });
    } else {
        // The path is the one machine-readable line on stdout; status on stderr.
        println!("{path_str}");
        ui::info("type", category.as_str());
        ui::info("bytes", &bytes_written.to_string());
        ui::end("Done");
    }
    exit_codes::EXIT_SUCCESS
}

fn print_list(entries: &[EntryProjectionResponseDto], json: bool) -> i32 {
    if json {
        let rows: Vec<ListRow> = entries
            .iter()
            .map(|e| ListRow {
                entry_id: &e.id,
                content_type: classify(e).as_str(),
                mime_type: &e.content_type,
                captured_at: e.captured_at,
                preview: &e.preview,
                lost: is_lost(e),
            })
            .collect();
        if let Ok(s) = serde_json::to_string_pretty(&rows) {
            println!("{s}");
        }
        return exit_codes::EXIT_SUCCESS;
    }

    if entries.is_empty() {
        ui::info("status", "clipboard history is empty");
        return exit_codes::EXIT_SUCCESS;
    }
    ui::header("Recent clipboard entries");
    for entry in entries {
        let tag = classify(entry).as_str();
        let lost = if is_lost(entry) { " [lost]" } else { "" };
        let preview = first_line(&entry.preview, 48);
        ui::info(short_hash(&entry.id), &format!("[{tag}]{lost} {preview}"));
    }
    ui::end("Done");
    exit_codes::EXIT_SUCCESS
}

fn content_unavailable(entry_id: &str) -> i32 {
    ui::error(&format!(
        "Entry {} has no materialized payload yet (Lost or not downloaded). \
         Re-send it from the source device.",
        short_hash(entry_id)
    ));
    exit_codes::EXIT_CONTENT_UNAVAILABLE
}

// ── Classification helpers ──────────────────────────────────────────

fn classify(entry: &EntryProjectionResponseDto) -> Category {
    let mime = entry.content_type.to_ascii_lowercase();
    if mime.starts_with("image/") {
        Category::Image
    } else if mime == "text/uri-list" || mime == "file/uri-list" {
        Category::File
    } else if entry
        .link_urls
        .as_ref()
        .is_some_and(|urls| !urls.is_empty())
    {
        Category::Link
    } else {
        Category::Text
    }
}

fn kind_to_category(kind: GetKind) -> Category {
    match kind {
        GetKind::Image => Category::Image,
        GetKind::File => Category::File,
        GetKind::Text => Category::Text,
        GetKind::Link => Category::Link,
    }
}

fn is_lost(entry: &EntryProjectionResponseDto) -> bool {
    entry
        .payload_state
        .as_deref()
        .is_some_and(|s| s.eq_ignore_ascii_case("lost"))
}

fn ext_from_mime(mime: &str) -> &'static str {
    match mime.to_ascii_lowercase().as_str() {
        "image/png" => "png",
        "image/jpeg" | "image/jpg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "image/bmp" => "bmp",
        "image/tiff" => "tiff",
        "image/svg+xml" => "svg",
        _ => "bin",
    }
}

// ── Filesystem / formatting helpers ─────────────────────────────────

/// Resolve the output directory: the user-chosen `--out`, or the default
/// per-user cache dir. Creates it if missing.
fn resolve_out_dir(out: Option<&str>) -> Result<PathBuf, String> {
    let dir = match out {
        Some(p) => PathBuf::from(p),
        None => default_out_dir(),
    };
    if !dir.exists() {
        std::fs::create_dir_all(&dir)
            .map_err(|err| format!("Failed to create output directory: {err}"))?;
    } else if !dir.is_dir() {
        return Err(format!("Output path is not a directory: {}", dir.display()));
    }
    dir.canonicalize()
        .map_err(|err| format!("Failed to canonicalize output directory: {err}"))
}

/// Default landing directory: `$XDG_CACHE_HOME/uniclip/get`, else
/// `$HOME/.cache/uniclip/get`, else a temp-dir fallback.
fn default_out_dir() -> PathBuf {
    if let Ok(xdg) = std::env::var("XDG_CACHE_HOME") {
        if !xdg.is_empty() {
            return PathBuf::from(xdg).join("uniclip").join("get");
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        if !home.is_empty() {
            return PathBuf::from(home)
                .join(".cache")
                .join("uniclip")
                .join("get");
        }
    }
    std::env::temp_dir().join("uniclip-get")
}

/// Strip path separators a malicious sender might inject. Never trust the
/// remote-supplied filename verbatim. Mirrors `recv::sanitize_filename`.
fn sanitize_filename(name: &str) -> String {
    let stripped: String = name
        .chars()
        .filter(|c| !matches!(c, '/' | '\\') && !c.is_control())
        .collect();
    if stripped.is_empty() || stripped == "." || stripped == ".." {
        "uniclip-get.bin".to_string()
    } else {
        stripped
    }
}

fn short_hash(s: &str) -> &str {
    match s.char_indices().nth(8) {
        Some((byte_idx, _)) => &s[..byte_idx],
        None => s,
    }
}

fn first_line(s: &str, max: usize) -> String {
    let line = s.lines().next().unwrap_or("");
    if line.chars().count() > max {
        let truncated: String = line.chars().take(max).collect();
        format!("{truncated}…")
    } else {
        line.to_string()
    }
}

// ── JSON output shapes (snake_case, mirroring `recv`) ────────────────

#[derive(Serialize)]
struct GetOutcome<'a> {
    entry_id: &'a str,
    /// Classified kind: `text` | `link` | `image` | `file`.
    content_type: &'a str,
    /// Raw representation MIME, e.g. `image/png`.
    mime_type: &'a str,
    /// Absolute path of the written file; null for text or `--out -`.
    path: Option<&'a str>,
    filename: Option<&'a str>,
    /// Text content; null for image/file.
    text: Option<&'a str>,
    bytes_written: Option<u64>,
    captured_at: i64,
    outcome: &'static str,
}

#[derive(Serialize)]
struct ListRow<'a> {
    entry_id: &'a str,
    content_type: &'a str,
    mime_type: &'a str,
    captured_at: i64,
    preview: &'a str,
    lost: bool,
}

fn print_json<T: Serialize>(value: &T) {
    if let Ok(s) = serde_json::to_string_pretty(value) {
        println!("{s}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a projection with sane defaults; tests override the fields they
    /// care about (content_type, link_urls, payload_state).
    fn projection(content_type: &str) -> EntryProjectionResponseDto {
        EntryProjectionResponseDto {
            id: "ent-test".to_string(),
            preview: "preview".to_string(),
            has_detail: true,
            size_bytes: 0,
            captured_at: 0,
            content_type: content_type.to_string(),
            thumbnail_url: None,
            is_encrypted: false,
            is_favorited: false,
            updated_at: 0,
            active_time: 0,
            file_transfer_status: None,
            file_transfer_reason: None,
            link_urls: None,
            link_domains: None,
            file_sizes: None,
            image_width: None,
            image_height: None,
            payload_state: None,
        }
    }

    #[test]
    fn classify_uses_mime_prefix() {
        assert_eq!(classify(&projection("image/png")), Category::Image);
        assert_eq!(classify(&projection("image/jpeg")), Category::Image);
        assert_eq!(classify(&projection("text/uri-list")), Category::File);
        assert_eq!(classify(&projection("text/plain")), Category::Text);
        // Unknown / empty mime falls back to text.
        assert_eq!(classify(&projection("unknown")), Category::Text);
    }

    #[test]
    fn classify_detects_link_via_link_urls() {
        let mut entry = projection("text/plain");
        entry.link_urls = Some(vec!["https://example.com".to_string()]);
        assert_eq!(classify(&entry), Category::Link);
        // Empty link_urls is NOT a link.
        entry.link_urls = Some(vec![]);
        assert_eq!(classify(&entry), Category::Text);
    }

    #[test]
    fn image_mime_wins_over_link_urls() {
        // An image entry that happens to carry link_urls is still an image.
        let mut entry = projection("image/png");
        entry.link_urls = Some(vec!["https://x".to_string()]);
        assert_eq!(classify(&entry), Category::Image);
    }

    #[test]
    fn ext_from_mime_maps_known_image_types() {
        assert_eq!(ext_from_mime("image/png"), "png");
        assert_eq!(ext_from_mime("image/jpeg"), "jpg");
        assert_eq!(ext_from_mime("IMAGE/PNG"), "png");
        assert_eq!(ext_from_mime("image/heic"), "bin");
    }

    #[test]
    fn is_lost_is_case_insensitive() {
        let mut entry = projection("image/png");
        assert!(!is_lost(&entry));
        entry.payload_state = Some("Lost".to_string());
        assert!(is_lost(&entry));
        entry.payload_state = Some("lost".to_string());
        assert!(is_lost(&entry));
        entry.payload_state = Some("Available".to_string());
        assert!(!is_lost(&entry));
    }

    #[test]
    fn sanitize_filename_strips_separators() {
        assert_eq!(sanitize_filename("../../etc/passwd"), "....etcpasswd");
        assert_eq!(sanitize_filename("a/b\\c"), "abc");
        assert_eq!(sanitize_filename(""), "uniclip-get.bin");
        assert_eq!(sanitize_filename(".."), "uniclip-get.bin");
        assert_eq!(sanitize_filename("photo.png"), "photo.png");
    }

    #[test]
    fn short_hash_truncates_to_eight() {
        assert_eq!(short_hash("0123456789abcdef"), "01234567");
        assert_eq!(short_hash("abc"), "abc");
    }

    #[test]
    fn first_line_truncates_and_takes_first_line() {
        assert_eq!(first_line("hello\nworld", 80), "hello");
        assert_eq!(first_line("abcdef", 3), "abc…");
        assert_eq!(first_line("", 10), "");
    }
}
