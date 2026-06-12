use std::time::{SystemTime, UNIX_EPOCH};
use uc_core::ports::ClockPort;

pub struct SystemClock;

impl ClockPort for SystemClock {
    fn now_ms(&self) -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("System time before UNIX EPOCH")
            .as_millis() as i64
    }
}
