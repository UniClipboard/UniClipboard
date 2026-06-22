//! Helpers for building file-list clipboard snapshots used by the restore
//! use case. Pure functions over `uc-core` types, no port dependency.

use std::path::PathBuf;

use uc_core::clipboard::{MimeType, ObservedClipboardRepresentation, SystemClipboardSnapshot};
use uc_core::ids::{FormatId, RepresentationId};

/// Build a newline-separated `text/uri-list`.
pub(crate) fn build_path_list(file_paths: &[PathBuf]) -> String {
    file_paths
        .iter()
        .map(|path| {
            url::Url::from_file_path(path)
                .map(|url| url.to_string())
                .unwrap_or_else(|_| path.to_string_lossy().into_owned())
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Build a `SystemClipboardSnapshot` with a `text/uri-list` representation.
pub(crate) fn build_file_snapshot(uri_list: &str) -> SystemClipboardSnapshot {
    SystemClipboardSnapshot {
        ts_ms: chrono::Utc::now().timestamp_millis(),
        representations: vec![ObservedClipboardRepresentation::new(
            RepresentationId::new(),
            FormatId::from("files"),
            Some(MimeType::uri_list()),
            uri_list.as_bytes().to_vec(),
        )],
        file_content_digests: Vec::new(),
    }
}
