//! Keeps a legacy file store authoritative for the entries it already holds.
//!
//! Before 1.0, the Desktop degraded to `FileSecureStorage` whenever the system
//! secret service failed its probe (no Secret Service on the session bus,
//! KWallet byte mangling, snap confinement). Such profiles keep their key
//! encryption key only under `<app_data_root>/keyring`. Selecting the system
//! store for them hands the Engine an unreachable or empty store, so startup
//! fails or asks for a passphrase although the key is intact on disk.
//!
//! The selection is made once per process, on first access. A recorded source
//! (`<app_data_root>/secure-storage-source.json`) always wins: if its store is
//! unavailable the access fails instead of switching stores. Without a record,
//! the choice follows what each store holds:
//!
//! - no legacy file entries: the system store, unchanged from the default
//!   behavior; an unavailable system store stays an error, no file store is
//!   created and no record is ever written;
//! - legacy file entries that the system store also holds, every one of them:
//!   the system store, which a working installation has been using;
//! - legacy file entries absent from, or unreachable in, the system store: the
//!   file store, which is the only place those entries exist;
//! - legacy file entries only some of which the system store holds: no store,
//!   because either one alone would hide entries that only the other holds.
//!
//! Such a choice is provisional. A choice made only because the system store
//! was unavailable is never recorded: it holds for that run, and a later run
//! that can consult both stores decides. Any other choice becomes the recorded
//! source only on evidence: the Engine unlocked the profile with the selected store's key
//! (`SecureStorageSource::confirm_unlocked_by_stored_key`), or the first write
//! or delete is about to change the selected store. The record is written
//! before that change, so the stores never diverge unrecorded; it is replaced
//! atomically, so a crash leaves no record (the next run decides again, nothing
//! has changed yet) or a complete one. Reads alone record nothing.
//!
//! Selection is per store, never per entry, so every read and write stays in
//! one store. Values are not compared and nothing is copied, migrated,
//! overwritten or deleted between the stores. An unreadable record or any
//! other undecidable state fails every operation.

use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use tracing::{info, warn};

use crate::{
    file_secure_storage::FileSecureStorage,
    ports::{SecureStorageError, SecureStorageProvider},
};

pub(super) fn system_storage_preserving_legacy_file_entries(
    system: Arc<dyn SecureStorageProvider>,
    legacy: FileSecureStorage,
    source_record: PathBuf,
) -> (Arc<dyn SecureStorageProvider>, SecureStorageSource) {
    let storage = Arc::new(LegacyFileAwareStorage {
        system,
        legacy,
        source_record,
        selection: OnceLock::new(),
        recorded: Mutex::new(false),
    });
    let source = SecureStorageSource {
        storage: Some(Arc::clone(&storage)),
    };
    (storage, source)
}

/// Lets the host report that the Engine opened the profile with the key held
/// by the selected store.
#[derive(Clone)]
pub struct SecureStorageSource {
    storage: Option<Arc<LegacyFileAwareStorage>>,
}

impl SecureStorageSource {
    pub(crate) fn untracked() -> Self {
        Self { storage: None }
    }

