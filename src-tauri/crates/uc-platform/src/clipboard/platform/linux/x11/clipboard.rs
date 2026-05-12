//! `SystemClipboardPort` impl for X11.
//!
//! Mirrors the worker-thread structure of `wayland::protocol::wlr::WlrClipboard`:
//!
//! - A dedicated thread owns the `X11Server` (connection + hidden window).
//! - `read_snapshot` / `write_snapshot` post a request over an mpsc channel
//!   and block on a `sync_channel` reply.
//! - The worker's main loop multiplexes [conn fd, wakeup eventfd] via
//!   poll(2) so it wakes immediately on either side.
//!
//! Why a worker thread: the X11 selection owner has to keep responding to
//! `SelectionRequest` events for as long as it owns `CLIPBOARD`. If we
//! moved ownership onto the caller thread, every paste from any X11
//! application would race the request channel — and the caller is the
//! tokio runtime, where blocking is forbidden.

use std::os::fd::{AsFd, AsRawFd, BorrowedFd, OwnedFd};
use std::sync::mpsc::{self, sync_channel, Receiver, SyncSender};
use std::sync::Mutex;
use std::thread::JoinHandle;
use std::time::Duration;

use anyhow::{Context, Result};
use rustix::event::{poll, PollFd, PollFlags};
use tracing::{debug, info, warn};
use uc_core::clipboard::SystemClipboardSnapshot;
use uc_core::ports::SystemClipboardPort;
use x11rb::connection::Connection;
use x11rb::protocol::Event;

use super::connection::X11Server;
use super::reader::read_snapshot;
use super::writer::{install_snapshot, service_selection_request, WriterState};

/// Upper bound on how long the caller waits for the worker to ack a
/// request. The worker's own read deadline is shorter (2 s in
/// `reader::READ_TIMEOUT`); this is a safety belt for the channel itself.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

pub struct X11Clipboard {
    request_tx: mpsc::Sender<Request>,
    wakeup_fd: OwnedFd,
    worker: Mutex<Option<JoinHandle<()>>>,
}

enum Request {
    Read(SyncSender<Result<SystemClipboardSnapshot>>),
    Write(SystemClipboardSnapshot, SyncSender<Result<()>>),
    Stop,
}

impl X11Clipboard {
    pub(super) fn spawn() -> Result<Self> {
        let wakeup_fd = rustix::event::eventfd(
            0,
            rustix::event::EventfdFlags::CLOEXEC | rustix::event::EventfdFlags::NONBLOCK,
        )
        .context("x11 clipboard: creating wakeup eventfd")?;
        let worker_wakeup_fd = wakeup_fd
            .try_clone()
            .context("x11 clipboard: dup wakeup eventfd for worker")?;

        let (request_tx, request_rx) = mpsc::channel::<Request>();

        let worker = std::thread::Builder::new()
            .name("x11-clipboard-worker".into())
            .spawn(move || {
                if let Err(e) = worker_main(request_rx, worker_wakeup_fd) {
                    warn!(error = ?e, "x11 clipboard worker exited with error");
                }
            })
            .context("x11 clipboard: spawning worker thread")?;

        Ok(Self {
            request_tx,
            wakeup_fd,
            worker: Mutex::new(Some(worker)),
        })
    }

    fn send_request(&self, req: Request) -> Result<()> {
        self.request_tx
            .send(req)
            .map_err(|e| anyhow::anyhow!("x11 clipboard: worker channel closed: {e}"))?;
        let buf = 1u64.to_ne_bytes();
        if let Err(e) = rustix::io::write(&self.wakeup_fd, &buf) {
            warn!(error = %e, "x11 clipboard: wakeup write failed");
        }
        Ok(())
    }
}

impl Drop for X11Clipboard {
    fn drop(&mut self) {
        // Best-effort stop; the worker also exits when the request channel
        // closes, so even if the signal never reaches it the thread will
        // unblock during the next poll iteration when its end of the
        // channel disconnects.
        let _ = self.request_tx.send(Request::Stop);
        let buf = 1u64.to_ne_bytes();
        let _ = rustix::io::write(&self.wakeup_fd, &buf);
        if let Some(handle) = self.worker.lock().ok().and_then(|mut g| g.take()) {
            if let Err(e) = handle.join() {
                warn!(?e, "x11 clipboard worker thread panicked on join");
            }
        }
    }
}

