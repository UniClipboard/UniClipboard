#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LocalCounterMetric {
    ClipboardCopy,
    ClipboardPaste,
    ClipboardSyncOutbound,
    ClipboardSyncInbound,
    AppLaunch,
}

impl LocalCounterMetric {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ClipboardCopy => "clipboard.copy",
            Self::ClipboardPaste => "clipboard.paste",
            Self::ClipboardSyncOutbound => "clipboard.sync.outbound",
            Self::ClipboardSyncInbound => "clipboard.sync.inbound",
            Self::AppLaunch => "app.launch",
        }
    }

    pub fn from_str(value: &str) -> Option<Self> {
        match value {
            "clipboard.copy" => Some(Self::ClipboardCopy),
            "clipboard.paste" => Some(Self::ClipboardPaste),
            "clipboard.sync.outbound" => Some(Self::ClipboardSyncOutbound),
            "clipboard.sync.inbound" => Some(Self::ClipboardSyncInbound),
            "app.launch" => Some(Self::AppLaunch),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LocalGaugeMetric {
    ProcessCpuPercent,
    ProcessMemoryBytes,
}

impl LocalGaugeMetric {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ProcessCpuPercent => "process.cpu.percent",
            Self::ProcessMemoryBytes => "process.memory.bytes",
        }
    }

    pub fn from_str(value: &str) -> Option<Self> {
        match value {
            "process.cpu.percent" => Some(Self::ProcessCpuPercent),
            "process.memory.bytes" => Some(Self::ProcessMemoryBytes),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalCounterBucket {
    pub metric: LocalCounterMetric,
    pub bucket_date: String,
    pub count: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LocalGaugeBucket {
    pub metric: LocalGaugeMetric,
    pub bucket_start_ms: i64,
    pub avg_value: f64,
    pub min_value: f64,
    pub max_value: f64,
    pub last_value: f64,
    pub sample_count: i32,
}

#[async_trait::async_trait]
pub trait LocalStatsRepositoryPort: Send + Sync {
    async fn record_counter(
        &self,
        metric: LocalCounterMetric,
        occurred_at_ms: i64,
    ) -> anyhow::Result<()>;

    async fn record_gauge(
        &self,
        metric: LocalGaugeMetric,
        value: f64,
        sampled_at_ms: i64,
    ) -> anyhow::Result<()>;

    async fn list_daily_counter_series(
        &self,
        metrics: Vec<LocalCounterMetric>,
        start_date: String,
        end_date: String,
    ) -> anyhow::Result<Vec<LocalCounterBucket>>;

    async fn list_gauge_series(
        &self,
        metric: LocalGaugeMetric,
        start_ms: i64,
        end_ms: i64,
    ) -> anyhow::Result<Vec<LocalGaugeBucket>>;
}

pub struct NoopLocalStatsRepositoryPort;

#[async_trait::async_trait]
impl LocalStatsRepositoryPort for NoopLocalStatsRepositoryPort {
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
        Ok(Vec::new())
    }

    async fn list_gauge_series(
        &self,
        _metric: LocalGaugeMetric,
        _start_ms: i64,
        _end_ms: i64,
    ) -> anyhow::Result<Vec<LocalGaugeBucket>> {
        Ok(Vec::new())
    }
}
