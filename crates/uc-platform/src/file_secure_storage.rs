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

    fn map_io_error(context: &str, err: io::Error) -> SecureStorageError {
        SecureStorageError::Other(format!("{context}: {err}"))
    }
}

impl SecureStoragePort for FileSecureStorage {
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
