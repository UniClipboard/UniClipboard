use std::sync::Arc;
use tracing::{debug, error, info, warn};

use super::event_bus::{PlatformCommandReceiver, PlatformEventReceiver, PlatformEventSender};
use crate::clipboard::watcher::ClipboardWatcher;
use crate::clipboard::LocalClipboard;
use crate::ipc::{PlatformCommand, PlatformEvent};
use crate::ports::PlatformCommandExecutorPort;
use anyhow::Result;
use chrono::Utc;
use clipboard_rs::common::RustImage;
use clipboard_rs::ClipboardContent;
use clipboard_rs::{
    ClipboardWatcher as RSClipboardWatcher, ClipboardWatcherContext, WatcherShutdown,
};
use tokio::task::JoinHandle;
use uc_core::clipboard::ObservedClipboardRepresentation;
use uc_core::ids::{FormatId, RepresentationId};
use uc_core::ports::ClipboardChangeHandler;
use uc_core::ports::SystemClipboardPort;
use uc_core::{MimeType, SystemClipboardSnapshot};

pub struct PlatformRuntime<E>
where
    E: PlatformCommandExecutorPort,
{
    #[allow(dead_code)]
    local_clipboard: Arc<dyn SystemClipboardPort>,
    #[allow(dead_code)]
    event_tx: PlatformEventSender,
    event_rx: PlatformEventReceiver,
    command_rx: PlatformCommandReceiver,
    #[allow(dead_code)]
    executor: Arc<E>,
    shutting_down: bool,
    #[allow(dead_code)]
    watcher_join: Option<JoinHandle<()>>,
    #[allow(dead_code)]
    watcher_handle: Option<WatcherShutdown>,
    watcher_running: bool,
    /// Callback handler for clipboard change events
    clipboard_handler: Option<Arc<dyn ClipboardChangeHandler>>,
}

