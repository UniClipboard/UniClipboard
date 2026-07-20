use std::fmt;

use serde::{Deserialize, Serialize};

pub const FILE_DISPLAY_METADATA_FORMAT: &str = "uniclipboard-file-display-metadata";
pub const FILE_DISPLAY_METADATA_MIME: &str =
    "application/x-uniclipboard-file-display-metadata+json";

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileDisplayMetadata {
    pub files: Vec<FileDisplayMetadataEntry>,
}

impl FileDisplayMetadata {
    pub fn encode(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec(self)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, serde_json::Error> {
        serde_json::from_slice(bytes)
    }

    pub fn display_name_for(&self, storage_name: &str) -> Option<&str> {
        self.files
            .iter()
            .find(|file| file.storage_name == storage_name)
            .map(|file| file.display_name.as_str())
    }
}

impl fmt::Debug for FileDisplayMetadata {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("FileDisplayMetadata")
            .field("file_count", &self.files.len())
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileDisplayMetadataEntry {
    pub storage_name: String,
    pub display_name: String,
}

impl fmt::Debug for FileDisplayMetadataEntry {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("FileDisplayMetadataEntry")
            .field("storage_name", &self.storage_name)
            .field("display_name", &"[REDACTED]")
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_round_trip_preserves_display_name_without_debugging_it() {
        let metadata = FileDisplayMetadata {
            files: vec![FileDisplayMetadataEntry {
                storage_name: "00000000".into(),
                display_name: "private report.txt".into(),
            }],
        };

        let encoded = metadata.encode().unwrap();
        let decoded = FileDisplayMetadata::decode(&encoded).unwrap();

        assert_eq!(decoded, metadata);
        assert_eq!(
            decoded.display_name_for("00000000"),
            Some("private report.txt")
        );
        assert!(!format!("{decoded:?}").contains("private report.txt"));
    }
}
