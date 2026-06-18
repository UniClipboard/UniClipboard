//! `DieselMobileDeviceRepository` —— `MobileDeviceStore` 的 sqlite
//! 实现(v3 SyncClipboard 兼容版)。
//!
//! ## 错误映射
//!
//! `save` 路径上,sqlite 的 UNIQUE 约束既保护 `device_id`(PK)又保护
//! `username`(显式 UNIQUE)。Diesel 在 SQLite 后端只把它统一报告为
//! `DatabaseErrorKind::UniqueViolation`,`column_name()` 在不同 SQLite /
//! libsqlite3-sys 版本上不稳定。为了把"哪边撞了"翻译成业务错误,我们在
//! 捕到 UniqueViolation 后顺手再做一次 device_id 主键存在性查询:
//!
//! - 主键命中 → `MobileDeviceError::AlreadyExists`
//! - 主键未中 → 必然是 username 冲突 → `UsernameCollision`
//!
//! 这次额外查询走主键索引,代价可忽略。它发生在 `save` 路径上:失败的
//! insert 与跟随的存在性查询都在同一个 `executor.run` 闭包(同一连接)内,
//! 但默认 autocommit 下二者是两条独立语句,并不共享同一事务 —— 这里的
//! 分类是失败 insert 之后的 best-effort post-hoc 读,而非事务内原子操作。

use async_trait::async_trait;
use diesel::prelude::*;
use diesel::result::{DatabaseErrorKind, Error as DieselError};

use uc_core::mobile_sync::{MobileDevice, MobileDeviceError, MobileDeviceId};
use uc_core::ports::MobileDeviceStore;

use crate::db::models::{MobileDeviceRow, NewMobileDeviceRow};
use crate::db::ports::{DbExecutor, InsertMapper, RowMapper};
use crate::db::schema::mobile_device::dsl::*;

/// `save` 闭包内部三态返回 —— 把"是否撞了什么唯一约束"原子地从事务里带出来,
/// 让外层把它翻译成正确的领域错误。
enum SaveOutcome {
    Inserted,
    DuplicateDeviceId,
    DuplicateUsername,
}

pub struct DieselMobileDeviceRepository<E, M> {
    executor: E,
    mapper: M,
}

impl<E, M> DieselMobileDeviceRepository<E, M> {
    pub fn new(executor: E, mapper: M) -> Self {
        Self { executor, mapper }
    }
}

