//! The field table: product path, legacy Engine path, grammar and default of
//! every product field (ADR-013 sections 2 and 8.1). Loading, writing and the
//! legacy import all go through it, so one field has one rule.

use std::collections::BTreeMap;

use serde_json::{json, Map, Value};

use crate::grammar;
use crate::model::{
    DesktopSettings, DesktopSettingsPatch, DoubleTapModifier, QuickPanelPosition, ShortcutBinding,
    StartupMode, Theme, TrayIcon, UpdateChannel, WindowSize,
};
use crate::SettingsSection;

pub(crate) const GENERAL: &str = "general";
pub(crate) const QUICK_PANEL: &str = "quickPanel";
pub(crate) const SHORTCUTS: &str = "keyboardShortcuts";
const SHORTCUT_PATH_PREFIX: &str = "keyboardShortcuts.";

#[derive(Clone, Copy)]
pub(crate) enum Kind {
    Bool,
    WindowSize,
    Enum(&'static [&'static str]),
    NullableEnum(&'static [&'static str]),
    NullablePreset,
    ThemeOverrides,
    NullableLanguage,
}

/// One scalar field: an object member of `general` or `quickPanel`.
pub(crate) struct Scalar {
    pub section: &'static str,
    pub key: &'static str,
    /// `(section, key)` in the Engine `settings.json`; `None` for P0 fields.
    pub legacy: Option<(&'static str, &'static str)>,
    pub kind: Kind,
    pub default: fn() -> Value,
}

impl Scalar {
    pub(crate) fn path(&self) -> String {
        format!("{}.{}", self.section, self.key)
    }

    pub(crate) fn accepts(&self, value: &Value) -> bool {
        accepts(self.kind, value)
    }

    /// The import field set that first copies this field from the Engine file
    /// (ADR-013 section 8.2); 0 for a field that never lived there.
    pub(crate) fn field_set(&self) -> u64 {
        match (self.legacy, self.key) {
            (None, _) => 0,
            (Some(_), "debugMode") => 2,
            (Some(_), _) => 1,
        }
    }
}

macro_rules! scalar {
    ($section:expr, $key:literal, $legacy:expr, $kind:expr, $default:tt) => {
        Scalar {
            section: $section,
            key: $key,
            legacy: $legacy,
            kind: $kind,
            default: || json!($default),
        }
    };
}

/// Every scalar field. [`Scalar::field_set`] says which import set copies it.
pub(crate) const SCALARS: &[Scalar] = &[
    scalar!(GENERAL, "mainWindowMaximized", None, Kind::Bool, false),
    Scalar {
        section: GENERAL,
        key: "mainWindowSize",
        legacy: None,
        kind: Kind::WindowSize,
        default: || WindowSize::default().to_value(),
    },
    scalar!(
        GENERAL,
        "trayIcon",
        None,
        Kind::Enum(TrayIcon::ALL),
        "glyph"
    ),
    scalar!(
        GENERAL,
        "autoStart",
        Some(("general", "auto_start")),
        Kind::Bool,
        false
    ),
    scalar!(
        GENERAL,
        "startupMode",
        Some(("general", "startup_mode")),
        Kind::Enum(StartupMode::ALL),
        "normal"
    ),
    scalar!(
        GENERAL,
        "restoreLastEntryOnStartup",
        Some(("general", "restore_last_entry_on_startup")),
        Kind::Bool,
        false
    ),
    scalar!(
        GENERAL,
        "autoCheckUpdate",
        Some(("general", "auto_check_update")),
        Kind::Bool,
        true
    ),
    scalar!(
        GENERAL,
        "autoDownloadUpdate",
        Some(("general", "auto_download_update")),
        Kind::Bool,
        true
    ),
    scalar!(
        GENERAL,
        "updateChannel",
        Some(("general", "update_channel")),
        Kind::NullableEnum(UpdateChannel::ALL),
        null
    ),
    scalar!(
        GENERAL,
        "theme",
        Some(("general", "theme")),
        Kind::Enum(Theme::ALL),
        "system"
    ),
    scalar!(
        GENERAL,
        "themeColorLight",
        Some(("general", "theme_color_light")),
        Kind::NullablePreset,
        null
    ),
    scalar!(
        GENERAL,
        "themeColorDark",
        Some(("general", "theme_color_dark")),
        Kind::NullablePreset,
        null
    ),
    scalar!(
        GENERAL,
        "themeOverridesLight",
        Some(("general", "theme_overrides_light")),
        Kind::ThemeOverrides,
        {}
    ),
    scalar!(
        GENERAL,
        "themeOverridesDark",
        Some(("general", "theme_overrides_dark")),
        Kind::ThemeOverrides,
        {}
    ),
    scalar!(
        GENERAL,
        "language",
        Some(("general", "language")),
        Kind::NullableLanguage,
        null
    ),
    scalar!(
        GENERAL,
        "usageAnalyticsEnabled",
        Some(("general", "usage_analytics_enabled")),
        Kind::Bool,
        true
    ),
    scalar!(
        GENERAL,
        "debugMode",
        Some(("general", "debug_mode")),
        Kind::Bool,
        false
    ),
    scalar!(
        QUICK_PANEL,
        "enabled",
        Some(("quick_panel", "enabled")),
        Kind::Bool,
        true
    ),
    scalar!(
        QUICK_PANEL,
        "position",
        Some(("quick_panel", "position")),
        Kind::Enum(QuickPanelPosition::ALL),
        "center"
    ),
    scalar!(
        QUICK_PANEL,
        "doubleTapModifier",
        Some(("quick_panel", "double_tap_modifier")),
        Kind::Enum(DoubleTapModifier::ALL),
        "disabled"
    ),
];

