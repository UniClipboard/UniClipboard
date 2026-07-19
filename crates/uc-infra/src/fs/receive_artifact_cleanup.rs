use async_trait::async_trait;
use uc_core::ports::{
    CleanupReceiveArtifactsPort, ReceiveArtifact, ReceiveArtifactLogError, ReceiveArtifactOwnership,
};

pub struct FsReceiveArtifactCleaner;

#[async_trait]
impl CleanupReceiveArtifactsPort for FsReceiveArtifactCleaner {
    async fn cleanup_receive_artifacts(
        &self,
        artifacts: &[ReceiveArtifact],
    ) -> Result<(), ReceiveArtifactLogError> {
        for artifact in artifacts {
            let paths = match artifact.ownership {
                ReceiveArtifactOwnership::ManagedStaging => {
                    if artifact.staged_path == artifact.final_path {
                        vec![&artifact.staged_path]
                    } else {
                        vec![&artifact.staged_path, &artifact.final_path]
                    }
                }
                ReceiveArtifactOwnership::UserDestination => {
                    if artifact.staged_path == artifact.final_path {
                        Vec::new()
                    } else {
                        vec![&artifact.staged_path]
                    }
                }
            };
            for path in paths {
                let metadata = match tokio::fs::symlink_metadata(path).await {
                    Ok(metadata) => metadata,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                    Err(error) => return Err(ReceiveArtifactLogError::Backend(error.to_string())),
                };
                let result = if metadata.is_dir() && !metadata.file_type().is_symlink() {
                    tokio::fs::remove_dir_all(path).await
                } else {
                    tokio::fs::remove_file(path).await
                };
                if let Err(error) = result {
                    if error.kind() != std::io::ErrorKind::NotFound {
                        return Err(ReceiveArtifactLogError::Backend(error.to_string()));
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use uc_core::ports::ReceiveArtifactOwnership;

    #[tokio::test]
    async fn managed_staging_artifacts_are_removed() {
        let directory = tempfile::tempdir().expect("temp dir");
        let staged = directory.path().join("staged.bin");
        let final_path = directory.path().join("final.bin");
        tokio::fs::write(&staged, b"staged")
            .await
            .expect("write staged");
        tokio::fs::write(&final_path, b"final")
            .await
            .expect("write final");

        FsReceiveArtifactCleaner
            .cleanup_receive_artifacts(&[artifact(
                staged.clone(),
                final_path.clone(),
                ReceiveArtifactOwnership::ManagedStaging,
            )])
            .await
            .expect("cleanup managed artifact");

        assert!(!staged.exists());
        assert!(!final_path.exists());
    }

    #[tokio::test]
    async fn user_destination_final_path_is_never_removed() {
        let directory = tempfile::tempdir().expect("temp dir");
        let final_path = directory.path().join("user-file.bin");
        tokio::fs::write(&final_path, b"user content")
            .await
            .expect("write user file");

        FsReceiveArtifactCleaner
            .cleanup_receive_artifacts(&[artifact(
                final_path.clone(),
                final_path.clone(),
                ReceiveArtifactOwnership::UserDestination,
            )])
            .await
            .expect("cleanup user destination metadata");

        assert_eq!(
            tokio::fs::read(&final_path)
                .await
                .expect("user file remains"),
            b"user content"
        );
    }

    #[tokio::test]
    async fn user_destination_cleanup_only_removes_distinct_staging_path() {
        let directory = tempfile::tempdir().expect("temp dir");
        let staged = directory.path().join("attempt-staging");
        let final_path = directory.path().join("user-folder");
        tokio::fs::create_dir_all(&staged)
            .await
            .expect("create staged");
        tokio::fs::create_dir_all(&final_path)
            .await
            .expect("create final");

        FsReceiveArtifactCleaner
            .cleanup_receive_artifacts(&[artifact(
                staged.clone(),
                final_path.clone(),
                ReceiveArtifactOwnership::UserDestination,
            )])
            .await
            .expect("cleanup user destination staging");

        assert!(!staged.exists());
        assert!(final_path.exists());
    }

    fn artifact(
        staged_path: PathBuf,
        final_path: PathBuf,
        ownership: ReceiveArtifactOwnership,
    ) -> ReceiveArtifact {
        ReceiveArtifact {
            item_id: "item-1".to_string(),
            staged_path,
            final_path,
            ownership,
        }
    }
}
