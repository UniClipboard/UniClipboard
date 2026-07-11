use crate::db::schema::entry_file_set;
use diesel::prelude::*;

// Field order MUST match the `table!` column order in `schema.rs` (and thus the
// physical DDL order the encrypt migration produces, added columns last):
// `#[derive(Queryable)]` maps positionally, so a reorder silently mismaps
// columns. Keep both in sync with `diesel print-schema`.
#[derive(Queryable)]
#[diesel(table_name = entry_file_set)]
pub struct EntryFileSetRow {
    pub entry_id: String,
    pub line_index: i64,
    pub kind: String,
    pub content_hash: Option<String>,
    pub blob_id: Option<String>,
    pub size_bytes: Option<i64>,
    pub exclude_reason: Option<String>,
    /// AEAD-sealed `original_text` (UCFS envelope). NULL only on rows written by
    /// an older schema (none survive the phase-3 encrypt migration, which clears
    /// the table); a NULL here is treated as a decode failure.
    pub original_text_ct: Option<Vec<u8>>,
    /// Directory-member root position. NULL for rows written before phase 3.
    pub root_index: Option<i64>,
    /// AEAD-sealed `relative_path` (UCFS envelope). NULL for legacy rows.
    pub relative_path_ct: Option<Vec<u8>>,
    /// Member kind tag (`f`/`x`/`d`); classification enum, kept plaintext. NULL
    /// for legacy rows.
    pub kind_tag: Option<String>,
    /// AEAD-sealed selected root basename. NULL only for legacy rows.
    pub root_name_ct: Option<Vec<u8>>,
}

#[derive(Insertable)]
#[diesel(table_name = entry_file_set)]
pub struct NewEntryFileSetRow {
    pub entry_id: String,
    pub line_index: i64,
    pub kind: String,
    pub content_hash: Option<String>,
    pub blob_id: Option<String>,
    pub size_bytes: Option<i64>,
    pub exclude_reason: Option<String>,
    pub original_text_ct: Option<Vec<u8>>,
    pub root_index: Option<i64>,
    pub relative_path_ct: Option<Vec<u8>>,
    pub kind_tag: Option<String>,
    pub root_name_ct: Option<Vec<u8>>,
}