    /// Records the selected store as this profile's key store.
    ///
    /// Call only after the Engine unlocked the profile with the stored key:
    /// that unlock is the evidence that the selected store holds the valid
    /// key. Profiles without legacy file entries never get a record.
    pub fn confirm_unlocked_by_stored_key(&self) {
        let Some(storage) = &self.storage else {
            return;
        };
        if let Err(failure) = storage.ensure_recorded() {
            warn!(
                error = %failure.message,
                "secure storage source could not be recorded; the selection stays provisional"
            );
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SelectedStore {
    System,
    LegacyFile,
}

#[derive(Clone, Copy, Debug)]
enum FailureKind {
    Unavailable,
    PermissionDenied,
    Corrupt,
    Other,
}

#[derive(Clone, Debug)]
struct SelectionFailure {
    kind: FailureKind,
    message: String,
}

impl SelectionFailure {
    fn from_storage_error(error: &SecureStorageError) -> Self {
        let (kind, message) = match error {
            SecureStorageError::Unavailable(message) => (FailureKind::Unavailable, message),
            SecureStorageError::PermissionDenied(message) => {
                (FailureKind::PermissionDenied, message)
            }
            SecureStorageError::Corrupt(message) => (FailureKind::Corrupt, message),
            SecureStorageError::Other(message) => (FailureKind::Other, message),
        };
        Self {
            kind,
            message: message.clone(),
        }
    }

    fn to_storage_error(&self) -> SecureStorageError {
        let message = self.message.clone();
        match self.kind {
            FailureKind::Unavailable => SecureStorageError::Unavailable(message),
            FailureKind::PermissionDenied => SecureStorageError::PermissionDenied(message),
            FailureKind::Corrupt => SecureStorageError::Corrupt(message),
            FailureKind::Other => SecureStorageError::Other(message),
        }
    }
}

/// The decided store and whether this profile's source must be recorded.
#[derive(Clone, Copy, Debug)]
struct Selection {
    store: SelectedStore,
    /// Legacy file entries exist, so the choice must not change across runs.
    tracked: bool,
    /// Both stores were consulted. A choice made only because the system
    /// store could not answer is never recorded, so an outage cannot pin a
    /// profile to the file store.
    recordable: bool,
}

const SYSTEM_RECORD: &str = r#"{"version":1,"store":"system"}"#;
const LEGACY_FILE_RECORD: &str = r#"{"version":1,"store":"legacy_file"}"#;

struct LegacyFileAwareStorage {
    system: Arc<dyn SecureStorageProvider>,
    legacy: FileSecureStorage,
    source_record: PathBuf,
    selection: OnceLock<Result<Selection, SelectionFailure>>,
    recorded: Mutex<bool>,
}

impl LegacyFileAwareStorage {
    fn selection(&self) -> Result<Selection, SecureStorageError> {
        self.selection
            .get_or_init(|| self.decide())
            .as_ref()
            .copied()
            .map_err(SelectionFailure::to_storage_error)
    }

    fn selected(&self) -> Result<&dyn SecureStorageProvider, SecureStorageError> {
        Ok(match self.selection()?.store {
            SelectedStore::System => self.system.as_ref(),
            SelectedStore::LegacyFile => &self.legacy,
        })
    }

    /// Mutations go only to a recorded store, so the two stores can never
    /// diverge without the record naming the one that changed.
    fn selected_for_write(&self) -> Result<&dyn SecureStorageProvider, SecureStorageError> {
        let store = self.selected()?;
        self.ensure_recorded()
            .map_err(|failure| failure.to_storage_error())?;
        Ok(store)
    }

    fn decide(&self) -> Result<Selection, SelectionFailure> {
        match self.read_record()? {
            Some(store) => {
                *self.recorded.lock().unwrap_or_else(|p| p.into_inner()) = true;
                info!(store = ?store, "using the recorded secure storage source");
                Ok(Selection {
                    store,
                    tracked: true,
                    recordable: true,
                })
            }
            None => self.select(),
        }
    }

    fn read_record(&self) -> Result<Option<SelectedStore>, SelectionFailure> {
        let content = match std::fs::read_to_string(&self.source_record) {
            Ok(content) => content,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                warn!(error = %error, "secure storage source record cannot be read");
                return Err(SelectionFailure {
                    kind: FailureKind::Corrupt,
                    message: format!("secure storage source record cannot be read: {error}"),
                });
            }
        };
        match content.trim_end() {
            SYSTEM_RECORD => Ok(Some(SelectedStore::System)),
            LEGACY_FILE_RECORD => Ok(Some(SelectedStore::LegacyFile)),
            _ => {
                warn!("secure storage source record is not recognized; refusing to choose a secure store");
                Err(SelectionFailure {
                    kind: FailureKind::Corrupt,
                    message: "secure storage source record is not recognized".to_owned(),
                })
            }
        }
    }

    fn ensure_recorded(&self) -> Result<(), SelectionFailure> {
        let selection = self.selection.get_or_init(|| self.decide()).clone()?;
        if !selection.tracked || !selection.recordable {
            return Ok(());
        }
        let mut recorded = self.recorded.lock().unwrap_or_else(|p| p.into_inner());
        if *recorded {
            return Ok(());
        }
        self.write_record(selection.store).map_err(|error| {
            warn!(error = %error, "secure storage source record cannot be written");
            SelectionFailure {
                kind: FailureKind::Other,
                message: format!("secure storage source record cannot be written: {error}"),
            }
        })?;
        *recorded = true;
        info!(store = ?selection.store, "recorded the secure storage source");
        Ok(())
    }

    /// Atomic replace: a crash leaves either no record or a complete one.
    fn write_record(&self, store: SelectedStore) -> std::io::Result<()> {
        let directory = self.source_record.parent().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::NotFound, "record has no directory")
        })?;
        let content = match store {
            SelectedStore::System => SYSTEM_RECORD,
            SelectedStore::LegacyFile => LEGACY_FILE_RECORD,
        };
        let mut temporary = tempfile::Builder::new()
            .prefix(".secure-storage-source.")
            .suffix(".tmp")
            .tempfile_in(directory)?;
        temporary.write_all(content.as_bytes())?;
        temporary.as_file().sync_all()?;
        temporary
            .persist(&self.source_record)
            .map_err(|error| error.error)?;
        #[cfg(unix)]
        std::fs::File::open(directory)?.sync_all()?;
        Ok(())
    }

    fn select(&self) -> Result<Selection, SelectionFailure> {
        let names = self.legacy.entry_names().map_err(|error| {
            let kind = if error.kind() == std::io::ErrorKind::PermissionDenied {
                FailureKind::PermissionDenied
            } else {
                FailureKind::Other
            };
            warn!(
                error = %error,
                "legacy file key store cannot be listed; refusing to choose a secure store"
            );
            SelectionFailure {
                kind,
                message: format!("legacy file key store cannot be listed: {error}"),
            }
        })?;
        if names.is_empty() {
            return Ok(Selection {
                store: SelectedStore::System,
                tracked: false,
                recordable: false,
            });
        }
        let mut held_by_system = 0;
        for name in &names {
            match self.system.get(name) {
                Ok(Some(_)) => held_by_system += 1,
                Ok(None) => {}
                Err(SecureStorageError::Unavailable(error)) if held_by_system == 0 => {
                    info!(
                        legacy_entry_count = names.len(),
                        system_error = %error,
                        "system secure store is unavailable; using the legacy file key store without recording it"
                    );
                    return Ok(Selection {
                        store: SelectedStore::LegacyFile,
                        tracked: true,
                        recordable: false,
                    });
                }
                Err(error) => {
                    warn!(
                        error = %error,
                        "system secure store lookup failed; refusing to choose a secure store"
                    );
                    return Err(SelectionFailure::from_storage_error(&error));
                }
            }
        }
        if held_by_system == names.len() {
            info!(
                legacy_entry_count = names.len(),
                "system secure store holds every legacy file entry; using the system store"
            );
            return Ok(Selection {
                store: SelectedStore::System,
                tracked: true,
                recordable: true,
            });
        }
        if held_by_system > 0 {
            // Either store alone would hide entries that only the other holds.
            warn!(
                legacy_entry_count = names.len(),
                system_entry_count = held_by_system,
                "legacy file key store and system secure store hold different entries; refusing to choose a secure store"
            );
            return Err(SelectionFailure {
                kind: FailureKind::Other,
                message: "legacy file key store and system secure store hold different entries"
                    .to_owned(),
            });
        }
        info!(
            legacy_entry_count = names.len(),
            "system secure store holds none of the legacy file entries; using the legacy file key store"
        );
        Ok(Selection {
            store: SelectedStore::LegacyFile,
            tracked: true,
            recordable: true,
        })
    }
}

