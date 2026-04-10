use std::sync::Arc;

use anyhow::Result;
use chrono::Utc;
use uc_core::ports::{LocalCounterMetric, LocalStatsRepositoryPort};

pub struct RecordLocalCounterMetric {
    stats_repo: Arc<dyn LocalStatsRepositoryPort>,
}

impl RecordLocalCounterMetric {
    pub fn new(stats_repo: Arc<dyn LocalStatsRepositoryPort>) -> Self {
        Self { stats_repo }
    }

    #[tracing::instrument(name = "usecase.local_stats.record_counter.execute", skip(self))]
    pub async fn execute(&self, metric: LocalCounterMetric) -> Result<()> {
        self.stats_repo
            .record_counter(metric, Utc::now().timestamp_millis())
            .await
    }
}
