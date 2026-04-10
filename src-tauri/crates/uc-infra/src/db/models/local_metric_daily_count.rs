use diesel::{Insertable, Queryable};

#[derive(Debug, Clone, Queryable)]
#[diesel(table_name = crate::db::schema::local_metric_daily_count)]
pub struct LocalMetricDailyCountRow {
    pub metric_name: String,
    pub bucket_date: String,
    pub count: i64,
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = crate::db::schema::local_metric_daily_count)]
pub struct NewLocalMetricDailyCountRow {
    pub metric_name: String,
    pub bucket_date: String,
    pub count: i64,
    pub updated_at_ms: i64,
}
