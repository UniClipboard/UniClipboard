use std::sync::Arc;

use diesel::r2d2::{ConnectionManager, Pool};
use diesel::SqliteConnection;

use crate::db::ports::DbExecutor;

pub struct DieselSqliteExecutor {
    pool: Arc<Pool<ConnectionManager<SqliteConnection>>>,
}

impl DieselSqliteExecutor {
    pub fn new(pool: Pool<ConnectionManager<SqliteConnection>>) -> Self {
        Self {
            pool: Arc::new(pool),
        }
    }
}

impl DbExecutor for DieselSqliteExecutor {
    fn run<T>(
        &self,
        f: impl FnOnce(&mut SqliteConnection) -> anyhow::Result<T>,
    ) -> anyhow::Result<T> {
        let mut conn = self.pool.get()?;
        f(&mut conn)
    }
}
