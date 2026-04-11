//! `uc-infra::search` — persistence foundation for local encrypted search.
//!
//! This module owns:
//! - `constants`: authoritative `CURRENT_INDEX_VERSION` and field-mask bit positions.
//! - `rows`: adapter-owned Diesel row types with `profile_id` and domain conversion helpers.
//!
//! Profile scoping (`profile_id`) is a persistence concern owned here.
//! It is NOT added to `uc-core` search domain structs.

pub mod constants;
pub mod pipeline;
pub mod rows;
pub mod search_key_derivation;
pub mod sqlite_index;
pub mod text_extractor;
pub mod tokenizer;

#[cfg(test)]
pub mod test_support;

pub use constants::*;
pub use pipeline::*;
pub use rows::*;
pub use search_key_derivation::*;
pub use sqlite_index::*;
pub use text_extractor::*;
pub use tokenizer::*;

#[cfg(test)]
mod migration_tests {
    use crate::db::pool::init_db_pool;
    use diesel::RunQueryDsl;
    use tempfile::NamedTempFile;

    /// Helper: column names returned by pragma_table_info for a given table.
    fn get_column_names(conn: &mut diesel::SqliteConnection, table: &str) -> Vec<String> {
        use diesel::sql_types::Text;
        use diesel::QueryableByName;

        #[derive(QueryableByName)]
        struct Col {
            #[diesel(sql_type = Text)]
            name: String,
        }

        let sql = format!("SELECT name FROM pragma_table_info('{table}')");
        diesel::sql_query(sql)
            .load::<Col>(conn)
            .unwrap_or_default()
            .into_iter()
            .map(|c| c.name)
            .collect()
    }

    #[test]
    fn migration_creates_search_document_with_profile_id_and_index_version() {
        let tmp = NamedTempFile::new().expect("temp file");
        let path = tmp.path().to_string_lossy().to_string();
        let pool = init_db_pool(&path).expect("pool init");
        let mut conn = pool.get().expect("conn");

        // Explicitly verify search_document columns via pragma_table_info('search_document').
        let cols = get_column_names(&mut conn, "search_document");
        assert!(
            cols.contains(&"profile_id".to_string()),
            "search_document must have profile_id, found: {:?}",
            cols
        );
        assert!(
            cols.contains(&"index_version".to_string()),
            "search_document must have index_version, found: {:?}",
            cols
        );
        // Hard-delete semantic: no soft-delete timestamp on search_document
        let forbidden = "deleted_at_ms";
        assert!(
            !cols.contains(&forbidden.to_string()),
            "search_document must NOT have {forbidden}, found: {:?}",
            cols
        );
    }

    #[test]
    fn migration_creates_search_posting_with_profile_id() {
        let tmp = NamedTempFile::new().expect("temp file");
        let path = tmp.path().to_string_lossy().to_string();
        let pool = init_db_pool(&path).expect("pool init");
        let mut conn = pool.get().expect("conn");

        let cols = get_column_names(&mut conn, "search_posting");
        assert!(
            cols.contains(&"profile_id".to_string()),
            "search_posting must have profile_id, found: {:?}",
            cols
        );
        assert!(
            cols.contains(&"term_tag".to_string()),
            "search_posting must have term_tag, found: {:?}",
            cols
        );
    }

    #[test]
    fn migration_creates_search_index_meta_with_profile_id_and_index_version() {
        let tmp = NamedTempFile::new().expect("temp file");
        let path = tmp.path().to_string_lossy().to_string();
        let pool = init_db_pool(&path).expect("pool init");
        let mut conn = pool.get().expect("conn");

        let cols = get_column_names(&mut conn, "search_index_meta");
        assert!(
            cols.contains(&"profile_id".to_string()),
            "search_index_meta must have profile_id, found: {:?}",
            cols
        );
        assert!(
            cols.contains(&"index_version".to_string()),
            "search_index_meta must have index_version, found: {:?}",
            cols
        );
    }

    #[test]
    fn migration_search_document_has_expected_columns() {
        let tmp = NamedTempFile::new().expect("temp file");
        let path = tmp.path().to_string_lossy().to_string();
        let pool = init_db_pool(&path).expect("pool init");
        let mut conn = pool.get().expect("conn");

        let cols = get_column_names(&mut conn, "search_document");
        let expected = [
            "profile_id",
            "entry_id",
            "event_id",
            "active_time_ms",
            "captured_at_ms",
            "file_type",
            "file_extensions",
            "mime_type",
            "indexed_at_ms",
            "index_version",
            "text_preview",
        ];
        for col in expected {
            assert!(
                cols.contains(&col.to_string()),
                "search_document missing column '{col}', found: {:?}",
                cols
            );
        }
    }
}
