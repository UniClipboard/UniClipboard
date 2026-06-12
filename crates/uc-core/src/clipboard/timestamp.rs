use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct TimestampMs(i64);

impl TimestampMs {
    /// Unix epoch milliseconds (UTC)
    pub fn from_epoch_millis(ms: i64) -> Self {
        Self(ms)
    }

    pub fn as_millis(&self) -> i64 {
        self.0
    }
}
