//! Global observability context shared across formatters/layers.
//!
//! Stores stable process-level fields (for example `device_id`) that should
//! be present on every emitted log event.

use std::sync::OnceLock;

static GLOBAL_DEVICE_ID: OnceLock<String> = OnceLock::new();

/// Set the global device identifier used by log formatters.
///
/// Returns `true` when the value is initialized by this call, `false` when
/// the value was already set and is left unchanged.
pub fn set_global_device_id(device_id: String) -> bool {
    GLOBAL_DEVICE_ID.set(device_id).is_ok()
}

/// Get the global device identifier if it was configured.
pub fn global_device_id() -> Option<&'static str> {
    GLOBAL_DEVICE_ID.get().map(String::as_str)
}
