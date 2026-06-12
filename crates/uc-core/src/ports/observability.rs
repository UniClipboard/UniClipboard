use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "specta-derive", derive(specta::Type))]
pub struct TraceMetadata {
    pub trace_id: Uuid,
    // Unix epoch ms 时间戳。u64 物理上是 64 位，但实际值落在
    // `Number.MAX_SAFE_INTEGER` (2^53−1) 内还有几十万年余量，所以前端
    // 一直按 JS `number` 处理。`#[specta(type = ...)]` 显式断言这一点，
    // 让生成的 binding 用 TS `number` 而不是 `bigint`。如果未来真要传
    // 微秒级 / 纳秒级时间戳越过 2^53，应改成 `string` 而不是再回 bigint。
    #[cfg_attr(feature = "specta-derive", specta(type = specta_typescript::Number<u64>))]
    pub timestamp: u64,
}

pub type OptionalTrace = Option<TraceMetadata>;

#[derive(Debug, Error)]
pub enum TraceParseError {
    #[error("Failed to parse trace metadata: {0}")]
    InvalidTrace(String),
}

pub fn extract_trace(args: &serde_json::Value) -> Result<OptionalTrace, TraceParseError> {
    let trace_value = match args.get("_trace") {
        Some(value) => value,
        None => return Ok(None),
    };

    serde_json::from_value(trace_value.clone())
        .map(Some)
        .map_err(|err| TraceParseError::InvalidTrace(err.to_string()))
}
