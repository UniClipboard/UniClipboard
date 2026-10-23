//! Windows typography projected from the GUI's Fluent theme.

use gpui::{font, App, Font};
use gpui_component::ActiveTheme;

#[cfg(windows)]
struct WindowsFonts {
    ui: gpui::SharedString,
    mono: gpui::SharedString,
    fallbacks: gpui::FontFallbacks,
}

#[cfg(windows)]
impl gpui::Global for WindowsFonts {}

#[cfg(windows)]
fn families(stack: &str) -> Vec<String> {
    stack
        .split(',')
        .map(|name| name.trim().trim_matches('\'').trim_matches('"').to_owned())
        .filter(|name| {
            !matches!(
                name.as_str(),
                "system-ui" | "-apple-system" | "sans-serif" | "monospace"
            )
        })
        .collect()
}

#[cfg(windows)]
fn select_family(stack: &[String], installed: &[String], default: &str) -> gpui::SharedString {
    stack
        .iter()
        .find(|name| {
            installed
                .iter()
                .any(|available| available.eq_ignore_ascii_case(name))
        })
        .map(String::as_str)
        .unwrap_or(default)
        .to_owned()
        .into()
}

pub(super) fn apply(cx: &mut App) -> anyhow::Result<()> {
    #[cfg(windows)]
    {
        if !cx.has_global::<WindowsFonts>() {
            let configuration: serde_json::Value = serde_json::from_str(include_str!(
                "../../../gui-go/frontend/src/themes/default.json"
            ))?;
            let theme = &configuration["platforms"]["windows"]["light"];
            let stack = |key: &str| -> anyhow::Result<Vec<String>> {
                Ok(families(theme[key].as_str().ok_or_else(|| {
                    anyhow::anyhow!("Missing Windows typography token: {key}")
                })?))
            };
            let ui = stack("fontFamilyBase")?;
            let mono = stack("fontFamilyMonospace")?;
            let installed = cx.text_system().all_font_names();
            let fallbacks = ui
                .iter()
                .filter(|name| {
                    installed
                        .iter()
                        .any(|available| available.eq_ignore_ascii_case(name))
                })
                .cloned()
                .collect();
            cx.set_global(WindowsFonts {
                ui: select_family(&ui, &installed, ".SystemUIFont"),
                mono: select_family(&mono, &installed, "Consolas"),
                fallbacks: gpui::FontFallbacks::from_fonts(fallbacks),
            });
        }
        let fonts = cx.global::<WindowsFonts>();
        let (ui, mono) = (fonts.ui.clone(), fonts.mono.clone());
        let theme = gpui_component::Theme::global_mut(cx);
        theme.font_family = ui;
        theme.mono_font_family = mono;
    }
    #[cfg(not(windows))]
    let _ = cx;
    Ok(())
}

pub(super) fn ui_font(cx: &App) -> Font {
    let result = font(cx.theme().font_family.clone());
    #[cfg(windows)]
    {
        let mut result = result;
        result.fallbacks = Some(cx.global::<WindowsFonts>().fallbacks.clone());
        return result;
    }
    #[cfg(not(windows))]
    result
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn directwrite_resolves_theme_fonts_with_explicit_fallbacks() {
        let app = gpui::Application::new();
        let text = app.text_system();
        let configuration: serde_json::Value = serde_json::from_str(include_str!(
            "../../../gui-go/frontend/src/themes/default.json"
        ))
        .unwrap();
        let theme = &configuration["platforms"]["windows"]["light"];
        let stack = families(theme["fontFamilyBase"].as_str().unwrap());
        let installed = text.all_font_names();
        for key in ["fontFamilyBase", "fontFamilyMonospace"] {
            let candidates = families(theme[key].as_str().unwrap());
            let mut selected = font(select_family(&candidates, &installed, ".SystemUIFont"));
            selected.fallbacks = Some(gpui::FontFallbacks::from_fonts(stack.clone()));
            let id = text.resolve_font(&selected);
            let resolved = text.get_font_for_id(id).unwrap();
            if selected.family.as_ref() != ".SystemUIFont" {
                assert_eq!(
                    resolved.family, selected.family,
                    "Unexpected font for {key}"
                );
            }
            assert!(text.advance(id, gpui::px(13.), 'A').unwrap().width > gpui::px(0.));
            assert_eq!(id, text.resolve_font(&selected));
        }
    }

    #[test]
    fn missing_preferred_font_uses_next_installed_family() {
        let stack = families("'Segoe UI Variable Text', 'Segoe UI', system-ui, sans-serif");
        assert_eq!(
            select_family(&stack, &["Segoe UI".into()], ".SystemUIFont").as_ref(),
            "Segoe UI"
        );
        assert_eq!(
            select_family(&stack, &[], ".SystemUIFont").as_ref(),
            ".SystemUIFont"
        );
    }

    #[test]
    fn windows_theme_keeps_typography_across_color_modes() {
        let configuration: serde_json::Value = serde_json::from_str(include_str!(
            "../../../gui-go/frontend/src/themes/default.json"
        ))
        .unwrap();
        let windows = &configuration["platforms"]["windows"];
        for key in ["fontFamilyBase", "fontFamilyMonospace"] {
            assert_eq!(windows["light"][key], windows["dark"][key]);
        }
        let stack = families(windows["light"]["fontFamilyBase"].as_str().unwrap());
        assert_eq!(stack[0], "Segoe UI Variable Text");
        assert_eq!(
            select_family(&stack, &["Microsoft YaHei UI".into()], ".SystemUIFont").as_ref(),
            "Microsoft YaHei UI"
        );
    }
}
