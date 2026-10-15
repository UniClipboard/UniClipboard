//! Configuration of the daemon's independent network-library log.
//!
//! The Engine records iroh and noq-proto events into its own directory, queue,
//! budget and status, separate from the business log. It only does so when the
//! host configures it *before* the process observability runtime is installed;
//! upgrading the Engine alone does not turn it on.
//!
//! The Engine has no default budget, so Desktop supplies its own built-in one.
//! A rejected explicit budget leaves the log `NotConfigured` rather than
//! silently falling back to a capacity nobody asked for.

use std::path::Path;
use std::time::Duration;

use uc_engine::observability::{
    known_network_alpns, NetworkLogBudget, NetworkLogConfig, NetworkProcessRole,
};

/// Overrides the built-in budget for development and verification runs:
/// `record_bytes=..,queue_records=..,file_bytes=..,run_bytes=..,root_bytes=..,retention_secs=..`.
/// It is not a product setting.
pub const NETWORK_LOG_BUDGET_ENV: &str = "UC_NETWORK_LOG_BUDGET";

/// Built-in capacity and retention of the network log in every build:
/// 8 KiB per record, 4 MiB per file, 16 MiB per run, 64 MiB for the whole
/// network root, 7 days of retention.
const PRODUCT_DEFAULT_BUDGET: Option<NetworkLogBudget> = Some(NetworkLogBudget {
    record_bytes: 8 * 1024,
    queue_records: 1024,
    file_bytes: 4 * 1024 * 1024,
    run_bytes: 16 * 1024 * 1024,
    root_bytes: 64 * 1024 * 1024,
    retention: Duration::from_secs(7 * 24 * 60 * 60),
});

/// How the daemon decided about the network log, for one startup record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NetworkLogSetup {
    /// No budget available: the log is `NotConfigured`.
    NotConfigured,
    /// The log is configured with the carried budget.
    Configured,
    /// An explicit budget was given but is malformed or inconsistent. The log is
    /// left off instead of guessing what was meant.
    RejectedBudget,
}

pub(crate) struct DaemonNetworkLog {
    pub(crate) config: Option<NetworkLogConfig>,
    pub(crate) budget: Option<NetworkLogBudget>,
    pub(crate) setup: NetworkLogSetup,
}

pub(crate) fn daemon_network_log(
    logs_dir: &Path,
    explicit_budget: Option<&str>,
) -> DaemonNetworkLog {
    let not_configured = |setup| DaemonNetworkLog {
        config: None,
        budget: None,
        setup,
    };
    let budget = match explicit_budget {
        Some(value) => match parse_budget(value) {
            Some(budget) => budget,
            None => return not_configured(NetworkLogSetup::RejectedBudget),
        },
        None => match PRODUCT_DEFAULT_BUDGET {
            Some(budget) => budget,
            None => return not_configured(NetworkLogSetup::NotConfigured),
        },
    };
    match NetworkLogConfig::new(logs_dir, NetworkProcessRole::Daemon, budget) {
        Ok(config) => DaemonNetworkLog {
            config: Some(config.with_known_alpns(known_network_alpns())),
            budget: Some(budget),
            setup: NetworkLogSetup::Configured,
        },
        Err(_) => not_configured(NetworkLogSetup::RejectedBudget),
    }
}

fn parse_budget(value: &str) -> Option<NetworkLogBudget> {
    let mut record_bytes = None;
    let mut queue_records = None;
    let mut file_bytes = None;
    let mut run_bytes = None;
    let mut root_bytes = None;
    let mut retention_secs = None;
    for pair in value.split(',') {
        let (key, number) = pair.split_once('=')?;
        let number: u64 = number.trim().parse().ok()?;
        let slot = match key.trim() {
            "record_bytes" => &mut record_bytes,
            "queue_records" => &mut queue_records,
            "file_bytes" => &mut file_bytes,
            "run_bytes" => &mut run_bytes,
            "root_bytes" => &mut root_bytes,
            "retention_secs" => &mut retention_secs,
            _ => return None,
        };
        if slot.replace(number).is_some() {
            return None;
        }
    }
    Some(NetworkLogBudget {
        record_bytes: usize::try_from(record_bytes?).ok()?,
        queue_records: usize::try_from(queue_records?).ok()?,
        file_bytes: file_bytes?,
        run_bytes: run_bytes?,
        root_bytes: root_bytes?,
        retention: Duration::from_secs(retention_secs?),
    })
}
