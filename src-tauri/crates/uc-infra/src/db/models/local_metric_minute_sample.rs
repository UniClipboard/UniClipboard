use diesel::{Insertable, Queryable};

#[derive(Debug, Clone, Queryable)]
#[diesel(table_name = crate::db::schema::local_metric_minute_sample)]
pub struct LocalMetricMinuteSampleRow {
    pub metric_name: String,
    pub bucket_start_ms: i64,
    pub avg_value: f64,
    pub min_value: f64,
    pub max_value: f64,
    pub last_value: f64,
    pub sample_count: i32,
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = crate::db::schema::local_metric_minute_sample)]
pub struct NewLocalMetricMinuteSampleRow {
    pub metric_name: String,
    pub bucket_start_ms: i64,
    pub avg_value: f64,
    pub min_value: f64,
    pub max_value: f64,
    pub last_value: f64,
    pub sample_count: i32,
    pub updated_at_ms: i64,
}
