use tokio::sync::mpsc;
use uc_core::ports::watcher_control::{WatcherControlError, WatcherControlPort};

use uc_platform::adapters::InMemoryWatcherControl;
use uc_platform::ipc::PlatformCommand;

#[tokio::test]
async fn test_start_watcher_sends_command() {
    let (cmd_tx, mut cmd_rx) = mpsc::channel(10);
    let control = InMemoryWatcherControl::new(cmd_tx);

    control.start_watcher().await.unwrap();

    let received = cmd_rx.recv().await.unwrap();
    assert!(matches!(received, PlatformCommand::StartClipboardWatcher));
}

#[tokio::test]
async fn test_stop_watcher_sends_command() {
    let (cmd_tx, mut cmd_rx) = mpsc::channel(10);
    let control = InMemoryWatcherControl::new(cmd_tx);

    control.stop_watcher().await.unwrap();

    let received = cmd_rx.recv().await.unwrap();
    assert!(matches!(received, PlatformCommand::StopClipboardWatcher));
}

#[tokio::test]
async fn test_start_watcher_channel_closed() {
    let (cmd_tx, cmd_rx) = mpsc::channel(1);
    drop(cmd_rx);

    let control = InMemoryWatcherControl::new(cmd_tx);

    let result = control.start_watcher().await;
    assert!(matches!(result, Err(WatcherControlError::ChannelClosed)));
}
