use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;

use crate::ports::{SecureStorageError, SecureStorageProvider};

/// File-based secure storage for development or headless environments.
///
/// 基于文件的安全存储（开发/无桌面环境回退）。
#[derive(Clone)]
pub struct FileSecureStorage {
    base_dir: PathBuf,
}

impl FileSecureStorage {
    /// Create file secure storage rooted at `<app_data_root>/keyring`.
    ///
    /// 在 `<app_data_root>/keyring` 下创建文件安全存储。
    pub fn new_in_app_data_root(app_data_root: PathBuf) -> Result<Self, io::Error> {
        let base_dir = app_data_root.join("keyring");
        fs::create_dir_all(&base_dir)?;
        Ok(Self { base_dir })
    }

    /// Construct with a concrete base directory.
    ///
    /// 使用指定目录创建实例。
    pub fn with_base_dir(base_dir: PathBuf) -> Self {
        Self { base_dir }
    }

    /// Directory holding this store's entry files.
    pub fn base_dir(&self) -> &std::path::Path {
        &self.base_dir
    }

    /// Names of the entries held by this store, without reading any value.
    ///
    /// A missing directory holds no entries. Interrupted writes and files that
    /// this store did not name are not entries.
    pub fn entry_names(&self) -> Result<Vec<String>, io::Error> {
        let entries = match fs::read_dir(&self.base_dir) {
            Ok(entries) => entries,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(err) => return Err(err),
        };
        let mut names = Vec::new();
        for entry in entries {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                continue;
            }
            if let Some(name) = entry
                .file_name()
                .to_str()
                .and_then(|file_name| file_name.strip_suffix(".bin"))
                .and_then(decode_key)
            {
                names.push(name);
            }
        }
        names.sort();
        Ok(names)
    }

    fn file_path(&self, key: &str) -> PathBuf {
        self.base_dir.join(format!("{}.bin", encode_key(key)))
    }

    fn map_io_error(context: &str, err: io::Error) -> SecureStorageError {
        SecureStorageError::Other(format!("{context}: {err}"))
    }
}

/// Entry file stem: the key's bytes as lowercase hex.
fn encode_key(key: &str) -> String {
    key.as_bytes()
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect()
}

/// Inverse of `encode_key`; rejects stems this store would not have written.
fn decode_key(encoded: &str) -> Option<String> {
    if encoded.is_empty() || !encoded.len().is_multiple_of(2) {
        return None;
    }
    let bytes = (0..encoded.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(encoded.get(index..index + 2)?, 16).ok())
        .collect::<Option<Vec<u8>>>()?;
    let key = String::from_utf8(bytes).ok()?;
    (encode_key(&key) == encoded).then_some(key)
}

impl SecureStorageProvider for FileSecureStorage {
    fn get(&self, key: &str) -> Result<Option<Vec<u8>>, SecureStorageError> {
        let path = self.file_path(key);
        match fs::read(&path) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(err) => Err(Self::map_io_error(
                "failed to read secure storage file",
                err,
            )),
        }
    }

    fn set(&self, key: &str, value: &[u8]) -> Result<(), SecureStorageError> {
        let mut directory = fs::DirBuilder::new();
        directory.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            directory.mode(0o700);
        }
        directory
            .create(&self.base_dir)
            .map_err(|err| Self::map_io_error("failed to create secure storage directory", err))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&self.base_dir, fs::Permissions::from_mode(0o700)).map_err(
                |err| Self::map_io_error("failed to set secure storage directory permissions", err),
            )?;
        }
        let path = self.file_path(key);
        let mut temporary_builder = tempfile::Builder::new();
        temporary_builder.prefix(".secure-storage-").suffix(".tmp");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            temporary_builder.permissions(fs::Permissions::from_mode(0o600));
        }
        let mut temporary = temporary_builder
            .tempfile_in(&self.base_dir)
            .map_err(|err| Self::map_io_error("failed to create secure storage temp file", err))?;
        temporary
            .write_all(value)
            .map_err(|err| Self::map_io_error("failed to write secure storage temp file", err))?;
        temporary
            .persist(&path)
            .map_err(|err| Self::map_io_error("failed to rename secure storage file", err.error))?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&path)
                .map_err(|err| Self::map_io_error("failed to read secure storage metadata", err))?
                .permissions();
            perms.set_mode(0o600);
            fs::set_permissions(&path, perms).map_err(|err| {
                Self::map_io_error("failed to set secure storage permissions", err)
            })?;
        }

        Ok(())
    }

    fn delete(&self, key: &str) -> Result<(), SecureStorageError> {
        let path = self.file_path(key);
        match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(Self::map_io_error(
                "failed to delete secure storage file",
                err,
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn set_repairs_directory_permissions_and_keeps_only_the_private_secret_file() {
        use std::os::unix::fs::PermissionsExt;

        let temporary = tempfile::tempdir().unwrap();
        let base_dir = temporary.path().join("keyring");
        fs::create_dir(&base_dir).unwrap();
        fs::set_permissions(&base_dir, fs::Permissions::from_mode(0o755)).unwrap();
        let storage = FileSecureStorage::with_base_dir(base_dir.clone());

        storage.set("profile-key", b"secret").unwrap();
        storage.set("profile-key", b"replacement").unwrap();

        assert_eq!(
            fs::metadata(&base_dir).unwrap().permissions().mode() & 0o777,
            0o700
        );
        let secret_path = storage.file_path("profile-key");
        assert_eq!(
            fs::metadata(&secret_path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(fs::read(secret_path).unwrap(), b"replacement");
        assert_eq!(fs::read_dir(base_dir).unwrap().count(), 1);
    }

    #[test]
    fn entry_names_list_stored_keys_without_reading_values() {
        let temporary = tempfile::tempdir().unwrap();
        let base_dir = temporary.path().join("keyring");
        let storage = FileSecureStorage::with_base_dir(base_dir.clone());
        assert!(storage.entry_names().unwrap().is_empty());

        storage.set("kek:v1:profile:default", b"secret").unwrap();
        storage.set("profile-extra", b"extra").unwrap();
        fs::write(base_dir.join(".secure-storage-interrupted.tmp"), b"partial").unwrap();
        fs::write(base_dir.join("not-hex.bin"), b"foreign").unwrap();
        fs::write(base_dir.join("6B6579.bin"), b"non-canonical").unwrap();
        fs::create_dir(base_dir.join("646972.bin")).unwrap();

        assert_eq!(
            storage.entry_names().unwrap(),
            vec![
                "kek:v1:profile:default".to_owned(),
                "profile-extra".to_owned()
            ]
        );
    }
}
