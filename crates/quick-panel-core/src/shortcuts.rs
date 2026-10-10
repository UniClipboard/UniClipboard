//! Which global shortcuts open the panel.
//!
//! Turns the shortcut strings of the settings (`meta+ctrl+v`, `mod+k mod+c`) into the physical-key
//! form the panel registers with the operating system, and picks the platform default when the
//! settings name none.

use std::collections::HashMap;

use uc_daemon_contract::api::dto::settings::ShortcutKeyDto;

/// The panel's default global shortcut, in physical-key form.
///
/// - macOS: `Cmd+Ctrl+V`
/// - Windows / Linux: `Ctrl+Alt+V`
#[cfg(target_os = "macos")]
pub const DEFAULT_QUICK_PANEL_SHORTCUT: &str = "super+ctrl+v";
#[cfg(not(target_os = "macos"))]
pub const DEFAULT_QUICK_PANEL_SHORTCUT: &str = "ctrl+alt+v";

/// Key of the "toggle quick panel" override in `Settings.keyboard_shortcuts`.
///
/// The main window's settings page and every desktop shell read and write the user's custom
/// shortcut under this name.
pub const QUICK_PANEL_SHORTCUT_SETTINGS_KEY: &str = "global.toggleQuickPanel";

/// The most strokes one binding may have (leader + second).
///
/// Matches the frontend's `MAX_CHORD_SEGMENTS`. Longer space-separated input is cut to its first
/// two strokes, so the registrar never receives a binding that could not be triggered.
pub const MAX_CHORD_SEGMENTS: usize = 2;

/// Normalizes a frontend shortcut string to the physical-key form.
///
/// Inputs look like `"meta+ctrl+v"`, `"mod+shift+v"`, `"Cmd+Alt+V"`, or a VS Code style two-stroke
/// chord separated by a space: `"meta+ctrl+v meta+ctrl+v"`.
///
/// Rules, applied per stroke and per token:
///   - `meta` / `super` (the physical Meta/Win/Cmd key) become `super`
///   - `mod` / `cmd` / `command` (the abstract platform modifier) become `super` on macOS and
///     `ctrl` elsewhere
///   - everything else is kept in lower case
///
/// Strokes are joined with a single space. Input with more than [`MAX_CHORD_SEGMENTS`] strokes is
/// cut to the first two.
pub fn normalize_to_physical_keys(key: &str) -> String {
    key.split(' ')
        .map(str::trim)
        .filter(|seg| !seg.is_empty())
        .take(MAX_CHORD_SEGMENTS)
        .map(normalize_single_combo)
        .collect::<Vec<_>>()
        .join(" ")
}

/// Normalizes one key combination (a single stroke, no chord space) to the physical-key form.
fn normalize_single_combo(combo: &str) -> String {
    combo
        .split('+')
        .map(|part| match part.trim().to_lowercase().as_str() {
            "meta" | "super" => "super".to_string(),
            "mod" | "cmd" | "command" => if cfg!(target_os = "macos") {
                "super"
            } else {
                "ctrl"
            }
            .to_string(),
            other => other.to_string(),
        })
        .collect::<Vec<_>>()
        .join("+")
}

/// Normalizes frontend shortcut strings, drops empty ones and returns the registrable list.
///
/// When `values` is `None` or normalizes to nothing, the result is `[DEFAULT_QUICK_PANEL_SHORTCUT]`:
/// the list is never empty.
pub fn resolve_shortcut_values<'a, I>(values: Option<I>) -> Vec<String>
where
    I: IntoIterator<Item = &'a str>,
{
    let Some(values) = values else {
        return vec![DEFAULT_QUICK_PANEL_SHORTCUT.to_string()];
    };

    let shortcuts: Vec<String> = values
        .into_iter()
        .map(normalize_to_physical_keys)
        .filter(|s| !s.is_empty())
        .collect();

    if shortcuts.is_empty() {
        vec![DEFAULT_QUICK_PANEL_SHORTCUT.to_string()]
    } else {
        shortcuts
    }
}

