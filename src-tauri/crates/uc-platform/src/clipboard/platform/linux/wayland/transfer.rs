//! Pipe-based MIME payload transfer for Wayland data-control offers.
//!
//! Protocol primer: when the compositor advertises clipboard contents, the
//! client receives a `data_offer` proxy that lists supported MIME types.
//! To actually fetch bytes for a given MIME, the client:
//!
//! 1. Creates an OS pipe `(read, write)`.
//! 2. Sends `offer.receive(mime, write_fd)` to the compositor.
//! 3. Closes its own copy of `write_fd` (the compositor still has its dup).
//! 4. Flushes the wayland connection so the compositor sees the request.
//! 5. Reads from `read_fd` until EOF.
//!
//! This module owns step 1, 3, 4, 5, plus a poll-based timeout so a
//! misbehaving source can't stall the watcher indefinitely. The shape of the
//! offer proxy is abstracted via [`super::backend::OfferLike`] so the same
//! code path serves both `wlr-data-control` and `ext-data-control`.

use anyhow::{Context, Result};
use rustix::event::{poll, PollFd, PollFlags};
use rustix::pipe::{pipe_with, PipeFlags};
use std::os::fd::AsFd;
use std::time::{Duration, Instant};
use wayland_client::Connection;

use super::backend::OfferLike;

/// Read the entire payload for `mime` from `offer`. Returns the bytes or an
/// error if the read times out, exceeds `max_bytes`, or the OS pipe fails.
///
/// Note: this function is synchronous and will block the calling thread up
/// to `timeout`. It MUST be called on the same thread that owns the wayland
/// `Connection` because it issues a `receive` request and then flushes.
pub(super) fn pipe_receive<O: OfferLike>(
    conn: &Connection,
    offer: &O,
    mime: &str,
    timeout: Duration,
    max_bytes: usize,
) -> Result<Vec<u8>> {
    let (read_fd, write_fd) =
        pipe_with(PipeFlags::CLOEXEC | PipeFlags::NONBLOCK).context("pipe2 failed")?;

    // Tell the compositor to write the payload into our write_fd.
    offer.receive_to(mime, write_fd.as_fd());

    // Flush so the request actually reaches the compositor.
    conn.flush().context("wayland flush failed")?;

    // Drop our copy of the write end. The compositor still has its dup; once
    // it finishes writing, its dup is closed and we get EOF.
    drop(write_fd);

    let deadline = Instant::now() + timeout;
    let mut buf = Vec::new();
    let mut tmp = [0u8; 8192];

    loop {
        if buf.len() >= max_bytes {
            anyhow::bail!(
                "clipboard mime '{}' payload exceeded {} bytes",
                mime,
                max_bytes
            );
        }

        let now = Instant::now();
        if now >= deadline {
            anyhow::bail!("clipboard read timed out for mime '{}'", mime);
        }
        let remaining_ms: i32 = (deadline - now)
            .as_millis()
            .min(i32::MAX as u128)
            .try_into()
            .unwrap_or(i32::MAX);

        let mut pfd = [PollFd::new(&read_fd, PollFlags::IN)];
        match poll(&mut pfd, remaining_ms) {
            Ok(0) => anyhow::bail!("clipboard read timed out for mime '{}'", mime),
            Ok(_) => {}
            Err(rustix::io::Errno::INTR) => continue,
            Err(e) => return Err(e.into()),
        }

        match rustix::io::read(&read_fd, &mut tmp) {
            Ok(0) => break, // EOF
            Ok(n) => buf.extend_from_slice(&tmp[..n]),
            Err(rustix::io::Errno::AGAIN) | Err(rustix::io::Errno::INTR) => continue,
            Err(e) => return Err(e.into()),
        }
    }

    Ok(buf)
}
