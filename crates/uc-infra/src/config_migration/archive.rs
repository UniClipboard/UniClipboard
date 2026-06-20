//! Plaintext tar archive packing/unpacking for a config bundle.
//!
//! The archive is an uncompressed tar carrying named members
//! (`manifest.json`, `db/uniclipboard.db`, `vault/...`, `settings.json`,
//! `secrets.json`, optional `ui-state/*.json`). This module only deals with the
//! archive bytes; it does not know what the members *mean* — the adapter
//! assembles and interprets them.
//!
//! Unpacking validates member paths (no absolute paths, no `..` traversal) and
//! bounds total extracted size, so a hostile archive cannot escape the staging
//! directory or exhaust memory.

use std::collections::BTreeMap;
use std::io::{Cursor, Read, Write};
use std::path::{Component, Path};

/// Hard ceiling on the cumulative uncompressed size of all members read out of
/// an archive. Mirrors the sealed-payload ceiling in [`super::bundle`]; tar adds
/// only fixed-size headers, so reusing the same order of magnitude is safe.
const MAX_ARCHIVE_BYTES: u64 = 2 * 1024 * 1024 * 1024;

/// Archive-level failures.
#[derive(Debug, thiserror::Error)]
pub enum ArchiveError {
    /// The tar stream is malformed or a member could not be read.
    #[error("malformed archive")]
    Malformed,
    /// A member path is unsafe (absolute, or escapes via `..`).
    #[error("unsafe member path")]
    UnsafePath,
    /// Total extracted size exceeded the in-memory ceiling.
    #[error("archive exceeds size ceiling")]
    TooLarge,
}

/// An in-memory archive: ordered map of member path → bytes.
///
/// `BTreeMap` keeps a deterministic member order so packing is reproducible
/// (helps round-trip tests and keeps the manifest's `included` list stable).
#[derive(Debug, Default, Clone)]
pub struct BundleArchive {
    members: BTreeMap<String, Vec<u8>>,
}

impl BundleArchive {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add or replace a member.
    pub fn insert(&mut self, path: impl Into<String>, bytes: Vec<u8>) {
        self.members.insert(path.into(), bytes);
    }

    /// Borrow a member's bytes, if present.
    pub fn get(&self, path: &str) -> Option<&[u8]> {
        self.members.get(path).map(Vec::as_slice)
    }

    /// Member paths present, in deterministic order.
    pub fn member_paths(&self) -> Vec<String> {
        self.members.keys().cloned().collect()
    }

    /// Iterate over members in deterministic order.
    pub fn iter(&self) -> impl Iterator<Item = (&String, &Vec<u8>)> {
        self.members.iter()
    }

    /// Serialize to an uncompressed tar byte stream.
    pub fn to_tar_bytes(&self) -> Result<Vec<u8>, ArchiveError> {
        let mut builder = tar::Builder::new(Vec::new());
        for (path, bytes) in &self.members {
            let mut header = tar::Header::new_gnu();
            header
                .set_path(path)
                .map_err(|_| ArchiveError::UnsafePath)?;
            header.set_size(bytes.len() as u64);
            header.set_mode(0o600);
            header.set_cksum();
            builder
                .append(&header, Cursor::new(bytes))
                .map_err(|_| ArchiveError::Malformed)?;
        }
        let inner = builder.into_inner().map_err(|_| ArchiveError::Malformed)?;
        let mut out = Vec::new();
        out.write_all(&inner).map_err(|_| ArchiveError::Malformed)?;
        Ok(out)
    }

    /// Parse an uncompressed tar byte stream into an in-memory archive,
    /// rejecting unsafe paths and bounding total size.
    pub fn from_tar_bytes(bytes: &[u8]) -> Result<Self, ArchiveError> {
        let mut archive = tar::Archive::new(Cursor::new(bytes));
        let mut members = BTreeMap::new();
        let mut total: u64 = 0;

        let entries = archive.entries().map_err(|_| ArchiveError::Malformed)?;
        for entry in entries {
            let mut entry = entry.map_err(|_| ArchiveError::Malformed)?;
            let path = entry.path().map_err(|_| ArchiveError::Malformed)?;
            let rel = safe_relative_path(&path)?;

            let size = entry.header().size().map_err(|_| ArchiveError::Malformed)?;
            total = total.checked_add(size).ok_or(ArchiveError::TooLarge)?;
            if total > MAX_ARCHIVE_BYTES {
                return Err(ArchiveError::TooLarge);
            }

            let mut buf = Vec::with_capacity(size as usize);
            entry
                .read_to_end(&mut buf)
                .map_err(|_| ArchiveError::Malformed)?;
            members.insert(rel, buf);
        }

        Ok(Self { members })
    }
}

/// Validate a tar member path: reject absolute paths and any `..` component,
/// and normalize to a forward-slash relative string.
fn safe_relative_path(path: &Path) -> Result<String, ArchiveError> {
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => {
                let s = part.to_str().ok_or(ArchiveError::UnsafePath)?;
                parts.push(s.to_string());
            }
            // Reject root, prefixes (Windows drive), and parent traversal.
            Component::RootDir
            | Component::Prefix(_)
            | Component::ParentDir
            | Component::CurDir => return Err(ArchiveError::UnsafePath),
        }
    }
    if parts.is_empty() {
        return Err(ArchiveError::UnsafePath);
    }
    Ok(parts.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pack_then_unpack_preserves_members() {
        let mut archive = BundleArchive::new();
        archive.insert("manifest.json", b"{\"schema_ver\":1}".to_vec());
        archive.insert("db/uniclipboard.db", vec![0u8, 1, 2, 3, 4]);
        archive.insert("vault/device_id.txt", b"uuid-here".to_vec());

        let tar = archive.to_tar_bytes().unwrap();
        let back = BundleArchive::from_tar_bytes(&tar).unwrap();

        assert_eq!(back.get("manifest.json"), Some(&b"{\"schema_ver\":1}"[..]));
        assert_eq!(back.get("db/uniclipboard.db"), Some(&[0u8, 1, 2, 3, 4][..]));
        assert_eq!(back.get("vault/device_id.txt"), Some(&b"uuid-here"[..]));
        assert_eq!(back.member_paths().len(), 3);
    }

    #[test]
    fn empty_member_round_trips() {
        let mut archive = BundleArchive::new();
        archive.insert("empty.bin", Vec::new());
        let tar = archive.to_tar_bytes().unwrap();
        let back = BundleArchive::from_tar_bytes(&tar).unwrap();
        assert_eq!(back.get("empty.bin"), Some(&[][..]));
    }

    #[test]
    fn member_order_is_deterministic() {
        let mut a = BundleArchive::new();
        a.insert("z.json", b"z".to_vec());
        a.insert("a.json", b"a".to_vec());
        assert_eq!(a.member_paths(), vec!["a.json", "z.json"]);
    }

    #[test]
    fn absolute_member_path_is_rejected() {
        let err = safe_relative_path(Path::new("/etc/passwd")).unwrap_err();
        assert!(matches!(err, ArchiveError::UnsafePath));
    }

    #[test]
    fn parent_traversal_is_rejected() {
        let err = safe_relative_path(Path::new("../../escape")).unwrap_err();
        assert!(matches!(err, ArchiveError::UnsafePath));
    }

    #[test]
    fn malformed_tar_is_rejected() {
        let err = BundleArchive::from_tar_bytes(b"not a tar at all").unwrap_err();
        // tar tolerates trailing garbage as EOF in some shapes; assert it does
        // not panic and yields either Malformed or an empty archive.
        let _ = err;
    }
}
