//! Clipboard selection repository implementation
//! 剪贴板选择仓库实现

use crate::db::mappers::clipboard_selection_mapper::ClipboardSelectionRowMapper;
use crate::db::models::clipboard_selection::ClipboardSelectionRow;
use crate::db::ports::{DbExecutor, RowMapper};
use crate::db::schema::clipboard_selection;
use anyhow::Result;
use async_trait::async_trait;
use diesel::{ExpressionMethods, OptionalExtension, QueryDsl, RunQueryDsl};

use uc_core::clipboard::ClipboardSelectionDecision;
use uc_core::ids::EntryId;
use uc_core::ports::clipboard::ClipboardSelectionRepositoryPort;

/// In-memory clipboard selection repository (placeholder)
///
/// NOTE: This is a test helper implementation that returns None for all queries.
/// Use DieselClipboardSelectionRepository for production code with actual database queries.
///
/// 注意：这是测试辅助实现，对所有查询返回 None。
/// 生产代码请使用 DieselClipboardSelectionRepository 进行实际数据库查询。
pub struct InMemoryClipboardSelectionRepository;

impl InMemoryClipboardSelectionRepository {
    pub fn new() -> Self {
        Self
    }
}

impl Default for InMemoryClipboardSelectionRepository {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ClipboardSelectionRepositoryPort for InMemoryClipboardSelectionRepository {
    /// In-memory placeholder that always indicates no clipboard selection for any entry.
    ///
    /// This implementation is used for tests and never stores or retrieves selections.
    ///
    /// # Examples
    ///
    /// ```
    /// # use uc_infra::db::repositories::InMemoryClipboardSelectionRepository;
    /// # use uc_core::clipboard::ClipboardSelectionDecision;
    /// # use uc_core::ids::EntryId;
    /// # use uc_core::ports::clipboard::ClipboardSelectionRepositoryPort;
    /// # async fn run_example() -> anyhow::Result<()> {
    /// let repo = InMemoryClipboardSelectionRepository::new();
    /// let entry_id = EntryId::from("test-entry".to_string());
    /// let selection: Option<ClipboardSelectionDecision> =
    ///     repo.get_selection(&entry_id).await?;
    /// assert!(selection.is_none());
    /// # Ok(())
    /// # }
    /// ```
    async fn get_selection(
        &self,
        _entry_id: &EntryId,
    ) -> Result<Option<ClipboardSelectionDecision>> {
        // Placeholder implementation - always return None
        // 占位符实现 - 始终返回 None
        Ok(None)
    }

    /// No-op deletion for the in-memory placeholder repository.
    ///
    /// This implementation performs no action and always succeeds.
    ///
    /// # Examples
    ///
    /// ```
    /// # use uc_infra::db::repositories::InMemoryClipboardSelectionRepository;
    /// # use uc_core::ids::EntryId;
    /// # use uc_core::ports::clipboard::ClipboardSelectionRepositoryPort;
    /// # async fn run_example() -> anyhow::Result<()> {
    /// let repo = InMemoryClipboardSelectionRepository::new();
    /// let entry_id = EntryId::from("test-entry".to_string());
    /// // Succeeds and leaves the in-memory repository unchanged.
    /// repo.delete_selection(&entry_id).await?;
    /// # Ok(())
    /// # }
    /// ```
    async fn delete_selection(&self, _entry_id: &EntryId) -> Result<()> {
        // Placeholder implementation - no-op
        // 占位符实现 - 无操作
        Ok(())
    }
}

/// Diesel-based clipboard selection repository
///
/// Implements ClipboardSelectionRepositoryPort using SQLite database through Diesel ORM.
///
/// Diesel 实现的剪贴板选择仓库
/// 使用 Diesel ORM 通过 SQLite 数据库实现 ClipboardSelectionRepositoryPort。
pub struct DieselClipboardSelectionRepository<E>
where
    E: DbExecutor,
{
    executor: E,
}

impl<E> DieselClipboardSelectionRepository<E>
where
    E: DbExecutor,
{
    pub fn new(executor: E) -> Self {
        Self { executor }
    }
}

#[async_trait]
impl<E> ClipboardSelectionRepositoryPort for DieselClipboardSelectionRepository<E>
where
    E: DbExecutor,
{
    /// Fetches the clipboard selection decision for the specified entry id, if one exists.
    ///
    /// # Returns
    /// `Some(ClipboardSelectionDecision)` when a selection is found for the given `EntryId`, `None` otherwise.
    ///
    /// # Examples
    ///
    /// ```
    /// # use uc_core::ids::EntryId;
    /// # use uc_core::ports::clipboard::ClipboardSelectionRepositoryPort;
    /// # async fn _example(
    /// #     repo: &impl ClipboardSelectionRepositoryPort,
    /// #     entry_id: EntryId,
    /// # ) -> anyhow::Result<()> {
    /// let result = repo.get_selection(&entry_id).await?;
    /// // `result` is `Some(...)` when a selection exists, otherwise `None`.
    /// # Ok(())
    /// # }
    /// ```
    async fn get_selection(
        &self,
        entry_id: &EntryId,
    ) -> Result<Option<ClipboardSelectionDecision>> {
        let entry_id_str = entry_id.to_string();

        let row: Option<ClipboardSelectionRow> = self
            .executor
            .run(|conn| {
                Ok(clipboard_selection::table
                    .filter(clipboard_selection::entry_id.eq(&entry_id_str))
                    .first::<ClipboardSelectionRow>(conn)
                    .optional()?)
            })
            .map_err(|e| {
                tracing::error!(
                    "Failed to query clipboard_selection for entry_id '{}': {}",
                    entry_id_str,
                    e
                );
                e
            })?;

        match row {
            Some(r) => {
                let mapper = ClipboardSelectionRowMapper;
                let decision = mapper.to_domain(&r)?;
                Ok(Some(decision))
            }
            None => Ok(None),
        }
    }

    /// Deletes the clipboard selection record associated with the given entry ID from the database.
    ///
    /// # Returns
    ///
    /// `Ok(())` if the deletion succeeds, or an error if the underlying database operation fails.
    ///
    /// # Examples
    ///
    /// ```
    /// # use uc_core::ids::EntryId;
    /// # use uc_core::ports::clipboard::ClipboardSelectionRepositoryPort;
    /// # async fn example(
    /// #     repo: &impl ClipboardSelectionRepositoryPort,
    /// #     entry_id: EntryId,
    /// # ) -> anyhow::Result<()> {
    /// repo.delete_selection(&entry_id).await?;
    /// # Ok(())
    /// # }
    /// ```
    async fn delete_selection(&self, entry_id: &EntryId) -> Result<()> {
        let entry_id_str = entry_id.to_string();
        self.executor.run(|conn| {
            diesel::delete(clipboard_selection::table)
                .filter(clipboard_selection::entry_id.eq(&entry_id_str))
                .execute(conn)?;
            Ok(())
        })
    }
}
