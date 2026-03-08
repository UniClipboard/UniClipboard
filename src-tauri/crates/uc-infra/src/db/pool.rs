use anyhow::Result;
use diesel::r2d2::{ConnectionManager, CustomizeConnection, Pool};
use diesel::sqlite::SqliteConnection;
use diesel::{Connection, RunQueryDsl};
use diesel_migrations::{embed_migrations, EmbeddedMigrations, MigrationHarness};
use tracing::{info, warn};

/// Embed all diesel migrations at compile time
pub const MIGRATIONS: EmbeddedMigrations = embed_migrations!("migrations");

/// Type alias for SQLite connection pool
pub type DbPool = Pool<ConnectionManager<SqliteConnection>>;

/// Connection customizer that sets per-connection SQLite pragmas on each new connection.
///
/// - `busy_timeout = 5000`: Tells SQLite to wait up to 5 seconds before returning
///   `SQLITE_BUSY`, giving concurrent writers time to finish instead of failing immediately.
/// - `foreign_keys = ON`: Enforces foreign-key constraints for data integrity.
///
/// Note: `journal_mode = WAL` is intentionally NOT set here. WAL mode is a database-file-level
/// setting that persists once set. It is configured once via [`enable_wal_mode`] before
/// pool creation, avoiding "database is locked" errors when r2d2 initializes multiple
/// connections concurrently.
#[derive(Debug)]
struct SqlitePragmaCustomizer;

impl CustomizeConnection<SqliteConnection, diesel::r2d2::Error> for SqlitePragmaCustomizer {
    fn on_acquire(
        &self,
        conn: &mut SqliteConnection,
    ) -> std::result::Result<(), diesel::r2d2::Error> {
        use diesel::r2d2::Error::QueryError;

        diesel::sql_query("PRAGMA busy_timeout = 5000")
            .execute(conn)
            .map_err(|e| {
                warn!(error = %e, "Failed to set busy_timeout");
                QueryError(e)
            })?;

        diesel::sql_query("PRAGMA foreign_keys = ON")
            .execute(conn)
            .map_err(|e| {
                warn!(error = %e, "Failed to set foreign_keys=ON");
                QueryError(e)
            })?;

        Ok(())
    }
}

/// Enable WAL journal mode on the database using a single dedicated connection.
///
/// WAL (Write-Ahead Logging) is a database-file-level setting that persists once set.
/// By setting it on a single connection before pool creation, we avoid "database is locked"
/// errors that occur when multiple r2d2 pool connections try to set it concurrently during
/// pool initialization.
///
/// # Errors
///
/// Returns an error if the connection cannot be established or the WAL pragma fails.
fn enable_wal_mode(database_url: &str) -> Result<()> {
    let mut conn = SqliteConnection::establish(database_url)
        .map_err(|e| anyhow::anyhow!("Failed to connect for WAL setup: {}", e))?;

    diesel::sql_query("PRAGMA journal_mode = WAL")
        .execute(&mut conn)
        .map_err(|e| anyhow::anyhow!("Failed to set journal_mode=WAL: {}", e))?;

    info!("WAL journal mode enabled");
    Ok(())
}

/// Initialize the database connection pool and apply embedded migrations.
///
/// This must be called once at application startup. On success it returns a ready-to-use
/// `DbPool` with all pending Diesel migrations applied.
///
/// WAL journal mode is set once on a dedicated connection before pool creation to avoid
/// lock contention. Each pool connection automatically gets a 5-second busy timeout and
/// foreign key enforcement via [`SqlitePragmaCustomizer`].
///
/// # Errors
///
/// Returns an `anyhow::Error` if enabling WAL mode, creating the connection pool,
/// obtaining a connection from the pool, or applying migrations fails.
///
/// # Examples
///
/// ```no_run
/// # use uc_infra::db::pool::init_db_pool;
/// let pool = init_db_pool(":memory:").expect("failed to initialize DB pool");
/// // use `pool` to acquire connections: let conn = pool.get().unwrap();
/// ```
pub fn init_db_pool(database_url: &str) -> Result<DbPool> {
    // Set WAL mode once on a single connection before pool creation.
    // WAL is a persistent database-file-level setting, so it only needs to be set once.
    // Doing it here avoids "database is locked" errors when r2d2 initializes multiple
    // connections concurrently, each trying to set WAL in on_acquire.
    enable_wal_mode(database_url)?;

    let manager = ConnectionManager::<SqliteConnection>::new(database_url);

    let pool = Pool::builder()
        .connection_customizer(Box::new(SqlitePragmaCustomizer))
        .build(manager)
        .map_err(|e| anyhow::anyhow!("Failed to create database pool: {}", e))?;

    run_migrations(&pool)?;

    Ok(pool)
}

/// Apply the embedded Diesel migrations using the supplied connection pool.
///
/// Obtains a connection from `pool` and runs all pending embedded migrations compiled into
/// `MIGRATIONS`. Logs progress and returns when migrations complete.
///
/// # Errors
///
/// Returns an error if acquiring a connection from the pool fails or if applying migrations fails.
fn run_migrations(pool: &DbPool) -> Result<()> {
    let mut conn = pool.get()?;

    info!("Running database migrations...");
    conn.run_pending_migrations(MIGRATIONS)
        .map_err(|e| anyhow::anyhow!("Migration failed: {}", e))?;
    info!("Database migrations completed");

    Ok(())
}
