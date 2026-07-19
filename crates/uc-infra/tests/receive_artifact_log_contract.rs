use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use diesel::prelude::*;
use tempfile::tempdir;
use uc_core::ids::ProfileId;
use uc_core::ports::security::current_profile::{CurrentProfileError, CurrentProfilePort};
use uc_core::ports::space::{DeriveSpaceSubkeyPort, SpaceAccessError};
use uc_core::ports::{
    GetReceiveArtifactRecordPort, ListUnsettledReceiveArtifactsPort, ReceiveArtifact,
    ReceiveArtifactLogError, ReceiveArtifactOwnership, ReceiveArtifactPhase, ReceiveArtifactRecord,
    ReceiveArtifactResolution, RecordReceiveArtifactsPort,
};
use uc_infra::db::executor::DieselSqliteExecutor;
use uc_infra::db::pool::init_db_pool;
use uc_infra::db::ports::DbExecutor;
use uc_infra::db::repositories::DieselReceiveArtifactLogRepository;
use uc_infra::db::schema::receive_artifact_log;

struct FixedSubkey;

#[async_trait]
impl DeriveSpaceSubkeyPort for FixedSubkey {
    async fn derive_subkey(
        &self,
        _salt: &[u8],
        _info: &[u8],
    ) -> Result<[u8; 32], SpaceAccessError> {
        Ok([0x6B; 32])
    }
}

struct FixedProfile;

#[async_trait]
impl CurrentProfilePort for FixedProfile {
    async fn current_profile(&self) -> Result<ProfileId, CurrentProfileError> {
        Ok(ProfileId::from("test"))
    }
}

type Repo = DieselReceiveArtifactLogRepository<DieselSqliteExecutor>;

fn setup() -> (Repo, DieselSqliteExecutor, tempfile::TempDir) {
    let directory = tempdir().unwrap();
    let database = directory.path().join("artifacts.sqlite");
    let pool = init_db_pool(database.to_str().unwrap()).unwrap();
    (
        Repo::new(
            DieselSqliteExecutor::new(pool.clone()),
            Arc::new(FixedSubkey),
            Arc::new(FixedProfile),
        ),
        DieselSqliteExecutor::new(pool),
        directory,
    )
}

fn record(
    entry: &str,
    attempt: &str,
    resolution: ReceiveArtifactResolution,
) -> ReceiveArtifactRecord {
    ReceiveArtifactRecord {
        entry_id: entry.to_owned(),
        attempt_id: attempt.to_owned(),
        phase: if resolution == ReceiveArtifactResolution::Pending {
            ReceiveArtifactPhase::Preparing
        } else {
            ReceiveArtifactPhase::Settled
        },
        resolution,
        artifacts: vec![ReceiveArtifact {
            item_id: "item-1".to_owned(),
            staged_path: PathBuf::from("/tmp/private-artifact-stage-8a7c"),
            final_path: PathBuf::from("/tmp/private-artifact-final-8a7c"),
            ownership: ReceiveArtifactOwnership::ManagedStaging,
        }],
        updated_at_ms: 5,
    }
}

#[tokio::test]
async fn encrypted_artifacts_round_trip_and_unsettled_query_is_exact() {
    let (repo, reader, directory) = setup();
    let pending = record("entry-1", "a1", ReceiveArtifactResolution::Pending);
    let landed = record("entry-2", "a2", ReceiveArtifactResolution::Landed);
    repo.record_receive_artifacts(&pending).await.unwrap();
    repo.record_receive_artifacts(&landed).await.unwrap();

    assert_eq!(
        repo.get_receive_artifact_record("entry-1", "a1")
            .await
            .unwrap(),
        Some(pending.clone())
    );
    assert_eq!(
        repo.list_unsettled_receive_artifacts().await.unwrap(),
        vec![pending]
    );

    drop(reader);
    for suffix in ["", "-wal", "-shm"] {
        let path = directory.path().join(format!("artifacts.sqlite{suffix}"));
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        assert!(!bytes
            .windows(b"private-artifact-final-8a7c".len())
            .any(|window| window == b"private-artifact-final-8a7c"));
    }
}

#[tokio::test]
async fn invalid_artifact_phase_is_rejected() {
    let (repo, writer, _directory) = setup();
    repo.record_receive_artifacts(&record("entry-1", "a1", ReceiveArtifactResolution::Pending))
        .await
        .unwrap();
    writer
        .run(|conn| {
            diesel::update(receive_artifact_log::table)
                .set(receive_artifact_log::phase.eq("unknown"))
                .execute(conn)?;
            Ok(())
        })
        .unwrap();

    assert!(matches!(
        repo.get_receive_artifact_record("entry-1", "a1").await,
        Err(ReceiveArtifactLogError::InvalidValue(value)) if value == "unknown"
    ));
}
