//! `wlr-data-control-unstable-v1` backend.
//!
//! Used on niri / sway / hyprland / KDE Plasma 5+ / wlroots compositors.
//! GNOME mutter does **not** advertise this protocol; for mutter use the
//! `ext-data-control-v1` backend in [`super::ext`].
//!
//! This module owns:
//! - the watcher (`WlrEventLoop`, drives the [`crate::clipboard::watcher::ClipboardWatcher`]
//!   pipeline)
//! - the `SystemClipboardPort` impl (`WlrClipboard`, used by daemon `apply_inbound`
//!   to write to the clipboard and by reads to query the current contents)
//!
//! Both halves bind the same `zwlr_data_control_manager_v1` global. The watcher
//! and the clipboard worker each open their own `EventQueue` (it's `!Send`, so
//! they have to be on dedicated threads anyway), but they share the same
//! wayland `Connection` object underneath.

use std::collections::HashMap;
use std::os::fd::{AsRawFd, BorrowedFd, OwnedFd};
use std::sync::mpsc::{self, sync_channel, Receiver, SyncSender};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use anyhow::{Context, Result};
use rustix::event::{poll, PollFd, PollFlags};
use tracing::{debug, info, warn};
use uc_core::clipboard::SystemClipboardSnapshot;
use uc_core::ports::SystemClipboardPort;
use wayland_client::backend::ObjectId;
use wayland_client::{
    event_created_child,
    protocol::{wl_registry, wl_registry::WlRegistry, wl_seat, wl_seat::WlSeat},
    Connection, Dispatch, EventQueue, Proxy, QueueHandle,
};
use wayland_protocols_wlr::data_control::v1::client::{
    zwlr_data_control_device_v1::{self, ZwlrDataControlDeviceV1, EVT_DATA_OFFER_OPCODE},
    zwlr_data_control_manager_v1::{self, ZwlrDataControlManagerV1},
    zwlr_data_control_offer_v1::{self, ZwlrDataControlOfferV1},
    zwlr_data_control_source_v1::{self, ZwlrDataControlSourceV1},
};

use crate::clipboard::event_loop::{PlatformClipboardEventLoop, ShutdownRx};
use crate::clipboard::watcher::ClipboardWatcher;

use super::super::backend::OfferLike;
use super::super::snapshot::build_from_offer;
use super::super::write_payload::write_payload;

const WL_SEAT_VERSION: u32 = 7;
const ZWLR_DATA_CONTROL_MANAGER_VERSION: u32 = 2;
const FALLBACK_POLL_TIMEOUT_MS: i32 = 250;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
const MANAGER_INTERFACE: &str = "zwlr_data_control_manager_v1";

// `OfferLike` impl so the protocol-agnostic `transfer::pipe_receive` /
// `snapshot::build_from_offer` can drive the wlr offer.
impl OfferLike for ZwlrDataControlOfferV1 {
    fn receive_to(&self, mime: &str, fd: BorrowedFd<'_>) {
        self.receive(mime.to_string(), fd);
    }
}

// ---------------------------------------------------------------------------
// Probe: does this compositor advertise wlr-data-control?
// ---------------------------------------------------------------------------

/// One-shot probe used by [`super::detect_data_control`]. Reuses the same
/// `Connection` for the real run loop; the probe just spends two roundtrips
/// on a throwaway `EventQueue`.
pub(super) fn probe(conn: &Connection) -> Result<bool> {
    let mut q = conn.new_event_queue::<ProbeState>();
    let qh = q.handle();
    let _registry = conn.display().get_registry(&qh, ());
    let mut s = ProbeState::default();
    q.roundtrip(&mut s)
        .context("wlr-data-control probe roundtrip 1 failed")?;
    q.roundtrip(&mut s)
        .context("wlr-data-control probe roundtrip 2 failed")?;
    if !s.has_manager {
        return Ok(false);
    }
    if !s.has_seat {
        warn!("wlr-data-control: manager present but no wl_seat");
        return Ok(false);
    }
    Ok(true)
}

#[derive(Default)]
struct ProbeState {
    has_seat: bool,
    has_manager: bool,
}

