use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use anyhow::{Context, Result};

use uc_app::app_paths::AppPaths;

/// Returns the default manager for standalone function use.
fn default_manager() -> &'static DaemonPidManager {
    static DEFAULT_MANAGER: OnceLock<DaemonPidManager> = OnceLock::new();
    DEFAULT_MANAGER.get_or_init(|| {
        DaemonPidManager::new(AppPaths::from_app_dirs(
            &uc_platform::app_dirs::default_app_dirs(),
        ))
    })
}

/// Manages the daemon PID metadata file lifecycle.
#[derive(Debug, Clone)]
pub struct DaemonPidManager {
    app_paths: AppPaths,
}

impl DaemonPidManager {
    /// Creates a new manager using the provided application paths.
    pub fn new(app_paths: AppPaths) -> Self {
        Self { app_paths }
    }

    fn pid_path(&self) -> PathBuf {
        self.app_paths.daemon_pid_path()
    }

    /// Persist the current process PID to the metadata file.
    pub fn write_current_pid(&self) -> Result<u32> {
        let pid_path = self.pid_path();
        let pid = std::process::id();

        if let Some(parent) = pid_path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("failed to create daemon pid directory {}", parent.display())
            })?;
        }

        fs::write(&pid_path, pid.to_string())
            .with_context(|| format!("failed to write daemon pid file {}", pid_path.display()))?;

        repair_pid_permissions(&pid_path)?;
        Ok(pid)
    }

    /// Remove the PID metadata file.
    pub fn remove_pid_file(&self) -> Result<()> {
        let pid_path = self.pid_path();
        match fs::remove_file(&pid_path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(anyhow::Error::new(error).context(format!(
                "failed to remove daemon pid file {}",
                pid_path.display()
            ))),
        }
    }

    /// Read the stored daemon PID, if the file exists.
    pub fn read_pid_file(&self) -> Result<Option<u32>> {
        let pid_path = self.pid_path();
        if !pid_path.exists() {
            return Ok(None);
        }

        let raw = fs::read_to_string(&pid_path)
            .with_context(|| format!("failed to read daemon pid file {}", pid_path.display()))?;
        let pid = raw.trim().parse::<u32>().with_context(|| {
            format!(
                "failed to parse daemon pid file {} contents as u32",
                pid_path.display()
            )
        })?;
        Ok(Some(pid))
    }

    /// Returns the PID file path for testing purposes.
    #[cfg(test)]
    pub fn pid_path_for_testing(&self) -> PathBuf {
        self.pid_path()
    }
}

fn repair_pid_permissions(pid_path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let metadata = fs::metadata(pid_path).with_context(|| {
            format!("failed to read daemon pid metadata {}", pid_path.display())
        })?;
        let current_mode = metadata.permissions().mode() & 0o777;
        if current_mode != 0o600 {
            fs::set_permissions(pid_path, fs::Permissions::from_mode(0o600)).with_context(
                || {
                    format!(
                        "failed to repair daemon pid permissions {}",
                        pid_path.display()
                    )
                },
            )?;
        }
    }

    Ok(())
}

// Backward-compatible standalone functions for external callers.

/// Read the stored daemon PID (standalone function).
pub fn read_pid_file() -> Result<Option<u32>> {
    default_manager().read_pid_file()
}

/// Persist the current daemon PID (standalone function).
pub fn write_current_pid() -> Result<u32> {
    default_manager().write_current_pid()
}

/// Remove the daemon PID metadata file (standalone function).
pub fn remove_pid_file() -> Result<()> {
    default_manager().remove_pid_file()
}

/// Resolve the PID metadata path (standalone function).
pub fn resolve_pid_path() -> PathBuf {
    default_manager().pid_path().to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;
    use std::sync::{Mutex, OnceLock};
    use uc_platform::ports::AppDirsPort;

    /// Temporarily sets `UC_PROFILE` and restores it after `f` completes.
    fn with_uc_profile<T>(profile: Option<&str>, f: impl FnOnce() -> T) -> T {
        static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        let _guard = ENV_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let previous = std::env::var("UC_PROFILE").ok();
        match profile {
            Some(p) => std::env::set_var("UC_PROFILE", p),
            None => std::env::remove_var("UC_PROFILE"),
        }
        let result = f();
        match previous {
            Some(p) => std::env::set_var("UC_PROFILE", p),
            None => std::env::remove_var("UC_PROFILE"),
        }
        result
    }

    /// Creates a DaemonPidManager rooted at the given base directory.
    fn pid_manager_for_testing(base: impl Into<PathBuf>) -> DaemonPidManager {
        let base: PathBuf = base.into();
        let app_dirs = uc_platform::app_dirs::DirsAppDirsAdapter::with_base_data_local_dir(base)
            .get_app_dirs()
            .expect("test base directory should resolve app dirs");
        let app_paths = uc_app::app_paths::AppPaths::from_app_dirs(&app_dirs);
        DaemonPidManager::new(app_paths)
    }

    #[test]
    fn pid_path_tracks_uc_profile() {
        let tempdir = tempfile::tempdir().expect("tempdir should be created");
        let base: PathBuf = tempdir.path().into();

        let path_a = with_uc_profile(Some("a"), || {
            pid_manager_for_testing(&base).pid_path_for_testing()
        });
        let path_b = with_uc_profile(Some("b"), || {
            pid_manager_for_testing(&base).pid_path_for_testing()
        });

        assert_eq!(
            path_a.file_name().and_then(std::ffi::OsStr::to_str),
            Some(".daemon-pid")
        );
        assert_eq!(
            path_b.file_name().and_then(std::ffi::OsStr::to_str),
            Some(".daemon-pid")
        );
        assert_eq!(
            path_a
                .parent()
                .and_then(|p| p.file_name())
                .and_then(OsStr::to_str),
            Some("app.uniclipboard.desktop-a")
        );
        assert_eq!(
            path_b
                .parent()
                .and_then(|p| p.file_name())
                .and_then(OsStr::to_str),
            Some("app.uniclipboard.desktop-b")
        );
        assert_ne!(path_a, path_b);
    }

    #[test]
    fn write_current_pid_persists_profile_aware_pid_file() {
        with_uc_profile(Some("a"), || {
            let mgr = pid_manager_for_testing(tempfile::tempdir().unwrap().path());
            let pid_path = mgr.pid_path_for_testing();
            if let Some(parent) = pid_path.parent() {
                std::fs::create_dir_all(parent).ok();
            }
            std::fs::write(&pid_path, std::process::id().to_string()).unwrap();

            let stored_pid = mgr
                .read_pid_file()
                .expect("pid file should be readable")
                .expect("pid file should exist");
            assert_eq!(stored_pid, std::process::id());

            std::fs::remove_file(&pid_path).ok();
        });
    }

    #[test]
    fn remove_pid_file_deletes_existing_pid_metadata() {
        with_uc_profile(Some("b"), || {
            let mgr = pid_manager_for_testing(tempfile::tempdir().unwrap().path());
            let pid_path = mgr.pid_path_for_testing();
            if let Some(parent) = pid_path.parent() {
                std::fs::create_dir_all(parent).ok();
            }
            std::fs::write(&pid_path, std::process::id().to_string()).unwrap();

            mgr.remove_pid_file().expect("pid file should be removed");
            assert!(mgr
                .read_pid_file()
                .expect("pid read should succeed")
                .is_none());
        });
    }
}
