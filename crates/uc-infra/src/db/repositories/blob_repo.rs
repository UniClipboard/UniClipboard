use crate::blob::{Blob, BlobRepositoryPort};
use crate::db::models::blob::NewBlobRow;
use crate::db::models::BlobRow;
use crate::db::ports::DbExecutor;
use crate::db::ports::{InsertMapper, RowMapper};
use crate::db::schema::blob;
use anyhow::Result;
use diesel::{ExpressionMethods, OptionalExtension, QueryDsl, RunQueryDsl};
use tracing::debug_span;
use uc_core::ContentHash;

pub struct DieselBlobRepository<E, IM, RM>
where
    E: DbExecutor,
    IM: InsertMapper<Blob, NewBlobRow>,
    RM: RowMapper<BlobRow, Blob>,
{
    executor: E,
    insert_mapper: IM,
    row_mapper: RM,
}

impl<E, IM, RM> DieselBlobRepository<E, IM, RM>
where
    E: DbExecutor,
    IM: InsertMapper<Blob, NewBlobRow>,
    RM: RowMapper<BlobRow, Blob>,
{
    pub fn new(executor: E, insert_mapper: IM, row_mapper: RM) -> Self {
        Self {
            executor,
            insert_mapper,
            row_mapper,
        }
    }
}

#[async_trait::async_trait]
impl<E, IM, RM> BlobRepositoryPort for DieselBlobRepository<E, IM, RM>
where
    E: DbExecutor,
    IM: InsertMapper<Blob, NewBlobRow>,
    RM: RowMapper<BlobRow, Blob>,
{
    async fn insert_blob(&self, blob_row: &Blob) -> Result<()> {
        let span = debug_span!(
            "infra.sqlite.insert_blob",
            table = "blob",
            blob_id = %blob_row.blob_id,
            size_bytes = blob_row.size_bytes,
        );
        span.in_scope(|| {
            let new_blob_row = self.insert_mapper.to_row(blob_row)?;
            self.executor.run(|conn| {
                diesel::insert_into(blob::table)
                    .values(&new_blob_row)
                    .execute(conn)?;
                Ok(())
            })
        })
    }

    async fn find_by_hash(&self, content_hash: &ContentHash) -> Result<Option<Blob>> {
        let span = debug_span!(
            "infra.sqlite.query_blob_by_hash",
            table = "blob",
            content_hash = %content_hash,
        );
        span.in_scope(|| {
            let blob_row: Option<BlobRow> = self.executor.run(|conn| {
                let result: Result<Option<BlobRow>, diesel::result::Error> = blob::table
                    .filter(blob::content_hash.eq(content_hash.to_string()))
                    .first::<BlobRow>(conn)
                    .optional();
                result.map_err(|e| anyhow::anyhow!("Database error: {}", e))
            })?;

            match blob_row {
                Some(row) => {
                    let blob = self.row_mapper.to_domain(&row)?;
                    Ok(Some(blob))
                }
                None => Ok(None),
            }
        })
    }
}
