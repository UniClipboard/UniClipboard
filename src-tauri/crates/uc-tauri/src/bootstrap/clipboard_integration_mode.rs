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