#[async_trait]
impl<E, M> MobileDeviceStore for DieselMobileDeviceRepository<E, M>
where
    E: DbExecutor,
    M: InsertMapper<MobileDevice, NewMobileDeviceRow>
        + RowMapper<MobileDeviceRow, MobileDevice>
        + Send
        + Sync,
{
    async fn save(&self, device: &MobileDevice) -> Result<(), MobileDeviceError> {
        let row = self
            .mapper
            .to_row(device)
            .map_err(|e| MobileDeviceError::Storage(e.to_string()))?;

        let outcome: SaveOutcome = self
            .executor
            .run(move |conn| {
                let result = diesel::insert_into(mobile_device)
                    .values(&row)
                    .execute(conn);

                match result {
                    Ok(_) => Ok(SaveOutcome::Inserted),
                    Err(DieselError::DatabaseError(DatabaseErrorKind::UniqueViolation, _)) => {
                        // 见模块文档:UniqueViolation 后用主键查询区分两种约束。
                        let id_taken: i64 = mobile_device
                            .filter(device_id.eq(&row.device_id))
                            .count()
                            .get_result(conn)
                            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
                        if id_taken > 0 {
                            Ok(SaveOutcome::DuplicateDeviceId)
                        } else {
                            Ok(SaveOutcome::DuplicateUsername)
                        }
                    }
                    Err(e) => Err(anyhow::anyhow!(e.to_string())),
                }
            })
            .map_err(|e| MobileDeviceError::Storage(e.to_string()))?;

        match outcome {
            SaveOutcome::Inserted => Ok(()),
            SaveOutcome::DuplicateDeviceId => {
                Err(MobileDeviceError::AlreadyExists(device.device_id.clone()))
            }
            SaveOutcome::DuplicateUsername => Err(MobileDeviceError::UsernameCollision),
        }
    }

    async fn find_by_username(
        &self,
        username_value: &str,
    ) -> Result<Option<MobileDevice>, MobileDeviceError> {
        let needle = username_value.to_string();
        self.executor
            .run(move |conn| {
                let row = mobile_device
                    .filter(username.eq(&needle))
                    .first::<MobileDeviceRow>(conn)
                    .optional()
                    .map_err(|e| anyhow::anyhow!(e.to_string()))?;
                match row {
                    Some(r) => self
                        .mapper
                        .to_domain(&r)
                        .map(Some)
                        .map_err(|e| anyhow::anyhow!(e.to_string())),
                    None => Ok(None),
                }
            })
            .map_err(|e| MobileDeviceError::Storage(e.to_string()))
    }

    async fn find_by_device_id(
        &self,
        device_id_value: &MobileDeviceId,
    ) -> Result<Option<MobileDevice>, MobileDeviceError> {
        let needle = device_id_value.as_str().to_string();
        self.executor
            .run(move |conn| {
                let row = mobile_device
                    .filter(device_id.eq(&needle))
                    .first::<MobileDeviceRow>(conn)
                    .optional()
                    .map_err(|e| anyhow::anyhow!(e.to_string()))?;
                match row {
                    Some(r) => self
                        .mapper
                        .to_domain(&r)
                        .map(Some)
                        .map_err(|e| anyhow::anyhow!(e.to_string())),
                    None => Ok(None),
                }
            })
            .map_err(|e| MobileDeviceError::Storage(e.to_string()))
    }

    async fn list_all(&self) -> Result<Vec<MobileDevice>, MobileDeviceError> {
        self.executor
            .run(|conn| {
                let rows = mobile_device
                    .load::<MobileDeviceRow>(conn)
                    .map_err(|e| anyhow::anyhow!(e.to_string()))?;
                let mut out = Vec::with_capacity(rows.len());
                for r in &rows {
                    let d = self
                        .mapper
                        .to_domain(r)
                        .map_err(|e| anyhow::anyhow!(e.to_string()))?;
                    out.push(d);
                }
                Ok(out)
            })
            .map_err(|e| MobileDeviceError::Storage(e.to_string()))
    }

    async fn delete(&self, device_id_value: &MobileDeviceId) -> Result<bool, MobileDeviceError> {
        let needle = device_id_value.as_str().to_string();
        let affected = self
            .executor
            .run(move |conn| {
                diesel::delete(mobile_device.filter(device_id.eq(&needle)))
                    .execute(conn)
                    .map_err(|e| anyhow::anyhow!(e.to_string()))
            })
            .map_err(|e| MobileDeviceError::Storage(e.to_string()))?;
        Ok(affected > 0)
    }

    async fn update_mobile_device(
        &self,
        updated: &MobileDevice,
    ) -> Result<bool, MobileDeviceError> {
        let current_device_id = updated.device_id.as_str().to_string();

        #[derive(AsChangeset)]
        #[diesel(table_name = crate::db::schema::mobile_device)]
        struct Changeset {
            label: String,
            username: String,
            password_hash: String,
        }

        let changeset = Changeset {
            label: updated.label.clone(),
            username: updated.username.clone(),
            password_hash: updated.password_hash.clone(),
        };

        let outcome = self
            .executor
            .run(move |conn| {
                let result = diesel::update(mobile_device.filter(device_id.eq(&current_device_id)))
                    .set(&changeset)
                    .execute(conn);

                match result {
                    Ok(affected) => Ok(Ok(affected > 0)),
                    Err(DieselError::DatabaseError(DatabaseErrorKind::UniqueViolation, _)) => {
                        // device_id is this UPDATE's WHERE key, so it cannot be
                        // violated by the row keeping its own primary key; the
                        // only reachable UNIQUE violation here is a cross-device
                        // username collision.
                        Ok(Err(MobileDeviceError::UsernameCollision))
                    }
                    Err(e) => Err(anyhow::anyhow!(e.to_string())),
                }
            })
            .map_err(|e| MobileDeviceError::Storage(e.to_string()))?;

        outcome
    }
}

// ---- Intent ports ----
//
// The single Diesel adapter satisfies every narrow device-repository port by
// delegating to its aggregate-store methods (UFCS disambiguates the same-named
// methods). The composition root coerces one `Arc<DieselMobileDeviceRepository>`
// into each port (see ports.md §8.3).
//
// These impls live in a private submodule so the narrow port traits do not leak
// into the test module's method-resolution scope (they share method names with
// the aggregate store); trait-impl coherence still applies crate-wide.
mod intent_ports {
    use super::*;
    use uc_core::ports::{
        DeleteMobileDevicePort, FindMobileDeviceByIdPort, FindMobileDeviceByUsernamePort,
        ListMobileDevicesPort, SaveMobileDevicePort, UpdateMobileDevicePort,
    };