impl<E> PlatformRuntime<E>
where
    E: PlatformCommandExecutorPort,
{
    pub fn new(
        event_tx: PlatformEventSender,
        event_rx: PlatformEventReceiver,
        command_rx: PlatformCommandReceiver,
        executor: Arc<E>,
        clipboard_handler: Option<Arc<dyn ClipboardChangeHandler>>,
    ) -> Result<PlatformRuntime<E>, anyhow::Error> {
        let local_clipboard = Arc::new(LocalClipboard::new()?);

        Ok(Self {
            local_clipboard,
            event_tx,
            event_rx,
            command_rx,
            executor,
            shutting_down: false,
            watcher_join: None,
            watcher_handle: None,
            watcher_running: false,
            clipboard_handler,
        })
    }

    /// Set the clipboard change handler callback.
    ///
    /// This can be called after construction if the handler is not available
    /// at initialization time.
    pub fn set_clipboard_handler(&mut self, handler: Arc<dyn ClipboardChangeHandler>) {
        self.clipboard_handler = Some(handler);
    }

    pub async fn start(mut self) {
        while !self.shutting_down {
            tokio::select! {
                Some(event) = self.event_rx.recv() => {
                    self.handle_event(event).await;
                }
                Some(cmd) = self.command_rx.recv() => {
                    self.handle_command(cmd).await;
                }
            }
        }
    }

    #[allow(dead_code)]
    fn start_clipboard_watcher(&mut self) -> Result<()> {
        if self.watcher_running {
            debug!("Clipboard watcher already running, skipping start");
            return Ok(());
        }

        let mut watcher_ctx = ClipboardWatcherContext::new()
            .map_err(|e| anyhow::anyhow!("Failed to create watcher context: {}", e))?;

        let handler = ClipboardWatcher::new(self.local_clipboard.clone(), self.event_tx.clone());

        let shutdown = watcher_ctx.add_handler(handler).get_shutdown_channel();

        let join = tokio::task::spawn_blocking(move || {
            info!("start clipboard watch");
            watcher_ctx.start_watch();
            info!("clipboard watch stopped");
        });

        self.watcher_join = Some(join);
        self.watcher_handle = Some(shutdown);
        self.watcher_running = true;
        Ok(())
    }

    async fn handle_event(&self, event: PlatformEvent) {
        match event {
            PlatformEvent::ClipboardChanged { snapshot } => {
                if snapshot.is_empty() {
                    debug!("Clipboard changed event had no representations; skipping callback");
                    return;
                }
                debug!(
                    representation_count = snapshot.representation_count(),
                    total_bytes = snapshot.total_size_bytes(),
                    "Clipboard changed"
                );

                // Call the registered callback handler
                if let Some(handler) = &self.clipboard_handler {
                    if let Err(e) = handler.on_clipboard_changed(snapshot).await {
                        error!(error = %e, "Failed to handle clipboard change");
                    }
                } else {
                    warn!("Clipboard changed but no handler registered");
                }
            }
            PlatformEvent::ClipboardSynced { peer_count } => {
                debug!(peer_count, "Clipboard synced to peers");
            }
            PlatformEvent::Started => {
                info!("Platform runtime started");
            }
            PlatformEvent::Stopped => {
                info!("Platform runtime stopped");
            }
            PlatformEvent::Error { message } => {
                error!(error = %message, "Platform error");
            }
        }
    }

    async fn handle_command(&mut self, command: PlatformCommand) {
        match command {
            PlatformCommand::Shutdown => {
                self.shutting_down = true;
                info!("Platform runtime shutting down");
            }
            PlatformCommand::ReadClipboard => match self.local_clipboard.read_snapshot() {
                Ok(snapshot) => {
                    debug!(
                        representation_count = snapshot.representation_count(),
                        total_bytes = snapshot.total_size_bytes(),
                        "Read clipboard"
                    );
                    if let Err(err) = self
                        .event_tx
                        .try_send(PlatformEvent::ClipboardChanged { snapshot })
                    {
                        warn!(error = %err, "Failed to emit clipboard snapshot event");
                    }
                }
                Err(e) => {
                    error!(error = %e, "Failed to read clipboard");
                }
            },
            PlatformCommand::WriteClipboard { content } => {
                match self.snapshot_from_content(content) {
                    Ok(snapshot) => {
                        if let Err(err) = self.local_clipboard.write_snapshot(snapshot) {
                            error!(error = %err, "Failed to write clipboard snapshot");
                        }
                    }
                    Err(err) => {
                        error!(error = %err, "Failed to convert clipboard content");
                    }
                }
            }
            PlatformCommand::StartClipboardWatcher => {
                debug!("StartClipboardWatcher command received");
                if let Err(e) = self.start_clipboard_watcher() {
                    error!(error = %e, "Failed to start clipboard watcher");
                }
            }
            PlatformCommand::StopClipboardWatcher => {
                debug!("StopClipboardWatcher command received");
                if let Some(handle) = self.watcher_handle.take() {
                    handle.stop();
                    self.watcher_running = false;
                    info!("Clipboard watcher stopped");
                } else {
                    if self.watcher_running {
                        self.watcher_running = false;
                    }
                    debug!("Clipboard watcher already stopped");
                }
            }
        }
    }

    fn snapshot_from_content(&self, content: ClipboardContent) -> Result<SystemClipboardSnapshot> {
        let (format_id, mime, bytes) = match content {
            ClipboardContent::Text(text) => (
                "text".to_string(),
                Some(MimeType::text_plain()),
                text.into_bytes(),
            ),
            ClipboardContent::Rtf(text) => (
                "rtf".to_string(),
                Some(MimeType::text_rtf()),
                text.into_bytes(),
            ),
            ClipboardContent::Html(text) => (
                "html".to_string(),
                Some(MimeType::text_html()),
                text.into_bytes(),
            ),
            ClipboardContent::Files(files) => (
                "files".to_string(),
                Some(MimeType::uri_list()),
                files.join("\n").into_bytes(),
            ),
            ClipboardContent::Other(format, bytes) => (format, None, bytes),
            ClipboardContent::Image(image) => {
                let png = image.to_png().map_err(|e| anyhow::anyhow!(e))?;
                (
                    "image".to_string(),
                    Some(MimeType("image/png".to_string())),
                    png.get_bytes().to_vec(),
                )
            }
        };

        Ok(SystemClipboardSnapshot {
            ts_ms: Utc::now().timestamp_millis(),
            representations: vec![ObservedClipboardRepresentation::new(
                RepresentationId::new(),
                FormatId::from(format_id),
                mime,
                bytes,
            )],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::PlatformRuntime;
    use crate::ipc::PlatformCommand;
    use crate::ports::PlatformCommandExecutorPort;
    use crate::runtime::event_bus::{
        PlatformCommandReceiver, PlatformEventReceiver, PlatformEventSender,
    };
    use anyhow::Result;
    use clipboard_rs::ClipboardContent;
    use std::sync::Arc;
    use std::sync::Mutex;
    use tokio::sync::mpsc;
    use tokio::time::{timeout, Duration};
    use uc_core::clipboard::ObservedClipboardRepresentation;
    use uc_core::ids::{FormatId, RepresentationId};
    use uc_core::ports::{ClipboardChangeHandler, SystemClipboardPort};
    use uc_core::{MimeType, SystemClipboardSnapshot};

    struct TestClipboard {
        snapshot: SystemClipboardSnapshot,
        writes: Arc<Mutex<Vec<SystemClipboardSnapshot>>>,
    }

    #[async_trait::async_trait]
    impl SystemClipboardPort for TestClipboard {
        fn read_snapshot(&self) -> Result<SystemClipboardSnapshot> {
            Ok(self.snapshot.clone())
        }

        fn write_snapshot(&self, snapshot: SystemClipboardSnapshot) -> Result<()> {
            let mut writes = self.writes.lock().expect("writes lock");
            writes.push(snapshot);
            Ok(())
        }
    }

    struct TestHandler {
        tx: mpsc::Sender<SystemClipboardSnapshot>,
    }

    #[async_trait::async_trait]
    impl ClipboardChangeHandler for TestHandler {
        async fn on_clipboard_changed(&self, snapshot: SystemClipboardSnapshot) -> Result<()> {
            self.tx
                .send(snapshot)
                .await
                .map_err(|err| anyhow::anyhow!("handler send failed: {err}"))
        }
    }

    struct TestExecutor;

    #[async_trait::async_trait]
    impl PlatformCommandExecutorPort for TestExecutor {
        async fn execute(&self, _command: PlatformCommand) -> Result<()> {
            Ok(())
        }
    }

    fn build_runtime(
        clipboard: Arc<dyn SystemClipboardPort>,
        event_tx: PlatformEventSender,
        event_rx: PlatformEventReceiver,
        command_rx: PlatformCommandReceiver,
        handler: Option<Arc<dyn ClipboardChangeHandler>>,
    ) -> PlatformRuntime<TestExecutor> {
        PlatformRuntime {
            local_clipboard: clipboard,
            event_tx,
            event_rx,
            command_rx,
            executor: Arc::new(TestExecutor),
            shutting_down: false,
            watcher_join: None,
            watcher_handle: None,
            watcher_running: false,
            clipboard_handler: handler,
        }
    }

    #[tokio::test]
    async fn read_clipboard_emits_snapshot_to_handler() {
        let writes = Arc::new(Mutex::new(Vec::new()));
        let snapshot = SystemClipboardSnapshot {
            ts_ms: 123,
            representations: vec![ObservedClipboardRepresentation::new(
                RepresentationId::from("rep-1".to_string()),
                FormatId::from("text/plain".to_string()),
                Some(MimeType::text_plain()),
                b"hello".to_vec(),
            )],
        };
        let clipboard: Arc<dyn SystemClipboardPort> = Arc::new(TestClipboard {
            snapshot: snapshot.clone(),
            writes: writes.clone(),
        });

        let (event_tx, event_rx) = mpsc::channel(8);
        let (command_tx, command_rx) = mpsc::channel(8);
        let (handler_tx, mut handler_rx) = mpsc::channel(1);
        let handler: Arc<dyn ClipboardChangeHandler> = Arc::new(TestHandler { tx: handler_tx });

        let runtime = build_runtime(clipboard, event_tx, event_rx, command_rx, Some(handler));
        let runtime_task = tokio::spawn(async move {
            runtime.start().await;
        });

        command_tx
            .send(PlatformCommand::ReadClipboard)
            .await
            .expect("send read clipboard");

        let received = timeout(Duration::from_millis(200), handler_rx.recv())
            .await
            .expect("handler recv timeout")
            .expect("handler recv");

        assert_eq!(received.ts_ms, snapshot.ts_ms);
        assert_eq!(
            received.representation_count(),
            snapshot.representation_count()
        );
        assert_eq!(
            received.representations[0].bytes,
            snapshot.representations[0].bytes
        );

        command_tx
            .send(PlatformCommand::Shutdown)
            .await
            .expect("send shutdown");

        let _ = timeout(Duration::from_millis(200), runtime_task)
            .await
            .expect("runtime shutdown timeout");
    }

    #[tokio::test]
    async fn write_clipboard_converts_text_to_snapshot() {
        let writes = Arc::new(Mutex::new(Vec::new()));
        let snapshot = SystemClipboardSnapshot {
            ts_ms: 0,
            representations: Vec::new(),
        };
        let clipboard: Arc<dyn SystemClipboardPort> = Arc::new(TestClipboard {
            snapshot,
            writes: writes.clone(),
        });

        let (event_tx, event_rx) = mpsc::channel(8);
        let (_command_tx, command_rx) = mpsc::channel(8);

        let mut runtime = build_runtime(clipboard, event_tx, event_rx, command_rx, None);
        runtime
            .handle_command(PlatformCommand::WriteClipboard {
                content: ClipboardContent::Text("hello".to_string()),
            })
            .await;

        let writes = writes.lock().expect("writes lock");
        assert_eq!(writes.len(), 1);
        let written = &writes[0];
        assert_eq!(written.representations.len(), 1);
        let rep = &written.representations[0];
        assert_eq!(rep.bytes, b"hello".to_vec());
        assert_eq!(rep.mime, Some(MimeType::text_plain()));
    }
}
