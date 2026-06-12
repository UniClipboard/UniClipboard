use crate::db::mappers::thumbnail_mapper::ThumbnailRowMapper;
use crate::db::models::clipboard_representation_thumbnail::ClipboardRepresentationThumbnailRow;
use crate::db::ports::{DbExecutor, InsertMapper, RowMapper};
use crate::db::schema::clipboard_representation_thumbnail;
use anyhow::Result;
use diesel::{ExpressionMethods, OptionalExtension, QueryDsl, RunQueryDsl};
use uc_core::clipboard::ThumbnailMetadata;
use uc_core::ids::RepresentationId;
use uc_core::ports::clipboard::ThumbnailRepositoryPort;

pub struct DieselThumbnailRepository<E>
where
    E: DbExecutor,
{
    executor: E,
}

impl<E> DieselThumbnailRepository<E>
where
    E: DbExecutor,
{
    pub fn new(executor: E) -> Self {
        Self { executor }
    }
}

#[async_trait::async_trait]
impl<E> ThumbnailRepositoryPort for DieselThumbnailRepository<E>
where
    E: DbExecutor,
{
    async fn get_by_representation_id(
        &self,
        representation_id: &RepresentationId,
    ) -> Result<Option<ThumbnailMetadata>> {
        let rep_id_str = representation_id.to_string();
        let row: Option<ClipboardRepresentationThumbnailRow> = self.executor.run(|conn| {
            let result: Result<Option<ClipboardRepresentationThumbnailRow>, diesel::result::Error> =
                clipboard_representation_thumbnail::table
                    .filter(clipboard_representation_thumbnail::representation_id.eq(&rep_id_str))
                    .first::<ClipboardRepresentationThumbnailRow>(conn)
                    .optional();
            result.map_err(|e| anyhow::anyhow!("Database error: {}", e))
        })?;

        match row {
            Some(row) => {
                let mapper = ThumbnailRowMapper;
                let metadata = mapper.to_domain(&row)?;
                Ok(Some(metadata))
            }
            None => Ok(None),
        }
    }

    async fn insert_thumbnail(&self, metadata: &ThumbnailMetadata) -> Result<()> {
        let mapper = ThumbnailRowMapper;
        let new_row = mapper.to_row(metadata)?;
        self.executor.run(|conn| {
            diesel::insert_into(clipboard_representation_thumbnail::table)
                .values(&new_row)
                .execute(conn)?;
            Ok(())
        })
    }
}
