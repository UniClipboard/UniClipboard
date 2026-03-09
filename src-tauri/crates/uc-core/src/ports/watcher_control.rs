use async_trait::async_trait;

/// Port for controlling the clipboard watcher lifecycle.
///
/// 剪贴板监控器生命周期控制端口。
///
/// # Behavior / 行为
/// - `start_watcher()` should be idempotent.
/// - `stop_watcher()` should be idempotent.
///
/// - `start_watcher()` 应当具备幂等性。
/// - `stop_watcher()` 应当具备幂等性。
#[async_trait]
pub trait WatcherControlPort: Send + Sync {
    /// Request the clipboard watcher to start.
    ///
    /// 请求启动剪贴板监控器。
    async fn start_watcher(&self) -> Result<(), WatcherControlError>;

    /// Request the clipboard watcher to stop.
    ///
    /// 请求停止剪贴板监控器。
    async fn stop_watcher(&self) -> Result<(), WatcherControlError>;
}

#[derive(Debug, thiserror::Error)]
pub enum WatcherControlError {
    #[error("Failed to send start command: {0}")]
    StartFailed(String),

    #[error("Failed to send stop command: {0}")]
    StopFailed(String),

    #[error("Watcher channel closed")]
    ChannelClosed,
}
