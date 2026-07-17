//! Tokio-based implementation of [`ReserveInboundFileTargetPort`].
//!
//! Resolves the user-configured save directory from settings and reserves a
//! collision-free path inside it for each inbound file. When no directory is
//! configured or it is currently unusable, returns `None` so callers fall back
//! to their own managed storage layout.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use tracing::{debug, warn};
use uc_core::ports::inbound_file_target::{
    ReserveInboundFileTargetPort, ResolveInboundSaveDirPort,
};
use uc_core::ports::settings::SettingsPort;

/// Upper bound on collision-suffix attempts before giving up and falling back.
const MAX_COLLISION_ATTEMPTS: u32 = 10_000;

/// Fallback when a file name sanitizes to nothing usable.
const FALLBACK_FILENAME: &str = "received-file";

pub struct FsInboundFileTarget {
    settings: Arc<dyn SettingsPort>,
}

impl FsInboundFileTarget {
    pub fn new(settings: Arc<dyn SettingsPort>) -> Arc<Self> {
        Arc::new(Self { settings })
    }

    /// Return the configured save directory, or `None` when it is unset/blank
    /// or settings cannot be read.
    async fn configured_dir(&self) -> Option<PathBuf> {
        let settings = match self.settings.load().await {
            Ok(s) => s,
            Err(e) => {
                warn!(
                    error = %e,
                    "reserve_target: failed to load settings; falling back to managed storage"
                );
                return None;
            }
        };
        let raw = settings.file_sync.auto_save_dir?;
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return None;
        }
        let path = PathBuf::from(trimmed);
        // The port contract promises an absolute destination. A relative value
        // (only reachable via a hand-edited setting / API patch — the folder
        // picker always yields an absolute path) would resolve against the
        // daemon's working directory, an unpredictable location. Reject it and
        // fall back to managed storage instead.
        if !path.is_absolute() {
            warn!(
                dir = %path.display(),
                "reserve_target: configured auto-save dir is not absolute; falling back to managed storage"
            );
            return None;
        }
        Some(path)
    }

    /// Resolve the configured directory and make sure it can hold a receive,
    /// without creating anything inside it.
    ///
    /// "Usable" here means resolvable and creatable — not writable. Proving
    /// write access would mean putting a probe on the final path, which is
    /// exactly what this step must not do (see the sibling `ResolveInboundSaveDirPort`
    /// contract). Write access is instead proven by the create each caller
    /// does next: [`reserve_target`](Self::reserve_target) creates the
    /// reservation placeholder, and the directory path creates its staging
    /// area. Both fail on an existing-but-unwritable directory and fall back to
    /// managed storage there, so returning the directory here defers only the
    /// detection, never the fallback.
    async fn usable_dir(&self) -> Option<PathBuf> {
        let dir = self.configured_dir().await?;

        // A configured-but-missing directory (e.g. an unmounted external
        // drive) must not break inbound sync — fall back to managed storage
        // instead. This also rejects a configured path that is not a directory
        // (e.g. an existing regular file), since `create_dir_all` fails there.
        if let Err(e) = tokio::fs::create_dir_all(&dir).await {
            warn!(
                error = %e,
                "auto-save dir unusable; falling back to managed storage"
            );
            return None;
        }
        Some(dir)
    }
}

#[async_trait]
impl ResolveInboundSaveDirPort for FsInboundFileTarget {
    async fn resolve_save_dir(&self) -> Option<PathBuf> {
        self.usable_dir().await
    }
}

#[async_trait]
impl ReserveInboundFileTargetPort for FsInboundFileTarget {
    async fn reserve_target(&self, file_name: &str) -> Option<PathBuf> {
        let dir = self.usable_dir().await?;

        let sanitized = sanitize_basename(file_name);
        match reserve_unique(&dir, &sanitized).await {
            Some(path) => {
                debug!("reserve_target: reserved auto-save path");
                Some(path)
            }
            None => {
                // Neither the name nor the resulting path may appear here: an
                // inbound file name is user content, and these logs are
                // plaintext.
                warn!(
                    "reserve_target: could not reserve a unique path; falling back to managed storage"
                );
                None
            }
        }
    }
}

