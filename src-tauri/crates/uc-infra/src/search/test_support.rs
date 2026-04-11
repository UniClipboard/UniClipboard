//! Test support helpers for search adapter integration tests.
//!
//! Provides fixtures and helpers for rebuild tests that need temp-file SQLite
//! databases and concurrent-read simulation.
//!
//! This module is gated to `#[cfg(test)]` and is not compiled into production builds.

use std::path::Path;

use diesel::{Connection, RunQueryDsl, SqliteConnection};

/// A held read transaction on a SQLite database.
///
/// Opens a dedicated connection (not from the pool) and begins `BEGIN`.
/// The transaction is held alive until this handle is dropped, at which point
/// `ROLLBACK` is issued automatically via `Drop`.
///
/// Used to simulate concurrent reader load during rebuild cutover tests.
pub struct ReadTxnHandle {
    conn: SqliteConnection,
}

impl ReadTxnHandle {
    fn new(conn: SqliteConnection) -> Self {
        Self { conn }
    }
}

impl Drop for ReadTxnHandle {
    fn drop(&mut self) {
        // Best-effort rollback. Swallow any error on cleanup.
        let _ = diesel::sql_query("ROLLBACK").execute(&mut self.conn);
    }
}

/// Open a dedicated SQLite connection to `db_path`, begin a read transaction,
/// and return a handle that keeps the transaction alive until dropped.
///
/// The opened connection is independent of the adapter's connection pool,
/// ensuring it holds a real WAL read lock while the adapter tries to finalize
/// the rebuild.
///
/// # Panics
///
/// Panics if the connection cannot be established or `BEGIN` fails.
pub fn hold_read_transaction(db_path: &Path) -> ReadTxnHandle {
    let url = db_path.to_string_lossy();
    let mut conn =
        SqliteConnection::establish(&url).expect("hold_read_transaction: establish failed");

    diesel::sql_query("BEGIN")
        .execute(&mut conn)
        .expect("hold_read_transaction: BEGIN failed");

    // Perform a minimal read to escalate the lock from deferred to read.
    let _ = diesel::sql_query("SELECT 1").execute(&mut conn);

    ReadTxnHandle::new(conn)
}
