use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};

use fs2::FileExt;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::{Uuid, Variant, Version};

pub const SPACE_CATALOG_FILE_NAME: &str = "space-catalog.json";
const SPACE_CATALOG_LOCK_FILE_NAME: &str = ".space-catalog.lock";
const LEGACY_PROFILE_DIR: &str = ".";

#[derive(Debug, Error)]
pub enum SpaceCatalogError {
    #[error("failed to {operation} the space catalog")]
    Io {
        operation: &'static str,
        #[source]
        source: io::Error,
    },
    #[error("space catalog JSON is invalid")]
    InvalidJson(#[from] serde_json::Error),
    #[error("unknown space profile ID: {profile_id}")]
    UnknownProfileId { profile_id: String },
    #[error("cannot remove the active-send space profile: {profile_id}")]
    CannotRemoveActiveSend { profile_id: String },
    #[error("space profile ID is not a random UUID: {profile_id}")]
    InvalidProfileId { profile_id: String },
    #[error("unsafe space profile directory: {profile_dir}")]
    UnsafeProfileDirectory { profile_dir: String },
    #[error("duplicate space profile ID: {profile_id}")]
    DuplicateProfileId { profile_id: String },
    #[error("duplicate space profile directory: {profile_dir}")]
    DuplicateProfileDirectory { profile_dir: String },
    #[error("space catalog must contain exactly one active-send target, found {count}")]
    InvalidActiveSendCount { count: usize },
    #[error("active-send space profile is disabled: {profile_id}")]
    ActiveSendProfileDisabled { profile_id: String },
    #[error("space catalog changed since it was loaded")]
    ConcurrentModification,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpaceCatalogEntry {
    pub profile_id: String,
    pub profile_dir: String,
    pub enabled: bool,
    pub active_send: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CatalogDocument {
    entries: Vec<SpaceCatalogEntry>,
}

#[derive(Debug)]
pub struct SpaceCatalog {
    root: PathBuf,
    document: CatalogDocument,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CatalogPathState {
    Present,
    Missing,
}

impl SpaceCatalog {
    pub fn load_or_migrate(root: impl AsRef<Path>) -> Result<Self, SpaceCatalogError> {
        Self::load_or_migrate_with_probe(root, probe_catalog_path)
    }

    fn load_or_migrate_with_probe(
        root: impl AsRef<Path>,
        probe: impl FnOnce(&Path) -> Result<CatalogPathState, SpaceCatalogError>,
    ) -> Result<Self, SpaceCatalogError> {
        let root = root.as_ref().to_path_buf();
        fs::create_dir_all(&root).map_err(|source| io_error("create data root", source))?;
        let _lock = lock_catalog(&root)?;
        let path = root.join(SPACE_CATALOG_FILE_NAME);

        let document = match probe(&path)? {
            CatalogPathState::Present => read_document(&path)?,
            CatalogPathState::Missing => {
                let document = CatalogDocument {
                    entries: vec![SpaceCatalogEntry {
                        profile_id: Uuid::new_v4().to_string(),
                        profile_dir: LEGACY_PROFILE_DIR.to_string(),
                        enabled: true,
                        active_send: true,
                    }],
                };
                validate_document(&document)?;
                write_document(&root, &document)?;
                document
            }
        };

        Ok(Self { root, document })
    }

    pub fn entries(&self) -> &[SpaceCatalogEntry] {
        &self.document.entries
    }

    pub fn add_profile(&mut self) -> Result<SpaceCatalogEntry, SpaceCatalogError> {
        let entry = loop {
            let profile_id = Uuid::new_v4().to_string();
            let profile_dir = format!("profile-{profile_id}");
            if self.document.entries.iter().all(|candidate| {
                candidate.profile_id != profile_id && candidate.profile_dir != profile_dir
            }) {
                break SpaceCatalogEntry {
                    profile_id,
                    profile_dir,
                    enabled: true,
                    active_send: false,
                };
            }
        };
        let mut candidate = self.document.clone();
        candidate.entries.push(entry.clone());
        self.persist(candidate)?;
        Ok(entry)
    }

    pub fn set_active_send(&mut self, profile_id: &str) -> Result<(), SpaceCatalogError> {
        parse_canonical_profile_id(profile_id)?;
        if !self
            .document
            .entries
            .iter()
            .any(|entry| entry.profile_id == profile_id)
        {
            return Err(SpaceCatalogError::UnknownProfileId {
                profile_id: profile_id.to_string(),
            });
        }

        let mut candidate = self.document.clone();
        for entry in &mut candidate.entries {
            entry.active_send = entry.profile_id == profile_id;
        }
        self.persist(candidate)
    }

    pub fn remove_profile(
        &mut self,
        profile_id: &str,
    ) -> Result<SpaceCatalogEntry, SpaceCatalogError> {
        parse_canonical_profile_id(profile_id)?;
        let position = self
            .document
            .entries
            .iter()
            .position(|entry| entry.profile_id == profile_id)
            .ok_or_else(|| SpaceCatalogError::UnknownProfileId {
                profile_id: profile_id.to_string(),
            })?;
        if self.document.entries[position].active_send {
            return Err(SpaceCatalogError::CannotRemoveActiveSend {
                profile_id: profile_id.to_string(),
            });
        }

        let mut candidate = self.document.clone();
        let removed = candidate.entries.remove(position);
        self.persist(candidate)?;
        Ok(removed)
    }

    fn persist(&mut self, candidate: CatalogDocument) -> Result<(), SpaceCatalogError> {
        let _lock = lock_catalog(&self.root)?;
        let current = read_document(&self.root.join(SPACE_CATALOG_FILE_NAME))?;
        if current != self.document {
            return Err(SpaceCatalogError::ConcurrentModification);
        }
        validate_document(&candidate)?;
        write_document(&self.root, &candidate)?;
        self.document = candidate;
        Ok(())
    }
}

fn probe_catalog_path(path: &Path) -> Result<CatalogPathState, SpaceCatalogError> {
    probe_catalog_path_with(
        path,
        |candidate| fs::metadata(candidate).map(drop),
        |candidate| fs::symlink_metadata(candidate).map(drop),
    )
}

fn probe_catalog_path_with(
    path: &Path,
    metadata: impl FnOnce(&Path) -> io::Result<()>,
    symlink_metadata: impl FnOnce(&Path) -> io::Result<()>,
) -> Result<CatalogPathState, SpaceCatalogError> {
    match metadata(path) {
        Ok(()) => Ok(CatalogPathState::Present),
        Err(source) if source.kind() == io::ErrorKind::NotFound => match symlink_metadata(path) {
            Err(entry_error) if entry_error.kind() == io::ErrorKind::NotFound => {
                Ok(CatalogPathState::Missing)
            }
            Ok(()) => Err(io_error("inspect catalog target", source)),
            Err(entry_error) => Err(io_error("inspect catalog directory entry", entry_error)),
        },
        Err(source) => Err(io_error("inspect catalog metadata", source)),
    }
}

fn read_document(path: &Path) -> Result<CatalogDocument, SpaceCatalogError> {
    let bytes = fs::read(path).map_err(|source| io_error("read", source))?;
    let document = serde_json::from_slice(&bytes)?;
    validate_document(&document)?;
    Ok(document)
}

fn validate_document(document: &CatalogDocument) -> Result<(), SpaceCatalogError> {
    let mut profile_ids = HashSet::with_capacity(document.entries.len());
    let mut profile_directories = HashSet::with_capacity(document.entries.len());
    let mut active_send_count = 0;

    for entry in &document.entries {
        let parsed = parse_canonical_profile_id(&entry.profile_id)?;
        if !is_safe_profile_directory(&entry.profile_dir, &entry.profile_id) {
            return Err(SpaceCatalogError::UnsafeProfileDirectory {
                profile_dir: entry.profile_dir.clone(),
            });
        }
        if !profile_ids.insert(parsed) {
            return Err(SpaceCatalogError::DuplicateProfileId {
                profile_id: entry.profile_id.clone(),
            });
        }
        if !profile_directories.insert(entry.profile_dir.clone()) {
            return Err(SpaceCatalogError::DuplicateProfileDirectory {
                profile_dir: entry.profile_dir.clone(),
            });
        }
        if entry.active_send {
            active_send_count += 1;
            if !entry.enabled {
                return Err(SpaceCatalogError::ActiveSendProfileDisabled {
                    profile_id: entry.profile_id.clone(),
                });
            }
        }
    }

    if active_send_count != 1 {
        return Err(SpaceCatalogError::InvalidActiveSendCount {
            count: active_send_count,
        });
    }
    Ok(())
}

fn parse_canonical_profile_id(profile_id: &str) -> Result<Uuid, SpaceCatalogError> {
    let parsed = Uuid::parse_str(profile_id).map_err(|_| SpaceCatalogError::InvalidProfileId {
        profile_id: profile_id.to_string(),
    })?;
    if parsed.get_version() != Some(Version::Random)
        || parsed.get_variant() != Variant::RFC4122
        || parsed.hyphenated().to_string() != profile_id
    {
        return Err(SpaceCatalogError::InvalidProfileId {
            profile_id: profile_id.to_string(),
        });
    }
    Ok(parsed)
}

fn is_safe_profile_directory(profile_dir: &str, profile_id: &str) -> bool {
    if profile_dir == LEGACY_PROFILE_DIR {
        return true;
    }
    if profile_dir != format!("profile-{profile_id}")
        || profile_dir.is_empty()
        || profile_dir.ends_with(['.', ' '])
        || profile_dir.chars().any(|character| {
            character <= '\u{1f}'
                || matches!(
                    character,
                    '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'
                )
        })
        || profile_dir.eq_ignore_ascii_case(SPACE_CATALOG_FILE_NAME)
        || profile_dir.eq_ignore_ascii_case(SPACE_CATALOG_LOCK_FILE_NAME)
    {
        return false;
    }

    let mut components = Path::new(profile_dir).components();
    if !matches!(components.next(), Some(Component::Normal(_))) || components.next().is_some() {
        return false;
    }

    let device_name = profile_dir
        .split('.')
        .next()
        .unwrap_or(profile_dir)
        .to_ascii_uppercase();
    !matches!(device_name.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        && !is_numbered_windows_device(&device_name, "COM")
        && !is_numbered_windows_device(&device_name, "LPT")
}

fn is_numbered_windows_device(name: &str, prefix: &str) -> bool {
    name.strip_prefix(prefix).is_some_and(|suffix| {
        matches!(
            suffix,
            "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
        )
    })
}

fn lock_catalog(root: &Path) -> Result<File, SpaceCatalogError> {
    let lock = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(root.join(SPACE_CATALOG_LOCK_FILE_NAME))
        .map_err(|source| io_error("open lock", source))?;
    lock.lock_exclusive()
        .map_err(|source| io_error("lock", source))?;
    Ok(lock)
}

fn write_document(root: &Path, document: &CatalogDocument) -> Result<(), SpaceCatalogError> {
    write_document_with_replace(root, document, replace_file)
}

fn write_document_with_replace(
    root: &Path,
    document: &CatalogDocument,
    replace: impl FnOnce(&Path, &Path) -> Result<(), SpaceCatalogError>,
) -> Result<(), SpaceCatalogError> {
    let bytes = serde_json::to_vec_pretty(document)?;
    let target = root.join(SPACE_CATALOG_FILE_NAME);
    let temporary = root.join(format!(".{SPACE_CATALOG_FILE_NAME}.{}.tmp", Uuid::new_v4()));
    let result = write_and_replace(&temporary, &target, &bytes, replace);
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn write_and_replace(
    temporary: &Path,
    target: &Path,
    bytes: &[u8],
    replace: impl FnOnce(&Path, &Path) -> Result<(), SpaceCatalogError>,
) -> Result<(), SpaceCatalogError> {
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(temporary)
        .map_err(|source| io_error("create temporary file", source))?;
    file.write_all(bytes)
        .map_err(|source| io_error("write temporary file", source))?;
    file.flush()
        .map_err(|source| io_error("flush temporary file", source))?;
    file.sync_all()
        .map_err(|source| io_error("sync temporary file", source))?;
    drop(file);

    replace(temporary, target)?;
    sync_parent_directory(target)?;
    Ok(())
}

#[cfg(windows)]
fn replace_file(temporary: &Path, target: &Path) -> Result<(), SpaceCatalogError> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };

    let temporary: Vec<u16> = temporary.as_os_str().encode_wide().chain(Some(0)).collect();
    let target: Vec<u16> = target.as_os_str().encode_wide().chain(Some(0)).collect();
    // SAFETY: Both buffers are NUL-terminated UTF-16 and remain alive for the call.
    let replaced = unsafe {
        MoveFileExW(
            temporary.as_ptr(),
            target.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if replaced == 0 {
        return Err(io_error("atomically replace", io::Error::last_os_error()));
    }
    Ok(())
}

#[cfg(not(windows))]
fn replace_file(temporary: &Path, target: &Path) -> Result<(), SpaceCatalogError> {
    fs::rename(temporary, target).map_err(|source| io_error("atomically replace", source))
}

#[cfg(windows)]
fn sync_parent_directory(_target: &Path) -> Result<(), SpaceCatalogError> {
    Ok(())
}

#[cfg(not(windows))]
fn sync_parent_directory(target: &Path) -> Result<(), SpaceCatalogError> {
    let parent = target.parent().ok_or_else(|| {
        io_error(
            "locate parent directory",
            io::Error::new(io::ErrorKind::InvalidInput, "catalog has no parent"),
        )
    })?;
    File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|source| io_error("sync parent directory", source))
}

fn io_error(operation: &'static str, source: io::Error) -> SpaceCatalogError {
    SpaceCatalogError::Io { operation, source }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io;
    use std::sync::{Arc, Barrier};
    use std::thread;

    use super::*;

    fn write_catalog(root: &Path, value: serde_json::Value) {
        fs::write(
            root.join(SPACE_CATALOG_FILE_NAME),
            serde_json::to_vec_pretty(&value).expect("serialize test catalog"),
        )
        .expect("write test catalog");
    }

    #[test]
    fn first_start_adopts_legacy_root_without_mutating_profile_data() {
        let root = tempfile::tempdir().expect("create temp data root");
        let legacy_db = root.path().join("clipboard.db");
        let legacy_bytes = b"existing encrypted profile bytes";
        fs::write(&legacy_db, legacy_bytes).expect("seed legacy profile");

        let catalog = SpaceCatalog::load_or_migrate(root.path()).expect("adopt legacy profile");

        assert_eq!(catalog.entries().len(), 1);
        let adopted = &catalog.entries()[0];
        assert_ne!(adopted.profile_id, "default");
        assert_eq!(adopted.profile_dir, ".");
        assert!(adopted.enabled);
        assert!(adopted.active_send);
        assert_eq!(
            fs::read(&legacy_db).expect("read legacy profile"),
            legacy_bytes
        );
        assert!(root.path().join(SPACE_CATALOG_FILE_NAME).is_file());
    }

    #[test]
    fn second_start_is_idempotent() {
        let root = tempfile::tempdir().expect("create temp data root");
        let first = SpaceCatalog::load_or_migrate(root.path()).expect("first startup");
        let persisted_before =
            fs::read(root.path().join(SPACE_CATALOG_FILE_NAME)).expect("read first catalog");

        let second = SpaceCatalog::load_or_migrate(root.path()).expect("second startup");
        let persisted_after =
            fs::read(root.path().join(SPACE_CATALOG_FILE_NAME)).expect("read second catalog");

        assert_eq!(second.entries(), first.entries());
        assert_eq!(persisted_after, persisted_before);
    }

    #[test]
    fn metadata_probe_error_never_triggers_first_start_migration() {
        let root = tempfile::tempdir().expect("create temp data root");

        let error = SpaceCatalog::load_or_migrate_with_probe(root.path(), |_| {
            Err(io_error(
                "inspect catalog metadata",
                io::Error::new(io::ErrorKind::PermissionDenied, "injected metadata failure"),
            ))
        })
        .expect_err("metadata failure must fail closed");

        assert!(matches!(
            error,
            SpaceCatalogError::Io { source, .. }
                if source.kind() == io::ErrorKind::PermissionDenied
        ));
        assert!(!root.path().join(SPACE_CATALOG_FILE_NAME).exists());
    }

    #[test]
    fn dangling_catalog_directory_entry_is_not_treated_as_missing() {
        let path = Path::new("space-catalog.json");

        let error = probe_catalog_path_with(
            path,
            |_| Err(io::Error::new(io::ErrorKind::NotFound, "target missing")),
            |_| Ok(()),
        )
        .expect_err("dangling directory entry must fail closed");

        assert!(matches!(
            error,
            SpaceCatalogError::Io { source, .. }
                if source.kind() == io::ErrorKind::NotFound
        ));
    }

    #[test]
    fn catalog_is_missing_only_when_metadata_and_directory_entry_are_not_found() {
        let path = Path::new("space-catalog.json");

        let state = probe_catalog_path_with(
            path,
            |_| Err(io::Error::new(io::ErrorKind::NotFound, "target missing")),
            |_| Err(io::Error::new(io::ErrorKind::NotFound, "entry missing")),
        )
        .expect("unambiguous missing catalog");

        assert_eq!(state, CatalogPathState::Missing);
    }

    #[test]
    fn add_profile_preserves_the_adopted_profile() {
        let root = tempfile::tempdir().expect("create temp data root");
        let mut catalog = SpaceCatalog::load_or_migrate(root.path()).expect("migrate catalog");
        let adopted = catalog.entries()[0].clone();

        let added = catalog.add_profile().expect("add profile");

        assert_eq!(catalog.entries().len(), 2);
        assert_eq!(catalog.entries()[0], adopted);
        assert_eq!(catalog.entries()[1], added);
        assert_ne!(added.profile_id, adopted.profile_id);
        assert_ne!(added.profile_dir, adopted.profile_dir);
        assert!(added.enabled);
        assert!(!added.active_send);
        assert_eq!(
            SpaceCatalog::load_or_migrate(root.path())
                .expect("reload catalog")
                .entries(),
            catalog.entries()
        );
    }

    #[test]
    fn set_active_send_changes_only_the_target_flags() {
        let root = tempfile::tempdir().expect("create temp data root");
        let mut catalog = SpaceCatalog::load_or_migrate(root.path()).expect("migrate catalog");
        let added = catalog.add_profile().expect("add profile");
        let before = catalog.entries().to_vec();

        catalog
            .set_active_send(&added.profile_id)
            .expect("set active send");

        assert_eq!(catalog.entries().len(), before.len());
        for (current, original) in catalog.entries().iter().zip(before) {
            assert_eq!(current.profile_id, original.profile_id);
            assert_eq!(current.profile_dir, original.profile_dir);
            assert_eq!(current.enabled, original.enabled);
            assert_eq!(current.active_send, current.profile_id == added.profile_id);
        }
    }

    #[test]
    fn remove_profile_removes_only_the_record_and_keeps_profile_data() {
        let root = tempfile::tempdir().expect("create temp data root");
        let mut catalog = SpaceCatalog::load_or_migrate(root.path()).expect("migrate catalog");
        let added = catalog.add_profile().expect("add profile");
        let profile_root = root.path().join(&added.profile_dir);
        fs::create_dir(&profile_root).expect("create profile data directory");
        fs::write(profile_root.join("clipboard.db"), b"retained profile data")
            .expect("seed profile data");

        let removed = catalog
            .remove_profile(&added.profile_id)
            .expect("remove profile record");

        assert_eq!(removed, added);
        assert!(catalog
            .entries()
            .iter()
            .all(|entry| entry.profile_id != added.profile_id));
        assert_eq!(
            fs::read(profile_root.join("clipboard.db")).expect("read retained profile data"),
            b"retained profile data"
        );
    }

    #[test]
    fn unknown_profile_ids_are_typed_errors() {
        let root = tempfile::tempdir().expect("create temp data root");
        let mut catalog = SpaceCatalog::load_or_migrate(root.path()).expect("migrate catalog");

        let set_error = catalog
            .set_active_send("00000000-0000-4000-8000-000000000000")
            .expect_err("unknown active-send target must fail");
        let remove_error = catalog
            .remove_profile("00000000-0000-4000-8000-000000000000")
            .expect_err("unknown removal target must fail");

        assert!(matches!(
            set_error,
            SpaceCatalogError::UnknownProfileId { .. }
        ));
        assert!(matches!(
            remove_error,
            SpaceCatalogError::UnknownProfileId { .. }
        ));
    }

    #[test]
    fn active_send_profile_cannot_be_removed_without_a_replacement_target() {
        let root = tempfile::tempdir().expect("create temp data root");
        let mut catalog = SpaceCatalog::load_or_migrate(root.path()).expect("migrate catalog");
        let active_id = catalog.entries()[0].profile_id.clone();

        let error = catalog
            .remove_profile(&active_id)
            .expect_err("active-send removal must fail closed");

        assert!(matches!(
            error,
            SpaceCatalogError::CannotRemoveActiveSend { profile_id }
                if profile_id == active_id
        ));
        assert_eq!(catalog.entries().len(), 1);
    }

    #[test]
    fn persisted_catalog_contains_only_non_sensitive_allowlisted_fields() {
        let root = tempfile::tempdir().expect("create temp data root");
        SpaceCatalog::load_or_migrate(root.path()).expect("migrate catalog");
        let wire: serde_json::Value = serde_json::from_slice(
            &fs::read(root.path().join(SPACE_CATALOG_FILE_NAME)).expect("read catalog"),
        )
        .expect("parse catalog");

        let top_level = wire.as_object().expect("catalog object");
        assert_eq!(top_level.keys().collect::<Vec<_>>(), vec!["entries"]);
        let entry = wire["entries"][0].as_object().expect("catalog entry");
        let mut keys = entry.keys().map(String::as_str).collect::<Vec<_>>();
        keys.sort_unstable();
        assert_eq!(
            keys,
            vec!["active_send", "enabled", "profile_dir", "profile_id"]
        );
        let profile_id = entry["profile_id"].as_str().expect("profile ID string");
        let parsed = Uuid::parse_str(profile_id).expect("profile ID UUID");
        assert_eq!(parsed.get_version_num(), 4);
    }

    #[test]
    fn corrupt_catalog_fails_closed_without_overwriting_it() {
        let root = tempfile::tempdir().expect("create temp data root");
        let corrupt = b"{ definitely-not-valid-json";
        fs::write(root.path().join(SPACE_CATALOG_FILE_NAME), corrupt)
            .expect("write corrupt catalog");

        let error = SpaceCatalog::load_or_migrate(root.path())
            .expect_err("corrupt catalog must not be reset");

        assert!(matches!(error, SpaceCatalogError::InvalidJson(_)));
        assert_eq!(
            fs::read(root.path().join(SPACE_CATALOG_FILE_NAME)).expect("read corrupt catalog"),
            corrupt
        );
    }

    #[test]
    fn replace_failure_preserves_old_catalog_and_cleans_temporary_file() {
        let root = tempfile::tempdir().expect("create temp data root");
        let catalog = SpaceCatalog::load_or_migrate(root.path()).expect("migrate catalog");
        let catalog_path = root.path().join(SPACE_CATALOG_FILE_NAME);
        let persisted_before = fs::read(&catalog_path).expect("read old catalog");
        let mut candidate = catalog.document.clone();
        candidate.entries.push(SpaceCatalogEntry {
            profile_id: "abcdefab-cdef-4abc-8def-abcdefabcdef".to_string(),
            profile_dir: "profile-abcdefab-cdef-4abc-8def-abcdefabcdef".to_string(),
            enabled: true,
            active_send: false,
        });

        let error = write_document_with_replace(root.path(), &candidate, |_, _| {
            Err(io_error(
                "injected replace",
                io::Error::new(io::ErrorKind::PermissionDenied, "injected replace failure"),
            ))
        })
        .expect_err("replace failure must be reported");

        assert!(matches!(
            error,
            SpaceCatalogError::Io { source, .. }
                if source.kind() == io::ErrorKind::PermissionDenied
        ));
        assert_eq!(
            fs::read(catalog_path).expect("read old catalog after failure"),
            persisted_before
        );
        let temporary_files = fs::read_dir(root.path())
            .expect("read catalog root")
            .filter_map(Result::ok)
            .filter(|entry| {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                name.starts_with(&format!(".{SPACE_CATALOG_FILE_NAME}.")) && name.ends_with(".tmp")
            })
            .count();
        assert_eq!(temporary_files, 0);
    }

    #[test]
    fn unsafe_profile_directory_is_rejected() {
        for unsafe_directory in [
            "../outside",
            "nested/profile",
            r"C:\outside",
            "NUL",
            "trailing.",
            "COM1",
            "lpt9.txt",
            "COM¹",
            "com².log",
            "CoM³",
            "LPT¹",
            "lpt².log",
            "LpT³",
        ] {
            let root = tempfile::tempdir().expect("create temp data root");
            write_catalog(
                root.path(),
                serde_json::json!({
                    "entries": [{
                        "profile_id": "11111111-1111-4111-8111-111111111111",
                        "profile_dir": unsafe_directory,
                        "enabled": true,
                        "active_send": true
                    }]
                }),
            );

            let error = SpaceCatalog::load_or_migrate(root.path())
                .expect_err("unsafe profile directory must fail");

            assert!(matches!(
                error,
                SpaceCatalogError::UnsafeProfileDirectory { profile_dir }
                    if profile_dir == unsafe_directory
            ));
        }
    }

    #[test]
    fn generated_profile_directory_must_exactly_match_canonical_profile_id() {
        let profile_id = "abcdefab-cdef-4abc-8def-abcdefabcdef";
        for invalid_profile_directory in [
            "profile-arbitrary",
            "profile-ABCDEFAB-CDEF-4ABC-8DEF-ABCDEFABCDEF",
            "profile-ａbcdefab-cdef-4abc-8def-abcdefabcdef",
        ] {
            let root = tempfile::tempdir().expect("create temp data root");
            write_catalog(
                root.path(),
                serde_json::json!({
                    "entries": [{
                        "profile_id": profile_id,
                        "profile_dir": invalid_profile_directory,
                        "enabled": true,
                        "active_send": true
                    }]
                }),
            );

            let error = SpaceCatalog::load_or_migrate(root.path())
                .expect_err("generated profile directory alias must fail");

            assert!(matches!(
                error,
                SpaceCatalogError::UnsafeProfileDirectory { profile_dir }
                    if profile_dir == invalid_profile_directory
            ));
        }
    }

    #[test]
    fn generated_profile_directory_matching_canonical_profile_id_is_accepted() {
        let root = tempfile::tempdir().expect("create temp data root");
        write_catalog(
            root.path(),
            serde_json::json!({
                "entries": [{
                    "profile_id": "abcdefab-cdef-4abc-8def-abcdefabcdef",
                    "profile_dir": "profile-abcdefab-cdef-4abc-8def-abcdefabcdef",
                    "enabled": true,
                    "active_send": true
                }]
            }),
        );

        let catalog = SpaceCatalog::load_or_migrate(root.path())
            .expect("matching generated profile directory must load");

        assert_eq!(
            catalog.entries()[0].profile_dir,
            "profile-abcdefab-cdef-4abc-8def-abcdefabcdef"
        );
    }

    #[test]
    fn non_random_profile_id_is_rejected() {
        let root = tempfile::tempdir().expect("create temp data root");
        write_catalog(
            root.path(),
            serde_json::json!({
                "entries": [{
                    "profile_id": "not-a-random-uuid",
                    "profile_dir": "profile-one",
                    "enabled": true,
                    "active_send": true
                }]
            }),
        );

        let error = SpaceCatalog::load_or_migrate(root.path())
            .expect_err("non-random profile ID must fail");

        assert!(matches!(
            error,
            SpaceCatalogError::InvalidProfileId { profile_id }
                if profile_id == "not-a-random-uuid"
        ));
    }

    #[test]
    fn non_canonical_or_non_rfc4122_profile_ids_are_rejected() {
        for invalid_profile_id in [
            "ABCDEFAB-CDEF-4ABC-8DEF-ABCDEFABCDEF",
            "abcdefabcdef4abc8defabcdefabcdef",
            "abcdefab-cdef-4abc-cdef-abcdefabcdef",
        ] {
            let root = tempfile::tempdir().expect("create temp data root");
            write_catalog(
                root.path(),
                serde_json::json!({
                    "entries": [{
                        "profile_id": invalid_profile_id,
                        "profile_dir": "profile-one",
                        "enabled": true,
                        "active_send": true
                    }]
                }),
            );

            let error = SpaceCatalog::load_or_migrate(root.path())
                .expect_err("non-canonical profile ID must fail");

            assert!(matches!(
                error,
                SpaceCatalogError::InvalidProfileId { profile_id }
                    if profile_id == invalid_profile_id
            ));
        }
    }

    #[test]
    fn api_lookup_rejects_non_canonical_profile_id_without_rewriting_catalog() {
        let root = tempfile::tempdir().expect("create temp data root");
        let second_id = "abcdefab-cdef-4abc-8def-abcdefabcdef";
        write_catalog(
            root.path(),
            serde_json::json!({
                "entries": [
                    {
                        "profile_id": "11111111-1111-4111-8111-111111111111",
                        "profile_dir": "profile-11111111-1111-4111-8111-111111111111",
                        "enabled": true,
                        "active_send": true
                    },
                    {
                        "profile_id": second_id,
                        "profile_dir": "profile-abcdefab-cdef-4abc-8def-abcdefabcdef",
                        "enabled": true,
                        "active_send": false
                    }
                ]
            }),
        );
        let catalog_path = root.path().join(SPACE_CATALOG_FILE_NAME);
        let persisted_before = fs::read(&catalog_path).expect("read catalog before lookup");
        let mut catalog = SpaceCatalog::load_or_migrate(root.path()).expect("load catalog");

        let set_error = catalog
            .set_active_send(&second_id.to_ascii_uppercase())
            .expect_err("uppercase lookup must fail");
        let remove_error = catalog
            .remove_profile(&second_id.replace('-', ""))
            .expect_err("simple UUID lookup must fail");

        assert!(matches!(
            set_error,
            SpaceCatalogError::InvalidProfileId { .. }
        ));
        assert!(matches!(
            remove_error,
            SpaceCatalogError::InvalidProfileId { .. }
        ));
        assert_eq!(
            fs::read(catalog_path).expect("read catalog after lookup"),
            persisted_before
        );
    }

    #[test]
    fn catalog_without_exactly_one_active_send_target_is_rejected() {
        let root = tempfile::tempdir().expect("create temp data root");
        write_catalog(
            root.path(),
            serde_json::json!({
                "entries": [{
                    "profile_id": "11111111-1111-4111-8111-111111111111",
                    "profile_dir": "profile-11111111-1111-4111-8111-111111111111",
                    "enabled": true,
                    "active_send": false
                }]
            }),
        );

        let error = SpaceCatalog::load_or_migrate(root.path())
            .expect_err("catalog without active-send target must fail");

        assert!(matches!(
            error,
            SpaceCatalogError::InvalidActiveSendCount { count: 0 }
        ));
    }

    #[test]
    fn duplicate_profile_ids_are_rejected() {
        let root = tempfile::tempdir().expect("create temp data root");
        write_catalog(
            root.path(),
            serde_json::json!({
                "entries": [
                    {
                        "profile_id": "11111111-1111-4111-8111-111111111111",
                        "profile_dir": ".",
                        "enabled": true,
                        "active_send": true
                    },
                    {
                        "profile_id": "11111111-1111-4111-8111-111111111111",
                        "profile_dir": "profile-11111111-1111-4111-8111-111111111111",
                        "enabled": true,
                        "active_send": false
                    }
                ]
            }),
        );

        let error = SpaceCatalog::load_or_migrate(root.path())
            .expect_err("duplicate profile IDs must fail");

        assert!(matches!(
            error,
            SpaceCatalogError::DuplicateProfileId { profile_id }
                if profile_id == "11111111-1111-4111-8111-111111111111"
        ));
    }

    #[test]
    fn duplicate_profile_directories_are_rejected() {
        let root = tempfile::tempdir().expect("create temp data root");
        write_catalog(
            root.path(),
            serde_json::json!({
                "entries": [
                    {
                        "profile_id": "11111111-1111-4111-8111-111111111111",
                        "profile_dir": ".",
                        "enabled": true,
                        "active_send": true
                    },
                    {
                        "profile_id": "22222222-2222-4222-8222-222222222222",
                        "profile_dir": ".",
                        "enabled": true,
                        "active_send": false
                    }
                ]
            }),
        );

        let error = SpaceCatalog::load_or_migrate(root.path())
            .expect_err("duplicate profile directories must fail");

        assert!(matches!(
            error,
            SpaceCatalogError::DuplicateProfileDirectory { profile_dir }
                if profile_dir == "."
        ));
    }

    #[test]
    fn stale_concurrent_writer_is_rejected_without_losing_the_first_write() {
        let root = tempfile::tempdir().expect("create temp data root");
        let mut first = SpaceCatalog::load_or_migrate(root.path()).expect("first catalog handle");
        let mut second = SpaceCatalog::load_or_migrate(root.path()).expect("second catalog handle");
        let barrier = Arc::new(Barrier::new(3));
        let first_barrier = Arc::clone(&barrier);
        let second_barrier = Arc::clone(&barrier);

        let first_writer = thread::spawn(move || {
            first_barrier.wait();
            first.add_profile()
        });
        let second_writer = thread::spawn(move || {
            second_barrier.wait();
            second.add_profile()
        });
        barrier.wait();

        let results = [
            first_writer.join().expect("join first writer"),
            second_writer.join().expect("join second writer"),
        ];
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        assert_eq!(
            results
                .iter()
                .filter(|result| matches!(result, Err(SpaceCatalogError::ConcurrentModification)))
                .count(),
            1
        );
        assert_eq!(
            SpaceCatalog::load_or_migrate(root.path())
                .expect("reload catalog")
                .entries()
                .len(),
            2
        );
    }

    #[test]
    fn mutations_remain_consistent_after_restart() {
        let root = tempfile::tempdir().expect("create temp data root");
        let mut catalog = SpaceCatalog::load_or_migrate(root.path()).expect("migrate catalog");
        let adopted_id = catalog.entries()[0].profile_id.clone();
        let added = catalog.add_profile().expect("add profile");
        catalog
            .set_active_send(&added.profile_id)
            .expect("set active send");
        drop(catalog);

        let mut reloaded = SpaceCatalog::load_or_migrate(root.path()).expect("reload catalog");
        assert_eq!(
            reloaded
                .entries()
                .iter()
                .find(|entry| entry.active_send)
                .map(|entry| entry.profile_id.as_str()),
            Some(added.profile_id.as_str())
        );
        reloaded
            .remove_profile(&adopted_id)
            .expect("remove inactive adopted record");
        drop(reloaded);

        let final_catalog = SpaceCatalog::load_or_migrate(root.path()).expect("final reload");
        assert_eq!(final_catalog.entries().len(), 1);
        let retained = &final_catalog.entries()[0];
        assert_eq!(retained.profile_id, added.profile_id);
        assert_eq!(retained.profile_dir, added.profile_dir);
        assert_eq!(retained.enabled, added.enabled);
        assert!(retained.active_send);
    }
}
