//! Seq ingestion module for streaming CLEF events to a local Seq instance.
//!
//! # Architecture
//!
//! - `SeqLayer` formats tracing events as CLEF JSON and sends via mpsc channel
//! - Background `sender_loop` batches events and POSTs to Seq's `/ingest/clef` endpoint
//! - `SeqGuard` signals shutdown and flushes remaining events on drop
//!
//! # Configuration
//!
//! - `UC_SEQ_URL` - Seq server URL (e.g., `http://localhost:5341`). If unset, Seq is disabled.
//! - `UC_SEQ_API_KEY` - Optional API key for Seq authentication.

mod layer;
mod sender;

pub use sender::SeqGuard;

use tracing::Subscriber;
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::Layer;

use crate::profile::LogProfile;

/// Build a Seq ingestion layer if `UC_SEQ_URL` is set.
///
/// Returns `None` if `UC_SEQ_URL` is not set (zero overhead).
/// Returns `Some((layer, guard))` when configured, where:
/// - `layer` is a filtered tracing layer that formats events as CLEF
/// - `guard` must be kept alive; dropping it flushes remaining events
///
/// # Arguments
///
/// * `profile` - The [`LogProfile`] controlling filter verbosity
/// * `device_id` - Optional device identifier for cross-device log correlation
pub fn build_seq_layer<S>(
    profile: &LogProfile,
    device_id: Option<&str>,
) -> Option<(impl Layer<S> + Send + Sync, SeqGuard)>
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    let url = std::env::var("UC_SEQ_URL").ok()?;
    if url.is_empty() {
        return None;
    }

    let api_key = std::env::var("UC_SEQ_API_KEY").ok();

    let client = reqwest::Client::new();
    let (tx, rx) = tokio::sync::mpsc::channel(1024);
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();

    let handle = tokio::spawn(sender::sender_loop(rx, shutdown_rx, client, url, api_key));

    let seq_layer = layer::SeqLayer::new(tx, device_id.map(String::from));
    let guard = SeqGuard::new(shutdown_tx, handle);

    let filtered_layer = seq_layer.with_filter(profile.json_filter());

    Some((filtered_layer, guard))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn test_build_seq_layer_returns_none_when_no_env() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var("UC_SEQ_URL");
        std::env::remove_var("UC_SEQ_API_KEY");
        std::env::remove_var("RUST_LOG");

        let result = build_seq_layer::<tracing_subscriber::Registry>(&LogProfile::Dev, None);
        assert!(
            result.is_none(),
            "Should return None when UC_SEQ_URL is not set"
        );
    }

    #[test]
    fn test_build_seq_layer_returns_none_when_empty_url() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("UC_SEQ_URL", "");
        std::env::remove_var("UC_SEQ_API_KEY");
        std::env::remove_var("RUST_LOG");

        let result = build_seq_layer::<tracing_subscriber::Registry>(&LogProfile::Dev, None);
        assert!(
            result.is_none(),
            "Should return None when UC_SEQ_URL is empty"
        );

        std::env::remove_var("UC_SEQ_URL");
    }
}
