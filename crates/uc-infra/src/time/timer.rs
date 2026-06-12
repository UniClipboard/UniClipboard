use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::Mutex;
use tokio::time::{sleep, Duration};
use tracing::debug;
use uc_core::{ports::TimerPort, SessionId};

pub struct Timer {
    timers: Arc<Mutex<HashMap<SessionId, tokio::task::AbortHandle>>>,
}

impl Timer {
    pub fn new() -> Self {
        Self {
            timers: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

#[async_trait::async_trait]
impl TimerPort for Timer {
    async fn start(&mut self, session_id: &SessionId, ttl_secs: u64) -> anyhow::Result<()> {
        let timers = Arc::clone(&self.timers);
        let session_id_clone = session_id.clone();

        let mut timers_guard = self.timers.lock().await;
        if let Some(existing) = timers_guard.remove(session_id) {
            existing.abort();
        }

        let handle = tokio::spawn(async move {
            sleep(Duration::from_secs(ttl_secs)).await;
            let mut timers_guard = timers.lock().await;
            timers_guard.remove(&session_id_clone);
        });

        timers_guard.insert(session_id.clone(), handle.abort_handle());
        debug!(session_id = %session_id, ttl_secs, "timer started");
        Ok(())
    }

    async fn stop(&mut self, session_id: &SessionId) -> anyhow::Result<()> {
        let mut timers_guard = self.timers.lock().await;
        if let Some(handle) = timers_guard.remove(session_id) {
            handle.abort();
            debug!(session_id = %session_id, "timer stopped");
        }
        Ok(())
    }
}
