use async_trait::async_trait;
use diesel::prelude::*;
use uc_core::ports::{
    LocalCounterBucket, LocalCounterMetric, LocalGaugeBucket, LocalGaugeMetric,
    LocalStatsRepositoryPort,
};

use crate::db::models::{
    LocalMetricDailyCountRow, LocalMetricMinuteSampleRow, NewLocalMetricDailyCountRow,
    NewLocalMetricMinuteSampleRow,
};
use crate::db::ports::DbExecutor;
use crate::db::schema::{
    local_metric_daily_count, local_metric_daily_count::dsl as daily_dsl,
    local_metric_minute_sample, local_metric_minute_sample::dsl as sample_dsl,
};

pub struct DieselLocalStatsRepository<E> {
    executor: E,
}

impl<E> DieselLocalStatsRepository<E> {
    pub fn new(executor: E) -> Self {
        Self { executor }
    }
}

#[async_trait]
impl<E: DbExecutor> LocalStatsRepositoryPort for DieselLocalStatsRepository<E> {
    async fn record_counter(
        &self,
        metric: LocalCounterMetric,
        occurred_at_ms: i64,
    ) -> anyhow::Result<()> {
        let metric_name_value = metric.as_str().to_string();
        let bucket_date_value = local_date_bucket(occurred_at_ms)?;

        self.executor.run(move |conn| {
            diesel::insert_into(local_metric_daily_count::table)
                .values(&NewLocalMetricDailyCountRow {
                    metric_name: metric_name_value.clone(),
                    bucket_date: bucket_date_value.clone(),
                    count: 1,
                    updated_at_ms: occurred_at_ms,
                })
                .on_conflict((
                    local_metric_daily_count::metric_name,
                    local_metric_daily_count::bucket_date,
                ))
                .do_update()
                .set((
                    local_metric_daily_count::count.eq(local_metric_daily_count::count + 1),
                    local_metric_daily_count::updated_at_ms.eq(occurred_at_ms),
                ))
                .execute(conn)?;
            Ok(())
        })
    }

    async fn record_gauge(
        &self,
        metric: LocalGaugeMetric,
        value: f64,
        sampled_at_ms: i64,
    ) -> anyhow::Result<()> {
        let metric_name_value = metric.as_str().to_string();
        let bucket_start_ms_value = minute_bucket_start(sampled_at_ms);

        self.executor.run(move |conn| {
            let existing = sample_dsl::local_metric_minute_sample
                .filter(sample_dsl::metric_name.eq(&metric_name_value))
                .filter(sample_dsl::bucket_start_ms.eq(bucket_start_ms_value))
                .first::<LocalMetricMinuteSampleRow>(conn)
                .optional()?;

            match existing {
                Some(row) => {
                    let next_sample_count = row.sample_count + 1;
                    let next_avg_value = ((row.avg_value * row.sample_count as f64) + value)
                        / next_sample_count as f64;

                    diesel::update(
                        sample_dsl::local_metric_minute_sample
                            .filter(sample_dsl::metric_name.eq(&metric_name_value))
                            .filter(sample_dsl::bucket_start_ms.eq(bucket_start_ms_value)),
                    )
                    .set((
                        sample_dsl::avg_value.eq(next_avg_value),
                        sample_dsl::min_value.eq(row.min_value.min(value)),
                        sample_dsl::max_value.eq(row.max_value.max(value)),
                        sample_dsl::last_value.eq(value),
                        sample_dsl::sample_count.eq(next_sample_count),
                        sample_dsl::updated_at_ms.eq(sampled_at_ms),
                    ))
                    .execute(conn)?;
                }
                None => {
                    diesel::insert_into(local_metric_minute_sample::table)
                        .values(&NewLocalMetricMinuteSampleRow {
                            metric_name: metric_name_value,
                            bucket_start_ms: bucket_start_ms_value,
                            avg_value: value,
                            min_value: value,
                            max_value: value,
                            last_value: value,
                            sample_count: 1,
                            updated_at_ms: sampled_at_ms,
                        })
                        .execute(conn)?;
                }
            }

            Ok(())
        })
    }

    async fn list_daily_counter_series(
        &self,
        metrics: Vec<LocalCounterMetric>,
        start_date: String,
        end_date: String,
    ) -> anyhow::Result<Vec<LocalCounterBucket>> {
        if metrics.is_empty() {
            return Ok(Vec::new());
        }

        let metric_names: Vec<String> = metrics
            .into_iter()
            .map(|metric| metric.as_str().to_string())
            .collect();

        self.executor.run(move |conn| {
            let rows = daily_dsl::local_metric_daily_count
                .filter(daily_dsl::metric_name.eq_any(metric_names))
                .filter(daily_dsl::bucket_date.ge(start_date))
                .filter(daily_dsl::bucket_date.le(end_date))
                .order((daily_dsl::bucket_date.asc(), daily_dsl::metric_name.asc()))
                .load::<LocalMetricDailyCountRow>(conn)?;

            rows.into_iter().map(map_daily_row).collect()
        })
    }