/// The physical-key shortcuts that toggle the panel, from the settings snapshot of the daemon.
///
/// Falls back to the default when none is configured or the configured value is empty.
pub fn resolve_quick_panel_shortcuts(
    keyboard_shortcuts: &HashMap<String, ShortcutKeyDto>,
) -> Vec<String> {
    match keyboard_shortcuts.get(QUICK_PANEL_SHORTCUT_SETTINGS_KEY) {
        Some(ShortcutKeyDto::Single(s)) => resolve_shortcut_values(Some(vec![s.as_str()])),
        Some(ShortcutKeyDto::Multiple(v)) => {
            resolve_shortcut_values(Some(v.iter().map(String::as_str).collect::<Vec<_>>()))
        }
        None => resolve_shortcut_values(None::<Vec<&str>>),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── normalize_to_physical_keys ───────────────────────────────────

    #[test]
    fn normalize_meta_to_super() {
        assert_eq!(normalize_to_physical_keys("meta+ctrl+v"), "super+ctrl+v");
        assert_eq!(normalize_to_physical_keys("Meta+Shift+V"), "super+shift+v");
    }

    #[test]
    fn normalize_mod_is_platform_specific() {
        let out = normalize_to_physical_keys("mod+v");
        if cfg!(target_os = "macos") {
            assert_eq!(out, "super+v");
        } else {
            assert_eq!(out, "ctrl+v");
        }
    }

    #[test]
    fn normalize_preserves_unknown_parts() {
        assert_eq!(normalize_to_physical_keys("ctrl+alt+f1"), "ctrl+alt+f1");
    }

    #[test]
    fn normalize_chord_sequence_normalizes_each_segment() {
        // A two-stroke chord is normalized stroke by stroke and rejoined with a space.
        assert_eq!(
            normalize_to_physical_keys("meta+ctrl+v meta+ctrl+v"),
            "super+ctrl+v super+ctrl+v"
        );
        let mixed = normalize_to_physical_keys("mod+k mod+c");
        if cfg!(target_os = "macos") {
            assert_eq!(mixed, "super+k super+c");
        } else {
            assert_eq!(mixed, "ctrl+k ctrl+c");
        }
    }

    #[test]
    fn normalize_clamps_multi_segment_to_two() {
        // Unsupported values such as "a b c" are cut to two strokes.
        assert_eq!(
            normalize_to_physical_keys("meta+a meta+b meta+c"),
            "super+a super+b"
        );
    }

    // ── resolve_shortcut_values ─────────────────────────────────────

    #[test]
    fn resolve_none_returns_default() {
        let out = resolve_shortcut_values(None::<Vec<&str>>);
        assert_eq!(out, vec![DEFAULT_QUICK_PANEL_SHORTCUT.to_string()]);
    }

    #[test]
    fn resolve_empty_returns_default() {
        let out = resolve_shortcut_values(Some(Vec::<&str>::new()));
        assert_eq!(out, vec![DEFAULT_QUICK_PANEL_SHORTCUT.to_string()]);
    }

    #[test]
    fn resolve_normalizes_each_entry() {
        let out = resolve_shortcut_values(Some(vec!["meta+ctrl+v", "ctrl+alt+v"]));
        assert_eq!(
            out,
            vec!["super+ctrl+v".to_string(), "ctrl+alt+v".to_string()]
        );
    }

    // ── resolve_quick_panel_shortcuts ───────────────────────────────

    #[test]
    fn resolve_quick_panel_uses_default_when_unset() {
        let shortcuts = HashMap::new();
        let out = resolve_quick_panel_shortcuts(&shortcuts);
        assert_eq!(out, vec![DEFAULT_QUICK_PANEL_SHORTCUT.to_string()]);
    }

    #[test]
    fn resolve_quick_panel_reads_single_override() {
        let mut shortcuts = HashMap::new();
        shortcuts.insert(
            QUICK_PANEL_SHORTCUT_SETTINGS_KEY.to_string(),
            ShortcutKeyDto::Single("meta+shift+v".to_string()),
        );
        let out = resolve_quick_panel_shortcuts(&shortcuts);
        assert_eq!(out, vec!["super+shift+v".to_string()]);
    }

    #[test]
    fn resolve_quick_panel_reads_multiple_override() {
        let mut shortcuts = HashMap::new();
        shortcuts.insert(
            QUICK_PANEL_SHORTCUT_SETTINGS_KEY.to_string(),
            ShortcutKeyDto::Multiple(vec!["meta+v".into(), "ctrl+alt+v".into()]),
        );
        let out = resolve_quick_panel_shortcuts(&shortcuts);
        assert_eq!(out, vec!["super+v".to_string(), "ctrl+alt+v".to_string()]);
    }
}
