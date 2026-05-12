/// Flow correlation ID for clipboard capture pipeline tracing.
///
/// Wraps a UUID v7 (time-ordered) to provide monotonic, unique identifiers
/// that can be attached as tracing span fields via the `Display` impl.
use std::fmt;
use std::str::FromStr;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FlowId(Uuid);

impl FlowId {
    /// Generate a new flow ID using UUID v7 (time-ordered).
    pub fn generate() -> Self {
        Self(Uuid::now_v7())
    }

    /// 从 wire header 的 UUID 字符串还原 flow id。
    pub fn parse_str(raw: &str) -> Result<Self, uuid::Error> {
        Uuid::parse_str(raw).map(Self)
    }
}

impl fmt::Display for FlowId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for FlowId {
    type Err = uuid::Error;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        Self::parse_str(raw)
    }
}
