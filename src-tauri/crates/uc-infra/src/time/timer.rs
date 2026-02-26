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

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::{advance, Duration};

    async fn wait_until_absent(timer: &Timer, session_id: &SessionId) {
        for _ in 0..64 {
            if !timer.timers.lock().await.contains_key(session_id) {
                return;
            }
            tokio::task::yield_now().await;
        }
    }

    #[tokio::test(flavor = "current_thread", start_paused = true)]
    async fn start_sends_timeout_after_ttl() -> anyhow::Result<()> {
        let mut timer = Timer::new();
        let session_id = SessionId::from("session-1");

        timer.start(&session_id, 5).await?;
        assert!(timer.timers.lock().await.contains_key(&session_id));
        tokio::task::yield_now().await;
        advance(Duration::from_secs(5)).await;
        wait_until_absent(&timer, &session_id).await;

        assert!(!timer.timers.lock().await.contains_key(&session_id));
        Ok(())
    }

    #[tokio::test(flavor = "current_thread", start_paused = true)]
    async fn stop_cancels_timer() -> anyhow::Result<()> {
        let mut timer = Timer::new();
        let session_id = SessionId::from("session-2");

        timer.start(&session_id, 5).await?;
        timer.stop(&session_id).await?;
        advance(Duration::from_secs(10)).await;
        wait_until_absent(&timer, &session_id).await;

        assert!(!timer.timers.lock().await.contains_key(&session_id));
        Ok(())
    }

    #[tokio::test(flavor = "current_thread", start_paused = true)]
    async fn start_replaces_existing_timer_for_same_session() -> anyhow::Result<()> {
        let mut timer = Timer::new();
        let session_id = SessionId::from("session-3");

        timer.start(&session_id, 5).await?;
        timer.start(&session_id, 10).await?;
        tokio::task::yield_now().await;
        advance(Duration::from_secs(5)).await;
        tokio::task::yield_now().await;

        assert!(timer.timers.lock().await.contains_key(&session_id));

        advance(Duration::from_secs(5)).await;
        wait_until_absent(&timer, &session_id).await;
        assert!(!timer.timers.lock().await.contains_key(&session_id));
        Ok(())
    }
}
