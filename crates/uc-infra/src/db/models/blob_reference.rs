use diesel::prelude::*;

use crate::db::schema::blob_reference;

#[derive(Debug, Queryable)]
#[diesel(table_name = blob_reference)]
pub struct BlobReferenceRow {
    pub plaintext_hash: String,
    pub digest: String,
    pub created_at: i64,
}

#[derive(Debug, Insertable, AsChangeset)]
#[diesel(table_name = blob_reference)]
pub struct NewBlobReferenceRow {
    pub plaintext_hash: String,
    pub digest: String,
    pub created_at: i64,
}
