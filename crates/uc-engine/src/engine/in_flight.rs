use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use tokio::sync::{Mutex, Notify};
use tokio_util::sync::CancellationToken;

pub(crate) struct RegisteredOperation {
    pub(crate) id: String,
    pub(crate) cancellation: CancellationToken,
}

pub(crate) struct InFlightOperations {
    operations: Mutex<HashMap<String, CancellationToken>>,
    changed: Notify,
    next_id: AtomicU64,
}

impl InFlightOperations {
    pub(crate) fn new() -> Self {
        Self {
            operations: Mutex::new(HashMap::new()),
            changed: Notify::new(),
            next_id: AtomicU64::new(1),
        }
    }

    pub(crate) async fn register(&self, prefix: &str) -> RegisteredOperation {
        let id = format!("{prefix}-{}", self.next_id.fetch_add(1, Ordering::Relaxed));
        let cancellation = CancellationToken::new();
        self.operations
            .lock()
            .await
            .insert(id.clone(), cancellation.clone());
        RegisteredOperation { id, cancellation }
    }

    pub(crate) async fn finish(&self, operation_id: &str) -> bool {
        let removed = self.operations.lock().await.remove(operation_id).is_some();
        if removed {
            self.changed.notify_one();
        }
        removed
    }

    pub(crate) async fn wait_until_empty(&self, deadline: Duration) -> bool {
        tokio::time::timeout(deadline, async {
            loop {
                let changed = self.changed.notified();
                if self.operations.lock().await.is_empty() {
                    break;
                }
                changed.await;
            }
        })
        .await
        .is_ok()
    }

    pub(crate) async fn cancel_all(&self) -> Vec<String> {
        let cancelled = self
            .operations
            .lock()
            .await
            .drain()
            .map(|(operation_id, cancellation)| {
                cancellation.cancel();
                operation_id
            })
            .collect();
        self.changed.notify_one();
        cancelled
    }
}
