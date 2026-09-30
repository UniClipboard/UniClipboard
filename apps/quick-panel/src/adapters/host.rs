//! Requests to the GUI that supervises this process.
//!
//! The helper has no main window of its own. Unlocking, settings and the history page belong to the
//! GUI, which reads one line of JSON per request from this process's standard output (see
//! `uc_desktop::quick_panel_helper::HelperRequest`). Without a supervisor nobody reads them, so
//! the requests are refused instead of printed to a terminal.

use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};

use quick_panel_core::ports::{HostLink, HostRequest};
use uc_desktop::quick_panel_helper::HelperRequest;

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

impl HostLink for SupervisorLink {
    fn send(&self, request: HostRequest) -> Result<(), String> {
        if !self.supervised.load(Ordering::Relaxed) {
            return Err(quick_panel_core::text::NEEDS_APP.into());
        }
        let line = match request {
            HostRequest::ShowMainWindow => HelperRequest::ShowMainWindow,
            HostRequest::OpenSettings => HelperRequest::OpenSettings,
        }
        .to_line();
        let mut out = std::io::stdout().lock();
        writeln!(out, "{line}")
            .and_then(|()| out.flush())
            .map_err(|_| quick_panel_core::text::HOST_GONE.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_are_refused_without_a_supervisor() {
        assert_eq!(
            SupervisorLink::default().send(HostRequest::OpenSettings),
            Err(quick_panel_core::text::NEEDS_APP.to_string())
        );
    }
}
