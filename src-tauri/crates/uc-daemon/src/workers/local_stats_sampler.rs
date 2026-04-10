use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use sysinfo::{get_current_pid, ProcessRefreshKind, ProcessesToUpdate, System};
use tokio::time::MissedTickBehavior;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};
use uc_app::runtime::CoreRuntime;
use uc_app::usecases::CoreUseCases;
use uc_core::ports::LocalGaugeMetric;

use crate::service::{DaemonService, ServiceHealth};

const PROCESS_SAMPLE_CACHE_TTL: Duration = Duration::from_millis(250);

#[derive(Clone, Copy)]
struct ProcessSample {
    cpu_percent: f64,
    memory_bytes: f64,
}

struct ProcessSampler {
    system: System,
    pid: sysinfo::Pid,
    last_sample: Option<ProcessSample>,
    last_sample_at: Option<Instant>,
}

impl ProcessSampler {
    fn new() -> Option<Self> {
        let pid = get_current_pid().ok()?;
        let mut system = System::new_all();
        system.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[pid]),
            false,
            ProcessRefreshKind::nothing().with_cpu().with_memory(),
        );
        Some(Self {
            system,
            pid,
            last_sample: None,
            last_sample_at: None,
        })
    }

    fn sample(&mut self) -> Option<ProcessSample> {
        if let (Some(last_sample), Some(last_sample_at)) = (self.last_sample, self.last_sample_at) {
            if last_sample_at.elapsed() < PROCESS_SAMPLE_CACHE_TTL {
                return Some(last_sample);
            }
        }

        self.system.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[self.pid]),
            false,
            ProcessRefreshKind::nothing().with_cpu().with_memory(),
        );

        let process = self.system.process(self.pid)?;
        let cpu_count = self.system.cpus().len().max(1) as f64;
        let sample = ProcessSample {
            cpu_percent: (process.cpu_usage() as f64 / cpu_count).clamp(0.0, 100.0),
            memory_bytes: process.memory() as f64,
        };
        self.last_sample = Some(sample);
        self.last_sample_at = Some(Instant::now());
        Some(sample)
    }
}

pub struct LocalStatsSamplerWorker {
    runtime: Arc<CoreRuntime>,
    sample_interval: Duration,
}

impl LocalStatsSamplerWorker {
    pub fn new(runtime: Arc<CoreRuntime>, sample_interval: Duration) -> Self {
        Self {
            runtime,
            sample_interval,
        }
    }
}

#[async_trait]
impl DaemonService for LocalStatsSamplerWorker {
    fn name(&self) -> &str {
        "local-stats-sampler"
    }

    async fn start(&self, cancel: CancellationToken) -> anyhow::Result<()> {
        let Some(mut sampler) = ProcessSampler::new() else {
            warn!("local stats sampler unavailable: failed to initialize process sampler");
            return Ok(());
        };

        let mut interval = tokio::time::interval(self.sample_interval);
        interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
        info!("local stats sampler starting");

        loop {
            tokio::select! {
                _ = cancel.cancelled() => {
                    info!("local stats sampler cancelled");
                    return Ok(());
                }
                _ = interval.tick() => {
                    let Some(sample) = sampler.sample() else {
                        warn!("local stats sampler failed to read process sample");
                        continue;
                    };

                    let usecases = CoreUseCases::new(self.runtime.as_ref());
                    if let Err(error) = usecases
                        .record_local_gauge_metric()
                        .execute(LocalGaugeMetric::ProcessCpuPercent, sample.cpu_percent)
                        .await
                    {
                        warn!(error = %error, "Failed to record local CPU sample");
                    }

                    if let Err(error) = usecases
                        .record_local_gauge_metric()
                        .execute(LocalGaugeMetric::ProcessMemoryBytes, sample.memory_bytes)
                        .await
                    {
                        warn!(error = %error, "Failed to record local memory sample");
                    }
                }
            }
        }
    }

    async fn stop(&self) -> anyhow::Result<()> {
        info!("local stats sampler stopped");
        Ok(())
    }

    fn health_check(&self) -> ServiceHealth {
        ServiceHealth::Healthy
    }
}
