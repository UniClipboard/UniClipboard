//! Global observability context shared across formatters/layers.
//!
//! Stores stable process-level fields that should be present on every emitted
//! log event: the process run identity (`run_id`, `pid`) and the local device
//! identifier, which is only ever written as a per-run reference.

use std::collections::hash_map::RandomState;
use std::hash::BuildHasher;
use std::sync::OnceLock;

static GLOBAL_DEVICE_ID: OnceLock<String> = OnceLock::new();
static RUN_ID: OnceLock<String> = OnceLock::new();
static PID: OnceLock<u32> = OnceLock::new();
static REF_KEYS: OnceLock<RandomState> = OnceLock::new();

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

/// Identity of this process run, generated on first use and stable until exit.
///
/// Every host record carries it, so two daemons that share a log directory (for
/// example one waiting on the instance lock while another runs) stay separable.
pub fn run_id() -> &'static str {
    RUN_ID.get_or_init(|| uuid::Uuid::now_v7().to_string())
}

/// Operating-system process id of this run.
pub fn pid() -> u32 {
    *PID.get_or_init(std::process::id)
}

/// Keyed reference of `value` that is stable within this run only.
///
/// The key is random per process and never persisted, so a reference can be
/// correlated across the records of one run but neither reversed nor linked to
/// another run or to the value itself.
pub fn run_scoped_ref(value: &str) -> String {
    let keys = REF_KEYS.get_or_init(RandomState::new);
    format!("{:016x}", keys.hash_one(value))
}
