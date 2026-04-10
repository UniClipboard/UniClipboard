CREATE TABLE local_metric_daily_count (
    metric_name   TEXT   NOT NULL,
    bucket_date   TEXT   NOT NULL,
    count         BIGINT NOT NULL DEFAULT 0,
    updated_at_ms BIGINT NOT NULL,
    PRIMARY KEY (metric_name, bucket_date)
);

CREATE INDEX idx_local_metric_daily_count_bucket_date
ON local_metric_daily_count (bucket_date DESC);

CREATE TABLE local_metric_minute_sample (
    metric_name     TEXT   NOT NULL,
    bucket_start_ms BIGINT NOT NULL,
    avg_value       DOUBLE NOT NULL,
    min_value       DOUBLE NOT NULL,
    max_value       DOUBLE NOT NULL,
    last_value      DOUBLE NOT NULL,
    sample_count    INTEGER NOT NULL,
    updated_at_ms   BIGINT NOT NULL,
    PRIMARY KEY (metric_name, bucket_start_ms)
);

CREATE INDEX idx_local_metric_minute_sample_bucket_start
ON local_metric_minute_sample (bucket_start_ms DESC);
