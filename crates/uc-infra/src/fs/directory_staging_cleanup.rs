use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use uc_app_paths::DIRECTORY_RECEIVE_STAGING_PREFIX;
use uc_core::ports::{CleanupDirectoryStagingPort, DirectoryStagingCleanupError};

#[derive(Debug, Default)]
pub struct FsDirectoryStagingCleaner;

impl FsDirectoryStagingCleaner {
    pub fn new() -> std::sync::Arc<Self> {
        std::sync::Arc::new(Self)
    }
}

fn staging_area_for_root(root: &Path) -> Result<PathBuf, DirectoryStagingCleanupError> {
    let area = root
        .parent()
        .ok_or(DirectoryStagingCleanupError::InvalidPath)?;
    let valid = area
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with(DIRECTORY_RECEIVE_STAGING_PREFIX));
    if !valid {
        return Err(DirectoryStagingCleanupError::InvalidPath);
    }
    Ok(area.to_path_buf())
}

#[async_trait]
impl CleanupDirectoryStagingPort for FsDirectoryStagingCleaner {
    async fn cleanup_staging_roots(
        &self,
        staged_roots: &[PathBuf],
    ) -> Result<(), DirectoryStagingCleanupError> {
        let areas = staged_roots
            .iter()
            .map(|root| staging_area_for_root(root))
            .collect::<Result<BTreeSet<_>, _>>()?;

        for area in areas {
            match tokio::fs::remove_dir_all(&area).await {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(DirectoryStagingCleanupError::Backend(error.to_string()));
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_staging_parent() {
        let result = staging_area_for_root(Path::new("/tmp/user-files/root"));
        assert!(matches!(
            result,
            Err(DirectoryStagingCleanupError::InvalidPath)
        ));
    }

    #[test]
    fn accepts_receive_staging_parent() {
        let root = Path::new("/tmp/.uniclip-incoming-entry/root");
        assert_eq!(
            staging_area_for_root(root).expect("valid staging root"),
            PathBuf::from("/tmp/.uniclip-incoming-entry")
        );
    }
}