#[async_trait::async_trait]
impl SystemClipboardPort for X11Clipboard {
    fn read_snapshot(&self) -> Result<SystemClipboardSnapshot> {
        let (tx, rx) = sync_channel::<Result<SystemClipboardSnapshot>>(1);
        self.send_request(Request::Read(tx))?;
        match rx.recv_timeout(REQUEST_TIMEOUT) {
            Ok(res) => res,
            Err(_) => Err(anyhow::anyhow!(
                "x11 clipboard read timed out after {:?}",
                REQUEST_TIMEOUT
            )),
        }
    }

    fn write_snapshot(&self, snapshot: SystemClipboardSnapshot) -> Result<()> {
        let (tx, rx) = sync_channel::<Result<()>>(1);
        self.send_request(Request::Write(snapshot, tx))?;
        match rx.recv_timeout(REQUEST_TIMEOUT) {
            Ok(res) => res,
            Err(_) => Err(anyhow::anyhow!(
                "x11 clipboard write timed out after {:?}",
                REQUEST_TIMEOUT
            )),
        }
    }
}

fn worker_main(request_rx: Receiver<Request>, wakeup_fd: OwnedFd) -> Result<()> {
    info!("x11 clipboard worker: starting");
    let server =
        X11Server::connect().context("x11 clipboard worker: failed to (re)connect to X display")?;
    let mut state = WriterState::new();

    loop {
        // Drain X11 events first. SelectionRequest events get serviced
        // synchronously here; SelectionClear means somebody else took the
        // selection and we must release our cached payloads.
        while let Some(event) = server
            .conn
            .poll_for_event()
            .context("x11 clipboard worker: poll_for_event failed")?
        {
            match event {
                Event::SelectionRequest(req) => {
                    if let Err(e) = service_selection_request(&server, &state, req) {
                        warn!(error = %e, "x11 clipboard worker: service_selection_request failed");
                    }
                }
                Event::SelectionClear(clear) => {
                    if clear.selection == server.atoms.CLIPBOARD {
                        debug!("x11 clipboard worker: lost CLIPBOARD ownership (SelectionClear)");
                        state.clear();
                    }
                }
                _ => {}
            }
        }

        // Drain requests.
        loop {
            match request_rx.try_recv() {
                Ok(Request::Read(reply)) => {
                    let res = if let Some(snap) = state.cached_snapshot.clone() {
                        // We currently own the selection — return what we
                        // installed; querying ourselves over the wire would
                        // race our own SelectionRequest servicing.
                        Ok(snap)
                    } else {
                        read_snapshot(&server, Some(&state))
                    };
                    let _ = reply.send(res);
                }
                Ok(Request::Write(snap, reply)) => {
                    let res = install_snapshot(&server, &mut state, snap);
                    let _ = reply.send(res);
                }
                Ok(Request::Stop) => {
                    info!("x11 clipboard worker: stop request received");
                    return Ok(());
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    info!("x11 clipboard worker: request channel disconnected");
                    return Ok(());
                }
            }
        }

        // Wait for either the X server, the wakeup eventfd, or a new
        // request. Blocking indefinitely — wakeup_fd ensures the request
        // channel still wakes us promptly.
        let stream = server.conn.stream().as_fd();
        let wakeup_raw = wakeup_fd.as_raw_fd();
        // SAFETY: `wakeup_fd` is owned by this worker's stack frame.
        let wakeup_borrow = unsafe { BorrowedFd::borrow_raw(wakeup_raw) };

        let mut pfds = [
            PollFd::new(&stream, PollFlags::IN),
            PollFd::new(&wakeup_borrow, PollFlags::IN),
        ];

        match poll(&mut pfds, -1) {
            Ok(_) => {}
            Err(rustix::io::Errno::INTR) => continue,
            Err(e) => return Err(e.into()),
        }

        let wakeup_revents = pfds[1].revents();
        if wakeup_revents.contains(PollFlags::IN) {
            let mut buf = [0u8; 8];
            let _ = rustix::io::read(&wakeup_fd, &mut buf);
        }
    }
}
