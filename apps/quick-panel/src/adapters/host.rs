//! Requests to the GUI that supervises this process.
//!
//! The helper has no main window of its own. Unlocking, settings and the history page belong to the
//! GUI, which reads one line of JSON per request from this process's standard output (see
//! [`wire_line`]). Without a supervisor nobody reads them, so the requests are refused instead of
//! printed to a terminal.

use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};

use quick_panel_core::ports::{HostLink, HostRequest};

#[derive(Default)]
pub struct SupervisorLink {
    supervised: AtomicBool,
}

impl SupervisorLink {
    /// Records that a GUI supervises this process and reads its output.
    pub fn mark_supervised(&self) {
        self.supervised.store(true, Ordering::Relaxed);
    }
}

/// The line the host reads for `request`, without the newline.
///
/// This is the protocol of the Go host (`ParseRequest` in `packages/desktop-host-go/quickpanelhelper`):
/// one line of JSON on standard output, `{"request":"<name>"}`. The host ignores lines that are
/// not known requests, so the two sides can be updated one at a time.
fn wire_line(request: HostRequest) -> &'static str {
    match request {
        HostRequest::ShowMainWindow => r#"{"request":"show_main_window"}"#,
        HostRequest::OpenSettings => r#"{"request":"open_settings"}"#,
    }
}

impl HostLink for SupervisorLink {
    fn send(&self, request: HostRequest) -> Result<(), String> {
        if !self.supervised.load(Ordering::Relaxed) {
            return Err(quick_panel_core::text::t().needs_app.into());
        }
        let line = wire_line(request);
        let mut out = std::io::stdout().lock();
        writeln!(out, "{line}")
            .and_then(|()| out.flush())
            .map_err(|_| quick_panel_core::text::t().host_gone.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_are_refused_without_a_supervisor() {
        assert_eq!(
            SupervisorLink::default().send(HostRequest::OpenSettings),
            Err(quick_panel_core::text::t().needs_app.to_string())
        );
    }
}