/// Atomically reserve a collision-free path in `dir` for `file_name` by
/// creating an empty placeholder with `create_new` (which fails if the name is
/// already taken). The first caller wins the bare name; subsequent callers get
/// `name (1).ext`, `name (2).ext`, ... The reserved placeholder is later
/// overwritten by the file writer.
async fn reserve_unique(dir: &Path, file_name: &str) -> Option<PathBuf> {
    for attempt in 0..MAX_COLLISION_ATTEMPTS {
        let candidate_name = if attempt == 0 {
            file_name.to_string()
        } else {
            suffixed_name(file_name, attempt)
        };
        let candidate = dir.join(&candidate_name);
        match tokio::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
            .await
        {
            Ok(_placeholder) => return Some(candidate),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => {
                warn!(
                    error = %e,
                    "reserve_target: failed to create reservation placeholder"
                );
                return None;
            }
        }
    }
    None
}

/// Insert a ` (n)` disambiguator before the extension: `report.pdf` →
/// `report (1).pdf`; `archive.tar.gz` → `archive.tar (1).gz`; extensionless
/// names get `README (1)`.
fn suffixed_name(file_name: &str, n: u32) -> String {
    let path = Path::new(file_name);
    match (
        path.file_stem().and_then(|s| s.to_str()),
        path.extension().and_then(|e| e.to_str()),
    ) {
        (Some(stem), Some(ext)) if !stem.is_empty() => format!("{stem} ({n}).{ext}"),
        _ => format!("{file_name} ({n})"),
    }
}

