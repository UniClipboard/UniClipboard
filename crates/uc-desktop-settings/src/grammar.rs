//! Per-field grammars (ADR-013 section 8.1). They keep invalid values away from
//! startup and from OS APIs; they are not a confidentiality measure.

use serde_json::Value;

/// Shortcut modifiers accepted by the three normalizers
/// (`apps/gui-go/global_shortcuts.go`, `crates/quick-panel-core/src/shortcuts.rs`,
/// `apps/gui-go/frontend/src/shortcuts/normalize.ts`).
const MODIFIERS: &[&str] = &[
    "ctrl", "control", "alt", "option", "shift", "meta", "super", "cmd", "command", "mod",
];

/// Named keys, fixed in code (ADR-013 section 8.1.1). Sources: the frontend
/// aliases, the single-word Wails v3 `namedKeys` and the `global-hotkey` 0.8
/// `parse_key` names. Two-word names (`page up`) are excluded: a space separates
/// strokes. Function keys are bounded at `f24` separately.
const NAMED_KEYS: &[&str] = &[
    // Frontend aliases (`normalize.ts`, `definitions.ts`).
    "esc",
    "equal",
    "minus",
    "add",
    "subtract",
    // Wails v3 `pkg/application/keys.go` (single-word names).
    "backspace",
    "tab",
    "return",
    "enter",
    "escape",
    "left",
    "right",
    "up",
    "down",
    "space",
    "delete",
    "home",
    "end",
    "numlock",
    "plus",
    // global-hotkey 0.8 `parse_key`.
    "backquote",
    "backslash",
    "bracketleft",
    "bracketright",
    "pause",
    "pausebreak",
    "comma",
    "period",
    "quote",
    "semicolon",
    "slash",
    "capslock",
    "insert",
    "pagedown",
    "pageup",
    "printscreen",
    "scrolllock",
    "arrowdown",
    "arrowleft",
    "arrowright",
    "arrowup",
    "numpadadd",
    "numadd",
    "numpadplus",
    "numplus",
    "numpaddecimal",
    "numdecimal",
    "numpaddivide",
    "numdivide",
    "numpadenter",
    "numenter",
    "numpadequal",
    "numequal",
    "numpadmultiply",
    "nummultiply",
    "numpadsubtract",
    "numsubtract",
    "audiovolumedown",
    "volumedown",
    "audiovolumeup",
    "volumeup",
    "audiovolumemute",
    "volumemute",
    "mediaplay",
    "mediapause",
    "mediaplaypause",
    "mediastop",
    "mediatracknext",
    "mediatrackprev",
    "mediatrackprevious",
];

const MAX_SHORTCUT_IDS: usize = 64;
const MAX_SHORTCUT_ID_LEN: usize = 64;
const MAX_ALTERNATIVES: usize = 4;
const MAX_STROKES: usize = 2;
const MAX_TOKENS_PER_STROKE: usize = 8;
const MAX_FUNCTION_KEY: u32 = 24;

/// Theme tokens a user may override.
const OVERRIDE_TOKENS: &[&str] = &["primary", "background", "foreground", "border"];

/// `[a-z][A-Za-z0-9]*(\.[A-Za-z0-9]+)*`, at most 64 characters.
pub(crate) fn is_shortcut_id(id: &str) -> bool {
    if id.is_empty() || id.len() > MAX_SHORTCUT_ID_LEN {
        return false;
    }
    let mut segments = id.split('.');
    let first_ok = segments.next().is_some_and(|first| {
        first.starts_with(|c: char| c.is_ascii_lowercase())
            && first.chars().all(|c| c.is_ascii_alphanumeric())
    });
    first_ok && segments.all(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric()))
}

pub(crate) fn shortcut_id_count_ok(count: usize) -> bool {
    count <= MAX_SHORTCUT_IDS
}

/// A binding string, or an array of 1–4 binding strings.
pub(crate) fn is_shortcut_value(value: &Value) -> bool {
    match value {
        Value::String(binding) => is_binding(binding),
        Value::Array(alternatives) => {
            (1..=MAX_ALTERNATIVES).contains(&alternatives.len())
                && alternatives
                    .iter()
                    .all(|item| item.as_str().is_some_and(is_binding))
        }
        _ => false,
    }
}

