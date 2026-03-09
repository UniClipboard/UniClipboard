use tokio::sync::mpsc;
use uc_core::ports::watcher_control::{WatcherControlError, WatcherControlPort};

use crate::ipc::PlatformCommand;

/// In-memory watcher control implementation.
///
/// 内存版的剪贴板监控器控制实现。
///
/// This adapter sends watcher lifecycle commands through an existing in-process channel.
///
/// 此适配器通过进程内的 channel 发送监控器生命周期命令。
pub struct InMemoryWatcherControl {
    cmd_tx: mpsc::Sender<PlatformCommand>,
}

impl InMemoryWatcherControl {
    pub fn new(cmd_tx: mpsc::Sender<PlatformCommand>) -> Self {
        Self { cmd_tx }
    }

    fn map_send_error(&self, err: mpsc::error::SendError<PlatformCommand>) -> WatcherControlError {
        if self.cmd_tx.is_closed() {
            WatcherControlError::ChannelClosed
        } else {
            WatcherControlError::StartFailed(err.to_string())
        }
    }
}

#[async_trait::async_trait]
impl WatcherControlPort for InMemoryWatcherControl {
    async fn start_watcher(&self) -> Result<(), WatcherControlError> {
        self.cmd_tx
            .send(PlatformCommand::StartClipboardWatcher)
            .await
            .map_err(|e| self.map_send_error(e))?;

        Ok(())
    }

    async fn stop_watcher(&self) -> Result<(), WatcherControlError> {
        self.cmd_tx
            .send(PlatformCommand::StopClipboardWatcher)
            .await
            .map_err(|e| {
                if self.cmd_tx.is_closed() {
                    WatcherControlError::ChannelClosed
                } else {
                    WatcherControlError::StopFailed(e.to_string())
                }
            })?;

        Ok(())
    }
}
