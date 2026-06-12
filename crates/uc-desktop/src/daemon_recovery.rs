//! Daemon recovery orchestration for desktop hosts.

use async_trait::async_trait;
use uc_daemon_client::{DaemonConnectionState, DaemonQueryClient};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnlockRecoveryOutcome {
    Unlocked,
    Unavailable,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DaemonRecoveryReport {
    pub unlock: UnlockRecoveryOutcome,
    pub lifecycle_ready: bool,
}

#[async_trait]
trait DaemonRecoveryClient: Send + Sync {
    async fn unlock_encryption(&self) -> anyhow::Result<bool>;
    async fn lifecycle_retry(&self) -> anyhow::Result<()>;
}

#[async_trait]
impl DaemonRecoveryClient for DaemonQueryClient {
    async fn unlock_encryption(&self) -> anyhow::Result<bool> {
        DaemonQueryClient::unlock_encryption(self).await
    }

    async fn lifecycle_retry(&self) -> anyhow::Result<()> {
        DaemonQueryClient::lifecycle_retry(self).await
    }
}

pub async fn recover_after_restart(
    connection_state: DaemonConnectionState,
) -> DaemonRecoveryReport {
    let client = DaemonQueryClient::new(connection_state);
    recover_after_restart_with(&client).await
}

async fn recover_after_restart_with(
    client: &(impl DaemonRecoveryClient + ?Sized),
) -> DaemonRecoveryReport {
    let unlock = match client.unlock_encryption().await {
        Ok(true) => {
            tracing::info!("encryption re-unlocked after daemon restart");
            UnlockRecoveryOutcome::Unlocked
        }
        Ok(false) => {
            tracing::info!("encryption not initialized or keyring miss after restart");
            UnlockRecoveryOutcome::Unavailable
        }
        Err(error) => {
            tracing::warn!(error = %error, "post-restart keyring unlock failed");
            UnlockRecoveryOutcome::Failed
        }
    };

    let lifecycle_ready = match client.lifecycle_retry().await {
        Ok(()) => {
            tracing::info!("post-restart lifecycle boot completed");
            true
        }
        Err(error) => {
            tracing::warn!(error = %error, "post-restart lifecycle retry failed");
            false
        }
    };

    DaemonRecoveryReport {
        unlock,
        lifecycle_ready,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use anyhow::anyhow;

    use super::*;

    #[derive(Clone)]
    struct RecordingClient {
        unlock_result: Result<bool, String>,
        calls: Arc<Mutex<Vec<&'static str>>>,
    }

    #[async_trait]
    impl DaemonRecoveryClient for RecordingClient {
        async fn unlock_encryption(&self) -> anyhow::Result<bool> {
            self.calls.lock().unwrap().push("unlock");
            self.unlock_result
                .as_ref()
                .map(|value| *value)
                .map_err(|error| anyhow!(error.clone()))
        }

        async fn lifecycle_retry(&self) -> anyhow::Result<()> {
            self.calls.lock().unwrap().push("lifecycle_retry");
            Ok(())
        }
    }

    #[tokio::test]
    async fn lifecycle_retry_runs_when_unlock_returns_false() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let client = RecordingClient {
            unlock_result: Ok(false),
            calls: calls.clone(),
        };

        recover_after_restart_with(&client).await;

        assert_eq!(
            calls.lock().unwrap().as_slice(),
            ["unlock", "lifecycle_retry"]
        );
    }

    #[tokio::test]
    async fn lifecycle_retry_runs_when_unlock_fails() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let client = RecordingClient {
            unlock_result: Err("keyring unavailable".to_string()),
            calls: calls.clone(),
        };

        recover_after_restart_with(&client).await;

        assert_eq!(
            calls.lock().unwrap().as_slice(),
            ["unlock", "lifecycle_retry"]
        );
    }
}
