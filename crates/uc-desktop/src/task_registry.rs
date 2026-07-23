use std::time::Duration;

use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

pub struct TaskRegistry {
    token: CancellationToken,
    tasks: tokio::sync::Mutex<JoinSet<()>>,
}

impl TaskRegistry {
    pub fn new() -> Self {
        Self {
            token: CancellationToken::new(),
            tasks: tokio::sync::Mutex::new(JoinSet::new()),
        }
    }

    pub fn token(&self) -> &CancellationToken {
        &self.token
    }

    pub async fn spawn<F, Fut>(&self, name: &'static str, task: F)
    where
        F: FnOnce(CancellationToken) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = ()> + Send + 'static,
    {
        let token = self.token.child_token();
        self.tasks.lock().await.spawn(async move {
            task(token).await;
            debug!(task = name, "Task completed");
        });
    }

    pub async fn shutdown(&self, timeout: Duration) {
        self.token.cancel();
        let mut tasks = self.tasks.lock().await;
        let deadline = tokio::time::sleep(timeout);
        tokio::pin!(deadline);

        loop {
            tokio::select! {
                result = tasks.join_next() => match result {
                    Some(Ok(())) => {}
                    Some(Err(error)) => warn!(error = %error, "Task join error"),
                    None => {
                        info!("All desktop tasks joined cleanly");
                        return;
                    }
                },
                _ = &mut deadline => {
                    warn!(remaining = tasks.len(), "Desktop task shutdown timed out");
                    tasks.abort_all();
                    return;
                }
            }
        }
    }
}

impl Default for TaskRegistry {
    fn default() -> Self {
        Self::new()
    }
}
