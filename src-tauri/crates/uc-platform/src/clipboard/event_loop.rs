//! Platform clipboard event loop abstraction.
//!
//! Defines a tiny trait that lets `uc-desktop` start an OS clipboard watcher
//! without naming any concrete crate (clipboard_rs, wayland-client, x11rb, …).
//! Each `target_os` provides one or more implementations under
//! `crate::clipboard::platform::*`, and the [`build_event_loop`] factory picks
//! the right one at runtime.
//!
//! The event loop runs on a dedicated blocking thread (the desktop worker
//! spawns it via `tokio::task::spawn_blocking`). [`ShutdownRx`] is the only
//! cross-thread interrupt signal the loop is required to honor.
//!
//! ## ShutdownRx semantics
//!
//! - On Linux the channel additionally exposes an `eventfd` so a Wayland / X11
//!   loop can include it directly in `poll(2)` and wake instantly without a
//!   helper thread. Adapters that block on a foreign C event loop (e.g.
//!   `clipboard_rs::ClipboardWatcherContext::start_watch`) instead spawn a tiny
//!   helper thread that calls [`ShutdownRx::wait`] and forwards the signal.
//!   macOS / Windows / other Unix have no eventfd and rely on the Condvar path
//!   exclusively.
//! - The channel is single-shot: signalling more than once is idempotent and
//!   waiting after a signal returns immediately.

use anyhow::Result;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
#[cfg(target_os = "linux")]
use tracing::warn;

#[cfg(target_os = "linux")]
use std::os::fd::{AsRawFd, OwnedFd, RawFd};

use super::watcher::ClipboardWatcher;

struct ShutdownInner {
    signaled: AtomicBool,
    cv_lock: Mutex<()>,
    cv: Condvar,
    #[cfg(target_os = "linux")]
    eventfd: Option<OwnedFd>,
}

/// Sender half of the single-shot shutdown channel. Cloneable.
#[derive(Clone)]
pub struct ShutdownTx {
    inner: Arc<ShutdownInner>,
}

/// Receiver half of the single-shot shutdown channel. Single owner.
pub struct ShutdownRx {
    inner: Arc<ShutdownInner>,
}

/// Create a new shutdown channel.
///
/// On Linux the inner [`ShutdownInner`] also owns a non-blocking `eventfd` so
/// pollable adapters can integrate it without spawning a helper thread. If
/// eventfd creation fails (extremely unusual) the channel still works via the
/// Condvar path; only [`ShutdownRx::raw_fd`] returns `None`. macOS / Windows /
/// other Unix have no eventfd and use the Condvar path exclusively.
pub fn shutdown_channel() -> (ShutdownTx, ShutdownRx) {
    #[cfg(target_os = "linux")]
    let eventfd = match rustix::event::eventfd(
        0,
        rustix::event::EventfdFlags::CLOEXEC | rustix::event::EventfdFlags::NONBLOCK,
    ) {
        Ok(fd) => Some(fd),
        Err(err) => {
            warn!(
                error = %err,
                "Failed to create shutdown eventfd; pollable adapters will fall back to Condvar wait"
            );
            None
        }
    };

    let inner = Arc::new(ShutdownInner {
        signaled: AtomicBool::new(false),
        cv_lock: Mutex::new(()),
        cv: Condvar::new(),
        #[cfg(target_os = "linux")]
        eventfd,
    });

    (
        ShutdownTx {
            inner: inner.clone(),
        },
        ShutdownRx { inner },
    )
}

impl ShutdownTx {
    /// Signal shutdown. Idempotent — repeated calls are no-ops.
    pub fn signal(&self) {
        if self.inner.signaled.swap(true, Ordering::SeqCst) {
            return;
        }
        // Wake Condvar waiters. Hold the mutex briefly so a waiter that already
        // observed `signaled == false` and is about to call `wait` doesn't miss
        // the notify (Condvar lost-wakeup avoidance).
        {
            let _g = self.inner.cv_lock.lock().unwrap_or_else(|p| p.into_inner());
            self.inner.cv.notify_all();
        }
        // Wake fd pollers.
        #[cfg(target_os = "linux")]
        {
            if let Some(fd) = self.inner.eventfd.as_ref() {
                let buf = 1u64.to_ne_bytes();
                if let Err(err) = rustix::io::write(fd, &buf) {
                    warn!(
                        error = %err,
                        "Failed to write to shutdown eventfd; pollable adapter may not wake until poll timeout"
                    );
                }
            }
        }
    }
}

impl ShutdownRx {
    /// Non-blocking check.
    pub fn is_signaled(&self) -> bool {
        self.inner.signaled.load(Ordering::SeqCst)
    }

    /// Block the calling thread until [`ShutdownTx::signal`] is invoked.
    pub fn wait(&self) {
        let mut guard = self.inner.cv_lock.lock().unwrap_or_else(|p| p.into_inner());
        while !self.inner.signaled.load(Ordering::SeqCst) {
            guard = self.inner.cv.wait(guard).unwrap_or_else(|p| p.into_inner());
        }
    }

    /// Linux-only raw fd of the underlying eventfd.
    ///
    /// Returns `None` if eventfd creation failed. Adapters using this for
    /// `poll(2)` must still fall back to checking [`Self::is_signaled`] in
    /// their poll loop in case the fd is unavailable. Only exposed on Linux:
    /// macOS / Windows / other Unix have no eventfd, so pollable adapters must
    /// be Linux-gated as well.
    #[cfg(target_os = "linux")]
    pub fn raw_fd(&self) -> Option<RawFd> {
        self.inner.eventfd.as_ref().map(|fd| fd.as_raw_fd())
    }
}

/// A platform clipboard event loop owns a connection to the OS clipboard
/// change-notification mechanism (XFIXES, `wlr-data-control` Selection,
/// `NSPasteboard.changeCount`, Win32 `AddClipboardFormatListener`, …) and
/// invokes `handler.notify_change()` for every observed change.
///
/// Implementations are constructed via [`build_event_loop`] and consumed by
/// `uc_desktop::daemon::workers::clipboard_watcher::ClipboardWatcherWorker`.
pub trait PlatformClipboardEventLoop: Send + 'static {
    /// Run the event loop on the calling thread until `shutdown_rx` fires.
    ///
    /// Must be called on a thread that does not block the tokio runtime
    /// (e.g. inside `tokio::task::spawn_blocking`). Implementations are
    /// expected to:
    ///
    /// 1. Establish their OS-level clipboard listener,
    /// 2. Loop until `shutdown_rx.is_signaled()` (or the analogous fd wake),
    ///    invoking `handler.notify_change()` for every detected change,
    /// 3. Release the listener and return `Ok(())`.
    fn run(self: Box<Self>, handler: ClipboardWatcher, shutdown_rx: ShutdownRx) -> Result<()>;
}

/// Build the default platform clipboard event loop for the current target.
///
/// Linux selects between Wayland and X11 at runtime (`WAYLAND_DISPLAY`
/// presence + protocol probe); other platforms wrap their existing
/// `clipboard_rs` listener.
pub fn build_event_loop() -> Result<Box<dyn PlatformClipboardEventLoop>> {
    crate::clipboard::platform::build_event_loop()
}
