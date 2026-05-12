//! `ext-data-control-v1` backend (staging Wayland protocol).
//!
//! Used on GNOME mutter ≥ 47, KDE Plasma 6, and any wlroots-based compositor
//! that has caught up with the standardized clipboard manager protocol. Where
//! both `ext-data-control` and `wlr-data-control` are advertised we prefer
//! `ext-data-control` — it's the cross-compositor standard and has wider
//! future support — but the two protocols are bit-identical in shape, so the
//! implementation here is the structural mirror of [`super::wlr`].
//!
//! See [`super::wlr`] for the design notes shared by both backends. Anything
//! protocol-specific (interface names, type paths, version) is captured in
//! the constants and Dispatch impls below.

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
use wayland_protocols::ext::data_control::v1::client::{
    ext_data_control_device_v1::{self, ExtDataControlDeviceV1, EVT_DATA_OFFER_OPCODE},
    ext_data_control_manager_v1::{self, ExtDataControlManagerV1},
    ext_data_control_offer_v1::{self, ExtDataControlOfferV1},
    ext_data_control_source_v1::{self, ExtDataControlSourceV1},
};

use crate::clipboard::event_loop::{PlatformClipboardEventLoop, ShutdownRx};
use crate::clipboard::watcher::ClipboardWatcher;

use super::super::backend::OfferLike;
use super::super::snapshot::build_from_offer;
use super::super::write_payload::write_payload;

const WL_SEAT_VERSION: u32 = 7;
const EXT_DATA_CONTROL_MANAGER_VERSION: u32 = 1;
const FALLBACK_POLL_TIMEOUT_MS: i32 = 250;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
const MANAGER_INTERFACE: &str = "ext_data_control_manager_v1";

impl OfferLike for ExtDataControlOfferV1 {
    fn receive_to(&self, mime: &str, fd: BorrowedFd<'_>) {
        self.receive(mime.to_string(), fd);
    }
}

// ---------------------------------------------------------------------------
// Probe
// ---------------------------------------------------------------------------