/// Reduce an inbound name (possibly attacker-controlled) to a safe bare file
/// name: basename only, dangerous characters neutralized, non-empty. Mirrors
/// the sanitization used on other inbound-file write paths; kept local to
/// avoid a cross-crate import of a private helper.
fn sanitize_basename(value: &str) -> String {
    let basename = Path::new(value)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(value);

    let cleaned: String = basename
        .chars()
        .map(|ch| match ch {
            '/' | '\\' | ':' | '\0' => '_',
            ch if ch.is_control() => '_',
            ch => ch,
        })
        .collect();
    let trimmed = cleaned.trim().trim_matches('.').trim().to_string();

    if trimmed.is_empty() {
        FALLBACK_FILENAME.to_string()
    } else {
        trimmed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use tempfile::TempDir;
    use uc_core::settings::model::Settings;

    /// Minimal settings fake returning a fixed `auto_save_dir`.
    struct FakeSettings {
        auto_save_dir: Option<String>,
    }

    #[async_trait]
    impl SettingsPort for FakeSettings {
        async fn load(&self) -> anyhow::Result<Settings> {
            let mut s = Settings::default();
            s.file_sync.auto_save_dir = self.auto_save_dir.clone();
            Ok(s)
        }
        async fn save(&self, _settings: &Settings) -> anyhow::Result<()> {
            Ok(())
        }
    }

    fn adapter(dir: Option<String>) -> Arc<FsInboundFileTarget> {
        FsInboundFileTarget::new(Arc::new(FakeSettings { auto_save_dir: dir }))
    }

    #[tokio::test]
    async fn returns_none_when_unset() {
        let a = adapter(None);
        assert!(a.reserve_target("report.pdf").await.is_none());
    }

    #[tokio::test]
    async fn returns_none_when_blank() {
        let a = adapter(Some("   ".to_string()));
        assert!(a.reserve_target("report.pdf").await.is_none());
    }

    #[tokio::test]
    async fn returns_none_when_relative() {
        // The port contract promises an absolute destination; a relative
        // configured value must fall back to managed storage rather than
        // resolve against the daemon's working directory.
        let a = adapter(Some("relative/sub".to_string()));
        assert!(a.reserve_target("report.pdf").await.is_none());
    }

    #[tokio::test]
    async fn reserves_flat_path_in_dir() {
        let tmp = TempDir::new().unwrap();
        let a = adapter(Some(tmp.path().display().to_string()));

        let path = a.reserve_target("report.pdf").await.expect("reserved");
        assert_eq!(path, tmp.path().join("report.pdf"));
        // placeholder must exist so a concurrent caller sees the collision
        assert!(path.exists());
    }

    #[tokio::test]
    async fn creates_dir_when_missing() {
        let tmp = TempDir::new().unwrap();
        let nested = tmp.path().join("a").join("b");
        let a = adapter(Some(nested.display().to_string()));

        let path = a.reserve_target("f.bin").await.expect("reserved");
        assert!(nested.exists());
        assert_eq!(path, nested.join("f.bin"));
    }

    #[tokio::test]
    async fn resolve_save_dir_returns_the_configured_dir_without_creating_anything_inside() {
        let tmp = TempDir::new().unwrap();
        let a = adapter(Some(tmp.path().display().to_string()));

        let dir = a.resolve_save_dir().await.expect("resolved");

        assert_eq!(dir, tmp.path());
        // The whole reason this port exists next to `reserve_target`: choosing
        // a destination must not put anything on the final path.
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
    }

    #[tokio::test]
    async fn resolve_save_dir_is_stable_across_calls() {
        let tmp = TempDir::new().unwrap();
        let a = adapter(Some(tmp.path().display().to_string()));

        assert_eq!(a.resolve_save_dir().await, a.resolve_save_dir().await);
    }

    #[tokio::test]
    async fn resolve_save_dir_returns_none_when_unset_or_relative() {
        assert!(adapter(None).resolve_save_dir().await.is_none());
        assert!(adapter(Some("   ".to_string()))
            .resolve_save_dir()
            .await
            .is_none());
        assert!(adapter(Some("relative/sub".to_string()))
            .resolve_save_dir()
            .await
            .is_none());
    }

    #[tokio::test]
    async fn resolve_save_dir_creates_the_dir_when_missing() {
        let tmp = TempDir::new().unwrap();
        let nested = tmp.path().join("a").join("b");
        let a = adapter(Some(nested.display().to_string()));

        assert_eq!(a.resolve_save_dir().await, Some(nested.clone()));
        assert!(nested.exists());
    }

    #[tokio::test]
    async fn resolve_save_dir_returns_none_when_configured_path_is_a_file() {
        // A configured path that already exists as a regular file cannot hold a
        // receive: `create_dir_all` fails on it, so the adapter must decline it
        // and let the caller fall back to managed storage rather than report it
        // as usable. (Root-proof: this failure is `NotADirectory`, not a
        // permission check.)
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("not-a-dir");
        std::fs::write(&file, b"x").unwrap();
        let a = adapter(Some(file.display().to_string()));

        assert!(a.resolve_save_dir().await.is_none());
    }

    #[tokio::test]
    async fn reserve_target_returns_none_when_configured_path_is_a_file() {
        // Same unusable destination as above, reached through the file port:
        // the reservation must fall back to managed storage.
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("not-a-dir");
        std::fs::write(&file, b"x").unwrap();
        let a = adapter(Some(file.display().to_string()));

        assert!(a.reserve_target("report.pdf").await.is_none());
    }

    #[tokio::test]
    async fn suffixes_on_collision() {
        let tmp = TempDir::new().unwrap();
        let a = adapter(Some(tmp.path().display().to_string()));

        let p1 = a.reserve_target("report.pdf").await.unwrap();
        let p2 = a.reserve_target("report.pdf").await.unwrap();
        let p3 = a.reserve_target("report.pdf").await.unwrap();
        assert_eq!(p1, tmp.path().join("report.pdf"));
        assert_eq!(p2, tmp.path().join("report (1).pdf"));
        assert_eq!(p3, tmp.path().join("report (2).pdf"));
    }

    #[tokio::test]
    async fn sanitizes_path_traversal() {
        let tmp = TempDir::new().unwrap();
        let a = adapter(Some(tmp.path().display().to_string()));

        let path = a.reserve_target("../../etc/passwd").await.unwrap();
        // must stay directly inside the configured dir
        assert_eq!(path.parent().unwrap(), tmp.path());
        assert_eq!(path.file_name().unwrap(), "passwd");
    }

    #[test]
    fn suffixed_name_handles_multi_dot_and_extensionless() {
        assert_eq!(suffixed_name("report.pdf", 1), "report (1).pdf");
        assert_eq!(suffixed_name("archive.tar.gz", 2), "archive.tar (2).gz");
        assert_eq!(suffixed_name("README", 1), "README (1)");
        assert_eq!(suffixed_name(".gitignore", 1), ".gitignore (1)");
    }

    #[test]
    fn sanitize_basename_neutralizes_dangerous_input() {
        assert_eq!(sanitize_basename("doc.pdf"), "doc.pdf");
        assert_eq!(sanitize_basename("/etc/shadow"), "shadow");
        assert_eq!(sanitize_basename("a/b/c.key"), "c.key");
        assert_eq!(sanitize_basename("evil\0.txt"), "evil_.txt");
        assert_eq!(sanitize_basename("   "), FALLBACK_FILENAME);
        assert_eq!(sanitize_basename("..."), FALLBACK_FILENAME);
    }
}
