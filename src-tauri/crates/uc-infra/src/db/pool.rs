use anyhow::Result;
use diesel::r2d2::{ConnectionManager, Pool};
use diesel::sqlite::SqliteConnection;
use diesel_migrations::{embed_migrations, EmbeddedMigrations, MigrationHarness};
use tracing::info;

/// Embed all diesel migrations at compile time
pub const MIGRATIONS: EmbeddedMigrations = embed_migrations!("migrations");

/// Type alias for SQLite connection pool
pub type DbPool = Pool<ConnectionManager<SqliteConnection>>;

/// Initialize the database connection pool and apply embedded migrations.
///
/// This must be called once at application startup. On success it returns a ready-to-use
/// `DbPool` with all pending Diesel migrations applied.
///
/// # Errors
///
/// Returns an `anyhow::Error` if creating the connection pool, obtaining a connection from
/// the pool, or applying migrations fails.
///
/// # Examples
///
/// ```no_run
/// # use uc_infra::db::pool::init_db_pool;
/// let pool = init_db_pool(":memory:").expect("failed to initialize DB pool");
/// // use `pool` to acquire connections: let conn = pool.get().unwrap();
/// ```
pub fn init_db_pool(database_url: &str) -> Result<DbPool> {
    let manager = ConnectionManager::<SqliteConnection>::new(database_url);

    let pool = Pool::builder()
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