/// Legacy section of the shortcut map in the Engine `settings.json`.
pub(crate) const LEGACY_SHORTCUTS: &str = "keyboard_shortcuts";

fn accepts(kind: Kind, value: &Value) -> bool {
    match kind {
        Kind::Bool => value.is_boolean(),
        Kind::WindowSize => WindowSize::from_value(value).is_some(),
        Kind::Enum(values) => value.as_str().is_some_and(|raw| values.contains(&raw)),
        Kind::NullableEnum(values) => {
            value.is_null() || value.as_str().is_some_and(|raw| values.contains(&raw))
        }
        Kind::NullablePreset => {
            value.is_null() || value.as_str().is_some_and(grammar::is_preset_name)
        }
        Kind::ThemeOverrides => grammar::is_theme_overrides(value),
        Kind::NullableLanguage => {
            value.is_null() || value.as_str().is_some_and(grammar::is_language_tag)
        }
    }
}

pub(crate) fn shortcut_path(id: &str) -> String {
    format!("{SHORTCUT_PATH_PREFIX}{id}")
}

pub(crate) fn section_of_path(path: &str) -> Option<SettingsSection> {
    if path.starts_with(SHORTCUT_PATH_PREFIX) {
        return Some(SettingsSection::KeyboardShortcuts);
    }
    match path.split_once('.').map(|(section, _)| section) {
        Some(GENERAL) => Some(SettingsSection::General),
        Some(QUICK_PANEL) => Some(SettingsSection::QuickPanel),
        _ => None,
    }
}

/// A field path with the value it sets; `None` removes the member.
pub(crate) struct Entry {
    pub path: String,
    pub section: &'static str,
    pub key: String,
    pub value: Option<Value>,
}

/// Read a member of a section object in a raw document.
pub(crate) fn member<'a>(
    document: Option<&'a Map<String, Value>>,
    section: &str,
    key: &str,
) -> Option<&'a Value> {
    document?.get(section)?.as_object()?.get(key)
}

/// Set or remove a member, replacing a section that is not an object.
pub(crate) fn set_member(
    document: &mut Map<String, Value>,
    section: &str,
    key: &str,
    value: Option<Value>,
) {
    if value.is_none() && !document.contains_key(section) {
        return;
    }
    let mut object = match document.remove(section) {
        Some(Value::Object(object)) => object,
        _ => Map::new(),
    };
    match value {
        Some(value) => {
            object.insert(key.to_owned(), value);
        }
        None => {
            object.remove(key);
        }
    }
    document.insert(section.to_owned(), Value::Object(object));
}

/// Why a write was rejected: the field path, never the value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidField {
    pub path: String,
}

impl std::fmt::Display for InvalidField {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "invalid value for {}", self.path)
    }
}

impl std::error::Error for InvalidField {}

pub(crate) fn validate_entries(entries: &[Entry]) -> Result<(), InvalidField> {
    let mut shortcut_ids = 0;
    for entry in entries {
        let valid = if entry.section == SHORTCUTS {
            shortcut_ids += 1;
            grammar::is_shortcut_id(&entry.key)
                && entry.value.as_ref().is_none_or(grammar::is_shortcut_value)
        } else {
            let scalar = SCALARS
                .iter()
                .find(|scalar| scalar.section == entry.section && scalar.key == entry.key);
            match (scalar, &entry.value) {
                (Some(scalar), Some(value)) => scalar.accepts(value),
                _ => false,
            }
        };
        if !valid {
            return Err(InvalidField {
                path: entry.path.clone(),
            });
        }
    }
    if !grammar::shortcut_id_count_ok(shortcut_ids) {
        return Err(InvalidField {
            path: SHORTCUTS.to_owned(),
        });
    }
    Ok(())
}

