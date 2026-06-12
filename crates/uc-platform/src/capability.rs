//! Platform capability detection.
//!
//! Detects whether the platform supports system keyring or requires file-based
//! fallback, and whether the current session exposes a system clipboard at all.

/// Represents the secure storage capability of the current platform.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecureStorageCapability {
    /// Platform has a working system keyring (macOS Keychain, Windows Credential Manager, Linux Secret Service)
    SystemKeyring,
    /// Platform requires file-based storage (WSL, headless Linux)
    FileBasedKeystore,
    /// Platform is not supported for secure storage
    Unsupported,
}

/// Detect the secure storage capability of the current platform.
///
/// # Detection Logic
///
/// - **macOS**: Always `SystemKeyring` (Keychain available)
/// - **Windows**: Always `SystemKeyring` (Credential Manager available)
/// - **Linux**:
///   - If WSL detected → `FileBasedKeystore`
///   - If desktop environment detected (DISPLAY + DBUS) → `SystemKeyring`
///   - Otherwise → `FileBasedKeystore`
/// - **Other**: `Unsupported`
pub fn detect_storage_capability() -> SecureStorageCapability {
    // macOS: Always has Keychain
    #[cfg(target_os = "macos")]
    {
        if dev_env_forces_file_storage() {
            tracing::warn!("⚠️  macOS dev environment detected. Using file-based secure storage.");
            return SecureStorageCapability::FileBasedKeystore;
        }
        return SecureStorageCapability::SystemKeyring;
    }

    // Windows: Always has Credential Manager
    #[cfg(target_os = "windows")]
    {
        return SecureStorageCapability::SystemKeyring;
    }

    // Linux: Need to distinguish Desktop vs WSL vs headless
    #[cfg(target_os = "linux")]
    {
        if is_wsl() {
            tracing::warn!("⚠️  WSL environment detected. Using file-based KEK storage (Dev Mode)");
            return SecureStorageCapability::FileBasedKeystore;
        }

        if has_desktop_environment() {
            tracing::info!("✅ Linux desktop environment detected. Using system keyring.");
            return SecureStorageCapability::SystemKeyring;
        }

        tracing::warn!("⚠️  No desktop environment detected. Using file-based KEK storage");
        SecureStorageCapability::FileBasedKeystore
    }

    // Unsupported platforms
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        tracing::error!("❌ Unsupported platform for secure storage");
        SecureStorageCapability::Unsupported
    }
}

/// Represents the system clipboard capability of the current session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemClipboardCapability {
    /// A graphical session is present; the real clipboard adapter is expected to work.
    Available,
    /// No graphical session (headless server, container, SSH without display
    /// forwarding) — the OS exposes no clipboard to talk to.
    NoDisplaySession,
}

/// Detect whether the current session exposes a system clipboard.
///
/// # Detection Logic
///
/// - **macOS / Windows**: always `Available` — the pasteboard / clipboard API
///   exists in every session. (The non-bundled-CLI AppKit caveat is handled by
///   callers via `UC_DISABLE_SYSTEM_CLIPBOARD`, not by this probe.)
/// - **Linux**: `Available` when a display-server session is announced via a
///   non-empty `DISPLAY` (X11) or `WAYLAND_DISPLAY` (Wayland); otherwise
///   `NoDisplaySession` — clipboard backends can only fail to connect.
///
/// This only reports capability; whether to substitute a no-op adapter is the
/// composition root's decision.
pub fn detect_system_clipboard_capability() -> SystemClipboardCapability {
    #[cfg(target_os = "linux")]
    {
        classify_display_session(
            std::env::var_os("DISPLAY"),
            std::env::var_os("WAYLAND_DISPLAY"),
        )
    }
    #[cfg(not(target_os = "linux"))]
    {
        SystemClipboardCapability::Available
    }
}

/// Pure classifier behind [`detect_system_clipboard_capability`] on Linux.
///
/// Empty values are treated as unset — an empty `DISPLAY` cannot address a
/// display server.
#[cfg(any(target_os = "linux", test))]
fn classify_display_session(
    display: Option<std::ffi::OsString>,
    wayland_display: Option<std::ffi::OsString>,
) -> SystemClipboardCapability {
    let is_set =
        |value: &Option<std::ffi::OsString>| value.as_deref().is_some_and(|v| !v.is_empty());
    if is_set(&display) || is_set(&wayland_display) {
        SystemClipboardCapability::Available
    } else {
        SystemClipboardCapability::NoDisplaySession
    }
}

#[cfg(target_os = "macos")]
fn dev_env_forces_file_storage() -> bool {
    std::env::var("UNICLIPBOARD_ENV")
        .map(|value| value == "development")
        .unwrap_or(false)
}

/// Detect if running under WSL (Windows Subsystem for Linux).
///
/// # Detection Methods
///
/// 1. Check `/proc/version` for "Microsoft" or "WSL" strings
/// 2. Check for WSL-specific environment variables:
///    - `WSL_DISTRO_NAME`
///    - `WSL_INTEROP`
#[cfg(target_os = "linux")]
fn is_wsl() -> bool {
    // Method 1: Check /proc/version
    if let Ok(version) = std::fs::read_to_string("/proc/version") {
        if version.contains("Microsoft") || version.contains("WSL") {
            return true;
        }
    }

    // Method 2: Check environment variables
    std::env::var("WSL_DISTRO_NAME").is_ok() || std::env::var("WSL_INTEROP").is_ok()
}

/// Detect if running in a Linux desktop environment.
///
/// # Detection Logic
///
/// A desktop environment is indicated by:
/// - `DISPLAY` environment variable (X11/Wayland display server)
/// - `DBUS_SESSION_BUS_ADDRESS` environment variable (D-Bus session bus)
///
/// Both are required for keyring daemons (gnome-keyring, kwallet, etc.) to function.
#[cfg(target_os = "linux")]
fn has_desktop_environment() -> bool {
    std::env::var("DISPLAY").is_ok() && std::env::var("DBUS_SESSION_BUS_ADDRESS").is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    fn env(value: &str) -> Option<OsString> {
        Some(OsString::from(value))
    }

    #[test]
    fn headless_session_has_no_clipboard() {
        // issue #1021: Ubuntu Server — neither DISPLAY nor WAYLAND_DISPLAY.
        assert_eq!(
            classify_display_session(None, None),
            SystemClipboardCapability::NoDisplaySession
        );
    }

    #[test]
    fn x11_session_has_clipboard() {
        assert_eq!(
            classify_display_session(env(":0"), None),
            SystemClipboardCapability::Available
        );
    }

    #[test]
    fn pure_wayland_session_has_clipboard() {
        assert_eq!(
            classify_display_session(None, env("wayland-0")),
            SystemClipboardCapability::Available
        );
    }

    #[test]
    fn empty_display_values_count_as_unset() {
        assert_eq!(
            classify_display_session(env(""), env("")),
            SystemClipboardCapability::NoDisplaySession
        );
    }
}