    #[async_trait]
    impl<E, M> FindMobileDeviceByUsernamePort for DieselMobileDeviceRepository<E, M>
    where
        E: DbExecutor,
        M: InsertMapper<MobileDevice, NewMobileDeviceRow>
            + RowMapper<MobileDeviceRow, MobileDevice>
            + Send
            + Sync,
    {
        async fn find_by_username(
            &self,
            username_value: &str,
        ) -> Result<Option<MobileDevice>, MobileDeviceError> {
            MobileDeviceStore::find_by_username(self, username_value).await
        }
    }

    #[async_trait]
    impl<E, M> FindMobileDeviceByIdPort for DieselMobileDeviceRepository<E, M>
    where
        E: DbExecutor,
        M: InsertMapper<MobileDevice, NewMobileDeviceRow>
            + RowMapper<MobileDeviceRow, MobileDevice>
            + Send
            + Sync,
    {
        async fn find_by_device_id(
            &self,
            device_id_value: &MobileDeviceId,
        ) -> Result<Option<MobileDevice>, MobileDeviceError> {
            MobileDeviceStore::find_by_device_id(self, device_id_value).await
        }
    }

    #[async_trait]
    impl<E, M> ListMobileDevicesPort for DieselMobileDeviceRepository<E, M>
    where
        E: DbExecutor,
        M: InsertMapper<MobileDevice, NewMobileDeviceRow>
            + RowMapper<MobileDeviceRow, MobileDevice>
            + Send
            + Sync,
    {
        async fn list_all(&self) -> Result<Vec<MobileDevice>, MobileDeviceError> {
            MobileDeviceStore::list_all(self).await
        }
    }

    #[async_trait]
    impl<E, M> SaveMobileDevicePort for DieselMobileDeviceRepository<E, M>
    where
        E: DbExecutor,
        M: InsertMapper<MobileDevice, NewMobileDeviceRow>
            + RowMapper<MobileDeviceRow, MobileDevice>
            + Send
            + Sync,
    {
        async fn save(&self, device: &MobileDevice) -> Result<(), MobileDeviceError> {
            MobileDeviceStore::save(self, device).await
        }
    }

    #[async_trait]
    impl<E, M> DeleteMobileDevicePort for DieselMobileDeviceRepository<E, M>
    where
        E: DbExecutor,
        M: InsertMapper<MobileDevice, NewMobileDeviceRow>
            + RowMapper<MobileDeviceRow, MobileDevice>
            + Send
            + Sync,
    {
        async fn delete(
            &self,
            device_id_value: &MobileDeviceId,
        ) -> Result<bool, MobileDeviceError> {
            MobileDeviceStore::delete(self, device_id_value).await
        }
    }

