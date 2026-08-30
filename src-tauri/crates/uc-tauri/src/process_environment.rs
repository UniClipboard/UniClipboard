//! Process-wide environment policy required before Tauri starts.

#[cfg(target_os = "linux")]
use std::{
    ffi::{OsStr, OsString},
    path::{Component, Path},
};

const LOOPBACK_PROXY_BYPASS: [&str; 3] = ["localhost", "127.0.0.1", "::1"];

#[cfg(target_os = "linux")]
fn appimage_gio_module_dir(
    appimage: Option<&OsStr>,
    appdir: Option<&OsStr>,
    gio_extra_modules: Option<&OsStr>,
) -> Option<OsString> {
    if appimage.is_none_or(|value| value.is_empty()) {
        return None;
    }

    let appdir = Path::new(appdir?);
    let modules = Path::new(gio_extra_modules?);
    let relative = modules.strip_prefix(appdir).ok()?;

    if !appdir.is_absolute()
        || relative
            .components()
            .any(|component| matches!(component, Component::ParentDir))
        || !relative.ends_with("gio/modules")
    {
        return None;
    }

    Some(modules.as_os_str().to_owned())
}

/// Apply process-wide environment policy before any GUI runtime initializes.
///
/// This must run at process entry, before tracing workers, GTK, WebKit, or the
/// daemon sidecar create threads or inherit the environment. For AppImage
/// launches, it isolates bundled GIO from ABI-incompatible host modules. For
/// every install kind, it ensures all GUI network stacks bypass proxies for
/// loopback; native daemon clients also enforce that policy at the HTTP layer.
pub fn prepare_process_environment() {
    #[cfg(target_os = "linux")]
    let appimage_gio_module_dir = appimage_gio_module_dir(
        std::env::var_os("APPIMAGE").as_deref(),
        std::env::var_os("APPDIR").as_deref(),
        std::env::var_os("GIO_EXTRA_MODULES").as_deref(),
    );

    let merged = merge_no_proxy_values([
        std::env::var("NO_PROXY").ok(),
        std::env::var("no_proxy").ok(),
    ]);

    // SAFETY: The binary calls this as the first operation in `main`, before
    // any worker threads, GTK/WebKit initialization, or sidecar spawning.
    unsafe {
        #[cfg(target_os = "linux")]
        if let Some(module_dir) = appimage_gio_module_dir {
            // linuxdeploy adds its package-local modules through
            // GIO_EXTRA_MODULES, but that variable is additive: bundled GIO
            // still scans its compiled-in host directory. Reusing the
            // validated package-local path as GIO_MODULE_DIR replaces that
            // default and prevents ABI-incompatible host modules from loading.
            std::env::set_var("GIO_MODULE_DIR", module_dir);
        }

        std::env::set_var("NO_PROXY", &merged);
        std::env::set_var("no_proxy", merged);
    }
}

fn merge_no_proxy_values(values: impl IntoIterator<Item = Option<String>>) -> String {
    let mut entries = Vec::new();

    for value in values.into_iter().flatten() {
        for entry in value
            .split(',')
            .map(str::trim)
            .filter(|entry| !entry.is_empty())
        {
            if entry == "*" {
                return "*".to_string();
            }
            if !entries.iter().any(|existing| existing == entry) {
                entries.push(entry.to_string());
            }
        }
    }

    for loopback in LOOPBACK_PROXY_BYPASS {
        if !entries.iter().any(|existing| existing == loopback) {
            entries.push(loopback.to_string());
        }
    }

    entries.join(",")
}

#[cfg(test)]
mod tests {
    #[cfg(target_os = "linux")]
    use super::appimage_gio_module_dir;
    use super::merge_no_proxy_values;
    #[cfg(target_os = "linux")]
    use std::{ffi::OsStr, path::Path};

    #[cfg(target_os = "linux")]
    #[test]
    fn isolates_appimage_gio_modules_to_linuxdeploy_directory() {
        let appdir = OsStr::new("/tmp/.mount_UniClip");
        let modules = OsStr::new("/tmp/.mount_UniClip/usr/lib/x86_64-linux-gnu/gio/modules");

        assert_eq!(
            appimage_gio_module_dir(
                Some(OsStr::new("/opt/UniClipboard.AppImage")),
                Some(appdir),
                Some(modules),
            )
            .as_deref(),
            Some(modules)
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn leaves_non_appimage_launches_on_host_gio_modules() {
        assert_eq!(
            appimage_gio_module_dir(
                None,
                Some(OsStr::new("/tmp/.mount_UniClip")),
                Some(OsStr::new(
                    "/tmp/.mount_UniClip/usr/lib/x86_64-linux-gnu/gio/modules",
                )),
            ),
            None
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn rejects_gio_module_directory_outside_appdir() {
        let result = appimage_gio_module_dir(
            Some(OsStr::new("/opt/UniClipboard.AppImage")),
            Some(OsStr::new("/tmp/.mount_UniClip")),
            Some(OsStr::new("/usr/lib/x86_64-linux-gnu/gio/modules")),
        );

        assert_eq!(result, None);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn accepts_only_the_linuxdeploy_gio_modules_subdirectory() {
        let appdir = Path::new("/tmp/.mount_UniClip");

        assert_eq!(
            appimage_gio_module_dir(
                Some(OsStr::new("/opt/UniClipboard.AppImage")),
                Some(appdir.as_os_str()),
                Some(appdir.join("usr/lib").as_os_str()),
            ),
            None
        );
    }

    #[test]
    fn adds_loopback_hosts_when_proxy_bypass_is_absent() {
        assert_eq!(
            merge_no_proxy_values([None, None]),
            "localhost,127.0.0.1,::1"
        );
    }

    #[test]
    fn preserves_both_variable_values_and_deduplicates_loopback_hosts() {
        assert_eq!(
            merge_no_proxy_values([
                Some("example.com, localhost".to_string()),
                Some("internal.test,127.0.0.1".to_string()),
            ]),
            "example.com,localhost,internal.test,127.0.0.1,::1"
        );
    }

    #[test]
    fn normalizes_empty_entries_and_whitespace() {
        assert_eq!(
            merge_no_proxy_values([Some(" , example.com ,, ".to_string()), None]),
            "example.com,localhost,127.0.0.1,::1"
        );
    }

    #[test]
    fn preserves_wildcard_bypass() {
        assert_eq!(
            merge_no_proxy_values([Some("example.com,*".to_string()), None]),
            "*"
        );
    }
}