impl SecureStorageProvider for LegacyFileAwareStorage {
    fn get(&self, key: &str) -> Result<Option<Vec<u8>>, SecureStorageError> {
        self.selected()?.get(key)
    }

    fn set(&self, key: &str, value: &[u8]) -> Result<(), SecureStorageError> {
        self.selected_for_write()?.set(key, value)
    }

    fn delete(&self, key: &str) -> Result<(), SecureStorageError> {
        self.selected_for_write()?.delete(key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::Path;
    use std::sync::Mutex;

    const PROFILE_KEY: &str = "kek:v1:profile:default";
    const SOURCE_RECORD: &str = "secure-storage-source.json";

    #[derive(Default)]
    struct RecordingStorage {
        map: Mutex<HashMap<String, Vec<u8>>>,
        writes: Mutex<Vec<String>>,
    }

    impl RecordingStorage {
        fn holding(key: &str, value: &[u8]) -> Self {
            let storage = Self::default();
            storage
                .map
                .lock()
                .unwrap()
                .insert(key.to_owned(), value.to_vec());
            storage
        }

        fn writes(&self) -> Vec<String> {
            self.writes.lock().unwrap().clone()
        }
    }

    impl SecureStorageProvider for RecordingStorage {
        fn get(&self, key: &str) -> Result<Option<Vec<u8>>, SecureStorageError> {
            Ok(self.map.lock().unwrap().get(key).cloned())
        }
        fn set(&self, key: &str, value: &[u8]) -> Result<(), SecureStorageError> {
            self.writes.lock().unwrap().push(format!("set:{key}"));
            self.map
                .lock()
                .unwrap()
                .insert(key.to_owned(), value.to_vec());
            Ok(())
        }
        fn delete(&self, key: &str) -> Result<(), SecureStorageError> {
            self.writes.lock().unwrap().push(format!("delete:{key}"));
            self.map.lock().unwrap().remove(key);
            Ok(())
        }
    }

    /// Models a session bus without a Secret Service (`The name is not activatable`).
    struct UnavailableStorage;

    impl SecureStorageProvider for UnavailableStorage {
        fn get(&self, _: &str) -> Result<Option<Vec<u8>>, SecureStorageError> {
            Err(SecureStorageError::Unavailable("no secret service".into()))
        }
        fn set(&self, _: &str, _: &[u8]) -> Result<(), SecureStorageError> {
            Err(SecureStorageError::Unavailable("no secret service".into()))
        }
        fn delete(&self, _: &str) -> Result<(), SecureStorageError> {
            Err(SecureStorageError::Unavailable("no secret service".into()))
        }
    }

    struct DeniedStorage;

    impl SecureStorageProvider for DeniedStorage {
        fn get(&self, _: &str) -> Result<Option<Vec<u8>>, SecureStorageError> {
            Err(SecureStorageError::PermissionDenied(
                "collection locked".into(),
            ))
        }
        fn set(&self, _: &str, _: &[u8]) -> Result<(), SecureStorageError> {
            panic!("an undecided selection must not write the system store");
        }
        fn delete(&self, _: &str) -> Result<(), SecureStorageError> {
            panic!("an undecided selection must not delete from the system store");
        }
    }

    fn open_storage(
        system: Arc<dyn SecureStorageProvider>,
        legacy: FileSecureStorage,
    ) -> Arc<dyn SecureStorageProvider> {
        let record = legacy_record_path(&legacy);
        system_storage_preserving_legacy_file_entries(system, legacy, record).0
    }

    fn legacy_record_path(legacy: &FileSecureStorage) -> std::path::PathBuf {
        legacy.base_dir().with_file_name(SOURCE_RECORD)
    }

    fn legacy_store(root: &Path) -> (FileSecureStorage, std::path::PathBuf) {
        let directory = root.join("keyring");
        (
            FileSecureStorage::with_base_dir(directory.clone()),
            directory,
        )
    }

    fn snapshot(directory: &Path) -> Vec<(std::ffi::OsString, Vec<u8>)> {
        let mut files = std::fs::read_dir(directory)
            .unwrap()
            .map(|entry| {
                let entry = entry.unwrap();
                (entry.file_name(), std::fs::read(entry.path()).unwrap())
            })
            .collect::<Vec<_>>();
        files.sort();
        files
    }

    #[test]
    fn legacy_file_entries_are_used_when_the_system_store_is_unavailable() {
        let temporary = tempfile::tempdir().unwrap();
        let (legacy, directory) = legacy_store(temporary.path());
        legacy.set(PROFILE_KEY, b"legacy-kek").unwrap();
        let before = snapshot(&directory);

        let storage = open_storage(Arc::new(UnavailableStorage), legacy.clone());

        assert_eq!(
            storage.get(PROFILE_KEY).unwrap().as_deref(),
            Some(&b"legacy-kek"[..])
        );
        assert_eq!(snapshot(&directory), before);
    }

    #[test]
    fn legacy_file_entries_stay_authoritative_after_the_system_store_becomes_available() {
        let temporary = tempfile::tempdir().unwrap();
        let (legacy, directory) = legacy_store(temporary.path());
        legacy.set(PROFILE_KEY, b"legacy-kek").unwrap();
        let system = Arc::new(RecordingStorage::default());

        let storage = open_storage(system.clone(), legacy.clone());

        assert_eq!(
            storage.get(PROFILE_KEY).unwrap().as_deref(),
            Some(&b"legacy-kek"[..])
        );
        storage.set("profile-extra", b"extra").unwrap();
        assert_eq!(
            legacy.get("profile-extra").unwrap().as_deref(),
            Some(&b"extra"[..])
        );
        assert!(system.writes().is_empty());
        assert!(system.map.lock().unwrap().is_empty());
        assert_eq!(snapshot(&directory).len(), 2);
    }

    #[test]
    fn a_system_store_holding_the_same_entries_keeps_authority() {
        let temporary = tempfile::tempdir().unwrap();
        let (legacy, directory) = legacy_store(temporary.path());
        legacy.set(PROFILE_KEY, b"stale-file-kek").unwrap();
        let before = snapshot(&directory);
        let system = Arc::new(RecordingStorage::holding(PROFILE_KEY, b"system-kek"));

        let storage = open_storage(system.clone(), legacy);

        assert_eq!(
            storage.get(PROFILE_KEY).unwrap().as_deref(),
            Some(&b"system-kek"[..])
        );
        storage.set(PROFILE_KEY, b"rotated").unwrap();
        assert_eq!(system.writes(), vec![format!("set:{PROFILE_KEY}")]);
        assert_eq!(snapshot(&directory), before);
    }

    #[test]
    fn entries_split_between_both_stores_fail_closed_without_writes() {
        const IDENTITY_KEY: &str = "iroh-identity:v1";
        let temporary = tempfile::tempdir().unwrap();
        let (legacy, directory) = legacy_store(temporary.path());
        legacy.set(PROFILE_KEY, b"file-kek").unwrap();
        legacy.set(IDENTITY_KEY, b"file-identity").unwrap();
        let before = snapshot(&directory);
        let system = Arc::new(RecordingStorage::holding(PROFILE_KEY, b"system-kek"));

        let storage = open_storage(system.clone(), legacy);

        assert!(matches!(
            storage.get(PROFILE_KEY),
            Err(SecureStorageError::Other(_))
        ));
        assert!(storage.set(PROFILE_KEY, b"replacement").is_err());
        assert!(storage.delete(IDENTITY_KEY).is_err());
        assert!(system.writes().is_empty());
        assert_eq!(
            system
                .map
                .lock()
                .unwrap()
                .get(PROFILE_KEY)
                .map(Vec::as_slice),
            Some(&b"system-kek"[..])
        );
        assert_eq!(snapshot(&directory), before);
    }

    #[test]
    fn a_system_store_holding_every_legacy_entry_keeps_authority() {
        const IDENTITY_KEY: &str = "iroh-identity:v1";
        let temporary = tempfile::tempdir().unwrap();
        let (legacy, directory) = legacy_store(temporary.path());
        legacy.set(PROFILE_KEY, b"file-kek").unwrap();
        let before = snapshot(&directory);
        let system = Arc::new(RecordingStorage::holding(PROFILE_KEY, b"system-kek"));
        system.set(IDENTITY_KEY, b"system-identity").unwrap();
        system.writes.lock().unwrap().clear();

        let storage = open_storage(system.clone(), legacy);

        assert_eq!(
            storage.get(IDENTITY_KEY).unwrap().as_deref(),
            Some(&b"system-identity"[..])
        );
        assert!(system.writes().is_empty());
        assert_eq!(snapshot(&directory), before);
    }

    #[test]
    fn without_legacy_entries_an_unavailable_system_store_fails_closed() {
        let temporary = tempfile::tempdir().unwrap();
        let (legacy, directory) = legacy_store(temporary.path());

        let storage = open_storage(Arc::new(UnavailableStorage), legacy);

        assert!(matches!(
            storage.get(PROFILE_KEY),
            Err(SecureStorageError::Unavailable(_))
        ));
        assert!(matches!(
            storage.set(PROFILE_KEY, b"new"),
            Err(SecureStorageError::Unavailable(_))
        ));
        assert!(!directory.exists(), "no empty file store may be created");
    }

    #[test]
    fn interrupted_writes_are_not_legacy_entries() {
        let temporary = tempfile::tempdir().unwrap();
        let (legacy, directory) = legacy_store(temporary.path());
        std::fs::create_dir(&directory).unwrap();
        std::fs::write(
            directory.join(".secure-storage-interrupted.tmp"),
            b"partial",
        )
        .unwrap();

        let storage = open_storage(Arc::new(UnavailableStorage), legacy);

        assert!(matches!(
            storage.get(PROFILE_KEY),
            Err(SecureStorageError::Unavailable(_))
        ));
    }

    #[test]
    fn an_undecidable_system_lookup_fails_closed_without_writes() {
        let temporary = tempfile::tempdir().unwrap();
        let (legacy, directory) = legacy_store(temporary.path());
        legacy.set(PROFILE_KEY, b"legacy-kek").unwrap();
        let before = snapshot(&directory);

        let storage = open_storage(Arc::new(DeniedStorage), legacy);

        assert!(matches!(
            storage.get(PROFILE_KEY),
            Err(SecureStorageError::PermissionDenied(_))
        ));
        assert!(storage.set(PROFILE_KEY, b"replacement").is_err());
        assert!(storage.delete(PROFILE_KEY).is_err());
        assert_eq!(snapshot(&directory), before);
    }

    #[cfg(unix)]
    #[test]
    fn an_unreadable_legacy_directory_fails_closed_even_with_a_system_store() {
        use std::os::unix::fs::PermissionsExt;

        let temporary = tempfile::tempdir().unwrap();
        let (legacy, directory) = legacy_store(temporary.path());
        legacy.set(PROFILE_KEY, b"legacy-kek").unwrap();
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o000)).unwrap();
        if std::fs::read_dir(&directory).is_ok() {
            // Privileged test runners bypass directory permissions.
            std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700)).unwrap();
            return;
        }
        let system = Arc::new(RecordingStorage::default());

        let with_system = open_storage(system.clone(), legacy.clone());
        let without_system = open_storage(Arc::new(UnavailableStorage), legacy);
        let with_system = with_system.get(PROFILE_KEY);
        let without_system = without_system.get(PROFILE_KEY);

        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert!(matches!(
            with_system,
            Err(SecureStorageError::PermissionDenied(_))
        ));
        assert!(matches!(
            without_system,
            Err(SecureStorageError::PermissionDenied(_))
        ));
        assert!(system.writes().is_empty());
    }

    // --- Stable source across restarts -------------------------------------

    fn open_tracked(
        system: Arc<dyn SecureStorageProvider>,
        root: &Path,
    ) -> (Arc<dyn SecureStorageProvider>, SecureStorageSource) {
        let (legacy, _) = legacy_store(root);
        system_storage_preserving_legacy_file_entries(system, legacy, root.join(SOURCE_RECORD))
    }

    fn record(root: &Path) -> Option<String> {
        std::fs::read_to_string(root.join(SOURCE_RECORD)).ok()
    }

    #[test]
    fn a_confirmed_system_source_is_not_replaced_when_the_service_disappears() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path();
        let (legacy, directory) = legacy_store(root);
        legacy.set(PROFILE_KEY, b"stale-file-kek").unwrap();
        let before = snapshot(&directory);
        let system = Arc::new(RecordingStorage::holding(PROFILE_KEY, b"system-kek"));

        let (first, source) = open_tracked(system.clone(), root);
        assert_eq!(
            first.get(PROFILE_KEY).unwrap().as_deref(),
            Some(&b"system-kek"[..])
        );
        source.confirm_unlocked_by_stored_key();

        let (restarted, _) = open_tracked(Arc::new(UnavailableStorage), root);
        assert!(matches!(
            restarted.get(PROFILE_KEY),
            Err(SecureStorageError::Unavailable(_))
        ));
        assert_eq!(snapshot(&directory), before);
    }

    #[test]
    fn a_confirmed_file_source_is_kept_when_the_service_holds_different_copies() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path();
        let (legacy, _) = legacy_store(root);
        legacy.set(PROFILE_KEY, b"file-kek").unwrap();

        let (first, source) = open_tracked(Arc::new(RecordingStorage::default()), root);
        assert_eq!(
            first.get(PROFILE_KEY).unwrap().as_deref(),
            Some(&b"file-kek"[..])
        );
        source.confirm_unlocked_by_stored_key();

        let system = Arc::new(RecordingStorage::holding(PROFILE_KEY, b"other-kek"));
        let (restarted, _) = open_tracked(system.clone(), root);
        assert_eq!(
            restarted.get(PROFILE_KEY).unwrap().as_deref(),
            Some(&b"file-kek"[..])
        );
        assert!(system.writes().is_empty());
    }

    #[test]
    fn the_first_write_records_the_provisional_store_before_changing_it() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path();
        let (legacy, _) = legacy_store(root);
        legacy.set(PROFILE_KEY, b"file-kek").unwrap();

        let (first, _) = open_tracked(Arc::new(RecordingStorage::default()), root);
        first.set(PROFILE_KEY, b"authenticated-kek").unwrap();
        assert!(
            record(root).is_some(),
            "a write must be preceded by a record"
        );

        let system = Arc::new(RecordingStorage::holding(PROFILE_KEY, b"stale-system-kek"));
        let (restarted, _) = open_tracked(system.clone(), root);
        assert_eq!(
            restarted.get(PROFILE_KEY).unwrap().as_deref(),
            Some(&b"authenticated-kek"[..])
        );
        assert!(system.writes().is_empty());
    }

    #[test]
    fn an_outage_never_records_the_file_store() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path();
        let (legacy, _) = legacy_store(root);
        legacy.set(PROFILE_KEY, b"stale-file-kek").unwrap();

        // Passphrase recovery during an outage rewrites the provisional store.
        let (first, source) = open_tracked(Arc::new(UnavailableStorage), root);
        first.set(PROFILE_KEY, b"authenticated-kek").unwrap();
        source.confirm_unlocked_by_stored_key();
        assert!(
            record(root).is_none(),
            "a choice made only because the system store was unavailable must stay provisional"
        );

        let system = Arc::new(RecordingStorage::holding(PROFILE_KEY, b"system-kek"));
        let (restarted, _) = open_tracked(system, root);
        assert_eq!(
            restarted.get(PROFILE_KEY).unwrap().as_deref(),
            Some(&b"system-kek"[..])
        );
    }

    #[test]
    fn reads_without_confirmation_record_nothing() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path();
        let (legacy, _) = legacy_store(root);
        legacy.set(PROFILE_KEY, b"stale-file-kek").unwrap();

        let (first, _) = open_tracked(Arc::new(UnavailableStorage), root);
        first.get(PROFILE_KEY).unwrap();
        assert!(record(root).is_none());

        let system = Arc::new(RecordingStorage::holding(PROFILE_KEY, b"system-kek"));
        let (restarted, _) = open_tracked(system, root);
        assert_eq!(
            restarted.get(PROFILE_KEY).unwrap().as_deref(),
            Some(&b"system-kek"[..])
        );
    }

    #[test]
    fn an_unreadable_source_record_fails_closed_without_writes() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path();
        let (legacy, directory) = legacy_store(root);
        legacy.set(PROFILE_KEY, b"file-kek").unwrap();
        let before = snapshot(&directory);
        std::fs::write(root.join(SOURCE_RECORD), b"{not json").unwrap();
        let system = Arc::new(RecordingStorage::default());

        let (storage, _) = open_tracked(system.clone(), root);

        assert!(matches!(
            storage.get(PROFILE_KEY),
            Err(SecureStorageError::Corrupt(_))
        ));
        assert!(storage.set(PROFILE_KEY, b"replacement").is_err());
        assert!(system.writes().is_empty());
        assert_eq!(snapshot(&directory), before);
    }

    #[test]
    fn a_recorded_file_source_is_honored_even_without_entries() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path();
        std::fs::write(
            root.join(SOURCE_RECORD),
            br#"{"version":1,"store":"legacy_file"}"#,
        )
        .unwrap();
        let system = Arc::new(RecordingStorage::holding(PROFILE_KEY, b"system-kek"));

        let (storage, _) = open_tracked(system.clone(), root);

        assert_eq!(storage.get(PROFILE_KEY).unwrap(), None);
        assert!(system.writes().is_empty());
    }

    #[test]
    fn profiles_without_legacy_entries_never_get_a_source_record() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path();
        let system = Arc::new(RecordingStorage::default());

        let (storage, source) = open_tracked(system.clone(), root);
        storage.set(PROFILE_KEY, b"system-kek").unwrap();
        source.confirm_unlocked_by_stored_key();

        assert!(record(root).is_none());
        assert!(!root.join("keyring").exists());
        assert_eq!(system.writes(), vec![format!("set:{PROFILE_KEY}")]);
    }

    #[cfg(unix)]
    #[test]
    fn a_failed_record_write_blocks_the_mutation() {
        use std::os::unix::fs::PermissionsExt;

        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("userdata");
        std::fs::create_dir(&root).unwrap();
        let (legacy, directory) = legacy_store(&root);
        legacy.set(PROFILE_KEY, b"file-kek").unwrap();
        let before = snapshot(&directory);
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o500)).unwrap();
        if std::fs::write(root.join(".probe"), b"").is_ok() {
            std::fs::remove_file(root.join(".probe")).unwrap();
            std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
            return;
        }

        let (storage, _) = open_tracked(Arc::new(RecordingStorage::default()), &root);
        let result = storage.set(PROFILE_KEY, b"replacement");

        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert!(result.is_err(), "an unrecorded store must not change");
        assert_eq!(snapshot(&directory), before);
        assert!(record(&root).is_none());
    }

    #[test]
    fn an_interrupted_record_write_leaves_no_record() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path();
        let (legacy, _) = legacy_store(root);
        legacy.set(PROFILE_KEY, b"stale-file-kek").unwrap();
        std::fs::write(root.join(".secure-storage-source.tmp"), b"{\"vers").unwrap();
        let system = Arc::new(RecordingStorage::holding(PROFILE_KEY, b"system-kek"));

        let (storage, _) = open_tracked(system, root);

        assert_eq!(
            storage.get(PROFILE_KEY).unwrap().as_deref(),
            Some(&b"system-kek"[..])
        );
    }
}
