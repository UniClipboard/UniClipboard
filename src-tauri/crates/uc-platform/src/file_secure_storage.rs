use std::fs;
use std::io;
use std::path::PathBuf;

use uc_core::ports::{SecureStorageError, SecureStoragePort};

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

    fn file_path(&self, key: &str) -> PathBuf {
        let safe: String = key
            .as_bytes()
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect();
        self.base_dir.join(format!("{safe}.bin"))
    }

    fn legacy_file_path(&self, key: &str) -> PathBuf {
        self.base_dir.join(format!("{key}.bin"))
    }

    fn map_io_error(context: &str, err: io::Error) -> SecureStorageError {
        SecureStorageError::Other(format!("{context}: {err}"))
    }
}

impl SecureStoragePort for FileSecureStorage {
    fn get(&self, key: &str) -> Result<Option<Vec<u8>>, SecureStorageError> {
        let path = self.file_path(key);
        match fs::read(&path) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(err) if err.kind() == io::ErrorKind::NotFound => {
                let legacy_path = self.legacy_file_path(key);
                match fs::read(&legacy_path) {
                    Ok(bytes) => Ok(Some(bytes)),
                    Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
                    Err(err) => Err(Self::map_io_error(
                        "failed to read legacy secure storage file",
                        err,
                    )),
                }
            }
            Err(err) => Err(Self::map_io_error(
                "failed to read secure storage file",
                err,
            )),
        }
    }

    fn set(&self, key: &str, value: &[u8]) -> Result<(), SecureStorageError> {
        let path = self.file_path(key);
        let temp_path = path.with_extension("tmp");
        fs::write(&temp_path, value)
            .map_err(|err| Self::map_io_error("failed to write secure storage temp file", err))?;
        fs::rename(&temp_path, &path)
            .map_err(|err| Self::map_io_error("failed to rename secure storage file", err))?;

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

    #[test]
    fn roundtrip_stores_and_loads() {
        let temp_dir = tempfile::TempDir::new().expect("temp dir");
        let storage = FileSecureStorage::with_base_dir(temp_dir.path().to_path_buf());
        storage.set("kek:v1:profile:demo", b"kek").expect("set");
        let loaded = storage.get("kek:v1:profile:demo").expect("get");
        assert_eq!(loaded, Some(b"kek".to_vec()));
    }

    #[test]
    fn missing_key_returns_none() {
        let temp_dir = tempfile::TempDir::new().expect("temp dir");
        let storage = FileSecureStorage::with_base_dir(temp_dir.path().to_path_buf());
        let loaded = storage.get("libp2p-identity:v1").expect("get");
        assert!(loaded.is_none());
    }

    #[test]
    fn get_reads_legacy_file_keyring_filename_for_backward_compatibility() {
        let temp_dir = tempfile::TempDir::new().expect("temp dir");
        let storage = FileSecureStorage::with_base_dir(temp_dir.path().to_path_buf());
        let legacy_path = temp_dir.path().join("kek:v1:profile:default.bin");

        fs::write(&legacy_path, b"legacy-kek").expect("write legacy key file");

        let loaded = storage
            .get("kek:v1:profile:default")
            .expect("get legacy key");

        assert_eq!(loaded, Some(b"legacy-kek".to_vec()));
    }
}
