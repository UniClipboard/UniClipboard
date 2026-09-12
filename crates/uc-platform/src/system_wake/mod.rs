//! Native system wake notifications; connection policy remains in Engine.
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc,
};
use std::thread::JoinHandle;
use std::time::Duration;

#[cfg(target_os = "macos")]
#[path = "macos.rs"]
mod backend;
#[cfg(target_os = "windows")]
#[path = "windows.rs"]
mod backend;
#[cfg(target_os = "linux")]
#[path = "linux.rs"]
mod backend;

pub struct SystemWakeMonitor {
    pub events: tokio::sync::mpsc::Receiver<()>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<anyhow::Result<()>>>,
}

impl SystemWakeMonitor {
    pub fn start() -> anyhow::Result<Self> {
        let (sender, events) = tokio::sync::mpsc::channel(1);
        let stop = Arc::new(AtomicBool::new(false));
        let (ready, started) = mpsc::sync_channel(1);
        let stopping = Arc::clone(&stop);
        let worker = std::thread::Builder::new()
            .name("system-wake".into())
            .spawn(move || backend::run(sender, stopping, ready))?;
        let mut monitor = Self {
            events,
            stop,
            worker: Some(worker),
        };
        if let Err(source) = started.recv_timeout(Duration::from_secs(5)) {
            monitor.stop_and_join()?;
            return Err(anyhow::Error::new(source).context("system wake monitor did not start"));
        }
        Ok(monitor)
    }

    fn stop_and_join(&mut self) -> anyhow::Result<()> {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            worker.thread().unpark();
            match worker.join() {
                Ok(result) => result?,
                Err(_) => anyhow::bail!("system wake worker panicked"),
            }
        }
        Ok(())
    }

    pub async fn shutdown(mut self) -> anyhow::Result<()> {
        tokio::task::spawn_blocking(move || self.stop_and_join()).await?
    }
}

impl Drop for SystemWakeMonitor {
    fn drop(&mut self) {
        if self.stop_and_join().is_err() {
            tracing::warn!(
                error_kind = "system_wake_shutdown",
                "system wake monitor shutdown failed"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn native_system_wake_registration_shuts_down_cleanly() {
        let monitor = tokio::task::spawn_blocking(SystemWakeMonitor::start)
            .await
            .unwrap()
            .unwrap();
        tokio::time::timeout(Duration::from_secs(3), monitor.shutdown())
            .await
            .unwrap()
            .unwrap();
    }
}
