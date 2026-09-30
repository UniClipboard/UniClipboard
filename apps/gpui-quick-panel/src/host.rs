//! Requests to the GUI that supervises this process.
//!
//! The helper has no main window of its own. Unlocking, settings and the history page belong to the
//! GUI, which reads one line of JSON per request from this process's standard output (see
//! `uc_desktop::quick_panel_helper::HelperRequest`). Without a supervisor nobody reads them, so
//! the requests are refused instead of printed to a terminal.

use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};

use uc_desktop::quick_panel_helper::HelperRequest;

static SUPERVISED: AtomicBool = AtomicBool::new(false);

/// Records that a GUI supervises this process and reads its output.
pub fn mark_supervised() {
    SUPERVISED.store(true, Ordering::Relaxed);
}

pub fn send(request: HelperRequest) -> Result<(), String> {
    if !SUPERVISED.load(Ordering::Relaxed) {
        return Err(crate::strings::NEEDS_APP.into());
    }
    let mut out = std::io::stdout().lock();
    writeln!(out, "{}", request.to_line())
        .and_then(|()| out.flush())
        .map_err(|_| crate::strings::HOST_GONE.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_are_refused_without_a_supervisor() {
        // Nothing marks this test process as supervised.
        assert_eq!(
            send(HelperRequest::OpenSettings),
            Err(crate::strings::NEEDS_APP.to_string())
        );
    }
}
