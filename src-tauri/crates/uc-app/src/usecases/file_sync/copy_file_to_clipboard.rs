use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{bail, Result};
use tracing::{info, info_span, warn, Instrument};

use uc_core::clipboard::{
    ClipboardIntegrationMode, MimeType, ObservedClipboardRepresentation, SystemClipboardSnapshot,
};
use uc_core::ids::{EntryId, FormatId, RepresentationId};
use uc_core::ports::{ClipboardEntryRepositoryPort, ClipboardRepresentationRepositoryPort};

use crate::usecases::clipboard::clipboard_write_coordinator::{
    ClipboardWriteCoordinator, ClipboardWriteIntent,
};

/// Use case for copying file references from a clipboard entry back to the system clipboard.
///
/// Used when user right-clicks a file entry in Dashboard and selects "Copy".
/// Validates file existence before writing to prevent pasting deleted files.
pub struct CopyFileToClipboardUseCase {
    entry_repo: Arc<dyn ClipboardEntryRepositoryPort>,
    representation_repo: Arc<dyn ClipboardRepresentationRepositoryPort>,
    coordinator: Arc<ClipboardWriteCoordinator>,
    mode: ClipboardIntegrationMode,
}

impl CopyFileToClipboardUseCase {
    pub fn new(
        entry_repo: Arc<dyn ClipboardEntryRepositoryPort>,
        representation_repo: Arc<dyn ClipboardRepresentationRepositoryPort>,
        coordinator: Arc<ClipboardWriteCoordinator>,
        mode: ClipboardIntegrationMode,
    ) -> Self {
        Self {
            entry_repo,
            representation_repo,
            coordinator,
            mode,
        }
    }

    /// Copy files from a persisted clipboard entry back to the system clipboard.
    ///
    /// Loads the entry's text/uri-list representation, validates file existence,
    /// then writes to system clipboard.
    pub async fn execute(&self, entry_id: &EntryId) -> Result<()> {
        async {
            if !self.mode.allow_os_write() {
                bail!("System clipboard writes disabled (UC_CLIPBOARD_MODE=passive)");
            }

            // Look up the entry to get its event_id
            let entry = self
                .entry_repo
                .get_entry(entry_id)
                .await?
                .ok_or_else(|| anyhow::anyhow!("Entry not found: {}", entry_id))?;

            // Load representations for this entry
            let reps = self
                .representation_repo
                .get_representations_for_event(&entry.event_id)
                .await?;

            // Find text/uri-list or file/uri-list representation
            let uri_rep = reps.iter().find(|r| {
                if let Some(mime) = &r.mime_type {
                    let m = mime.as_str();
                    m == "text/uri-list" || m == "file/uri-list"
                } else {
                    false
                }
            });

            let uri_rep = match uri_rep {
                Some(r) => r,
                None => bail!("No file URI representation found for entry {}", entry_id),
            };

            // Get the bytes (inline or from blob)
            let bytes = match &uri_rep.inline_data {
                Some(data) => data.clone(),
                None => bail!(
                    "File URI representation has no inline data for entry {}",
                    entry_id
                ),
            };

            let uri_string = String::from_utf8(bytes)?;

            // Parse and validate file paths (native paths or backward-compat file:// URIs)
            let mut file_paths = Vec::new();
            for line in uri_string.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                if line.starts_with("file://") {
                    // Backward compat: older entries stored as file:// URIs
                    match url::Url::parse(line) {
                        Ok(url) => {
                            if let Ok(path) = url.to_file_path() {
                                file_paths.push(path);
                            } else {
                                warn!(uri = %line, "Failed to convert URI to file path");
                            }
                        }
                        Err(e) => {
                            warn!(uri = %line, error = %e, "Failed to parse file URI");
                        }
                    }
                } else {
                    // Native path (new format)
                    file_paths.push(PathBuf::from(line));
                }
            }

            if file_paths.is_empty() {
                bail!("No valid file paths found in entry {}", entry_id);
            }

            // Validate all files exist
            for path in &file_paths {
                if !path.exists() {
                    bail!("File deleted: {}", path.display());
                }
            }

            // Build snapshot and write
            self.write_files_to_clipboard(&file_paths).await
        }
        .instrument(info_span!(
            "usecase.file_sync.copy_file_to_clipboard",
            entry_id = %entry_id,
        ))
        .await
    }

    /// Write file paths directly to the system clipboard.
    ///
    /// Used both by `execute` (from entry_id) and by the auto-write path in wiring.rs.
    pub async fn execute_from_paths(&self, file_paths: Vec<PathBuf>) -> Result<()> {
        if !self.mode.allow_os_write() {
            bail!("System clipboard writes disabled (UC_CLIPBOARD_MODE=passive)");
        }

        // Validate all files exist
        for path in &file_paths {
            if !path.exists() {
                bail!("File deleted: {}", path.display());
            }
        }

        self.write_files_to_clipboard(&file_paths).await
    }

    async fn write_files_to_clipboard(&self, file_paths: &[PathBuf]) -> Result<()> {
        let path_list = build_path_list(file_paths);
        let snapshot = build_file_snapshot(&path_list);
        self.coordinator
            .write(snapshot, ClipboardWriteIntent::LocalRestore)
            .await?;
        info!(
            file_count = file_paths.len(),
            "Files written to system clipboard"
        );
        Ok(())
    }
}

/// Build a newline-separated list of native file paths.
pub fn build_path_list(file_paths: &[PathBuf]) -> String {
    file_paths
        .iter()
        .map(|p| p.to_string_lossy())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Build a SystemClipboardSnapshot with a text/uri-list representation.
pub fn build_file_snapshot(uri_list: &str) -> SystemClipboardSnapshot {
    SystemClipboardSnapshot {
        ts_ms: chrono::Utc::now().timestamp_millis(),
        representations: vec![ObservedClipboardRepresentation::new(
            RepresentationId::new(),
            FormatId::from("files"),
            Some(MimeType::uri_list()),
            uri_list.as_bytes().to_vec(),
        )],
    }
}