/// 1–2 strokes separated by exactly one space; each stroke is 1–8 `+`-joined tokens.
fn is_binding(binding: &str) -> bool {
    let strokes: Vec<&str> = binding.split(' ').collect();
    (1..=MAX_STROKES).contains(&strokes.len())
        && strokes.iter().all(|stroke| {
            let tokens: Vec<&str> = stroke.split('+').collect();
            (1..=MAX_TOKENS_PER_STROKE).contains(&tokens.len())
                && tokens.iter().all(|token| is_token(token))
        })
}

fn is_token(token: &str) -> bool {
    let lower = token.to_ascii_lowercase();
    if lower.len() == 1 {
        // Letters, digits and printable ASCII symbols except `+` and space.
        let c = lower.as_bytes()[0];
        return c.is_ascii_graphic() && c != b'+';
    }
    if MODIFIERS.contains(&lower.as_str()) || NAMED_KEYS.contains(&lower.as_str()) {
        return true;
    }
    if let Some(digits) = lower
        .strip_prefix("numpad")
        .or_else(|| lower.strip_prefix("num"))
    {
        if digits.len() == 1 && digits.as_bytes()[0].is_ascii_digit() {
            return true;
        }
    }
    lower
        .strip_prefix('f')
        .filter(|digits| !digits.starts_with('0') && digits.chars().all(|c| c.is_ascii_digit()))
        .and_then(|digits| digits.parse::<u32>().ok())
        .is_some_and(|n| (1..=MAX_FUNCTION_KEY).contains(&n))
}

/// Preset name `[a-z0-9-]{1,32}` (D4).
pub(crate) fn is_preset_name(name: &str) -> bool {
    (1..=32).contains(&name.len())
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// Override map: keys from the four theme tokens; values `oklch(L C H)` or `#rrggbb`.
pub(crate) fn is_theme_overrides(value: &Value) -> bool {
    value.as_object().is_some_and(|map| {
        map.iter().all(|(token, color)| {
            OVERRIDE_TOKENS.contains(&token.as_str()) && color.as_str().is_some_and(is_color)
        })
    })
}

fn is_color(color: &str) -> bool {
    if let Some(hex) = color.strip_prefix('#') {
        return hex.len() == 6 && hex.bytes().all(|b| b.is_ascii_hexdigit());
    }
    let Some(inner) = color
        .strip_prefix("oklch(")
        .and_then(|rest| rest.strip_suffix(')'))
    else {
        return false;
    };
    let parts: Vec<&str> = inner.split(' ').collect();
    let [l, c, h] = parts.as_slice() else {
        return false;
    };
    in_range(l, 0.0, 1.0) && in_range(c, 0.0, 1.0) && in_range(h, 0.0, 360.0)
}

/// A plain decimal number (`0`, `0.6193`) within `[min, max]`.
fn in_range(number: &str, min: f64, max: f64) -> bool {
    let (whole, fraction) = number.split_once('.').unwrap_or((number, "0"));
    let digits_ok = !whole.is_empty()
        && whole.len() <= 3
        && whole.bytes().all(|b| b.is_ascii_digit())
        && !fraction.is_empty()
        && fraction.len() <= 8
        && fraction.bytes().all(|b| b.is_ascii_digit());
    digits_ok
        && number
            .parse::<f64>()
            .is_ok_and(|value| (min..=max).contains(&value))
}

/// A language tag: 1–8 letters, then subtags of 1–8 letters or digits separated
/// by `-` or `_` (the separators the frontend `normalizeLanguage` accepts), at
/// most 35 characters.
pub(crate) fn is_language_tag(tag: &str) -> bool {
    if tag.is_empty() || tag.len() > 35 {
        return false;
    }
    let mut subtags = tag.split(['-', '_']);
    let primary_ok = subtags.next().is_some_and(|primary| {
        (1..=8).contains(&primary.len()) && primary.bytes().all(|b| b.is_ascii_alphabetic())
    });
    primary_ok
        && subtags.all(|subtag| {
            (1..=8).contains(&subtag.len()) && subtag.bytes().all(|b| b.is_ascii_alphanumeric())
        })
}