pub(super) fn probe(conn: &Connection) -> Result<bool> {
    let mut q = conn.new_event_queue::<ProbeState>();
    let qh = q.handle();
    let _registry = conn.display().get_registry(&qh, ());
    let mut s = ProbeState::default();
    q.roundtrip(&mut s)
        .context("ext-data-control probe roundtrip 1 failed")?;
    q.roundtrip(&mut s)
        .context("ext-data-control probe roundtrip 2 failed")?;
    if !s.has_manager {
        return Ok(false);
    }
    if !s.has_seat {
        warn!("ext-data-control: manager present but no wl_seat");
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
// Watcher
// ---------------------------------------------------------------------------

pub(crate) struct ExtEventLoop {
    conn: Connection,
}

impl ExtEventLoop {
    pub(super) fn with_connection(conn: Connection) -> Self {
        Self { conn }
    }
}

impl PlatformClipboardEventLoop for ExtEventLoop {
    fn run(self: Box<Self>, mut handler: ClipboardWatcher, shutdown_rx: ShutdownRx) -> Result<()> {
        info!("ext-data-control event loop: starting");

        let conn = self.conn;
        let mut event_queue = conn.new_event_queue::<WatcherState>();
        let qh = event_queue.handle();
        let _registry = conn.display().get_registry(&qh, ());

        let mut state = WatcherState::new();

        event_queue
            .roundtrip(&mut state)
            .context("ext-data-control startup roundtrip failed")?;

        let manager = state
            .manager
            .clone()
            .context("ext-data-control manager disappeared after probe")?;
        let seat = state
            .seat
            .clone()
            .context("wl_seat disappeared after probe")?;

        state.device = Some(manager.get_data_device(&seat, &qh, ()));

        event_queue
            .roundtrip(&mut state)
            .context("ext-data-control device-bind roundtrip failed")?;

        for snap in state.pending_snapshots.drain(..) {
            handler.notify_with_snapshot(snap);
        }

        loop {
            event_queue
                .dispatch_pending(&mut state)
                .context("ext-data-control dispatch_pending failed")?;
            for snap in state.pending_snapshots.drain(..) {
                handler.notify_with_snapshot(snap);
            }

            if shutdown_rx.is_signaled() {
                debug!("ext-data-control event loop: shutdown observed before poll");
                break;
            }

            event_queue
                .flush()
                .context("ext-data-control event_queue flush failed")?;

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
                debug!("ext-data-control event loop: shutdown signal received");
                break;
            }

            if wl_revents.contains(PollFlags::IN) {
                if let Err(e) = read_guard.read() {
                    return Err(anyhow::anyhow!(
                        "ext-data-control read events failed: {e:?}"
                    ));
                }
            } else {
                drop(read_guard);
            }
        }

        info!("ext-data-control event loop: stopped");
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Watcher state + dispatch
// ---------------------------------------------------------------------------

struct WatcherState {
    seat: Option<WlSeat>,
    manager: Option<ExtDataControlManagerV1>,
    device: Option<ExtDataControlDeviceV1>,
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
                    let v = version.min(EXT_DATA_CONTROL_MANAGER_VERSION);
                    state.manager =
                        Some(registry.bind::<ExtDataControlManagerV1, (), Self>(name, v, qh, ()));
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

impl Dispatch<ExtDataControlManagerV1, ()> for WatcherState {
    fn event(
        _: &mut Self,
        _: &ExtDataControlManagerV1,
        _: ext_data_control_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ExtDataControlDeviceV1, ()> for WatcherState {
    fn event(
        state: &mut Self,
        _device: &ExtDataControlDeviceV1,
        event: ext_data_control_device_v1::Event,
        _: &(),
        conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        match event {
            ext_data_control_device_v1::Event::DataOffer { id } => {
                state.offers_in_flight.insert(id.id(), Vec::new());
            }
            ext_data_control_device_v1::Event::Selection { id } => {
                let Some(offer) = id else {
                    debug!("ext-data-control: selection cleared");
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
                    Err(e) => warn!(error = %e, "ext-data-control: snapshot capture failed"),
                }
                offer.destroy();
            }
            ext_data_control_device_v1::Event::PrimarySelection { id } => {
                if let Some(offer) = id {
                    state.offers_in_flight.remove(&offer.id());
                    offer.destroy();
                }
            }
            ext_data_control_device_v1::Event::Finished => {
                debug!("ext-data-control: data_control_device finished");
                state.device = None;
            }
            _ => {}
        }
    }

    event_created_child!(WatcherState, ExtDataControlDeviceV1, [
        EVT_DATA_OFFER_OPCODE => (ExtDataControlOfferV1, ()),
    ]);
}

impl Dispatch<ExtDataControlOfferV1, ()> for WatcherState {
    fn event(
        state: &mut Self,
        offer: &ExtDataControlOfferV1,
        event: ext_data_control_offer_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let ext_data_control_offer_v1::Event::Offer { mime_type } = event {
            if let Some(mimes) = state.offers_in_flight.get_mut(&offer.id()) {
                mimes.push(mime_type);
            }
        }
    }
}

impl Dispatch<ExtDataControlSourceV1, ()> for WatcherState {
    fn event(
        _: &mut Self,
        _: &ExtDataControlSourceV1,
        _: ext_data_control_source_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

// ---------------------------------------------------------------------------
// Clipboard read/write
// ---------------------------------------------------------------------------

pub(crate) struct ExtClipboard {
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

impl ExtClipboard {
    pub(super) fn spawn(conn: Connection) -> Result<Self> {
        let wakeup_fd = rustix::event::eventfd(
            0,
            rustix::event::EventfdFlags::CLOEXEC | rustix::event::EventfdFlags::NONBLOCK,
        )
        .context("creating ext-data-control wakeup eventfd")?;

        let worker_wakeup_fd = wakeup_fd
            .try_clone()
            .context("dup ext-data-control wakeup eventfd for worker")?;

        let (request_tx, request_rx) = mpsc::channel::<Request>();

        let worker = std::thread::Builder::new()
            .name("ext-data-control-worker".into())
            .spawn(move || {
                if let Err(e) = worker_main(conn, request_rx, worker_wakeup_fd) {
                    warn!(error = ?e, "ext-data-control worker exited with error");
                }
            })
            .context("spawning ext-data-control worker thread")?;

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
            .map_err(|e| anyhow::anyhow!("ext-data-control worker channel closed: {e}"))?;
        let buf = 1u64.to_ne_bytes();
        if let Err(e) = rustix::io::write(&self.inner.wakeup_fd, &buf) {
            warn!(error = %e, "ext-data-control wakeup write failed");
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
                warn!(?e, "ext-data-control worker thread panicked on join");
            }
        }
    }
}

#[async_trait::async_trait]
impl SystemClipboardPort for ExtClipboard {
    fn read_snapshot(&self) -> Result<SystemClipboardSnapshot> {
        let (tx, rx) = sync_channel::<Result<SystemClipboardSnapshot>>(1);
        self.send_request(Request::Read(tx))?;
        match rx.recv_timeout(REQUEST_TIMEOUT) {
            Ok(res) => res,
            Err(_) => Err(anyhow::anyhow!(
                "ext-data-control read timed out after {:?}",
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
                "ext-data-control write timed out after {:?}",
                REQUEST_TIMEOUT
            )),
        }
    }
}

// ---------------------------------------------------------------------------
// Worker
// ---------------------------------------------------------------------------

struct ActiveSource {
    source: ExtDataControlSourceV1,
    payloads: HashMap<String, Arc<Vec<u8>>>,
}

struct WorkerState {
    seat: Option<WlSeat>,
    manager: Option<ExtDataControlManagerV1>,
    device: Option<ExtDataControlDeviceV1>,
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
    info!("ext-data-control worker: starting");

    let mut event_queue: EventQueue<WorkerState> = conn.new_event_queue();
    let qh = event_queue.handle();
    let _registry = conn.display().get_registry(&qh, ());

    let mut state = WorkerState::new();

    event_queue
        .roundtrip(&mut state)
        .context("ext-data-control worker startup roundtrip failed")?;

    let manager = state
        .manager
        .clone()
        .context("ext-data-control manager disappeared after probe")?;
    let seat = state
        .seat
        .clone()
        .context("wl_seat disappeared after probe")?;

    state.device = Some(manager.get_data_device(&seat, &qh, ()));

    event_queue
        .roundtrip(&mut state)
        .context("ext-data-control worker device-bind roundtrip failed")?;

    loop {
        event_queue
            .dispatch_pending(&mut state)
            .context("ext-data-control worker dispatch_pending failed")?;

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
                    info!("ext-data-control worker: stop request received");
                    return Ok(());
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    info!("ext-data-control worker: request channel disconnected");
                    return Ok(());
                }
            }
        }

        event_queue
            .flush()
            .context("ext-data-control worker event_queue flush failed")?;

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
                    "ext-data-control worker read events failed: {e:?}"
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
    manager: &ExtDataControlManagerV1,
    snapshot: SystemClipboardSnapshot,
) -> Result<()> {
    let device = state
        .device
        .as_ref()
        .context("ext-data-control worker has no data device")?
        .clone();

    if snapshot.representations.is_empty() {
        if let Some(prev) = state.active_source.take() {
            prev.source.destroy();
        }
        device.set_selection(None);
        state.cached_snapshot = None;
        state.self_echo_pending = state.self_echo_pending.saturating_add(1);
        return Ok(());
    }

    let source = manager.create_data_source(qh, ());
    let mut payloads: HashMap<String, Arc<Vec<u8>>> = HashMap::new();

    for rep in &snapshot.representations {
        let mime_str = rep
            .mime
            .as_ref()
            .map(|m| m.0.clone())
            .or_else(|| super::default_mime_for_format(&rep.format_id).map(String::from));

        if let Some(mime) = mime_str {
            if payloads.contains_key(&mime) {
                continue;
            }
            source.offer(mime.clone());
            payloads.insert(mime, Arc::new(rep.bytes.clone()));
        }
    }

    if payloads.is_empty() {
        source.destroy();
        anyhow::bail!("ext-data-control write: no mime could be derived from snapshot");
    }

    if let Some(prev) = state.active_source.take() {
        prev.source.destroy();
    }

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
                    let v = version.min(EXT_DATA_CONTROL_MANAGER_VERSION);
                    state.manager =
                        Some(registry.bind::<ExtDataControlManagerV1, (), Self>(name, v, qh, ()));
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

impl Dispatch<ExtDataControlManagerV1, ()> for WorkerState {
    fn event(
        _: &mut Self,
        _: &ExtDataControlManagerV1,
        _: ext_data_control_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ExtDataControlDeviceV1, ()> for WorkerState {
    fn event(
        state: &mut Self,
        _device: &ExtDataControlDeviceV1,
        event: ext_data_control_device_v1::Event,
        _: &(),
        conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        match event {
            ext_data_control_device_v1::Event::DataOffer { id } => {
                state.offers_in_flight.insert(id.id(), Vec::new());
            }
            ext_data_control_device_v1::Event::Selection { id } => {
                let Some(offer) = id else {
                    debug!("ext-data-control worker: selection cleared");
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
                    state.self_echo_pending -= 1;
                    debug!(
                        ?oid,
                        mime_count = mimes.len(),
                        "ext-data-control worker: skipping self-echo selection"
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
                    Err(e) => warn!(error = %e, "ext-data-control worker: snapshot capture failed"),
                }
                offer.destroy();
            }
            ext_data_control_device_v1::Event::PrimarySelection { id } => {
                if let Some(offer) = id {
                    state.offers_in_flight.remove(&offer.id());
                    offer.destroy();
                }
            }
            ext_data_control_device_v1::Event::Finished => {
                debug!("ext-data-control worker: data_control_device finished");
                state.device = None;
            }
            _ => {}
        }
    }

    event_created_child!(WorkerState, ExtDataControlDeviceV1, [
        EVT_DATA_OFFER_OPCODE => (ExtDataControlOfferV1, ()),
    ]);
}

impl Dispatch<ExtDataControlOfferV1, ()> for WorkerState {
    fn event(
        state: &mut Self,
        offer: &ExtDataControlOfferV1,
        event: ext_data_control_offer_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let ext_data_control_offer_v1::Event::Offer { mime_type } = event {
            if let Some(mimes) = state.offers_in_flight.get_mut(&offer.id()) {
                mimes.push(mime_type);
            }
        }
    }
}

impl Dispatch<ExtDataControlSourceV1, ()> for WorkerState {
    fn event(
        state: &mut Self,
        source: &ExtDataControlSourceV1,
        event: ext_data_control_source_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            ext_data_control_source_v1::Event::Send { mime_type, fd } => {
                let active = match &state.active_source {
                    Some(a) if a.source.id() == source.id() => a,
                    _ => {
                        debug!(mime = %mime_type, "ext-data-control worker: Send for stale source — ignoring");
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
                            "ext-data-control worker: paster requested mime we don't carry — closing fd"
                        );
                        drop(fd);
                    }
                }
            }
            ext_data_control_source_v1::Event::Cancelled => {
                if let Some(active) = &state.active_source {
                    if active.source.id() == source.id() {
                        debug!("ext-data-control worker: source cancelled by compositor");
                        if let Some(prev) = state.active_source.take() {
                            prev.source.destroy();
                        }
                    }
                }
            }
            _ => {}
        }
    }
}
