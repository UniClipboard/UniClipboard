//! Storage management Tauri commands
//! 存储管理相关的 Tauri 命令

use crate::commands::error::CommandError;
use crate::commands::record_trace_fields;
use crate::commands::TraceMetadata;
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;
use tracing::{info_span, Instrument};

/// Open the application data directory in the system file manager.
/// 在系统文件管理器中打开应用数据目录。
#[tauri::command]
#[specta::specta]
pub async fn open_data_directory(
    app: tauri::AppHandle,
    runtime: tauri::State<'_, std::sync::Arc<crate::bootstrap::TauriAppRuntime>>,
    _trace: Option<TraceMetadata>,
) -> Result<(), CommandError> {
    let span = info_span!(
        "command.storage.open_data_dir",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
    );
    record_trace_fields(&span, &_trace);

    async move {
        let dir = runtime.desktop().storage_paths().app_data_root_dir.clone();
        if !dir.exists() {
            return Err(CommandError::NotFound(format!(
                "Directory does not exist: {}",
                dir.display()
            )));
        }

        app.opener()
            .open_path(dir.to_string_lossy(), None::<&str>)
            .map_err(|e| CommandError::InternalError(e.to_string()))?;

        tracing::info!(dir = %dir.display(), "Opened data directory");
        Ok(())
    }
    .instrument(span)
    .await
}

/// Open the application logs directory in the system file manager.
/// 在系统文件管理器中打开应用日志目录。
#[tauri::command]
#[specta::specta]
pub async fn open_logs_directory(
    app: tauri::AppHandle,
    runtime: tauri::State<'_, std::sync::Arc<crate::bootstrap::TauriAppRuntime>>,
    _trace: Option<TraceMetadata>,
) -> Result<(), CommandError> {
    let span = info_span!(
        "command.storage.open_logs_dir",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
    );
    record_trace_fields(&span, &_trace);

    async move {
        let dir = runtime.desktop().storage_paths().logs_dir.clone();
        std::fs::create_dir_all(&dir).map_err(|e| {
            CommandError::InternalError(format!(
                "Failed to create logs directory {}: {e}",
                dir.display()
            ))
        })?;

        app.opener()
            .open_path(dir.to_string_lossy(), None::<&str>)
            .map_err(|e| CommandError::InternalError(e.to_string()))?;

        tracing::info!(dir = %dir.display(), "Opened logs directory");
        Ok(())
    }
    .instrument(span)
    .await
}

/// Reveal a file or directory in the system file manager, opening its
/// containing folder with the item selected (Finder / Explorer / file
/// manager). Used after a log export to show the user where the zip landed.
/// 在系统文件管理器中定位文件/目录：打开其所在目录并选中该项。
#[tauri::command]
#[specta::specta]
pub async fn reveal_path(
    app: tauri::AppHandle,
    path: String,
    _trace: Option<TraceMetadata>,
) -> Result<(), CommandError> {
    let span = info_span!(
        "command.storage.reveal_path",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
    );
    record_trace_fields(&span, &_trace);

    async move {
        let target = std::path::PathBuf::from(&path);
        if !target.exists() {
            return Err(CommandError::NotFound(format!(
                "Path does not exist: {path}"
            )));
        }

        app.opener()
            .reveal_item_in_dir(&target)
            .map_err(|e| CommandError::InternalError(e.to_string()))?;

        tracing::info!(path = %target.display(), "Revealed path in file manager");
        Ok(())
    }
    .instrument(span)
    .await
}

/// Directory under the OS temp dir that holds the one image handed to an
/// external viewer. Wiped before each hand-off so at most one decrypted copy
/// lingers, and never anywhere the app indexes or syncs.
const IMAGE_HANDOFF_DIR: &str = "uniclipboard-image-handoff";

/// Reduce a caller-supplied file name to a safe basename: no directory parts,
/// no control characters, never empty.
fn sanitize_image_file_name(raw: &str) -> String {
    let base = raw
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or_default()
        .chars()
        .filter(|c| !c.is_control())
        .collect::<String>();
    let base = base.trim().trim_start_matches('.').to_string();
    if base.is_empty() {
        "image".to_string()
    } else {
        base
    }
}

/// Ask where to save a decoded clipboard image and write it there. Returns the
/// chosen path, or `None` when the user cancels the dialog.
/// 弹出保存对话框并写入图片；用户取消时返回 `None`。
#[tauri::command]
#[specta::specta]
pub async fn save_image_as(
    app: tauri::AppHandle,
    file_name: String,
    data: Vec<u8>,
    _trace: Option<TraceMetadata>,
) -> Result<Option<String>, CommandError> {
    let span = info_span!(
        "command.storage.save_image_as",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
    );
    record_trace_fields(&span, &_trace);

    async move {
        let name = sanitize_image_file_name(&file_name);
        let picked = tauri::async_runtime::spawn_blocking(move || {
            app.dialog().file().set_file_name(name).blocking_save_file()
        })
        .await
        .map_err(|e| {
            CommandError::InternalError(format!("save dialog task failed to join: {e}"))
        })?;

        let Some(picked) = picked else {
            return Ok(None);
        };
        let target = picked
            .into_path()
            .map_err(|e| CommandError::InternalError(e.to_string()))?;
        std::fs::write(&target, &data).map_err(|e| {
            CommandError::InternalError(format!("Failed to write {}: {e}", target.display()))
        })?;

        tracing::info!(
            bytes = data.len(),
            "Saved clipboard image to a user-chosen path"
        );
        Ok(Some(target.to_string_lossy().into_owned()))
    }
    .instrument(span)
    .await
}

/// Write a decoded clipboard image to a temporary file and open it in the
/// system's default image viewer (Preview on macOS).
/// 将图片写入临时文件，并用系统默认图片查看器打开。
#[tauri::command]
#[specta::specta]
pub async fn open_image_externally(
    app: tauri::AppHandle,
    file_name: String,
    data: Vec<u8>,
    _trace: Option<TraceMetadata>,
) -> Result<(), CommandError> {
    let span = info_span!(
        "command.storage.open_image_externally",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
    );
    record_trace_fields(&span, &_trace);

    async move {
        let dir = std::env::temp_dir().join(IMAGE_HANDOFF_DIR);
        // Drop the previous hand-off first; a missing directory is fine.
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).map_err(|e| {
            CommandError::InternalError(format!("Failed to create {}: {e}", dir.display()))
        })?;
        let target = dir.join(sanitize_image_file_name(&file_name));
        std::fs::write(&target, &data).map_err(|e| {
            CommandError::InternalError(format!("Failed to write {}: {e}", target.display()))
        })?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o600));
        }

        app.opener()
            .open_path(target.to_string_lossy(), None::<&str>)
            .map_err(|e| CommandError::InternalError(e.to_string()))?;

        tracing::info!(
            bytes = data.len(),
            "Opened clipboard image in the default viewer"
        );
        Ok(())
    }
    .instrument(span)
    .await
}

#[cfg(test)]
mod tests {
    use super::sanitize_image_file_name;

    #[test]
    fn sanitize_keeps_only_a_plain_basename() {
        assert_eq!(sanitize_image_file_name("../../etc/passwd"), "passwd");
        assert_eq!(sanitize_image_file_name("C:\\tmp\\shot.png"), "shot.png");
        assert_eq!(sanitize_image_file_name(".hidden.png"), "hidden.png");
        assert_eq!(sanitize_image_file_name("  "), "image");
        assert_eq!(sanitize_image_file_name("a/"), "image");
    }
}
