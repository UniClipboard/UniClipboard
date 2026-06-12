use diesel::SqliteConnection;
use std::sync::Arc;

pub trait DbExecutor: Send + Sync {
    fn run<T>(
        &self,
        f: impl FnOnce(&mut SqliteConnection) -> anyhow::Result<T>,
    ) -> anyhow::Result<T>;
}

// Implement DbExecutor for Arc<T> where T: DbExecutor
// This allows sharing the executor across multiple repositories
impl<T: DbExecutor> DbExecutor for Arc<T> {
    fn run<U>(
        &self,
        f: impl FnOnce(&mut SqliteConnection) -> anyhow::Result<U>,
    ) -> anyhow::Result<U> {
        self.as_ref().run(f)
    }
}
