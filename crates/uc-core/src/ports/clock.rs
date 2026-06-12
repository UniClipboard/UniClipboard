use std::sync::Arc;

pub trait ClockPort: Send + Sync {
    fn now_ms(&self) -> i64;
}

impl<T: ClockPort + ?Sized> ClockPort for Arc<T> {
    fn now_ms(&self) -> i64 {
        (**self).now_ms()
    }
}
