//! Typed product settings, their defaults and the write patch.
//!
//! Defaults equal the Engine defaults of the pinned Engine
//! (`uc-core/src/settings/defaults.rs`), so moving a field changes no
//! effective value (ADR-013 section 8.1).

use std::collections::BTreeMap;

use serde_json::Value;

macro_rules! string_enum {
    ($(#[$meta:meta])* $name:ident { $($(#[$vmeta:meta])* $variant:ident => $text:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
        pub enum $name {
            $(
                $(#[$vmeta])*
                $variant,
            )+
        }

        impl $name {
            /// Every spelling, in declaration order.
            pub(crate) const ALL: &'static [&'static str] = &[$($text),+];

            pub fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $text,)+
                }
            }

            pub fn parse(raw: &str) -> Option<Self> {
                match raw {
                    $($text => Some(Self::$variant),)+
                    _ => None,
                }
            }
        }
    };
}

string_enum! {
    /// The tray icon artwork.
    TrayIcon {
        /// The monochrome app glyph.
        #[default]
        Glyph => "glyph",
        /// The cat artwork.
        Cat => "cat",
    }
}

string_enum! {
    /// How the app presents itself at launch.
    StartupMode {
        #[default]
        Normal => "normal",
        Silent => "silent",
        Lightweight => "lightweight",
    }
}

string_enum! {
    /// Explicit update channel; absent means "detect from the version".
    UpdateChannel {
        #[default]
        Stable => "stable",
        Alpha => "alpha",
        Beta => "beta",
        Rc => "rc",
    }
}

string_enum! {
    /// Window appearance.
    Theme {
        Light => "light",
        Dark => "dark",
        #[default]
        System => "system",
    }
}

string_enum! {
    /// Where the quick panel appears.
    QuickPanelPosition {
        #[default]
        Center => "center",
        FollowCursor => "follow_cursor",
    }
}

string_enum! {
    /// A modifier that toggles the quick panel when tapped twice.
    DoubleTapModifier {
        #[default]
        Disabled => "disabled",
        Alt => "alt",
        Control => "control",
        Meta => "meta",
    }
}

/// A keyboard shortcut value: one binding or alternatives.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShortcutBinding {
    Single(String),
    Multiple(Vec<String>),
}

impl ShortcutBinding {
    pub(crate) fn to_value(&self) -> Value {
        match self {
            Self::Single(binding) => Value::String(binding.clone()),
            Self::Multiple(bindings) => {
                Value::Array(bindings.iter().cloned().map(Value::String).collect())
            }
        }
    }

    /// Only called on values that passed the shortcut grammar.
    pub(crate) fn from_value(value: &Value) -> Option<Self> {
        match value {
            Value::String(binding) => Some(Self::Single(binding.clone())),
            Value::Array(items) => items
                .iter()
                .map(|item| item.as_str().map(str::to_owned))
                .collect::<Option<Vec<_>>>()
                .map(Self::Multiple),
            _ => None,
        }
    }
}

/// Effective product settings: every field has a valid value.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DesktopSettings {
    pub general: GeneralSettings,
    /// Only valid entries; an absent id uses the built-in binding.
    pub keyboard_shortcuts: BTreeMap<String, ShortcutBinding>,
    pub quick_panel: QuickPanelSettings,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GeneralSettings {
    pub tray_icon: TrayIcon,
    pub auto_start: bool,
    pub startup_mode: StartupMode,
    pub restore_last_entry_on_startup: bool,
    pub auto_check_update: bool,
    pub auto_download_update: bool,
    pub update_channel: Option<UpdateChannel>,
    pub theme: Theme,
    pub theme_color_light: Option<String>,
    pub theme_color_dark: Option<String>,
    pub theme_overrides_light: BTreeMap<String, String>,
    pub theme_overrides_dark: BTreeMap<String, String>,
    pub language: Option<String>,
    pub usage_analytics_enabled: bool,
    /// Whether the next daemon start installs the debug log profile. The profile
    /// this process actually installed is a separate fact.
    pub debug_mode: bool,
}

impl Default for GeneralSettings {
    fn default() -> Self {
        Self {
            tray_icon: TrayIcon::Glyph,
            auto_start: false,
            startup_mode: StartupMode::Normal,
            restore_last_entry_on_startup: false,
            auto_check_update: true,
            auto_download_update: true,
            update_channel: None,
            theme: Theme::System,
            theme_color_light: None,
            theme_color_dark: None,
            theme_overrides_light: BTreeMap::new(),
            theme_overrides_dark: BTreeMap::new(),
            language: None,
            usage_analytics_enabled: true,
            debug_mode: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QuickPanelSettings {
    pub enabled: bool,
    pub position: QuickPanelPosition,
    pub double_tap_modifier: DoubleTapModifier,
}

impl Default for QuickPanelSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            position: QuickPanelPosition::Center,
            double_tap_modifier: DoubleTapModifier::Disabled,
        }
    }
}

/// An explicit write. `None` leaves a field as it is on disk; for nullable
/// fields `Some(None)` stores `null`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DesktopSettingsPatch {
    pub general: GeneralSettingsPatch,
    /// `Some(binding)` sets an id, `None` removes it (the built-in binding applies).
    pub keyboard_shortcuts: BTreeMap<String, Option<ShortcutBinding>>,
    pub quick_panel: QuickPanelSettingsPatch,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GeneralSettingsPatch {
    pub tray_icon: Option<TrayIcon>,
    pub auto_start: Option<bool>,
    pub startup_mode: Option<StartupMode>,
    pub restore_last_entry_on_startup: Option<bool>,
    pub auto_check_update: Option<bool>,
    pub auto_download_update: Option<bool>,
    pub update_channel: Option<Option<UpdateChannel>>,
    pub theme: Option<Theme>,
    /// Legacy single preset (wire `themeColor`). Folded like the import does:
    /// it sets each split preset that this patch does not name and whose
    /// effective value is `null` (ADR-013 section 8.3).
    pub theme_color: Option<String>,
    pub theme_color_light: Option<Option<String>>,
    pub theme_color_dark: Option<Option<String>>,
    pub theme_overrides_light: Option<BTreeMap<String, String>>,
    pub theme_overrides_dark: Option<BTreeMap<String, String>>,
    pub language: Option<Option<String>>,
    pub usage_analytics_enabled: Option<bool>,
    pub debug_mode: Option<bool>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct QuickPanelSettingsPatch {
    pub enabled: Option<bool>,
    pub position: Option<QuickPanelPosition>,
    pub double_tap_modifier: Option<DoubleTapModifier>,
}

impl DesktopSettingsPatch {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}
