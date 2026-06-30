//! Default host-event transport for non-GUI / CLI processes.
//!
//! The composition root pre-registers [`LoggingHostEventEmitter`] on the shared
//! host-event bus at wire time so processes without a richer transport (Tauri
//! webview, daemon WS) still surface a minimal observability signal. It lives
//! here — below the entrypoint layer — so the common wiring root stays
//! independent of any specific scenario entrypoint.

use uc_application::facade::{EmitError, HostEvent, HostEventEmitterPort};

/// Event emitter that logs event type names via `tracing::debug!`.
///
/// Always returns `Ok(())` — infallible by design. Inner event payloads are
/// NOT logged because they may contain sensitive data (clipboard content,
/// pairing codes/fingerprints, transfer file paths).
pub(crate) struct LoggingHostEventEmitter;

impl HostEventEmitterPort for LoggingHostEventEmitter {
    fn emit(&self, event: HostEvent) -> Result<(), EmitError> {
        match event {
            HostEvent::Clipboard(_) => {
                tracing::debug!(event_type = "clipboard", "host event (non-gui)");
            }
            HostEvent::Transfer(_) => {
                tracing::debug!(event_type = "transfer", "host event (non-gui)");
            }
            HostEvent::Delivery(_) => {
                // delivery 事件不包含明文,可直接打 event_type;后续如要细化
                // 子状态(Delivered / Failed)再扩展,目前只关心"事件经过了
                // emitter"这一可观测性事实。
                tracing::debug!(event_type = "delivery", "host event (non-gui)");
            }
        }
        Ok(())
    }
}
