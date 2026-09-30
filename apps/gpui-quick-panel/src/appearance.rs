use gpui::{App, Hsla, Rgba, Window};
use gpui_component::{Theme, ThemeMode};
use uc_daemon_contract::api::dto::settings::{GeneralSettingsDto, ThemeDto};

pub struct Surfaces {
    pub background: Hsla,
    pub card: Hsla,
}
impl gpui::Global for Surfaces {}

pub fn apply(
    settings: Option<&GeneralSettingsDto>,
    window: &mut Window,
    cx: &mut App,
) -> anyhow::Result<()> {
    let dark = match settings.map(|s| &s.theme) {
        Some(ThemeDto::Dark) => true,
        Some(ThemeDto::Light) => false,
        _ => matches!(
            window.appearance(),
            gpui::WindowAppearance::Dark | gpui::WindowAppearance::VibrantDark
        ),
    };
    Theme::change(
        if dark {
            ThemeMode::Dark
        } else {
            ThemeMode::Light
        },
        Some(window),
        cx,
    );
    let preset = settings
        .and_then(|s| {
            if dark {
                s.theme_color_dark.as_deref()
            } else {
                s.theme_color_light.as_deref()
            }
        })
        .or_else(|| settings.and_then(|s| s.theme_color.as_deref()))
        .unwrap_or("zinc");
    let presets: serde_json::Value = serde_json::from_str(include_str!("../assets/themes.json"))?;
    let tokens = presets
        .get(preset)
        .or_else(|| presets.get("zinc"))
        .and_then(|p| p.get(if dark { "dark" } else { "light" }))
        .ok_or_else(|| anyhow::anyhow!("Missing theme preset"))?;
    let overrides = settings.map(|s| {
        if dark {
            &s.theme_overrides_dark
        } else {
            &s.theme_overrides_light
        }
    });
    let color = |name: &str| -> anyhow::Result<Hsla> {
        let value = overrides
            .and_then(|v| v.get(name).map(String::as_str))
            .or_else(|| tokens.get(name).and_then(|v| v.as_str()))
            .ok_or_else(|| anyhow::anyhow!("Missing theme token"))?;
        let [r, g, b, a] = csscolorparser::parse(value)?.to_array();
        Ok(Rgba { r, g, b, a }.into())
    };
    cx.set_global(Surfaces {
        background: color("background")?,
        card: color("card")?,
    });
    let theme = Theme::global_mut(cx);
    theme.colors.background = color("background")?;
    theme.colors.foreground = color("foreground")?;
    theme.colors.primary = color("primary")?;
    theme.colors.primary_foreground = color("primaryForeground")?;
    theme.colors.muted = color("muted")?;
    theme.colors.muted_foreground = color("mutedForeground")?;
    theme.colors.border = color("border")?;
    theme.colors.input = color("input")?;
    theme.colors.accent = color("accent")?;
    theme.colors.accent_foreground = color("accentForeground")?;
    theme.colors.popover = color("popover")?;
    theme.colors.popover_foreground = color("popoverForeground")?;
    theme.colors.danger = color("destructive")?;
    theme.colors.ring = color("ring")?;
    // Root paints the framework background; floating cards own their separate surfaces.
    theme.colors.background = gpui::transparent_black();
    // Fonts stay at the system defaults of gpui-component (the system UI font; Menlo on macOS and
    // Consolas elsewhere for monospace), so nothing is bundled.
    Ok(())
}
