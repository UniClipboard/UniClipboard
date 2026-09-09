use super::{DesktopTheme, DesktopThemeState};
use notify::{RecursiveMode, Watcher};
use std::{
    collections::BTreeMap,
    io::Read,
    path::{Path, PathBuf},
};
use tauri::Emitter;
use tokio::sync::mpsc;
use tracing::Instrument;

const EVENT: &str = "desktop-theme://changed";
const MAX_PALETTE_BYTES: u64 = 64 * 1024;

// An installed package alone is not an active Omarchy session.
fn source_directory(home: Option<PathBuf>, installation: Option<PathBuf>) -> Option<PathBuf> {
    let installation = installation?;
    if !installation.join("bin/omarchy-theme-set").is_file() {
        return None;
    }
    let current = home?.join(".local/state/omarchy/current");
    current.is_dir().then_some(current)
}

pub(super) fn install(
    app: tauri::AppHandle,
    state: DesktopThemeState,
    cancel: tokio_util::sync::CancellationToken,
) {
    let Some(directory) = source_directory(
        std::env::var_os("HOME").map(PathBuf::from),
        std::env::var_os("OMARCHY_PATH").map(PathBuf::from),
    ) else {
        return;
    };
    let span = tracing::info_span!("desktop_theme.watch", source = "omarchy");
    let (tx, mut rx) = mpsc::channel(1);
    let watcher = span.in_scope(|| watch_directory(&directory, tx));
    let watcher = match watcher {
        Ok(watcher) => watcher,
        Err(error) => {
            tracing::warn!(parent: &span, %error, "Unable to watch desktop theme");
            return;
        }
    };
    // Read after subscribing so a switch during startup cannot be lost.
    span.in_scope(|| publish(&app, &state, read_theme(&directory)));
    tauri::async_runtime::spawn(
        async move {
            let _watcher = watcher;
            tracing::debug!("Desktop theme watcher started");
            loop {
                tokio::select! {
                    _ = cancel.cancelled() => break,
                    event = rx.recv() => if event.is_none() { break; },
                }
                tokio::select! {
                    _ = cancel.cancelled() => break,
                    _ = tokio::time::sleep(std::time::Duration::from_millis(150)) => {},
                }
                while rx.try_recv().is_ok() {}
                let source = directory.clone();
                let result = tokio::task::spawn_blocking(move || read_theme(&source)).await;
                match result {
                    Ok(theme) => publish(&app, &state, theme),
                    Err(error) => tracing::warn!(%error, "Desktop theme reader failed"),
                }
            }
            tracing::debug!("Desktop theme watcher stopped");
        }
        .instrument(span),
    );
}

fn watch_directory(
    directory: &Path,
    tx: mpsc::Sender<()>,
) -> notify::Result<notify::RecommendedWatcher> {
    let span = tracing::Span::current();
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        match event {
            Ok(event) if matches!(event.kind, notify::EventKind::Access(_)) => {}
            // A full channel already contains the invalidation we need.
            Ok(_) => {
                let _ = tx.try_send(());
            }
            Err(error) => {
                tracing::warn!(parent: &span, %error, "Desktop theme filesystem watch failed")
            }
        }
    })?;
    // The theme directory is replaced on every switch. Watch its stable parent.
    watcher.watch(directory, RecursiveMode::Recursive)?;
    Ok(watcher)
}

fn publish<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    state: &DesktopThemeState,
    result: Result<DesktopTheme, &'static str>,
) {
    let theme = match result {
        Ok(theme) => theme,
        // Preserve the last valid palette during directory replacement or a broken edit.
        Err(error) => {
            tracing::warn!(
                error,
                "Desktop theme unavailable; retaining last valid palette"
            );
            return;
        }
    };
    let snapshot = match state.0.lock() {
        Ok(mut value) => {
            if value.theme.as_ref() == Some(&theme) {
                return;
            }
            value.theme = Some(theme);
            value.revision = value.revision.saturating_add(1);
            value.clone()
        }
        Err(_) => {
            tracing::warn!("Desktop theme state unavailable");
            return;
        }
    };
    if let Err(error) = app.emit(EVENT, snapshot) {
        tracing::warn!(%error, "Desktop theme broadcast failed");
    }
}

fn read_theme(directory: &Path) -> Result<DesktopTheme, &'static str> {
    let mut text = String::new();
    std::fs::File::open(directory.join("theme/colors.toml"))
        .map_err(|_| "palette_open")?
        .take(MAX_PALETTE_BYTES + 1)
        .read_to_string(&mut text)
        .map_err(|_| "palette_read")?;
    if text.len() as u64 > MAX_PALETTE_BYTES {
        return Err("palette_too_large");
    }
    parse_palette(&text, directory.join("theme/light.mode").is_file())
}

