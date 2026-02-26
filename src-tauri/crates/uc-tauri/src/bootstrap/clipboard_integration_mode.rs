use tracing::warn;
use uc_app::usecases::clipboard::ClipboardIntegrationMode;

fn parse_clipboard_integration_mode(raw: Option<&str>) -> ClipboardIntegrationMode {
    let Some(raw_value) = raw else {
        return ClipboardIntegrationMode::Full;
    };

    let normalized = raw_value.trim();

    if normalized.eq_ignore_ascii_case("passive") {
        return ClipboardIntegrationMode::Passive;
    }

    if normalized.eq_ignore_ascii_case("full") {
        return ClipboardIntegrationMode::Full;
    }

    warn!(
        uc_clipboard_mode = %raw_value,
        "Invalid UC_CLIPBOARD_MODE value; falling back to full integration"
    );
    ClipboardIntegrationMode::Full
}

pub fn resolve_clipboard_integration_mode() -> ClipboardIntegrationMode {
    let raw = std::env::var("UC_CLIPBOARD_MODE").ok();
    parse_clipboard_integration_mode(raw.as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    #[test]
    fn parse_clipboard_integration_mode_table_driven() {
        let cases = [
            (
                "none defaults to full",
                None,
                ClipboardIntegrationMode::Full,
            ),
            (
                "mixed case passive",
                Some("PaSsIvE"),
                ClipboardIntegrationMode::Passive,
            ),
            (
                "trimmed full",
                Some(" full "),
                ClipboardIntegrationMode::Full,
            ),
            (
                "whitespace only falls back to full",
                Some("   "),
                ClipboardIntegrationMode::Full,
            ),
            (
                "invalid falls back to full",
                Some("invalid-value"),
                ClipboardIntegrationMode::Full,
            ),
        ];

        for (name, raw, expected) in cases {
            assert_eq!(parse_clipboard_integration_mode(raw), expected, "{name}");
        }
    }

    #[test]
    fn resolve_clipboard_integration_mode_table_driven() {
        let _guard = env_lock().lock().expect("env lock");
        let key = "UC_CLIPBOARD_MODE";
        let original = std::env::var(key).ok();

        let cases = [
            (
                "none defaults to full",
                None,
                ClipboardIntegrationMode::Full,
            ),
            (
                "mixed case passive",
                Some("PaSsIvE"),
                ClipboardIntegrationMode::Passive,
            ),
            (
                "trimmed full",
                Some(" full "),
                ClipboardIntegrationMode::Full,
            ),
            (
                "whitespace only falls back to full",
                Some("   "),
                ClipboardIntegrationMode::Full,
            ),
            (
                "invalid falls back to full",
                Some("totally-invalid"),
                ClipboardIntegrationMode::Full,
            ),
        ];

        for (name, raw, expected) in cases {
            match raw {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
            assert_eq!(resolve_clipboard_integration_mode(), expected, "{name}");
        }

        match original {
            Some(value) => std::env::set_var(key, value),
            None => std::env::remove_var(key),
        }
    }
}
