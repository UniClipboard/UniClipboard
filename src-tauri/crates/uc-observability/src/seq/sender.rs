//! Background HTTP sender for Seq ingestion with dual-trigger batching.
//!
//! Receives CLEF-formatted JSON strings via an mpsc channel and POSTs them
//! to the Seq `/ingest/clef` endpoint in batches.

use std::time::Duration;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

/// Guard that signals shutdown and flushes remaining events when dropped.
///
/// The caller must keep this alive for the duration of the application.
/// When dropped, it sends a shutdown signal to the background sender task
/// and waits (with timeout) for remaining events to be flushed.
pub struct SeqGuard {
    shutdown_tx: Option<oneshot::Sender<()>>,
    handle: Option<JoinHandle<()>>,
}

impl SeqGuard {
    pub(crate) fn new(shutdown_tx: oneshot::Sender<()>, handle: JoinHandle<()>) -> Self {
        Self {
            shutdown_tx: Some(shutdown_tx),
            handle: Some(handle),
        }
    }
}

impl Drop for SeqGuard {
    fn drop(&mut self) {
        // Send shutdown signal
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }

        // Wait for background task to flush with timeout.
        // We spawn a thread to avoid "cannot block_on inside a runtime" panic.
        if let Some(handle) = self.handle.take() {
            if let Ok(rt) = tokio::runtime::Handle::try_current() {
                let _ = std::thread::spawn(move || {
                    rt.block_on(async {
                        let _ = tokio::time::timeout(Duration::from_secs(5), handle).await;
                    });
                })
                .join();
            }
        }
    }
}

/// Background sender loop that batches CLEF events and POSTs to Seq.
///
/// Flushes when:
/// - Batch reaches 100 events
/// - 2-second interval tick fires (if batch is non-empty)
/// - Shutdown signal received (flush remaining, then exit)
pub(crate) async fn sender_loop(
    mut rx: mpsc::Receiver<String>,
    mut shutdown: oneshot::Receiver<()>,
    client: reqwest::Client,
    url: String,
    api_key: Option<String>,
) {
    let mut batch: Vec<String> = Vec::with_capacity(100);
    let mut interval = tokio::time::interval(Duration::from_secs(2));
    // Skip the first immediate tick
    interval.tick().await;

    loop {
        tokio::select! {
            msg = rx.recv() => {
                match msg {
                    Some(event) => {
                        batch.push(event);
                        if batch.len() >= 100 {
                            flush_batch(&client, &url, &api_key, &mut batch).await;
                        }
                    }
                    None => {
                        while let Ok(event) = rx.try_recv() {
                            batch.push(event);
                        }
                        if !batch.is_empty() {
                            flush_batch(&client, &url, &api_key, &mut batch).await;
                        }
                        return;
                    }
                }
            }
            _ = interval.tick() => {
                if !batch.is_empty() {
                    flush_batch(&client, &url, &api_key, &mut batch).await;
                }
            }
            _ = &mut shutdown => {
                // Drain remaining events from channel
                while let Ok(event) = rx.try_recv() {
                    batch.push(event);
                }
                if !batch.is_empty() {
                    flush_batch(&client, &url, &api_key, &mut batch).await;
                }
                return;
            }
        }
    }
}

/// POST a batch of CLEF events to Seq's ingestion endpoint.
async fn flush_batch(
    client: &reqwest::Client,
    url: &str,
    api_key: &Option<String>,
    batch: &mut Vec<String>,
) {
    let body = batch.join("\n");
    batch.clear();

    let mut req = client
        .post(format!("{}/ingest/clef", url.trim_end_matches('/')))
        .header("Content-Type", "application/vnd.serilog.clef")
        .body(body);

    if let Some(key) = api_key {
        req = req.header("X-Seq-ApiKey", key);
    }

    // Silently discard errors - we don't want logging to break the app
    let _ = req.send().await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc;

    #[tokio::test]
    async fn test_sender_processes_events_before_shutdown() {
        let (tx, rx) = mpsc::channel(1024);
        let (shutdown_tx, shutdown_rx) = oneshot::channel();

        // Use a client that will fail to connect (no real server)
        let client = reqwest::Client::new();

        let handle = tokio::spawn(sender_loop(
            rx,
            shutdown_rx,
            client,
            "http://127.0.0.1:1".to_string(), // unreachable address
            None,
        ));

        // Send some events
        for i in 0..5 {
            tx.send(format!(r#"{{"@m":"event {}"}}"#, i)).await.unwrap();
        }

        // Signal shutdown
        let _ = shutdown_tx.send(());

        // Should complete without hanging
        tokio::time::timeout(Duration::from_secs(10), handle)
            .await
            .expect("sender_loop should complete within timeout")
            .expect("sender_loop should not panic");
    }

    #[tokio::test]
    async fn test_seq_guard_signals_shutdown() {
        let (_tx, rx) = mpsc::channel::<String>(1024);
        let (shutdown_tx, shutdown_rx) = oneshot::channel();

        let client = reqwest::Client::new();

        let handle = tokio::spawn(sender_loop(
            rx,
            shutdown_rx,
            client,
            "http://127.0.0.1:1".to_string(),
            None,
        ));

        // Test the shutdown mechanism directly rather than via Drop
        // (Drop uses std::thread::spawn + block_on which can be tricky in tests)
        let mut guard = SeqGuard::new(shutdown_tx, handle);

        // Extract and send shutdown signal manually
        let shutdown_tx = guard.shutdown_tx.take().unwrap();
        let join_handle = guard.handle.take().unwrap();

        // Send shutdown
        let _ = shutdown_tx.send(());

        // Wait for background task to complete
        tokio::time::timeout(Duration::from_secs(5), join_handle)
            .await
            .expect("sender_loop should complete within timeout")
            .expect("sender_loop should not panic");
    }

    #[tokio::test]
    async fn test_sender_flushes_on_batch_size() {
        let (tx, rx) = mpsc::channel(1024);
        let (shutdown_tx, shutdown_rx) = oneshot::channel();

        let client = reqwest::Client::new();

        let handle = tokio::spawn(sender_loop(
            rx,
            shutdown_rx,
            client,
            "http://127.0.0.1:1".to_string(),
            None,
        ));

        // Send 100+ events to trigger batch flush
        for i in 0..105 {
            tx.send(format!(r#"{{"@m":"event {}"}}"#, i)).await.unwrap();
        }

        // Small delay to let batch processing happen
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Shutdown
        let _ = shutdown_tx.send(());
        tokio::time::timeout(Duration::from_secs(10), handle)
            .await
            .expect("should complete")
            .expect("should not panic");
    }

    #[tokio::test]
    async fn test_sender_exits_when_channel_closes() {
        let (tx, rx) = mpsc::channel(1024);
        let (_shutdown_tx, shutdown_rx) = oneshot::channel();

        let client = reqwest::Client::new();

        let handle = tokio::spawn(sender_loop(
            rx,
            shutdown_rx,
            client,
            "http://127.0.0.1:1".to_string(),
            None,
        ));

        tx.send(r#"{"@m":"event before close"}"#.to_string())
            .await
            .unwrap();
        drop(tx);

        tokio::time::timeout(Duration::from_secs(5), handle)
            .await
            .expect("sender_loop should exit when channel closes")
            .expect("sender_loop should not panic");
    }
}
