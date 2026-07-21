use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

use uc_engine::{
    HostCapabilities, HostCapabilityError, HostCapabilityErrorCategory, HostClipboard,
    HostClipboardRepresentation, HostClipboardSnapshot, HostDirectories, HostFileAccess,
    HostFileHandle, HostFileMetadata, HostSecureStorage,
};

#[derive(Default)]
struct MemorySecureStorage {
    values: Mutex<HashMap<String, Vec<u8>>>,
}

impl MemorySecureStorage {
    fn values(&self) -> MutexGuard<'_, HashMap<String, Vec<u8>>> {
        match self.values.lock() {
            Ok(values) => values,
            Err(poisoned) => poisoned.into_inner(),
        }
    }
}

impl HostSecureStorage for MemorySecureStorage {
    fn get(&self, key: &str) -> Result<Option<Vec<u8>>, HostCapabilityError> {
        Ok(self.values().get(key).cloned())
    }

    fn set(&self, key: &str, value: &[u8]) -> Result<(), HostCapabilityError> {
        self.values().insert(key.to_owned(), value.to_vec());
        Ok(())
    }

    fn delete(&self, key: &str) -> Result<(), HostCapabilityError> {
        self.values().remove(key);
        Ok(())
    }
}

struct EmptyClipboard;

impl HostClipboard for EmptyClipboard {
    fn read(&self) -> Result<HostClipboardSnapshot, HostCapabilityError> {
        Ok(HostClipboardSnapshot {
            observed_at_ms: 1,
            representations: Vec::new(),
        })
    }

    fn write(&self, _snapshot: HostClipboardSnapshot) -> Result<(), HostCapabilityError> {
        Ok(())
    }
}

struct EmptyFiles;

impl HostFileAccess for EmptyFiles {
    fn metadata(&self, _handle: &HostFileHandle) -> Result<HostFileMetadata, HostCapabilityError> {
        Ok(HostFileMetadata {
            display_name: "private.txt".into(),
            size_bytes: 0,
            mime_type: Some("text/plain".into()),
        })
    }

    fn read_chunk(
        &self,
        _handle: &HostFileHandle,
        _offset: u64,
        _max_bytes: u32,
    ) -> Result<Vec<u8>, HostCapabilityError> {
        Ok(Vec::new())
    }

    fn write_chunk(
        &self,
        _handle: &HostFileHandle,
        _offset: u64,
        _bytes: &[u8],
    ) -> Result<(), HostCapabilityError> {
        Ok(())
    }

    fn finish_write(&self, _handle: &HostFileHandle) -> Result<(), HostCapabilityError> {
        Ok(())
    }
}

#[test]
fn host_capabilities_are_constructible_without_internal_ports() {
    let capabilities = HostCapabilities::new(
        HostDirectories::new(
            PathBuf::from("/private/data"),
            PathBuf::from("/private/cache"),
            PathBuf::from("/private/temp"),
        ),
        Box::new(MemorySecureStorage::default()),
        Box::new(EmptyClipboard),
        Box::new(EmptyFiles),
    );

    let debug = format!("{capabilities:?}");
    assert!(!debug.contains("/private"));
    assert!(!debug.contains("MemorySecureStorage"));
    assert_eq!(
        capabilities.directories().private_data(),
        PathBuf::from("/private/data")
    );
    let snapshot = match capabilities.clipboard().read() {
        Ok(snapshot) => snapshot,
        Err(error) => panic!("clipboard read failed: {error}"),
    };
    assert!(snapshot.representations.is_empty());
}

#[test]
fn host_boundary_debug_output_redacts_user_content_and_details() {
    let snapshot = HostClipboardSnapshot {
        observed_at_ms: 1,
        representations: vec![
            HostClipboardRepresentation::Inline {
                format: "public.utf8-plain-text".into(),
                mime_type: Some("text/plain".into()),
                bytes: b"private clipboard text".to_vec(),
            },
            HostClipboardRepresentation::File {
                format: "files".into(),
                handle: HostFileHandle::new("private-handle"),
                display_name: "private.txt".into(),
                mime_type: Some("text/plain".into()),
                size_bytes: 42,
            },
        ],
    };
    let metadata = HostFileMetadata {
        display_name: "private.txt".into(),
        size_bytes: 42,
        mime_type: Some("text/plain".into()),
    };
    let error = HostCapabilityError::new(
        HostCapabilityErrorCategory::Unavailable,
        "secret platform error detail",
    );
    let debug = format!("{snapshot:?} {metadata:?} {error:?}");

    for secret in [
        "private clipboard text",
        "private-handle",
        "private.txt",
        "secret platform error detail",
    ] {
        assert!(!debug.contains(secret), "debug output leaked {secret}");
    }
    assert_eq!(error.category(), HostCapabilityErrorCategory::Unavailable);
}
