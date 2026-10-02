//! Desktop-session defaults available before the webview or daemon starts.

fn prefers_no_title_bar(
    current_desktop: Option<&str>,
    session_desktop: Option<&str>,
    desktop_session: Option<&str>,
) -> bool {
    [current_desktop, session_desktop, desktop_session]
        .into_iter()
        .flatten()
        .find(|value| !value.trim().is_empty())
        .is_some_and(|desktop| {
            desktop.split(':').any(|name| {
                matches!(
                    name.trim().to_ascii_lowercase().as_str(),
                    "niri" | "hyprland"
                )
            })
        })
}

pub(crate) fn initialization_script() -> &'static str {
    let no_title_bar = cfg!(target_os = "linux")
        && prefers_no_title_bar(
            std::env::var("XDG_CURRENT_DESKTOP").ok().as_deref(),
            std::env::var("XDG_SESSION_DESKTOP").ok().as_deref(),
            std::env::var("DESKTOP_SESSION").ok().as_deref(),
        );
    // Only expose the resolved default, never raw environment values.
    if no_title_bar {
        "window.__UC_WINDOW_FRAME_DEFAULT__ = 'none';"
    } else {
        "window.__UC_WINDOW_FRAME_DEFAULT__ = 'custom';"
    }
}

#[cfg(test)]
mod tests {
    use super::prefers_no_title_bar;

    #[test]
    fn recognizes_tiling_sessions_without_matching_substrings() {
        for desktop in ["niri", "Hyprland", "Hyprland:wlroots", " NIRI "] {
            assert!(prefers_no_title_bar(Some(desktop), None, None));
        }
        for desktop in ["GNOME", "KDE", "", "my-niri-theme", "wayland"] {
            assert!(!prefers_no_title_bar(Some(desktop), None, None));
        }
    }

    #[test]
    fn current_desktop_takes_priority_over_session_fallbacks() {
        assert!(!prefers_no_title_bar(Some("KDE"), Some("niri"), None));
        assert!(prefers_no_title_bar(None, Some("Hyprland"), None));
        assert!(prefers_no_title_bar(Some(""), None, Some("niri")));
        assert!(!prefers_no_title_bar(None, None, None));
    }
}
