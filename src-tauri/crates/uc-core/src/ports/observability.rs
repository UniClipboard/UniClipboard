use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceMetadata {
    pub trace_id: Uuid,
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn extracts_trace_metadata() {
        let args = json!({
            "_trace": {
                "trace_id": "a1b2c3d4-e5f6-7890-abcd-ef1234567890",
                "timestamp": 1737100000000u64
            }
        });

        let trace = extract_trace(&args)
            .expect("trace metadata parse error")
            .expect("trace metadata missing");
        assert_eq!(
            trace.trace_id.to_string(),
            "a1b2c3d4-e5f6-7890-abcd-ef1234567890"
        );
        assert_eq!(trace.timestamp, 1737100000000u64);
    }
}
