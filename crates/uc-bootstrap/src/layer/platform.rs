//! Desktop system-clipboard selection.

use std::sync::Arc;

use uc_core::ports::{PlatformClipboardPort, SystemClipboardPort};
#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
use uc_platform::clipboard::LocalClipboard;
use uc_platform::clipboard::NoopSystemClipboard;

use crate::wiring::deps::{WiringError, WiringResult};
pub use uc_engine::internal::clipboard::SystemClipboardWiring;
pub(crate) use uc_engine::internal::platform::SystemClipboardLayer;

fn noop_system_clipboard() -> (Arc<dyn PlatformClipboardPort>, Arc<dyn SystemClipboardPort>) {
    let noop: Arc<NoopSystemClipboard> = Arc::new(NoopSystemClipboard);
    (noop.clone(), noop)
}

#[cfg(test)]
pub(crate) fn noop_system_clipboard_layer() -> SystemClipboardLayer {
    let (clipboard, system_clipboard) = noop_system_clipboard();
    SystemClipboardLayer::new(clipboard, system_clipboard, SystemClipboardWiring::Noop)
}

pub fn create_desktop_system_clipboard() -> WiringResult<SystemClipboardLayer> {
    let clipboard_opt_out = std::env::var("UC_DISABLE_SYSTEM_CLIPBOARD").ok();
    let disable_system_clipboard = is_clipboard_disabled(clipboard_opt_out.as_deref());
    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
    let system_clipboard_wiring = if disable_system_clipboard {
        tracing::info!(
            "UC_DISABLE_SYSTEM_CLIPBOARD set; substituting NoopSystemClipboard \
             (any clipboard capture / write is a no-op)"
        );
        SystemClipboardWiring::Noop
    } else if uc_platform::capability::detect_system_clipboard_capability()
        == uc_platform::capability::SystemClipboardCapability::NoDisplaySession
    {
        tracing::warn!(
            "no graphical session detected (DISPLAY / WAYLAND_DISPLAY unset); \
             substituting NoopSystemClipboard so the process can still serve \
             history / transfer / API duties on this headless host"
        );
        SystemClipboardWiring::Noop
    } else {
        SystemClipboardWiring::Real
    };

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    let system_clipboard_wiring = {
        let _ = disable_system_clipboard;
        SystemClipboardWiring::Noop
    };
    let (clipboard, system_clipboard): (
        Arc<dyn PlatformClipboardPort>,
        Arc<dyn SystemClipboardPort>,
    ) = match system_clipboard_wiring {
        SystemClipboardWiring::Noop => noop_system_clipboard(),
        SystemClipboardWiring::Real => {
            #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
            {
                let clipboard_impl = LocalClipboard::new().map_err(|e| {
                    WiringError::ClipboardInit(format!("Failed to create clipboard: {}", e))
                })?;
                let clipboard_impl = Arc::new(clipboard_impl);
                (clipboard_impl.clone(), clipboard_impl)
            }
            #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
            {
                return Err(WiringError::ClipboardInit(
                    "mobile hosts must inject a clipboard implementation".to_string(),
                ));
            }
        }
    };

    Ok(SystemClipboardLayer::new(
        clipboard,
        system_clipboard,
        system_clipboard_wiring,
    ))
}

fn is_clipboard_disabled(value: Option<&str>) -> bool {
    value.is_some_and(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
}

#[cfg(test)]
mod tests {
    use super::is_clipboard_disabled;

    #[test]
    fn clipboard_opt_out_only_accepts_boolean_true_values() {
        for value in ["1", "true", "TRUE", "yes", "on", " On "] {
            assert!(is_clipboard_disabled(Some(value)));
        }
        for value in ["0", "false", "no", "off", "", "anything"] {
            assert!(!is_clipboard_disabled(Some(value)));
        }
        assert!(!is_clipboard_disabled(None));
    }
}
