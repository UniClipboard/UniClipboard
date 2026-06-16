use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use chrono::{DateTime, Utc};
use serde::Serialize;
use tracing::instrument;
use uc_core::ports::SettingsPort;
use zip::write::SimpleFileOptions;

#[derive(Clone)]
pub struct DiagnosticsFacadeDeps {
    pub settings: Arc<dyn SettingsPort>,
    pub logs_dir: PathBuf,
    pub app_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DebugStatusView {
    pub debug_mode: bool,
    pub effective_log_profile: String,
    pub restart_required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateDebugModeView {
    pub debug_mode: bool,
    pub restart_required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogExportView {
    pub path: String,
    pub included_files: Vec<String>,
    pub since: DateTime<Utc>,
}

#[derive(Debug, thiserror::Error)]
pub enum DiagnosticsFacadeError {
    #[error("failed to load settings: {0}")]
    LoadSettings(String),
    #[error("failed to save settings: {0}")]
    SaveSettings(String),
    #[error("Downloads directory is unavailable")]
    DownloadsUnavailable,
    #[error("failed to export logs: {0}")]
    Export(String),
}

pub struct DiagnosticsFacade {
    deps: DiagnosticsFacadeDeps,
}

impl DiagnosticsFacade {
    pub fn new(deps: DiagnosticsFacadeDeps) -> Self {
        Self { deps }
    }

    #[instrument(skip_all)]
    pub async fn debug_status(&self) -> Result<DebugStatusView, DiagnosticsFacadeError> {
        let settings = self
            .deps
            .settings
            .load()
            .await
            .map_err(|err| DiagnosticsFacadeError::LoadSettings(err.to_string()))?;
        Ok(DebugStatusView {
            debug_mode: settings.general.debug_mode,
            effective_log_profile: current_effective_log_profile(settings.general.debug_mode),
            restart_required: false,
        })
    }

    #[instrument(skip_all, fields(enabled))]
    pub async fn set_debug_mode(
        &self,
        enabled: bool,
    ) -> Result<UpdateDebugModeView, DiagnosticsFacadeError> {
        let mut settings = self
            .deps
            .settings
            .load()
            .await
            .map_err(|err| DiagnosticsFacadeError::LoadSettings(err.to_string()))?;
        settings.general.debug_mode = enabled;
        self.deps
            .settings
            .save(&settings)
            .await
            .map_err(|err| DiagnosticsFacadeError::SaveSettings(err.to_string()))?;

        Ok(UpdateDebugModeView {
            debug_mode: enabled,
            restart_required: true,
        })
    }

    #[instrument(skip_all, fields(since_hours))]
    pub async fn export_logs(
        &self,
        since_hours: Option<u32>,
    ) -> Result<LogExportView, DiagnosticsFacadeError> {
        let downloads_dir =
            dirs::download_dir().ok_or(DiagnosticsFacadeError::DownloadsUnavailable)?;
        self.export_logs_to_dir(since_hours, downloads_dir).await
    }

    #[instrument(skip_all, fields(since_hours))]
    pub async fn export_logs_to_dir(
        &self,
        since_hours: Option<u32>,
        downloads_dir: PathBuf,
    ) -> Result<LogExportView, DiagnosticsFacadeError> {
        let settings = self
            .deps
            .settings
            .load()
            .await
            .map_err(|err| DiagnosticsFacadeError::LoadSettings(err.to_string()))?;
        let debug_mode = settings.general.debug_mode;
        let effective_log_profile = current_effective_log_profile(debug_mode);
        let since_hours = since_hours.unwrap_or(24).max(1);
        let now = Utc::now();
        let since = now - chrono::Duration::hours(i64::from(since_hours));
        let zip_name = format!(
            "uniclipboard-debug-logs-{}.zip",
            now.format("%Y%m%d-%H%M%S")
        );
        let output_path = downloads_dir.join(zip_name);
        let output_path_for_worker = output_path.clone();
        let deps = self.deps.clone();

        let included_files = tokio::task::spawn_blocking(move || {
            export_logs_blocking(
                &deps.logs_dir,
                &output_path_for_worker,
                since,
                now,
                &deps.app_version,
                debug_mode,
                &effective_log_profile,
            )
        })
        .await
        .map_err(|err| DiagnosticsFacadeError::Export(err.to_string()))??;

        Ok(LogExportView {
            path: output_path.to_string_lossy().to_string(),
            included_files,
            since,
        })
    }
}

fn current_effective_log_profile(debug_mode: bool) -> String {
    if std::env::var("RUST_LOG").is_ok() {
        "rust_log".to_string()
    } else if let Ok(profile) = std::env::var("UC_LOG_PROFILE") {
        if !profile.trim().is_empty() {
            profile
        } else if debug_mode {
            "debug".to_string()
        } else {
            default_profile_name()
        }
    } else if debug_mode {
        "debug".to_string()
    } else {
        default_profile_name()
    }
}

fn default_profile_name() -> String {
    if cfg!(debug_assertions) {
        "dev".to_string()
    } else {
        "prod".to_string()
    }
}

fn export_logs_blocking(
    logs_dir: &Path,
    output_path: &Path,
    since: DateTime<Utc>,
    exported_at: DateTime<Utc>,
    app_version: &str,
    debug_mode: bool,
    effective_log_profile: &str,
) -> Result<Vec<String>, DiagnosticsFacadeError> {
    fs::create_dir_all(
        output_path
            .parent()
            .ok_or_else(|| DiagnosticsFacadeError::Export("output path has no parent".into()))?,
    )
    .map_err(|err| DiagnosticsFacadeError::Export(err.to_string()))?;

    let mut files = collect_recent_log_files(logs_dir, since)?;
    files.sort_by(|a, b| a.archive_name.cmp(&b.archive_name));

    let output =
        File::create(output_path).map_err(|err| DiagnosticsFacadeError::Export(err.to_string()))?;
    let mut zip = zip::ZipWriter::new(output);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let mut included = Vec::with_capacity(files.len());

    for file in &files {
        zip.start_file(format!("logs/{}", file.archive_name), options)
            .map_err(|err| DiagnosticsFacadeError::Export(err.to_string()))?;
        let mut input = File::open(&file.path)
            .map_err(|err| DiagnosticsFacadeError::Export(err.to_string()))?;
        let mut buf = Vec::new();
        input
            .read_to_end(&mut buf)
            .map_err(|err| DiagnosticsFacadeError::Export(err.to_string()))?;
        zip.write_all(&buf)
            .map_err(|err| DiagnosticsFacadeError::Export(err.to_string()))?;
        included.push(file.archive_name.clone());
    }

    let manifest = LogExportManifest {
        exported_at,
        since,
        debug_mode,
        effective_log_profile,
        included_files: included.clone(),
        app_version,
        platform: std::env::consts::OS,
    };
    let manifest_json = serde_json::to_vec_pretty(&manifest)
        .map_err(|err| DiagnosticsFacadeError::Export(err.to_string()))?;
    zip.start_file("manifest.json", options)
        .map_err(|err| DiagnosticsFacadeError::Export(err.to_string()))?;
    zip.write_all(&manifest_json)
        .map_err(|err| DiagnosticsFacadeError::Export(err.to_string()))?;
    zip.finish()
        .map_err(|err| DiagnosticsFacadeError::Export(err.to_string()))?;

    Ok(included)
}

#[derive(Debug)]
struct LogFileCandidate {
    path: PathBuf,
    archive_name: String,
}

fn collect_recent_log_files(
    logs_dir: &Path,
    since: DateTime<Utc>,
) -> Result<Vec<LogFileCandidate>, DiagnosticsFacadeError> {
    if !logs_dir.exists() {
        return Ok(Vec::new());
    }
    let since_system = system_time_from_datetime(since);
    let mut out = Vec::new();
    let entries =
        fs::read_dir(logs_dir).map_err(|err| DiagnosticsFacadeError::Export(err.to_string()))?;
    for entry in entries {
        let entry = entry.map_err(|err| DiagnosticsFacadeError::Export(err.to_string()))?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(name) = path
            .file_name()
            .and_then(|v| v.to_str())
            .map(str::to_string)
        else {
            continue;
        };
        if !is_supported_log_file(&name) {
            continue;
        }
        let metadata = entry
            .metadata()
            .map_err(|err| DiagnosticsFacadeError::Export(err.to_string()))?;
        let modified = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        if modified < since_system && !rolling_name_is_in_window(&name, since) {
            continue;
        }
        out.push(LogFileCandidate {
            path,
            archive_name: name,
        });
    }
    Ok(out)
}

fn is_supported_log_file(name: &str) -> bool {
    [
        "uniclipboard-gui.json.",
        "uniclipboard-daemon.json.",
        "uniclipboard-cli.json.",
    ]
    .iter()
    .any(|prefix| name.starts_with(prefix))
}

fn rolling_name_is_in_window(name: &str, since: DateTime<Utc>) -> bool {
    let Some(date_part) = name.rsplit('.').next() else {
        return false;
    };
    let Ok(date) = chrono::NaiveDate::parse_from_str(date_part, "%Y-%m-%d") else {
        return false;
    };
    date >= since.date_naive()
}

fn system_time_from_datetime(dt: DateTime<Utc>) -> SystemTime {
    if dt.timestamp() >= 0 {
        SystemTime::UNIX_EPOCH + Duration::from_secs(dt.timestamp() as u64)
    } else {
        SystemTime::UNIX_EPOCH
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LogExportManifest<'a> {
    exported_at: DateTime<Utc>,
    since: DateTime<Utc>,
    debug_mode: bool,
    effective_log_profile: &'a str,
    included_files: Vec<String>,
    app_version: &'a str,
    platform: &'a str,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::Mutex;
    use uc_core::settings::model::Settings;

    #[derive(Default)]
    struct InMemorySettings {
        settings: Mutex<Settings>,
    }

    #[async_trait::async_trait]
    impl SettingsPort for InMemorySettings {
        async fn load(&self) -> anyhow::Result<Settings> {
            Ok(self.settings.lock().await.clone())
        }

        async fn save(&self, settings: &Settings) -> anyhow::Result<()> {
            *self.settings.lock().await = settings.clone();
            Ok(())
        }
    }

    fn facade(temp: &tempfile::TempDir) -> DiagnosticsFacade {
        DiagnosticsFacade::new(DiagnosticsFacadeDeps {
            settings: Arc::new(InMemorySettings::default()),
            logs_dir: temp.path().join("logs"),
            app_version: "test-version".to_string(),
        })
    }

    #[tokio::test]
    async fn set_debug_mode_persists_and_requires_restart() {
        let temp = tempfile::TempDir::new().expect("temp");
        let facade = facade(&temp);

        let result = facade.set_debug_mode(true).await.expect("set debug");
        let status = facade.debug_status().await.expect("status");

        assert!(result.debug_mode);
        assert!(result.restart_required);
        assert!(status.debug_mode);
    }

    #[tokio::test]
    async fn export_logs_includes_recent_supported_files_and_manifest() {
        let temp = tempfile::TempDir::new().expect("temp");
        let downloads = temp.path().join("downloads");
        let facade = facade(&temp);
        let logs = temp.path().join("logs");
        fs::create_dir_all(&logs).expect("logs");
        fs::write(logs.join("uniclipboard-gui.json.2099-01-01"), b"gui").expect("write gui");
        fs::write(logs.join("uniclipboard-daemon.json.2099-01-01"), b"daemon")
            .expect("write daemon");
        fs::write(logs.join("other.log"), b"ignore").expect("write other");

        let result = facade
            .export_logs_to_dir(Some(24), downloads)
            .await
            .expect("export logs");

        assert_eq!(
            result.included_files,
            vec![
                "uniclipboard-daemon.json.2099-01-01".to_string(),
                "uniclipboard-gui.json.2099-01-01".to_string(),
            ]
        );
        let file = File::open(&result.path).expect("open zip");
        let mut zip = zip::ZipArchive::new(file).expect("zip");
        assert!(zip.by_name("manifest.json").is_ok());
        assert!(zip.by_name("logs/uniclipboard-gui.json.2099-01-01").is_ok());
        assert!(zip.by_name("logs/other.log").is_err());
    }
}
