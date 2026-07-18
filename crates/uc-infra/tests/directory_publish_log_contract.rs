use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use diesel::prelude::*;
use tempfile::tempdir;
use uc_core::ids::ProfileId;
use uc_core::ports::security::current_profile::{CurrentProfileError, CurrentProfilePort};
use uc_core::ports::space::{DeriveSpaceSubkeyPort, SpaceAccessError};
use uc_core::ports::{
    GetDirectoryPublishRecordPort, PublishLogError, PublishPhase, RecordDirectoryPublishPort,
};
use uc_infra::db::executor::DieselSqliteExecutor;
use uc_infra::db::models::DirectoryPublishLogRow;
use uc_infra::db::pool::init_db_pool;
use uc_infra::db::ports::DbExecutor;
use uc_infra::db::repositories::DieselDirectoryPublishLogRepository;
use uc_infra::db::schema::directory_publish_log;

struct FixedSubkey;

#[async_trait]
impl DeriveSpaceSubkeyPort for FixedSubkey {
    async fn derive_subkey(
        &self,
        _salt: &[u8],
        _info: &[u8],
    ) -> Result<[u8; 32], SpaceAccessError> {
        Ok([9; 32])
    }
}

struct LockedSubkey;

#[async_trait]
impl DeriveSpaceSubkeyPort for LockedSubkey {
    async fn derive_subkey(
        &self,
        _salt: &[u8],
        _info: &[u8],
    ) -> Result<[u8; 32], SpaceAccessError> {
        Err(SpaceAccessError::NotUnlocked)
    }
}

struct FixedProfile;

#[async_trait]
impl CurrentProfilePort for FixedProfile {
    async fn current_profile(&self) -> Result<ProfileId, CurrentProfileError> {
        Ok(ProfileId::from("default"))
    }
}

#[tokio::test]
async fn encrypted_root_map_round_trips_and_partial_marker_updates() {
    let directory = tempdir().unwrap();
    let database = directory.path().join("publish-log.sqlite");
    let pool = init_db_pool(database.to_str().unwrap()).unwrap();
    let repo = DieselDirectoryPublishLogRepository::new(
        DieselSqliteExecutor::new(pool.clone()),
        Arc::new(FixedSubkey),
        Arc::new(FixedProfile),
    );
    let reader = DieselSqliteExecutor::new(pool);
    let roots = vec![(
        PathBuf::from("/tmp/.uniclip-incoming-entry/root"),
        PathBuf::from("/tmp/private-directory-name"),
    )];

    repo.record_phase("entry", "a1", PublishPhase::Staging, &roots, 10)
        .await
        .unwrap();
    let stored = reader
        .run(|conn| {
            Ok(directory_publish_log::table
                .select(DirectoryPublishLogRow::as_select())
                .first(conn)?)
        })
        .unwrap();
    let ciphertext = stored.root_map_ciphertext.unwrap();
    assert!(!ciphertext
        .windows(b"private-directory-name".len())
        .any(|window| window == b"private-directory-name"));
    assert!(!stored.partial_publication);
    assert!(!stored.landed);

    let record = repo
        .get_publish_record("entry", "a1")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(record.phase, PublishPhase::Staging);
    assert_eq!(record.root_map, roots);
    assert_eq!(record.partial_visible_roots, None);

    repo.record_partial("entry", "a1", 1, 11).await.unwrap();
    let partial = repo
        .get_publish_record("entry", "a1")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(partial.partial_visible_roots, Some(1));
}

#[tokio::test]
async fn ciphertext_is_bound_to_attempt_and_locked_session_defers_open() {
    let directory = tempdir().unwrap();
    let database = directory.path().join("publish-log.sqlite");
    let pool = init_db_pool(database.to_str().unwrap()).unwrap();
    let repo = DieselDirectoryPublishLogRepository::new(
        DieselSqliteExecutor::new(pool.clone()),
        Arc::new(FixedSubkey),
        Arc::new(FixedProfile),
    );
    repo.record_phase(
        "entry",
        "a1",
        PublishPhase::Publishing,
        &[(PathBuf::from("staged"), PathBuf::from("final"))],
        10,
    )
    .await
    .unwrap();

    let writer = DieselSqliteExecutor::new(pool.clone());
    writer
        .run(|conn| {
            diesel::update(directory_publish_log::table)
                .set(directory_publish_log::attempt_id.eq("a2"))
                .execute(conn)?;
            Ok(())
        })
        .unwrap();
    assert!(matches!(
        repo.get_publish_record("entry", "a2").await,
        Err(PublishLogError::InvalidCiphertext)
    ));

    let locked = DieselDirectoryPublishLogRepository::new(
        DieselSqliteExecutor::new(pool),
        Arc::new(LockedSubkey),
        Arc::new(FixedProfile),
    );
    assert!(matches!(
        locked.get_publish_record("entry", "a2").await,
        Err(PublishLogError::EncryptionUnavailable(_))
    ));
}

#[tokio::test]
async fn landed_cannot_be_recorded_outside_atomic_commit() {
    let directory = tempdir().unwrap();
    let database = directory.path().join("publish-log.sqlite");
    let pool = init_db_pool(database.to_str().unwrap()).unwrap();
    let repo = DieselDirectoryPublishLogRepository::new(
        DieselSqliteExecutor::new(pool),
        Arc::new(FixedSubkey),
        Arc::new(FixedProfile),
    );

    assert!(matches!(
        repo.record_phase("entry", "a1", PublishPhase::Landed, &[], 1)
            .await,
        Err(PublishLogError::Backend(_))
    ));
}
