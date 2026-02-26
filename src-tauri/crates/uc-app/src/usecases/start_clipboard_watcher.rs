//! Use case for starting the clipboard watcher
//! 启动剪贴板监控器的用例

use crate as uc_app;
use tracing::{info, info_span, Instrument};
use uc_app::usecases::clipboard::ClipboardIntegrationMode;
use uc_core::ports::WatcherControlPort;

/// Error type for clipboard watcher startup failures.
/// 剪贴板监控器启动失败的错误类型。
#[derive(Debug, thiserror::Error)]
pub enum StartClipboardWatcherError {
    #[error("Failed to start clipboard watcher: {0}")]
    StartFailed(String),
}

impl From<uc_core::ports::WatcherControlError> for StartClipboardWatcherError {
    fn from(err: uc_core::ports::WatcherControlError) -> Self {
        StartClipboardWatcherError::StartFailed(err.to_string())
    }
}

/// Use case for starting the clipboard watcher.
///
/// ## Behavior / 行为
/// - Requests the clipboard watcher to start through the WatcherControlPort
/// - The watcher should be idempotent - calling start multiple times is safe
///
/// ## English
/// Requests the clipboard watcher to start by calling the WatcherControlPort.
/// This operation is idempotent - starting an already-running watcher is safe.
pub struct StartClipboardWatcher {
    watcher_control: std::sync::Arc<dyn WatcherControlPort>,
    mode: ClipboardIntegrationMode,
}

impl StartClipboardWatcher {
    /// Create a new StartClipboardWatcher use case.
    pub fn new(
        watcher_control: std::sync::Arc<dyn WatcherControlPort>,
        mode: ClipboardIntegrationMode,
    ) -> Self {
        Self {
            watcher_control,
            mode,
        }
    }

    /// Create a new StartClipboardWatcher use case from an Arc port.
    ///
    /// This is a convenience method for the UseCases accessor pattern.
    pub fn from_port(
        watcher_control: std::sync::Arc<dyn WatcherControlPort>,
        mode: ClipboardIntegrationMode,
    ) -> Self {
        Self::new(watcher_control, mode)
    }

    /// Execute the use case.
    ///
    /// # Returns / 返回值
    /// - `Ok(())` if the watcher was started successfully
    /// - `Err(StartClipboardWatcherError)` if starting the watcher failed
    pub async fn execute(&self) -> Result<(), StartClipboardWatcherError> {
        let span = info_span!("usecase.start_clipboard_watcher.execute");

        async {
            if !self.mode.observe_os_clipboard() {
                info!("Clipboard watcher disabled by integration mode (passive)");
                return Ok(());
            }

            info!("Requesting clipboard watcher to start");

            self.watcher_control.start_watcher().await?;

            info!("Clipboard watcher started successfully");
            Ok(())
        }
        .instrument(span)
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::Arc;
    use uc_core::ports::WatcherControlError;

    /// Mock WatcherControlPort
    struct MockWatcherControl {
        started: Arc<std::sync::atomic::AtomicBool>,
        should_fail: bool,
    }

    impl MockWatcherControl {
        fn new() -> Self {
            Self {
                started: Arc::new(std::sync::atomic::AtomicBool::new(false)),
                should_fail: false,
            }
        }

        fn fail_on_start() -> Self {
            Self {
                started: Arc::new(std::sync::atomic::AtomicBool::new(false)),
                should_fail: true,
            }
        }

        fn was_started(&self) -> bool {
            self.started.load(std::sync::atomic::Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl WatcherControlPort for MockWatcherControl {
        async fn start_watcher(&self) -> Result<(), WatcherControlError> {
            if self.should_fail {
                return Err(WatcherControlError::StartFailed("mock failure".to_string()));
            }
            self.started
                .store(true, std::sync::atomic::Ordering::SeqCst);
            Ok(())
        }

        async fn stop_watcher(&self) -> Result<(), WatcherControlError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_start_clipboard_watcher_succeeds() {
        let watcher = Arc::new(MockWatcherControl::new());
        let use_case = StartClipboardWatcher::new(watcher.clone(), ClipboardIntegrationMode::Full);

        let result = use_case.execute().await;

        assert!(result.is_ok(), "start_watcher should succeed");
        assert!(watcher.was_started(), "watcher should be started");
    }

    #[tokio::test]
    async fn test_start_clipboard_watcher_propagates_error() {
        let watcher = Arc::new(MockWatcherControl::fail_on_start());
        let use_case = StartClipboardWatcher::new(watcher, ClipboardIntegrationMode::Full);

        let result = use_case.execute().await;

        assert!(result.is_err(), "start_watcher should fail");
        let err = result.unwrap_err();
        assert!(matches!(err, StartClipboardWatcherError::StartFailed(_)));
    }

    #[tokio::test]
    async fn test_from_port_creates_use_case() {
        let watcher = Arc::new(MockWatcherControl::new());
        let use_case =
            StartClipboardWatcher::from_port(watcher.clone(), ClipboardIntegrationMode::Full);

        let result = use_case.execute().await;

        assert!(result.is_ok(), "use_case created from_port should work");
    }

    #[tokio::test]
    async fn test_start_clipboard_watcher_is_noop_in_passive_mode() {
        let watcher = Arc::new(MockWatcherControl::new());
        let use_case =
            StartClipboardWatcher::new(watcher.clone(), ClipboardIntegrationMode::Passive);

        let result = use_case.execute().await;

        assert!(result.is_ok(), "passive mode should skip watcher startup");
        assert!(!watcher.was_started(), "watcher should not be started");
    }
}
