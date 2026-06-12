use async_trait::async_trait;
use diesel::prelude::*;

use uc_core::{DeviceId, MemberRepositoryPort, MembershipError, SpaceMember};

use crate::db::models::{NewSpaceMemberRow, SpaceMemberRow};
use crate::db::ports::{DbExecutor, InsertMapper, RowMapper};
use crate::db::schema::space_member::dsl::*;

pub struct DieselSpaceMemberRepository<E, M> {
    executor: E,
    mapper: M,
}

impl<E, M> DieselSpaceMemberRepository<E, M> {
    pub fn new(executor: E, mapper: M) -> Self {
        Self { executor, mapper }
    }
}

#[async_trait]
impl<E, M> MemberRepositoryPort for DieselSpaceMemberRepository<E, M>
where
    E: DbExecutor,
    M: InsertMapper<SpaceMember, NewSpaceMemberRow>
        + RowMapper<SpaceMemberRow, SpaceMember>
        + Send
        + Sync,
{
    async fn get(
        &self,
        device_id_value: &DeviceId,
    ) -> Result<Option<SpaceMember>, MembershipError> {
        let id = device_id_value.as_str().to_string();
        self.executor
            .run(move |conn| {
                let row = space_member
                    .filter(device_id.eq(&id))
                    .first::<SpaceMemberRow>(conn)
                    .optional()
                    .map_err(|e| anyhow::anyhow!(e.to_string()))?;

                match row {
                    Some(r) => {
                        let member = self
                            .mapper
                            .to_domain(&r)
                            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
                        Ok(Some(member))
                    }
                    None => Ok(None),
                }
            })
            .map_err(|e| MembershipError::Repository(e.to_string()))
    }

    async fn list(&self) -> Result<Vec<SpaceMember>, MembershipError> {
        self.executor
            .run(|conn| {
                let rows = space_member
                    .load::<SpaceMemberRow>(conn)
                    .map_err(|e| anyhow::anyhow!(e.to_string()))?;

                let mut members = Vec::with_capacity(rows.len());
                for row in rows {
                    let id = row.device_id.clone();
                    let member = self.mapper.to_domain(&row).map_err(|e| {
                        anyhow::anyhow!("Failed to map space_member device_id {}: {}", id, e)
                    })?;
                    members.push(member);
                }

                Ok(members)
            })
            .map_err(|e| MembershipError::Repository(e.to_string()))
    }

    async fn save(&self, member: &SpaceMember) -> Result<(), MembershipError> {
        let row = self
            .mapper
            .to_row(member)
            .map_err(|e| MembershipError::Repository(e.to_string()))?;

        self.executor
            .run(move |conn| {
                diesel::insert_into(space_member)
                    .values(&row)
                    .on_conflict(device_id)
                    .do_update()
                    .set((
                        device_name.eq(row.device_name.clone()),
                        identity_fingerprint.eq(row.identity_fingerprint.clone()),
                        joined_at.eq(row.joined_at),
                        sync_preferences.eq(row.sync_preferences.clone()),
                    ))
                    .execute(conn)
                    .map_err(|e| anyhow::anyhow!(e.to_string()))?;
                Ok(())
            })
            .map_err(|e| MembershipError::Repository(e.to_string()))
    }

    async fn remove(&self, device_id_value: &DeviceId) -> Result<bool, MembershipError> {
        let id = device_id_value.as_str().to_string();
        let affected = self
            .executor
            .run(move |conn| {
                diesel::delete(space_member.filter(device_id.eq(&id)))
                    .execute(conn)
                    .map_err(|e| anyhow::anyhow!(e.to_string()))
            })
            .map_err(|e| MembershipError::Repository(e.to_string()))?;

        Ok(affected > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::executor::DieselSqliteExecutor;
    use crate::db::mappers::space_member_mapper::SpaceMemberRowMapper;
    use crate::db::pool::init_db_pool;
    use chrono::Utc;
    use tempfile::{tempdir, TempDir};
    use uc_core::security::IdentityFingerprint;
    use uc_core::{DeviceId, MemberSyncPreferences, SpaceMember};

    fn make_repo() -> (
        DieselSpaceMemberRepository<DieselSqliteExecutor, SpaceMemberRowMapper>,
        TempDir,
    ) {
        let tempdir = tempdir().unwrap();
        let database_url = tempdir.path().join("space-member.sqlite");
        let pool = init_db_pool(database_url.to_str().unwrap()).unwrap();
        let repo =
            DieselSpaceMemberRepository::new(DieselSqliteExecutor::new(pool), SpaceMemberRowMapper);
        (repo, tempdir)
    }

    /// Pad a short seed into a valid 16-char alphanumeric fingerprint.
    fn fixture_fingerprint(seed: &str) -> IdentityFingerprint {
        let mut raw: String = seed.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
        raw.make_ascii_uppercase();
        while raw.len() < 16 {
            raw.push('A');
        }
        IdentityFingerprint::from_raw_string(&raw[..16]).unwrap()
    }

    fn fixture_member(id: &str) -> SpaceMember {
        SpaceMember {
            device_id: DeviceId::new(id),
            device_name: format!("device-{id}"),
            identity_fingerprint: fixture_fingerprint(&format!("FP{id}")),
            joined_at: Utc::now(),
            sync_preferences: MemberSyncPreferences::default(),
        }
    }

    #[tokio::test]
    async fn save_then_get_roundtrip() {
        let (repo, _tempdir) = make_repo();
        let member = fixture_member("dev-a");
        repo.save(&member).await.unwrap();

        let loaded = repo.get(&member.device_id).await.unwrap().unwrap();
        assert_eq!(loaded.device_id, member.device_id);
        assert_eq!(loaded.device_name, member.device_name);
        assert_eq!(loaded.identity_fingerprint, member.identity_fingerprint);
        assert_eq!(loaded.sync_preferences, member.sync_preferences);
    }

    #[tokio::test]
    async fn get_missing_returns_none() {
        let (repo, _tempdir) = make_repo();
        let result = repo.get(&DeviceId::new("missing")).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn save_is_upsert() {
        let (repo, _tempdir) = make_repo();
        let mut member = fixture_member("dev-b");
        repo.save(&member).await.unwrap();

        member.device_name = "renamed".to_string();
        repo.save(&member).await.unwrap();

        let loaded = repo.get(&member.device_id).await.unwrap().unwrap();
        assert_eq!(loaded.device_name, "renamed");
    }

    #[tokio::test]
    async fn list_returns_all_saved() {
        let (repo, _tempdir) = make_repo();
        repo.save(&fixture_member("a")).await.unwrap();
        repo.save(&fixture_member("b")).await.unwrap();
        repo.save(&fixture_member("c")).await.unwrap();

        let mut members = repo.list().await.unwrap();
        members.sort_by(|x, y| x.device_id.as_str().cmp(y.device_id.as_str()));
        assert_eq!(members.len(), 3);
        assert_eq!(members[0].device_id.as_str(), "a");
        assert_eq!(members[2].device_id.as_str(), "c");
    }

    #[tokio::test]
    async fn remove_returns_true_when_present_false_when_absent() {
        let (repo, _tempdir) = make_repo();
        let member = fixture_member("dev-c");
        repo.save(&member).await.unwrap();

        let first = repo.remove(&member.device_id).await.unwrap();
        let second = repo.remove(&member.device_id).await.unwrap();
        assert!(first);
        assert!(!second);
        assert!(repo.get(&member.device_id).await.unwrap().is_none());
    }

    // NOTE: 2026-04-18-000001_create_space_member 里从 `paired_device` 搬迁数据的
    // `migration_copies_trusted_paired_devices_with_default_preferences` 测试在
    // phase 4b PR-5 随 `DROP TABLE paired_device` 一并删除 —— 迁移本身在 Phase 1
    // 已在生产库落地执行过，`paired_device` 表此后被 2026-04-20 迁移移除，fresh
    // DB 下该测试的前置插入语句无表可写。历史行为由 Phase 1 commit 5f5c6f4c 验证。
}
