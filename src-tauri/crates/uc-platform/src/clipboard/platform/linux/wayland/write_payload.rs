//! Write a single MIME payload to a paster-supplied fd.
//!
//! Compositor delivers a `Send { mime_type, fd }` event when some app pastes
//! and asks for the bytes for `mime_type`. We get the write end of a pipe;
//! the paster reads from the read end. Spec: write the bytes for the mime,
//! then close the fd to signal EOF.
//!
//! Edge cases handled here:
//!
//! - Slow paster: cap total wall time so a wedged paster can't block the
//!   worker indefinitely.
//! - Partial writes / `EAGAIN`: poll for writability and retry.
//! - Close fd on completion or on error so the paster doesn't hang reading.
//!
//! Protocol-free; both wlr- and ext-data-control reuse the same body.

use std::os::fd::OwnedFd;
use std::time::{Duration, Instant};

use rustix::event::{poll, PollFd, PollFlags};
use tracing::warn;

/// 5 s upper bound on serving a single Send event. Covers slow pastes
/// without letting a wedged client hold the worker.
const WRITE_DEADLINE: Duration = Duration::from_secs(5);

pub(super) fn write_payload(fd: OwnedFd, bytes: &[u8], mime: &str) {
    let deadline = Instant::now() + WRITE_DEADLINE;
    let mut written = 0;

    while written < bytes.len() {
        let now = Instant::now();
        if now >= deadline {
            warn!(
                mime = %mime,
                wrote = written,
                total = bytes.len(),
                "wayland clipboard: write to paster timed out"
            );
            return;
        }
        let remaining_ms: i32 = (deadline - now)
            .as_millis()
            .min(i32::MAX as u128)
            .try_into()
            .unwrap_or(i32::MAX);

        // Wait for the fd to be writable.
        let mut pfd = [PollFd::new(&fd, PollFlags::OUT)];
        match poll(&mut pfd, remaining_ms) {
            Ok(0) => {
                warn!(mime = %mime, "wayland clipboard: write poll timed out");
                return;
            }
            Ok(_) => {}
            Err(rustix::io::Errno::INTR) => continue,
            Err(e) => {
                warn!(mime = %mime, error = %e, "wayland clipboard: write poll failed");
                return;
            }
        }

        match rustix::io::write(&fd, &bytes[written..]) {
            Ok(0) => {
                warn!(mime = %mime, "wayland clipboard: write returned 0");
                return;
            }
            Ok(n) => written += n,
            Err(rustix::io::Errno::AGAIN) | Err(rustix::io::Errno::INTR) => continue,
            Err(e) => {
                warn!(mime = %mime, error = %e, "wayland clipboard: write failed");
                return;
            }
        }
    }
    // Closing fd signals EOF to the paster.
    drop(fd);
}
