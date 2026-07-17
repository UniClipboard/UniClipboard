//! The non-payload halves of the Sentry telemetry gate.
//!
//! Sentry's `before_send` / `before_breadcrumb` / `before_send_log` hooks each
//! gate one payload kind and live inline at the `sentry::init` call site in
//! [`super::tracing`]. This module holds the two attachment points that are not
//! payload hooks:
//!
//! - [`transaction_sample_rate`] — refuses to record new transactions while the
//!   gate is off, so no span data is even collected.
//! - [`TelemetryGatedTransportFactory`] — the final envelope boundary. Required
//!   in addition to the sampler because it is the only thing that covers
//!   envelopes no `before_*` hook sees: transactions sampled before the user
//!   toggled telemetry off, `release-health` session updates (see sentry-core
//!   `session.rs`, which calls `send_envelope` directly), and any envelope type
//!   a future SDK upgrade introduces.

use std::sync::Arc;
use std::time::Duration;

use sentry::{ClientOptions, Envelope, TransactionContext, Transport, TransportFactory};

/// Baseline transaction sample rate — the **single source of truth**.
///
/// Emergency reduction to 2% for quota control: by the end of 2026-05 the
/// monthly quota was 80% consumed, and triage attributed 81% of it to the iroh
/// `poll_send` and presence paths. Combined with the Sentry target filter in
/// [`super::tracing`] (`SENTRY_MUTED_TARGET_PREFIXES`), 2% still retains useful
/// observability.
///
/// Do **not** restate this value as `ClientOptions::traces_sample_rate`. Once
/// `traces_sampler` is set, sentry-core ignores `traces_sample_rate` entirely
/// (`performance.rs`: `(Some(traces_sampler), _) => traces_sampler(ctx)`), so a
/// second copy would be dead config that silently disagrees with this one.
const BASE_TRACE_SAMPLE_RATE: f32 = 0.02;

/// Transaction names hard-muted to 0% regardless of the gate.
///
/// Backstop against quota burn: returning 0.0 by name means adding an
/// `#[instrument]` on one of these paths later cannot silently start billing
/// again.
///
/// The list comes from the 2026-05 monthly quota diagnosis — each name is a
/// root transaction from iroh / noq_proto. `SENTRY_MUTED_TARGET_PREFIXES` in
/// [`super::tracing`] already mutes these crates by tracing target, but that
/// filter keys off the target while sentry-tracing can, on some paths, name the
/// transaction with the shorter span name alone; this is the second line of
/// defense for exactly that case.
///
/// Recovery: drop an entry once the corresponding path stops being hot, or once
/// per-transaction quota controls land on the Sentry side.
const MUTED_TRANSACTION_NAMES: &[&str] = &["poll_send", "connect", "QADv4", "tx", "state"];

pub(super) struct TelemetryGatedTransportFactory;

impl TransportFactory for TelemetryGatedTransportFactory {
    fn create_transport(&self, options: &ClientOptions) -> Arc<dyn Transport> {
        let inner = sentry::transports::DefaultTransportFactory.create_transport(options);
        Arc::new(TelemetryGatedTransport::new(inner))
    }
}

struct TelemetryGatedTransport {
    inner: Arc<dyn Transport>,
}

impl TelemetryGatedTransport {
    fn new(inner: Arc<dyn Transport>) -> Self {
        Self { inner }
    }
}

impl Transport for TelemetryGatedTransport {
    fn send_envelope(&self, envelope: Envelope) {
        if uc_observability::is_telemetry_enabled() {
            self.inner.send_envelope(envelope);
        }
    }

    fn flush(&self, timeout: Duration) -> bool {
        self.inner.flush(timeout)
    }

    fn shutdown(&self, timeout: Duration) -> bool {
        self.inner.shutdown(timeout)
    }
}

/// `ClientOptions::traces_sampler`: resolves the sample rate for one new
/// transaction.
///
/// Returns 0% while the telemetry gate is off or when the transaction is in
/// [`MUTED_TRANSACTION_NAMES`]; otherwise [`BASE_TRACE_SAMPLE_RATE`].
pub(super) fn transaction_sample_rate(ctx: &TransactionContext) -> f32 {
    if !uc_observability::is_telemetry_enabled() || MUTED_TRANSACTION_NAMES.contains(&ctx.name()) {
        0.0
    } else {
        BASE_TRACE_SAMPLE_RATE
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use sentry::protocol::Envelope;
    use sentry::Transport;

    use super::*;

    static TEST_LOCK: Mutex<()> = Mutex::new(());

    #[derive(Default)]
    struct CountingTransport {
        sent: AtomicUsize,
    }

    impl Transport for CountingTransport {
        fn send_envelope(&self, _envelope: Envelope) {
            self.sent.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[test]
    fn transaction_started_before_disable_is_blocked_at_transport() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|error| error.into_inner());
        let inner = Arc::new(CountingTransport::default());
        let inner_for_factory = inner.clone();
        let client = Arc::new(sentry::Client::from((
            "https://public@example.com/1",
            ClientOptions {
                transport: Some(Arc::new(move |_options: &ClientOptions| {
                    Arc::new(TelemetryGatedTransport::new(inner_for_factory.clone()))
                        as Arc<dyn Transport>
                })),
                default_integrations: false,
                traces_sampler: Some(Arc::new(|_| 1.0)),
                enable_logs: false,
                ..Default::default()
            },
        )));
        let hub = Arc::new(sentry::Hub::new(Some(client), Arc::new(Default::default())));

        uc_observability::set_telemetry_enabled(true);
        sentry::Hub::run(hub, || {
            let transaction = sentry::start_transaction(TransactionContext::new(
                "started-before-disable",
                "test",
            ));
            assert!(transaction.is_sampled());

            uc_observability::set_telemetry_enabled(false);
            transaction.finish();
            assert_eq!(inner.sent.load(Ordering::SeqCst), 0);

            uc_observability::set_telemetry_enabled(true);
            sentry::start_transaction(TransactionContext::new("started-after-enable", "test"))
                .finish();
            assert_eq!(inner.sent.load(Ordering::SeqCst), 1);
        });
    }

    #[test]
    fn sampler_blocks_disabled_and_muted_transactions() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|error| error.into_inner());
        let ordinary = TransactionContext::new("settings.load", "test");

        uc_observability::set_telemetry_enabled(false);
        assert_eq!(transaction_sample_rate(&ordinary), 0.0);

        uc_observability::set_telemetry_enabled(true);
        assert_eq!(transaction_sample_rate(&ordinary), BASE_TRACE_SAMPLE_RATE);
        for name in MUTED_TRANSACTION_NAMES {
            let transaction = TransactionContext::new(name, "test");
            assert_eq!(transaction_sample_rate(&transaction), 0.0);
        }
    }
}
