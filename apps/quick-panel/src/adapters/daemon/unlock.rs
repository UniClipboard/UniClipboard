use std::time::Duration;

use quick_panel_core::ports::{Secret, UnlockFailure};
use uc_daemon_client::DaemonRequestError as Error;

use super::DaemonHistory;

/// Checking a passphrase derives the key, which takes a moment on purpose.
const UNLOCK_TIMEOUT: Duration = Duration::from_secs(20);

impl DaemonHistory {
    pub(super) async fn unlock_content_from_keyring(&self) -> Result<bool, UnlockFailure> {
        let operation = async {
            self.context()
                .map_err(|_| UnlockFailure::Disconnected)?
                .query_client()
                .unlock_content_from_keyring()
                .await
                .map(|status| status.unlocked)
                .map_err(unlock_failure)
        };
        // The user may need time to respond to the system authorization dialog.
        tokio::time::timeout(Duration::from_secs(60), operation)
            .await
            .map_err(|_| UnlockFailure::Timeout)?
    }

    pub(super) async fn unlock_content(&self, passphrase: Secret) -> Result<(), UnlockFailure> {
        let operation = async {
            let query = self
                .context()
                .map_err(|_| UnlockFailure::Disconnected)?
                .query_client();
            query
                .unlock_content(passphrase.expose())
                .await
                .map(|_| ())
                .map_err(unlock_failure)
        };
        tokio::time::timeout(UNLOCK_TIMEOUT, operation)
            .await
            .map_err(|_| UnlockFailure::Timeout)?
    }
}

fn unlock_failure(error: anyhow::Error) -> UnlockFailure {
    match error.downcast_ref::<Error>() {
        Some(Error::NotConnected | Error::Transport { .. } | Error::Auth { .. }) => {
            UnlockFailure::Disconnected
        }
        Some(Error::Status { status, .. }) => match status.as_u16() {
            403 => UnlockFailure::WrongPassphrase,
            409 => UnlockFailure::NotReady,
            422 => UnlockFailure::KeyUnreadable,
            _ => UnlockFailure::Failed,
        },
        _ => UnlockFailure::Failed,
    }
}