/// The entries a patch writes. `current` resolves the legacy `themeColor` fold.
pub(crate) fn patch_entries(patch: &DesktopSettingsPatch, current: &DesktopSettings) -> Vec<Entry> {
    let mut entries = Vec::new();
    let mut push = |section: &'static str, key: &str, value: Option<Value>| {
        let path = if section == SHORTCUTS {
            shortcut_path(key)
        } else {
            format!("{section}.{key}")
        };
        entries.push(Entry {
            path,
            section,
            key: key.to_owned(),
            value,
        });
    };
    let general = &patch.general;
    let string = |value: &str| Value::String(value.to_owned());
    let nullable = |value: &Option<String>| value.as_deref().map_or(Value::Null, string);
    let overrides = |map: &BTreeMap<String, String>| {
        Value::Object(map.iter().map(|(k, v)| (k.clone(), string(v))).collect())
    };

    if let Some(v) = general.tray_icon {
        push(GENERAL, "trayIcon", Some(string(v.as_str())));
    }
    if let Some(v) = general.main_window_maximized {
        push(GENERAL, "mainWindowMaximized", Some(Value::Bool(v)));
    }
    if let Some(v) = general.main_window_size {
        push(GENERAL, "mainWindowSize", Some(v.to_value()));
    }
    if let Some(v) = general.auto_start {
        push(GENERAL, "autoStart", Some(Value::Bool(v)));
    }
    if let Some(v) = general.startup_mode {
        push(GENERAL, "startupMode", Some(string(v.as_str())));
    }
    if let Some(v) = general.restore_last_entry_on_startup {
        push(GENERAL, "restoreLastEntryOnStartup", Some(Value::Bool(v)));
    }
    if let Some(v) = general.auto_check_update {
        push(GENERAL, "autoCheckUpdate", Some(Value::Bool(v)));
    }
    if let Some(v) = general.auto_download_update {
        push(GENERAL, "autoDownloadUpdate", Some(Value::Bool(v)));
    }
    if let Some(v) = general.update_channel {
        push(
            GENERAL,
            "updateChannel",
            Some(v.map_or(Value::Null, |c| string(c.as_str()))),
        );
    }
    if let Some(v) = general.theme {
        push(GENERAL, "theme", Some(string(v.as_str())));
    }
    let light = general.theme_color_light.clone().or_else(|| {
        general
            .theme_color
            .clone()
            .filter(|_| current.general.theme_color_light.is_none())
            .map(Some)
    });
    let dark = general.theme_color_dark.clone().or_else(|| {
        general
            .theme_color
            .clone()
            .filter(|_| current.general.theme_color_dark.is_none())
            .map(Some)
    });
    if let Some(v) = &light {
        push(GENERAL, "themeColorLight", Some(nullable(v)));
    }
    if let Some(v) = &dark {
        push(GENERAL, "themeColorDark", Some(nullable(v)));
    }
    if let Some(v) = &general.theme_overrides_light {
        push(GENERAL, "themeOverridesLight", Some(overrides(v)));
    }
    if let Some(v) = &general.theme_overrides_dark {
        push(GENERAL, "themeOverridesDark", Some(overrides(v)));
    }
    if let Some(v) = &general.language {
        push(GENERAL, "language", Some(nullable(v)));
    }
    if let Some(v) = general.usage_analytics_enabled {
        push(GENERAL, "usageAnalyticsEnabled", Some(Value::Bool(v)));
    }
    if let Some(v) = general.debug_mode {
        push(GENERAL, "debugMode", Some(Value::Bool(v)));
    }
    for (id, binding) in &patch.keyboard_shortcuts {
        push(
            SHORTCUTS,
            id,
            binding.as_ref().map(ShortcutBinding::to_value),
        );
    }
    let quick_panel = &patch.quick_panel;
    if let Some(v) = quick_panel.enabled {
        push(QUICK_PANEL, "enabled", Some(Value::Bool(v)));
    }
    if let Some(v) = quick_panel.position {
        push(QUICK_PANEL, "position", Some(string(v.as_str())));
    }
    if let Some(v) = quick_panel.double_tap_modifier {
        push(QUICK_PANEL, "doubleTapModifier", Some(string(v.as_str())));
    }
    entries
}

