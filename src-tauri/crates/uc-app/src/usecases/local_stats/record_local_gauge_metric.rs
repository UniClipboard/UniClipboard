use std::sync::Arc;

use anyhow::Result;
use chrono::Utc;
use uc_core::ports::{LocalGaugeMetric, LocalStatsRepositoryPort};

pub struct RecordLocalGaugeMetric {
    stats_repo: Arc<dyn LocalStatsRepositoryPort>,
}

impl RecordLocalGaugeMetric {
    pub fn new(stats_repo: Arc<dyn LocalStatsRepositoryPort>) -> Self {
        Self { stats_repo }
    }

    #[tracing::instrument(name = "usecase.local_stats.record_gauge.execute", skip(self))]
    pub async fn execute(&self, metric: LocalGaugeMetric, value: f64) -> Result<()> {
        self.stats_repo
            .record_gauge(metric, value, Utc::now().timestamp_millis())
            .await
    }
}