    async fn list_gauge_series(
        &self,
        metric: LocalGaugeMetric,
        start_ms: i64,
        end_ms: i64,
    ) -> anyhow::Result<Vec<LocalGaugeBucket>> {
        let metric_name_value = metric.as_str().to_string();

        self.executor.run(move |conn| {
            let rows = sample_dsl::local_metric_minute_sample
                .filter(sample_dsl::metric_name.eq(metric_name_value))
                .filter(sample_dsl::bucket_start_ms.ge(start_ms))
                .filter(sample_dsl::bucket_start_ms.le(end_ms))
                .order(sample_dsl::bucket_start_ms.asc())
                .load::<LocalMetricMinuteSampleRow>(conn)?;

            rows.into_iter().map(map_sample_row).collect()
        })
    }
}

fn map_daily_row(row: LocalMetricDailyCountRow) -> anyhow::Result<LocalCounterBucket> {
    let metric = LocalCounterMetric::from_str(&row.metric_name).ok_or_else(|| {
        anyhow::anyhow!(
            "unknown local counter metric stored in database: {}",
            row.metric_name
        )
    })?;

    Ok(LocalCounterBucket {
        metric,
        bucket_date: row.bucket_date,
        count: row.count,
    })
}

fn map_sample_row(row: LocalMetricMinuteSampleRow) -> anyhow::Result<LocalGaugeBucket> {
    let metric = LocalGaugeMetric::from_str(&row.metric_name).ok_or_else(|| {
        anyhow::anyhow!(
            "unknown local gauge metric stored in database: {}",
            row.metric_name
        )
    })?;

    Ok(LocalGaugeBucket {
        metric,
        bucket_start_ms: row.bucket_start_ms,
        avg_value: row.avg_value,
        min_value: row.min_value,
        max_value: row.max_value,
        last_value: row.last_value,
        sample_count: row.sample_count,
    })
}

fn local_date_bucket(timestamp_ms: i64) -> anyhow::Result<String> {
    let Some(timestamp) = chrono::DateTime::<chrono::Utc>::from_timestamp_millis(timestamp_ms)
    else {
        anyhow::bail!("invalid timestamp_ms: {timestamp_ms}");
    };

    Ok(timestamp
        .with_timezone(&chrono::Local)
        .format("%Y-%m-%d")
        .to_string())
}

fn minute_bucket_start(timestamp_ms: i64) -> i64 {
    timestamp_ms - timestamp_ms.rem_euclid(60_000)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::executor::DieselSqliteExecutor;
    use crate::db::pool::init_db_pool;
    use tempfile::TempDir;

    fn make_repo() -> DieselLocalStatsRepository<std::sync::Arc<DieselSqliteExecutor>> {
        let dir = TempDir::new().expect("temp dir");
        let db_path = dir.path().join("local-stats.db");
        let pool = init_db_pool(db_path.to_str().expect("db path")).expect("db pool");
        let executor = std::sync::Arc::new(DieselSqliteExecutor::new(pool));
        // Keep tempdir alive by leaking it for the test duration.
        std::mem::forget(dir);
        DieselLocalStatsRepository::new(executor)
    }

    #[tokio::test]
    async fn record_counter_accumulates_same_day_bucket() {
        let repo = make_repo();
        let timestamp_ms = 1_744_150_400_000;

        repo.record_counter(LocalCounterMetric::ClipboardCopy, timestamp_ms)
            .await
            .unwrap();
        repo.record_counter(LocalCounterMetric::ClipboardCopy, timestamp_ms + 1_000)
            .await
            .unwrap();

        let rows = repo
            .list_daily_counter_series(
                vec![LocalCounterMetric::ClipboardCopy],
                "2025-04-08".to_string(),
                "2026-04-10".to_string(),
            )
            .await
            .unwrap();

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].count, 2);
    }

    #[tokio::test]
    async fn record_gauge_aggregates_same_minute() {
        let repo = make_repo();
        let timestamp_ms = 1_744_150_400_000;

        repo.record_gauge(LocalGaugeMetric::ProcessCpuPercent, 10.0, timestamp_ms)
            .await
            .unwrap();
        repo.record_gauge(
            LocalGaugeMetric::ProcessCpuPercent,
            20.0,
            timestamp_ms + 10_000,
        )
        .await
        .unwrap();

        let rows = repo
            .list_gauge_series(
                LocalGaugeMetric::ProcessCpuPercent,
                timestamp_ms - 60_000,
                timestamp_ms + 60_000,
            )
            .await
            .unwrap();

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].sample_count, 2);
        assert_eq!(rows[0].min_value, 10.0);
        assert_eq!(rows[0].max_value, 20.0);
        assert_eq!(rows[0].last_value, 20.0);
        assert!((rows[0].avg_value - 15.0).abs() < f64::EPSILON);
    }
}