    #[async_trait]
    impl<E, M> UpdateMobileDevicePort for DieselMobileDeviceRepository<E, M>
    where
        E: DbExecutor,
        M: InsertMapper<MobileDevice, NewMobileDeviceRow>
            + RowMapper<MobileDeviceRow, MobileDevice>
            + Send
            + Sync,
    {
        async fn update_mobile_device(
            &self,
            updated: &MobileDevice,
        ) -> Result<bool, MobileDeviceError> {
            MobileDeviceStore::update_mobile_device(self, updated).await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::executor::DieselSqliteExecutor;
    use crate::db::mappers::mobile_device_mapper::MobileDeviceRowMapper;
    use crate::db::pool::init_db_pool;
    use tempfile::{tempdir, TempDir};
    use uc_core::mobile_sync::MobileClientType;

    fn make_repo() -> (
        DieselMobileDeviceRepository<DieselSqliteExecutor, MobileDeviceRowMapper>,
        TempDir,
    ) {
        let tmp = tempdir().unwrap();
        let url = tmp.path().join("mobile-device.sqlite");
        let pool = init_db_pool(url.to_str().unwrap()).unwrap();
        let repo = DieselMobileDeviceRepository::new(
            DieselSqliteExecutor::new(pool),
            MobileDeviceRowMapper,
        );
        (repo, tmp)
    }

    fn fixture(id: &str, username_suffix: &str, label_text: &str) -> MobileDevice {
        MobileDevice {
            device_id: MobileDeviceId::new(id),
            label: label_text.into(),
            client_type: MobileClientType::IosShortcut,
            username: format!("mobile_{username_suffix}"),
            password_hash: format!(
                "$argon2id$v=19$m=64,t=1,p=1$AAAAAAAAAAAAAAAA$AAAAAAAAAAAAAAAAAAAAAAAAA-{username_suffix}",
            ),
            created_at_ms: 1_700_000_000_000,
            last_seen_at_ms: None,
            last_seen_ip: None,
            reported_name: None,
            reported_os: None,
        }
    }

    #[tokio::test]
    async fn save_then_find_by_device_id_returns_full_device() {
        let (repo, _t) = make_repo();
        let d = fixture("did_x", "0001", "phone");
        repo.save(&d).await.unwrap();
        let got = repo
            .find_by_device_id(&d.device_id)
            .await
            .unwrap()
            .expect("must hit");
        assert_eq!(got, d);
    }

    #[tokio::test]
    async fn save_then_find_by_username_returns_full_device() {
        let (repo, _t) = make_repo();
        let d = fixture("did_y", "0009", "phone");
        repo.save(&d).await.unwrap();
        let got = repo
            .find_by_username(&d.username)
            .await
            .unwrap()
            .expect("must hit");
        assert_eq!(got.device_id, d.device_id);
    }

    #[tokio::test]
    async fn save_rejects_duplicate_device_id_with_already_exists() {
        let (repo, _t) = make_repo();
        let d1 = fixture("did_dup", "0001", "first");
        let d2 = fixture("did_dup", "0002", "second"); // 同 id, 不同 username
        repo.save(&d1).await.unwrap();
        let err = repo.save(&d2).await.unwrap_err();
        assert!(matches!(err, MobileDeviceError::AlreadyExists(_)));
    }

    #[tokio::test]
    async fn save_rejects_duplicate_username_with_collision_error() {
        let (repo, _t) = make_repo();
        let d1 = fixture("did_a", "abcd", "first");
        let d2 = fixture("did_b", "abcd", "second"); // 不同 id, 同 username
        repo.save(&d1).await.unwrap();
        let err = repo.save(&d2).await.unwrap_err();
        assert!(matches!(err, MobileDeviceError::UsernameCollision));
    }

    #[tokio::test]
    async fn find_returns_none_when_missing() {
        let (repo, _t) = make_repo();
        assert!(repo
            .find_by_device_id(&MobileDeviceId::new("did_ghost"))
            .await
            .unwrap()
            .is_none());
        assert!(repo
            .find_by_username("mobile_ghost")
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn list_all_returns_every_saved_device() {
        let (repo, _t) = make_repo();
        repo.save(&fixture("did_a", "aaaa", "A")).await.unwrap();
        repo.save(&fixture("did_b", "bbbb", "B")).await.unwrap();
        repo.save(&fixture("did_c", "cccc", "C")).await.unwrap();
        let mut all = repo.list_all().await.unwrap();
        all.sort_by(|x, y| x.device_id.as_str().cmp(y.device_id.as_str()));
        assert_eq!(all.len(), 3);
        assert_eq!(all[0].device_id.as_str(), "did_a");
        assert_eq!(all[2].device_id.as_str(), "did_c");
    }

    #[tokio::test]
    async fn delete_returns_true_then_false() {
        let (repo, _t) = make_repo();
        let d = fixture("did_x", "0001", "phone");
        repo.save(&d).await.unwrap();
        assert!(repo.delete(&d.device_id).await.unwrap());
        assert!(!repo.delete(&d.device_id).await.unwrap());
        assert!(repo
            .find_by_device_id(&d.device_id)
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn update_mobile_device_replaces_label_username_and_password_hash_only() {
        let (repo, _t) = make_repo();
        let d = fixture("did_x", "0001", "phone");
        repo.save(&d).await.unwrap();

        let mut updated = d.clone();
        updated.label = "renamed".into();
        updated.username = "mobile_0002".into();
        updated.password_hash = "$argon2id$test$updated".into();

        assert!(repo.update_mobile_device(&updated).await.unwrap());

        let got = repo.find_by_device_id(&d.device_id).await.unwrap().unwrap();
        assert_eq!(got.label, "renamed");
        assert_eq!(got.username, "mobile_0002");
        assert_eq!(got.password_hash, "$argon2id$test$updated");
        assert_eq!(got.created_at_ms, d.created_at_ms);
        assert_eq!(got.last_seen_at_ms, d.last_seen_at_ms);
    }

    #[tokio::test]
    async fn update_mobile_device_rejects_username_collision() {
        let (repo, _t) = make_repo();
        let mut d1 = fixture("did_a", "0001", "first");
        let d2 = fixture("did_b", "0002", "second");
        repo.save(&d1).await.unwrap();
        repo.save(&d2).await.unwrap();

        d1.username = d2.username.clone();
        let err = repo.update_mobile_device(&d1).await.unwrap_err();
        assert!(matches!(err, MobileDeviceError::UsernameCollision));
    }

    #[tokio::test]
    async fn update_mobile_device_returns_false_when_missing() {
        let (repo, _t) = make_repo();
        let d = fixture("did_ghost", "0001", "phone");
        assert!(!repo.update_mobile_device(&d).await.unwrap());
    }
}