#[derive(Clone, Copy)]
struct Color([u8; 3]);
impl Color {
    fn parse(value: &str) -> Option<Self> {
        let hex = value.strip_prefix('#')?;
        if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        Some(Self([
            u8::from_str_radix(&hex[0..2], 16).ok()?,
            u8::from_str_radix(&hex[2..4], 16).ok()?,
            u8::from_str_radix(&hex[4..6], 16).ok()?,
        ]))
    }
    fn css(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.0[0], self.0[1], self.0[2])
    }
    fn mix(self, other: Self, amount: f64) -> Self {
        Self(std::array::from_fn(|i| {
            (f64::from(self.0[i]) * (1.0 - amount) + f64::from(other.0[i]) * amount).round() as u8
        }))
    }
    fn luminance(self) -> f64 {
        let linear = self.0.map(|value| {
            let c = f64::from(value) / 255.0;
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        });
        linear[0] * 0.2126 + linear[1] * 0.7152 + linear[2] * 0.0722
    }
    fn contrast_text(self) -> Self {
        if (self.luminance() + 0.05) / 0.05 >= 1.05 / (self.luminance() + 0.05) {
            Self([0; 3])
        } else {
            Self([255; 3])
        }
    }
}

fn parse_palette(text: &str, light_marker: bool) -> Result<DesktopTheme, &'static str> {
    // Never propagate TOML errors: their diagnostics may include arbitrary file contents.
    let palette: toml::Table = toml::from_str(text).map_err(|_| "invalid_toml")?;
    let get = |name: &str| {
        palette
            .get(name)
            .and_then(toml::Value::as_str)
            .and_then(Color::parse)
    };
    let bg = get("background").ok_or("missing_background")?;
    let fg = get("foreground").ok_or("missing_foreground")?;
    let primary = get("accent").or_else(|| get("blue")).unwrap_or(fg);
    let mode = palette
        .get("mode")
        .or_else(|| palette.get("theme_type"))
        .and_then(toml::Value::as_str);
    let dark = match mode {
        Some("dark") => true,
        Some("light") => false,
        Some(_) => return Err("invalid_mode"),
        // Match Omarchy's mode inference for palettes without mode metadata.
        None => !light_marker && bg.0.iter().map(|c| u16::from(*c)).sum::<u16>() <= 382,
    };
    let surface = get("lighter_background").unwrap_or(bg.mix(fg, 0.05));
    let sidebar = get("dark_background").unwrap_or(bg.mix(fg, 0.025));
    let selection = get("selection").unwrap_or(bg.mix(primary, 0.25));
    let border = get("muted").unwrap_or(bg.mix(fg, 0.2));
    let muted_fg = bg.mix(fg, 0.7);
    let destructive = get("red").unwrap_or(Color([220, 50, 47]));
    let pairs = [
        ("background", bg),
        ("foreground", fg),
        ("card", surface),
        ("card-foreground", fg),
        ("popover", surface),
        ("popover-foreground", fg),
        ("primary", primary),
        ("primary-foreground", primary.contrast_text()),
        ("secondary", surface),
        ("secondary-foreground", fg),
        ("muted", surface),
        ("muted-foreground", muted_fg),
        ("accent", selection),
        ("accent-foreground", fg),
        ("destructive", destructive),
        ("destructive-foreground", destructive.contrast_text()),
        ("border", border),
        ("input", border),
        ("ring", primary),
        ("sidebar", sidebar),
        ("sidebar-foreground", fg),
        ("sidebar-primary", primary),
        ("sidebar-primary-foreground", primary.contrast_text()),
        ("sidebar-accent", selection),
        ("sidebar-accent-foreground", fg),
        ("sidebar-border", border),
        ("sidebar-ring", primary),
        ("chart-1", primary),
        ("chart-2", get("green").unwrap_or(primary)),
        ("chart-3", get("yellow").unwrap_or(fg)),
        ("chart-4", get("magenta").unwrap_or(primary)),
        ("chart-5", get("cyan").unwrap_or(fg)),
    ];
    Ok(DesktopTheme {
        dark,
        variables: pairs
            .into_iter()
            .map(|(key, color)| (format!("--{key}"), color.css()))
            .collect::<BTreeMap<_, _>>(),
    })
}

#[cfg(test)]
mod tests;