/// Every product path with its normalized value: the stored value when it
/// passes its grammar, otherwise the default. Shortcuts keep only valid ids.
/// `on_invalid` receives the path of each value that was replaced.
pub(crate) fn normalized(
    document: Option<&Map<String, Value>>,
    mut on_invalid: impl FnMut(&str),
) -> BTreeMap<String, Value> {
    let mut values = BTreeMap::new();
    for scalar in SCALARS {
        let value = match member(document, scalar.section, scalar.key) {
            Some(value) if scalar.accepts(value) => value.clone(),
            Some(_) => {
                on_invalid(&scalar.path());
                (scalar.default)()
            }
            None => (scalar.default)(),
        };
        values.insert(scalar.path(), value);
    }
    let shortcuts = document
        .and_then(|document| document.get(SHORTCUTS))
        .and_then(Value::as_object);
    if let Some(shortcuts) = shortcuts {
        for (id, value) in shortcuts {
            let path = shortcut_path(id);
            if grammar::is_shortcut_id(id) && grammar::is_shortcut_value(value) {
                values.insert(path, value.clone());
            } else {
                on_invalid(&path);
            }
        }
    }
    values
}

/// Typed settings from normalized values. Every value already passed its
/// grammar, so each parse succeeds; a failed parse would keep the default.
pub(crate) fn typed(values: &BTreeMap<String, Value>) -> DesktopSettings {
    let mut settings = DesktopSettings::default();
    let get = |path: &str| values.get(path);
    let flag = |path: &str, slot: &mut bool| {
        if let Some(v) = get(path).and_then(Value::as_bool) {
            *slot = v;
        }
    };
    let text = |path: &str| get(path).and_then(Value::as_str).map(str::to_owned);
    let map = |path: &str| -> BTreeMap<String, String> {
        get(path)
            .and_then(Value::as_object)
            .map(|object| {
                object
                    .iter()
                    .filter_map(|(k, v)| v.as_str().map(|v| (k.clone(), v.to_owned())))
                    .collect()
            })
            .unwrap_or_default()
    };

    let general = &mut settings.general;
    flag(
        "general.mainWindowMaximized",
        &mut general.main_window_maximized,
    );
    if let Some(v) = get("general.mainWindowSize").and_then(WindowSize::from_value) {
        general.main_window_size = v;
    }
    if let Some(v) = text("general.trayIcon")
        .as_deref()
        .and_then(TrayIcon::parse)
    {
        general.tray_icon = v;
    }
    flag("general.autoStart", &mut general.auto_start);
    if let Some(v) = text("general.startupMode")
        .as_deref()
        .and_then(StartupMode::parse)
    {
        general.startup_mode = v;
    }
    flag(
        "general.restoreLastEntryOnStartup",
        &mut general.restore_last_entry_on_startup,
    );
    flag("general.autoCheckUpdate", &mut general.auto_check_update);
    flag(
        "general.autoDownloadUpdate",
        &mut general.auto_download_update,
    );
    general.update_channel = text("general.updateChannel")
        .as_deref()
        .and_then(UpdateChannel::parse);
    if let Some(v) = text("general.theme").as_deref().and_then(Theme::parse) {
        general.theme = v;
    }
    general.theme_color_light = text("general.themeColorLight");
    general.theme_color_dark = text("general.themeColorDark");
    general.theme_overrides_light = map("general.themeOverridesLight");
    general.theme_overrides_dark = map("general.themeOverridesDark");
    general.language = text("general.language");
    flag(
        "general.usageAnalyticsEnabled",
        &mut general.usage_analytics_enabled,
    );
    flag("general.debugMode", &mut general.debug_mode);

    let quick_panel = &mut settings.quick_panel;
    flag("quickPanel.enabled", &mut quick_panel.enabled);
    if let Some(v) = text("quickPanel.position")
        .as_deref()
        .and_then(QuickPanelPosition::parse)
    {
        quick_panel.position = v;
    }
    if let Some(v) = text("quickPanel.doubleTapModifier")
        .as_deref()
        .and_then(DoubleTapModifier::parse)
    {
        quick_panel.double_tap_modifier = v;
    }

    for (path, value) in values {
        if let Some(id) = path.strip_prefix(SHORTCUT_PATH_PREFIX) {
            if let Some(binding) = ShortcutBinding::from_value(value) {
                settings.keyboard_shortcuts.insert(id.to_owned(), binding);
            }
        }
    }
    settings
}