impl Dispatch<WlRegistry, ()> for ProbeState {
    fn event(
        s: &mut Self,
        _r: &WlRegistry,
        e: wl_registry::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global { interface, .. } = e {
            match interface.as_str() {
                "wl_seat" => s.has_seat = true,
                MANAGER_INTERFACE => s.has_manager = true,
                _ => {}
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Watcher: WlrEventLoop
// ---------------------------------------------------------------------------

pub(crate) struct WlrEventLoop {
    conn: Connection,
}

impl WlrEventLoop {
    pub(super) fn with_connection(conn: Connection) -> Self {
        Self { conn }
    }
}

impl PlatformClipboardEventLoop for WlrEventLoop {
    fn run(self: Box<Self>, mut handler: ClipboardWatcher, shutdown_rx: ShutdownRx) -> Result<()> {
        info!("wlr-data-control event loop: starting");

        let conn = self.conn;
        let mut event_queue = conn.new_event_queue::<WatcherState>();
        let qh = event_queue.handle();
        let _registry = conn.display().get_registry(&qh, ());

        let mut state = WatcherState::new();

        event_queue
            .roundtrip(&mut state)
            .context("wlr-data-control startup roundtrip failed")?;

        let manager = state
            .manager
            .clone()
            .context("wlr-data-control manager disappeared after probe")?;
        let seat = state
            .seat
            .clone()
            .context("wl_seat disappeared after probe")?;

        state.device = Some(manager.get_data_device(&seat, &qh, ()));

        event_queue
            .roundtrip(&mut state)
            .context("wlr-data-control device-bind roundtrip failed")?;

        for snap in state.pending_snapshots.drain(..) {
            handler.notify_with_snapshot(snap);
        }

        loop {
            event_queue
                .dispatch_pending(&mut state)
                .context("wlr-data-control dispatch_pending failed")?;
            for snap in state.pending_snapshots.drain(..) {
                handler.notify_with_snapshot(snap);
            }

            if shutdown_rx.is_signaled() {
                debug!("wlr-data-control event loop: shutdown observed before poll");
                break;
            }

            event_queue
                .flush()
                .context("wlr-data-control event_queue flush failed")?;

            let read_guard = match conn.prepare_read() {
                Some(g) => g,
                None => continue,
            };

            let wl_raw_fd = read_guard.connection_fd().as_raw_fd();
            let shutdown_raw_fd = shutdown_rx.raw_fd();

            // SAFETY: wayland fd is kept alive by `read_guard`; shutdown
            // eventfd is owned by `ShutdownInner` (Arc-shared with sender).
            let wl_borrow = unsafe { BorrowedFd::borrow_raw(wl_raw_fd) };

            let poll_result;
            let wl_revents;
            let shutdown_woke;

            if let Some(s_raw) = shutdown_raw_fd {
                let s_borrow = unsafe { BorrowedFd::borrow_raw(s_raw) };
                let mut pfds = [
                    PollFd::new(&wl_borrow, PollFlags::IN),
                    PollFd::new(&s_borrow, PollFlags::IN),
                ];
                poll_result = poll(&mut pfds, -1);
                wl_revents = pfds[0].revents();
                shutdown_woke = pfds[1].revents().contains(PollFlags::IN);
            } else {
                let mut pfds = [PollFd::new(&wl_borrow, PollFlags::IN)];
                poll_result = poll(&mut pfds, FALLBACK_POLL_TIMEOUT_MS);
                wl_revents = pfds[0].revents();
                shutdown_woke = false;
            }

            match poll_result {
                Ok(_) => {}
                Err(rustix::io::Errno::INTR) => {
                    drop(read_guard);
                    continue;
                }
                Err(e) => return Err(e.into()),
            }

            if shutdown_woke || shutdown_rx.is_signaled() {
                drop(read_guard);
                debug!("wlr-data-control event loop: shutdown signal received");
                break;
            }

            if wl_revents.contains(PollFlags::IN) {
                if let Err(e) = read_guard.read() {
                    return Err(anyhow::anyhow!(
                        "wlr-data-control read events failed: {e:?}"
                    ));
                }
            } else {
                drop(read_guard);
            }
        }

        info!("wlr-data-control event loop: stopped");
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Watcher state + dispatch
// ---------------------------------------------------------------------------

struct WatcherState {
    seat: Option<WlSeat>,
    manager: Option<ZwlrDataControlManagerV1>,
    device: Option<ZwlrDataControlDeviceV1>,
    offers_in_flight: HashMap<ObjectId, Vec<String>>,
    pending_snapshots: Vec<SystemClipboardSnapshot>,
}

impl WatcherState {
    fn new() -> Self {
        Self {
            seat: None,
            manager: None,
            device: None,
            offers_in_flight: HashMap::new(),
            pending_snapshots: Vec::new(),
        }
    }
}

impl Dispatch<WlRegistry, ()> for WatcherState {
    fn event(
        state: &mut Self,
        registry: &WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _conn: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
        {
            match interface.as_str() {
                "wl_seat" if state.seat.is_none() => {
                    let v = version.min(WL_SEAT_VERSION);
                    state.seat = Some(registry.bind::<WlSeat, (), Self>(name, v, qh, ()));
                }
                MANAGER_INTERFACE if state.manager.is_none() => {
                    let v = version.min(ZWLR_DATA_CONTROL_MANAGER_VERSION);
                    state.manager =
                        Some(registry.bind::<ZwlrDataControlManagerV1, (), Self>(name, v, qh, ()));
                }
                _ => {}
            }
        }
    }
}

impl Dispatch<WlSeat, ()> for WatcherState {
    fn event(
        _: &mut Self,
        _: &WlSeat,
        _: wl_seat::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwlrDataControlManagerV1, ()> for WatcherState {
    fn event(
        _: &mut Self,
        _: &ZwlrDataControlManagerV1,
        _: zwlr_data_control_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwlrDataControlDeviceV1, ()> for WatcherState {
    fn event(
        state: &mut Self,
        _device: &ZwlrDataControlDeviceV1,
        event: zwlr_data_control_device_v1::Event,
        _: &(),
        conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_data_control_device_v1::Event::DataOffer { id } => {
                state.offers_in_flight.insert(id.id(), Vec::new());
            }
            zwlr_data_control_device_v1::Event::Selection { id } => {
                let Some(offer) = id else {
                    debug!("wlr-data-control: selection cleared");
                    return;
                };
                let oid = offer.id();
                let mimes = state.offers_in_flight.remove(&oid).unwrap_or_default();
                if mimes.is_empty() {
                    offer.destroy();
                    return;
                }
                match build_from_offer(conn, &offer, &mimes) {
                    Ok(snap) => state.pending_snapshots.push(snap),
                    Err(e) => warn!(error = %e, "wlr-data-control: snapshot capture failed"),
                }
                offer.destroy();
            }
            zwlr_data_control_device_v1::Event::PrimarySelection { id } => {
                if let Some(offer) = id {
                    state.offers_in_flight.remove(&offer.id());
                    offer.destroy();
                }
            }
            zwlr_data_control_device_v1::Event::Finished => {
                debug!("wlr-data-control: data_control_device finished");
                state.device = None;
            }
            _ => {}
        }
    }

    event_created_child!(WatcherState, ZwlrDataControlDeviceV1, [
        EVT_DATA_OFFER_OPCODE => (ZwlrDataControlOfferV1, ()),
    ]);
}

impl Dispatch<ZwlrDataControlOfferV1, ()> for WatcherState {
    fn event(
        state: &mut Self,
        offer: &ZwlrDataControlOfferV1,
        event: zwlr_data_control_offer_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zwlr_data_control_offer_v1::Event::Offer { mime_type } = event {
            if let Some(mimes) = state.offers_in_flight.get_mut(&offer.id()) {
                mimes.push(mime_type);
            }
        }
    }
}

impl Dispatch<ZwlrDataControlSourceV1, ()> for WatcherState {
    fn event(
        _: &mut Self,
        _: &ZwlrDataControlSourceV1,
        _: zwlr_data_control_source_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // Watcher never owns a source.
    }
}

// ---------------------------------------------------------------------------
// Clipboard read/write: WlrClipboard
// ---------------------------------------------------------------------------

pub(crate) struct WlrClipboard {
    inner: Arc<Inner>,
}

struct Inner {
    request_tx: mpsc::Sender<Request>,
    wakeup_fd: OwnedFd,
    worker: std::sync::Mutex<Option<JoinHandle<()>>>,
}

enum Request {
    Read(SyncSender<Result<SystemClipboardSnapshot>>),
    Write(SystemClipboardSnapshot, SyncSender<Result<()>>),
    Stop,
}

impl WlrClipboard {
    /// Spawn the worker against an already-validated connection. Caller has
    /// done the protocol probe via [`probe`].
    pub(super) fn spawn(conn: Connection) -> Result<Self> {
        let wakeup_fd = rustix::event::eventfd(
            0,
            rustix::event::EventfdFlags::CLOEXEC | rustix::event::EventfdFlags::NONBLOCK,
        )
        .context("creating wlr-data-control wakeup eventfd")?;

        let worker_wakeup_fd = wakeup_fd
            .try_clone()
            .context("dup wlr-data-control wakeup eventfd for worker")?;

        let (request_tx, request_rx) = mpsc::channel::<Request>();

        let worker = std::thread::Builder::new()
            .name("wlr-data-control-worker".into())
            .spawn(move || {
                if let Err(e) = worker_main(conn, request_rx, worker_wakeup_fd) {
                    warn!(error = ?e, "wlr-data-control worker exited with error");
                }
            })
            .context("spawning wlr-data-control worker thread")?;

        Ok(Self {
            inner: Arc::new(Inner {
                request_tx,
                wakeup_fd,
                worker: std::sync::Mutex::new(Some(worker)),
            }),
        })
    }

    fn send_request(&self, req: Request) -> Result<()> {
        self.inner
            .request_tx
            .send(req)
            .map_err(|e| anyhow::anyhow!("wlr-data-control worker channel closed: {e}"))?;
        let buf = 1u64.to_ne_bytes();
        if let Err(e) = rustix::io::write(&self.inner.wakeup_fd, &buf) {
            warn!(error = %e, "wlr-data-control wakeup write failed");
        }
        Ok(())
    }
}

impl Drop for Inner {
    fn drop(&mut self) {
        let _ = self.request_tx.send(Request::Stop);
        let buf = 1u64.to_ne_bytes();
        let _ = rustix::io::write(&self.wakeup_fd, &buf);
        if let Some(handle) = self.worker.lock().ok().and_then(|mut g| g.take()) {
            if let Err(e) = handle.join() {
                warn!(?e, "wlr-data-control worker thread panicked on join");
            }
        }
    }
}

#[async_trait::async_trait]
impl SystemClipboardPort for WlrClipboard {
    fn read_snapshot(&self) -> Result<SystemClipboardSnapshot> {
        let (tx, rx) = sync_channel::<Result<SystemClipboardSnapshot>>(1);
        self.send_request(Request::Read(tx))?;
        match rx.recv_timeout(REQUEST_TIMEOUT) {
            Ok(res) => res,
            Err(_) => Err(anyhow::anyhow!(
                "wlr-data-control read timed out after {:?}",
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
                "wlr-data-control write timed out after {:?}",
                REQUEST_TIMEOUT
            )),
        }
    }
}

// ---------------------------------------------------------------------------
// Worker
// ---------------------------------------------------------------------------

struct ActiveSource {
    source: ZwlrDataControlSourceV1,
    payloads: HashMap<String, Arc<Vec<u8>>>,
}

struct WorkerState {
    seat: Option<WlSeat>,
    manager: Option<ZwlrDataControlManagerV1>,
    device: Option<ZwlrDataControlDeviceV1>,
    offers_in_flight: HashMap<ObjectId, Vec<String>>,
    cached_snapshot: Option<SystemClipboardSnapshot>,
    active_source: Option<ActiveSource>,
    self_echo_pending: u32,
}

impl WorkerState {
    fn new() -> Self {
        Self {
            seat: None,
            manager: None,
            device: None,
            offers_in_flight: HashMap::new(),
            cached_snapshot: None,
            active_source: None,
            self_echo_pending: 0,
        }
    }
}

fn worker_main(conn: Connection, request_rx: Receiver<Request>, wakeup_fd: OwnedFd) -> Result<()> {
    info!("wlr-data-control worker: starting");

    let mut event_queue: EventQueue<WorkerState> = conn.new_event_queue();
    let qh = event_queue.handle();
    let _registry = conn.display().get_registry(&qh, ());

    let mut state = WorkerState::new();

    event_queue
        .roundtrip(&mut state)
        .context("wlr-data-control worker startup roundtrip failed")?;

    let manager = state
        .manager
        .clone()
        .context("wlr-data-control manager disappeared after probe")?;
    let seat = state
        .seat
        .clone()
        .context("wl_seat disappeared after probe")?;

    state.device = Some(manager.get_data_device(&seat, &qh, ()));

    event_queue
        .roundtrip(&mut state)
        .context("wlr-data-control worker device-bind roundtrip failed")?;

    loop {
        event_queue
            .dispatch_pending(&mut state)
            .context("wlr-data-control worker dispatch_pending failed")?;

        loop {
            match request_rx.try_recv() {
                Ok(Request::Read(reply)) => {
                    let snap = state.cached_snapshot.clone().unwrap_or_else(empty_snapshot);
                    let _ = reply.send(Ok(snap));
                }
                Ok(Request::Write(snap, reply)) => {
                    let res = handle_write(&mut state, &qh, &manager, snap);
                    let _ = reply.send(res);
                }
                Ok(Request::Stop) => {
                    info!("wlr-data-control worker: stop request received");
                    return Ok(());
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    info!("wlr-data-control worker: request channel disconnected");
                    return Ok(());
                }
            }
        }

        event_queue
            .flush()
            .context("wlr-data-control worker event_queue flush failed")?;

        let read_guard = match conn.prepare_read() {
            Some(g) => g,
            None => continue,
        };
        let wl_raw_fd = read_guard.connection_fd().as_raw_fd();
        let wakeup_raw_fd = wakeup_fd.as_raw_fd();

        // SAFETY: both fds outlive this poll call (`read_guard` keeps the
        // wayland fd alive, `wakeup_fd` is owned by the worker stack).
        let wl_borrow = unsafe { BorrowedFd::borrow_raw(wl_raw_fd) };
        let wakeup_borrow = unsafe { BorrowedFd::borrow_raw(wakeup_raw_fd) };
        let mut pfds = [
            PollFd::new(&wl_borrow, PollFlags::IN),
            PollFd::new(&wakeup_borrow, PollFlags::IN),
        ];

        let poll_res = poll(&mut pfds, -1);
        let wl_revents = pfds[0].revents();
        let wakeup_revents = pfds[1].revents();

        match poll_res {
            Ok(_) => {}
            Err(rustix::io::Errno::INTR) => {
                drop(read_guard);
                continue;
            }
            Err(e) => return Err(e.into()),
        }

        if wakeup_revents.contains(PollFlags::IN) {
            let mut buf = [0u8; 8];
            let _ = rustix::io::read(&wakeup_fd, &mut buf);
        }

        if wl_revents.contains(PollFlags::IN) {
            if let Err(e) = read_guard.read() {
                return Err(anyhow::anyhow!(
                    "wlr-data-control worker read events failed: {e:?}"
                ));
            }
        } else {
            drop(read_guard);
        }
    }
}

fn empty_snapshot() -> SystemClipboardSnapshot {
    SystemClipboardSnapshot {
        ts_ms: chrono::Utc::now().timestamp_millis(),
        representations: Vec::new(),
    }
}

fn handle_write(
    state: &mut WorkerState,
    qh: &QueueHandle<WorkerState>,
    manager: &ZwlrDataControlManagerV1,
    snapshot: SystemClipboardSnapshot,
) -> Result<()> {
    let device = state
        .device
        .as_ref()
        .context("wlr-data-control worker has no data device")?
        .clone();

    if snapshot.representations.is_empty() {
        // Clear the selection. Do NOT destroy the previous source eagerly:
        // `set_selection(None)` makes the compositor cancel it, and the
        // `Cancelled` handler destroys it. An eager destroy here used to make
        // the compositor emit an extra `Selection { id: None }` event on top of
        // the clear, desyncing `self_echo_pending` (see the replace path below
        // for the full self-deadlock failure mode).
        state.active_source = None;
        device.set_selection(None);
        state.cached_snapshot = None;
        state.self_echo_pending = state.self_echo_pending.saturating_add(1);
        return Ok(());
    }

    let source = manager.create_data_source(qh, ());
    let mut payloads: HashMap<String, Arc<Vec<u8>>> = HashMap::new();

    for rep in &snapshot.representations {
        let Some(primary_mime) = rep
            .mime
            .as_ref()
            .map(|m| m.0.clone())
            .or_else(|| super::default_mime_for_format(&rep.format_id).map(String::from))
        else {
            continue;
        };

        // 用 `rep_bytes` 而非 `expect_inline_bytes`：远端 push 的 image rep 由
        // `apply_inbound::materializer` 合成为 `LocalFile` source（指向 blob cache
        // 文件），直接调 `expect_inline_bytes` 会 panic（见
        // `clipboard::payload::rep_bytes` 注释）。读盘失败时跳过该 rep + warn,
        // 与 macOS / Windows 平台同语义。
        let bytes = match crate::clipboard::payload::rep_bytes(rep) {
            Ok(b) => Arc::new(b.into_owned()),
            Err(err) => {
                warn!(
                    error = %err,
                    format_id = %rep.format_id,
                    mime = %primary_mime,
                    "wlr-data-control write: read LocalFile rep failed; skipping this mime"
                );
                continue;
            }
        };

        // Advertise every MIME alias a paster might request (text expands to the
        // full UTF-8 family) so apps like Firefox that only negotiate
        // `text/plain;charset=utf-8` / `UTF8_STRING` can paste. All aliases share
        // the same payload bytes.
        for mime in super::offer_mimes_for(&primary_mime) {
            if payloads.contains_key(&mime) {
                continue;
            }
            source.offer(mime.clone());
            payloads.insert(mime, Arc::clone(&bytes));
        }
    }

    if payloads.is_empty() {
        source.destroy();
        anyhow::bail!("wlr-data-control write: no mime could be derived from snapshot");
    }

    // Do NOT destroy the previous source before installing the new one.
    // `set_selection` atomically replaces the selection and the compositor
    // sends `Cancelled` for the old source, which the source Dispatch handler
    // destroys. Destroying it eagerly here made the compositor emit a spurious
    // `Selection { id: None }` (clear) event *in addition to* the `Selection`
    // for the new source — two self-originated selection events for a single
    // `self_echo_pending += 1`. The clear consumed the only echo token, so the
    // new selection (our own write) was mis-classified as an external change
    // and read back via `build_from_offer` on this very worker thread. That
    // read can only be served by this same thread, so it self-deadlocked until
    // the 2s per-mime read timeout fired — blocking real apps' paste requests
    // for seconds. Replacing without the eager destroy keeps echo accounting
    // balanced (one self `Selection`, one token).
    device.set_selection(Some(&source));
    state.cached_snapshot = Some(snapshot);
    state.self_echo_pending = state.self_echo_pending.saturating_add(1);
    state.active_source = Some(ActiveSource { source, payloads });
    Ok(())
}

// ---------------------------------------------------------------------------
// Worker dispatch impls
// ---------------------------------------------------------------------------

impl Dispatch<WlRegistry, ()> for WorkerState {
    fn event(
        state: &mut Self,
        registry: &WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _conn: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
        {
            match interface.as_str() {
                "wl_seat" if state.seat.is_none() => {
                    let v = version.min(WL_SEAT_VERSION);
                    state.seat = Some(registry.bind::<WlSeat, (), Self>(name, v, qh, ()));
                }
                MANAGER_INTERFACE if state.manager.is_none() => {
                    let v = version.min(ZWLR_DATA_CONTROL_MANAGER_VERSION);
                    state.manager =
                        Some(registry.bind::<ZwlrDataControlManagerV1, (), Self>(name, v, qh, ()));
                }
                _ => {}
            }
        }
    }
}

impl Dispatch<WlSeat, ()> for WorkerState {
    fn event(
        _: &mut Self,
        _: &WlSeat,
        _: wl_seat::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwlrDataControlManagerV1, ()> for WorkerState {
    fn event(
        _: &mut Self,
        _: &ZwlrDataControlManagerV1,
        _: zwlr_data_control_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwlrDataControlDeviceV1, ()> for WorkerState {
    fn event(
        state: &mut Self,
        _device: &ZwlrDataControlDeviceV1,
        event: zwlr_data_control_device_v1::Event,
        _: &(),
        conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_data_control_device_v1::Event::DataOffer { id } => {
                state.offers_in_flight.insert(id.id(), Vec::new());
            }
            zwlr_data_control_device_v1::Event::Selection { id } => {
                let Some(offer) = id else {
                    debug!("wlr-data-control worker: selection cleared");
                    if state.self_echo_pending > 0 {
                        state.self_echo_pending -= 1;
                    } else {
                        state.cached_snapshot = None;
                    }
                    return;
                };
                let oid = offer.id();
                let mimes = state.offers_in_flight.remove(&oid).unwrap_or_default();

                if state.self_echo_pending > 0 {
                    // Echo of our own set_selection. cached_snapshot was set
                    // eagerly in handle_write; trying to read this offer back
                    // would deadlock (Send sits behind us in the queue).
                    state.self_echo_pending -= 1;
                    debug!(
                        ?oid,
                        mime_count = mimes.len(),
                        "wlr-data-control worker: skipping self-echo selection"
                    );
                    offer.destroy();
                    return;
                }

                if mimes.is_empty() {
                    offer.destroy();
                    return;
                }
                match build_from_offer(conn, &offer, &mimes) {
                    Ok(snap) => state.cached_snapshot = Some(snap),
                    Err(e) => warn!(error = %e, "wlr-data-control worker: snapshot capture failed"),
                }
                offer.destroy();
            }
            zwlr_data_control_device_v1::Event::PrimarySelection { id } => {
                if let Some(offer) = id {
                    state.offers_in_flight.remove(&offer.id());
                    offer.destroy();
                }
            }
            zwlr_data_control_device_v1::Event::Finished => {
                debug!("wlr-data-control worker: data_control_device finished");
                state.device = None;
            }
            _ => {}
        }
    }

    event_created_child!(WorkerState, ZwlrDataControlDeviceV1, [
        EVT_DATA_OFFER_OPCODE => (ZwlrDataControlOfferV1, ()),
    ]);
}

impl Dispatch<ZwlrDataControlOfferV1, ()> for WorkerState {
    fn event(
        state: &mut Self,
        offer: &ZwlrDataControlOfferV1,
        event: zwlr_data_control_offer_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zwlr_data_control_offer_v1::Event::Offer { mime_type } = event {
            if let Some(mimes) = state.offers_in_flight.get_mut(&offer.id()) {
                mimes.push(mime_type);
            }
        }
    }
}

impl Dispatch<ZwlrDataControlSourceV1, ()> for WorkerState {
    fn event(
        state: &mut Self,
        source: &ZwlrDataControlSourceV1,
        event: zwlr_data_control_source_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_data_control_source_v1::Event::Send { mime_type, fd } => {
                let active = match &state.active_source {
                    Some(a) if a.source.id() == source.id() => a,
                    _ => {
                        debug!(mime = %mime_type, "wlr-data-control worker: Send for stale source — ignoring");
                        return;
                    }
                };
                match active.payloads.get(&mime_type) {
                    Some(bytes) => {
                        write_payload(fd, bytes, &mime_type);
                    }
                    None => {
                        debug!(
                            mime = %mime_type,
                            "wlr-data-control worker: paster requested mime we don't carry — closing fd"
                        );
                        drop(fd);
                    }
                }
            }
            zwlr_data_control_source_v1::Event::Cancelled => {
                // A source is cancelled either because another client took over
                // the selection, or because we replaced our own source via a
                // fresh `set_selection`. Destroy it in both cases to release the
                // proxy. Only drop `active_source` when the *current* source
                // lost the selection; a cancelled *previous* source is just our
                // own replace cleanup and must not disturb the live one.
                let is_active = state
                    .active_source
                    .as_ref()
                    .map(|active| active.source.id() == source.id())
                    .unwrap_or(false);
                if is_active {
                    debug!("wlr-data-control worker: active source cancelled by compositor");
                    state.active_source = None;
                } else {
                    debug!("wlr-data-control worker: replaced source cancelled — cleaning up");
                }
                source.destroy();
            }
            _ => {}
        }
    }
}
