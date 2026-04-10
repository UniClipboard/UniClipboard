use std::collections::BTreeMap;
use std::sync::Arc;

use anyhow::Result;
use chrono::{Duration, Local, Utc};
use serde::Serialize;
use uc_core::ports::{
    LocalCounterBucket, LocalCounterMetric, LocalGaugeBucket, LocalGaugeMetric,
    LocalStatsRepositoryPort,
};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalStatsTodaySummary {
    pub copy_count: i64,
    pub paste_count: i64,
    pub sync_outbound_count: i64,
    pub sync_inbound_count: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalStatsDailySummary {
    pub bucket_date: String,
    pub copy_count: i64,
    pub paste_count: i64,
    pub sync_outbound_count: i64,
    pub sync_inbound_count: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalStatsGaugePoint {
    pub bucket_start_ms: i64,
    pub avg_value: f64,
    pub min_value: f64,
    pub max_value: f64,
    pub last_value: f64,
    pub sample_count: i32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalStatsDashboardResult {
    pub generated_at_ms: i64,
    pub today: LocalStatsTodaySummary,
    pub last_7_days: Vec<LocalStatsDailySummary>,
    pub cpu_24h: Vec<LocalStatsGaugePoint>,
    pub memory_24h: Vec<LocalStatsGaugePoint>,
}

pub struct GetLocalStatsDashboard {
    stats_repo: Arc<dyn LocalStatsRepositoryPort>,
}

impl GetLocalStatsDashboard {
    pub fn new(stats_repo: Arc<dyn LocalStatsRepositoryPort>) -> Self {
        Self { stats_repo }
    }

    #[tracing::instrument(name = "usecase.local_stats.dashboard.execute", skip(self))]
    pub async fn execute(&self) -> Result<LocalStatsDashboardResult> {
        let generated_at_ms = Utc::now().timestamp_millis();
        let today = Local::now().date_naive();
        let start_date = (today - Duration::days(6)).format("%Y-%m-%d").to_string();
        let end_date = today.format("%Y-%m-%d").to_string();
        let start_ms = generated_at_ms - Duration::hours(24).num_milliseconds();

        let counter_series = self
            .stats_repo
            .list_daily_counter_series(
                vec![
                    LocalCounterMetric::ClipboardCopy,
                    LocalCounterMetric::ClipboardPaste,
                    LocalCounterMetric::ClipboardSyncOutbound,
                    LocalCounterMetric::ClipboardSyncInbound,
                ],
                start_date,
                end_date.clone(),
            )
            .await?;
        let cpu_24h = self
            .stats_repo
            .list_gauge_series(
                LocalGaugeMetric::ProcessCpuPercent,
                start_ms,
                generated_at_ms,
            )
            .await?;
        let memory_24h = self
            .stats_repo
            .list_gauge_series(
                LocalGaugeMetric::ProcessMemoryBytes,
                start_ms,
                generated_at_ms,
            )
            .await?;

        let mut by_date = seed_last_seven_days(today);
        apply_counter_series(&mut by_date, counter_series);

        let mut last_7_days: Vec<LocalStatsDailySummary> = by_date.into_values().collect();
        last_7_days.sort_by(|a, b| a.bucket_date.cmp(&b.bucket_date));
        let today_summary = last_7_days
            .iter()
            .find(|item| item.bucket_date == end_date)
            .cloned()
            .unwrap_or_default();

        Ok(LocalStatsDashboardResult {
            generated_at_ms,
            today: LocalStatsTodaySummary {
                copy_count: today_summary.copy_count,
                paste_count: today_summary.paste_count,
                sync_outbound_count: today_summary.sync_outbound_count,
                sync_inbound_count: today_summary.sync_inbound_count,
            },
            last_7_days,
            cpu_24h: cpu_24h.into_iter().map(map_gauge_bucket).collect(),
            memory_24h: memory_24h.into_iter().map(map_gauge_bucket).collect(),
        })
    }
}

fn seed_last_seven_days(today: chrono::NaiveDate) -> BTreeMap<String, LocalStatsDailySummary> {
    let mut seeded = BTreeMap::new();
    for offset in (0..7).rev() {
        let date = today - Duration::days(offset);
        let key = date.format("%Y-%m-%d").to_string();
        seeded.insert(
            key.clone(),
            LocalStatsDailySummary {
                bucket_date: key,
                ..LocalStatsDailySummary::default()
            },
        );
    }
    seeded
}

fn apply_counter_series(
    by_date: &mut BTreeMap<String, LocalStatsDailySummary>,
    counter_series: Vec<LocalCounterBucket>,
) {
    for bucket in counter_series {
        let Some(summary) = by_date.get_mut(&bucket.bucket_date) else {
            continue;
        };

        match bucket.metric {
            LocalCounterMetric::ClipboardCopy => summary.copy_count = bucket.count,
            LocalCounterMetric::ClipboardPaste => summary.paste_count = bucket.count,
            LocalCounterMetric::ClipboardSyncOutbound => summary.sync_outbound_count = bucket.count,
            LocalCounterMetric::ClipboardSyncInbound => summary.sync_inbound_count = bucket.count,
            LocalCounterMetric::AppLaunch => {}
        }
    }
}

fn map_gauge_bucket(bucket: LocalGaugeBucket) -> LocalStatsGaugePoint {
    LocalStatsGaugePoint {
        bucket_start_ms: bucket.bucket_start_ms,
        avg_value: bucket.avg_value,
        min_value: bucket.min_value,
        max_value: bucket.max_value,
        last_value: bucket.last_value,
        sample_count: bucket.sample_count,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use async_trait::async_trait;
    use uc_core::ports::LocalStatsRepositoryPort;

    struct MockLocalStatsRepo {
        daily: Vec<LocalCounterBucket>,
        cpu: Vec<LocalGaugeBucket>,
        memory: Vec<LocalGaugeBucket>,
    }

    #[async_trait]
    impl LocalStatsRepositoryPort for MockLocalStatsRepo {
        async fn record_counter(
            &self,
            _metric: LocalCounterMetric,
            _occurred_at_ms: i64,
        ) -> anyhow::Result<()> {
            Ok(())
        }

        async fn record_gauge(
            &self,
            _metric: LocalGaugeMetric,
            _value: f64,
            _sampled_at_ms: i64,
        ) -> anyhow::Result<()> {
            Ok(())
        }

        async fn list_daily_counter_series(
            &self,
            _metrics: Vec<LocalCounterMetric>,
            _start_date: String,
            _end_date: String,
        ) -> anyhow::Result<Vec<LocalCounterBucket>> {
            Ok(self.daily.clone())
        }

        async fn list_gauge_series(
            &self,
            metric: LocalGaugeMetric,
            _start_ms: i64,
            _end_ms: i64,
        ) -> anyhow::Result<Vec<LocalGaugeBucket>> {
            match metric {
                LocalGaugeMetric::ProcessCpuPercent => Ok(self.cpu.clone()),
                LocalGaugeMetric::ProcessMemoryBytes => Ok(self.memory.clone()),
            }
        }
    }

    #[tokio::test]
    async fn execute_fills_missing_days_and_maps_gauge_series() {
        let today = Local::now().date_naive();
        let today_key = today.format("%Y-%m-%d").to_string();
        let older_key = (today - Duration::days(6)).format("%Y-%m-%d").to_string();

        let repo: Arc<dyn LocalStatsRepositoryPort> = Arc::new(MockLocalStatsRepo {
            daily: vec![
                LocalCounterBucket {
                    metric: LocalCounterMetric::ClipboardCopy,
                    bucket_date: today_key.clone(),
                    count: 3,
                },
                LocalCounterBucket {
                    metric: LocalCounterMetric::ClipboardPaste,
                    bucket_date: older_key.clone(),
                    count: 1,
                },
            ],
            cpu: vec![LocalGaugeBucket {
                metric: LocalGaugeMetric::ProcessCpuPercent,
                bucket_start_ms: 1,
                avg_value: 10.0,
                min_value: 8.0,
                max_value: 12.0,
                last_value: 11.0,
                sample_count: 2,
            }],
            memory: vec![LocalGaugeBucket {
                metric: LocalGaugeMetric::ProcessMemoryBytes,
                bucket_start_ms: 2,
                avg_value: 100.0,
                min_value: 90.0,
                max_value: 110.0,
                last_value: 105.0,
                sample_count: 2,
            }],
        });

        let result = GetLocalStatsDashboard::new(repo).execute().await.unwrap();

        assert_eq!(result.last_7_days.len(), 7);
        assert_eq!(result.today.copy_count, 3);
        assert_eq!(result.today.paste_count, 0);
        assert_eq!(result.last_7_days[0].bucket_date, older_key);
        assert_eq!(result.last_7_days[0].paste_count, 1);
        assert_eq!(result.cpu_24h.len(), 1);
        assert_eq!(result.memory_24h.len(), 1);
        assert_eq!(result.cpu_24h[0].last_value, 11.0);
        assert_eq!(result.memory_24h[0].last_value, 105.0);
    }
}
