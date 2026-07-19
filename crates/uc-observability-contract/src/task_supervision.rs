//! Panic-visible fire-and-forget task spawning.
//!
//! Detached `tokio::spawn` tasks swallow panics: the runtime turns the panic
//! into a `JoinError` on the `JoinHandle`, and when that handle is dropped the
//! panic vanishes with no trace. For genuinely one-shot, best-effort work
//! (where retaining a handle for deterministic shutdown would be overkill) this
//! helper preserves the fire-and-forget ergonomics while making a panic
//! surface as a `WARN` log line (`event = "task.panicked"`) instead of
//! disappearing silently.
//!
//! This is the "log the panic" layer referenced by `uc-infra/AGENTS.md
//! §13.3.1` ("失败可见 / 不允许悄悄死掉"). Long-lived looping tasks should
//! instead retain their `JoinHandle` and abort+join it on shutdown (see
//! `crates/uc-infra/src/network/iroh/net_recovery.rs` +
//! `network/iroh/node.rs::shutdown` for the canonical pattern), which surfaces
//! panics through the join at shutdown; reach for `spawn_supervised` only when
//! there is no natural lifecycle owner to hold the handle.

use std::future::Future;

use tracing::{warn, Instrument, Span};

/// Spawn a fire-and-forget task whose panic is logged rather than silently
/// dropped.
///
/// The `name` is emitted as a stable `task` field on the panic log line so
/// field-based log queries can pinpoint which background task died. The
/// returned handle drives an internal supervisor that awaits the real work;
/// callers that do not need it may drop it (the work still runs to completion
/// and its panic is still logged). Aborting the returned handle does **not**
/// abort the underlying work — if you need deterministic cancellation, retain
/// the work's own handle instead of using this helper.
///
/// The panic log inherits the caller's tracing span (captured at call time),
/// so the `warn!` carries the same context (`trace_id`, span fields) the work
/// was spawned under rather than logging as a context-less orphan.
pub fn spawn_supervised<F>(name: &'static str, fut: F) -> tokio::task::JoinHandle<()>
where
    F: Future<Output = ()> + Send + 'static,
{
    let work = tokio::spawn(fut);
    // Capture the caller's span so a panic logs with its context, not as an
    // orphan on the detached supervisor task.
    let caller_span = Span::current();
    tokio::spawn(
        async move {
            match work.await {
                Ok(()) => {}
                // Cancellation is a normal shutdown signal, not a failure. It
                // only occurs if a caller aborts the returned supervisor handle.
                Err(err) if err.is_cancelled() => {}
                Err(err) => {
                    warn!(
                        event = "task.panicked",
                        task = name,
                        error = %err,
                        "background task panicked"
                    );
                }
            }
        }
        .instrument(caller_span),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn completes_normally_without_logging() {
        let handle = spawn_supervised("ok_task", async {});
        handle.await.expect("supervisor should not fail");
    }

    #[tokio::test]
    async fn supervisor_survives_a_panicking_task() {
        // The supervisor must finish cleanly (logging the panic) even though
        // the inner task panicked — it must not re-propagate the panic.
        let handle = spawn_supervised("panicking_task", async {
            panic!("boom");
        });
        handle.await.expect("supervisor must not itself panic");
    }
}
