//! Wayland event dispatch state for the data-control client.
//!
//! `State` owns the set of bound globals and the in-flight offer table. It
//! gets handed to `EventQueue::dispatch_pending` / `roundtrip` and the
//! `Dispatch` impls below populate it from compositor events.
//!
//! Snapshot capture happens **inline inside the `Selection` event handler**:
//! the compositor has just told us "the clipboard now points at this offer",
//! we pull the bytes via `pipe + receive` (synchronous, bounded by the
//! per-mime timeout in `transfer.rs`), build a `SystemClipboardSnapshot`,
//! and queue it on `pending_snapshots` for the main loop in
//! [`super::event_loop`] to pick up after `dispatch_pending` returns.
//!
//! Why inline rather than queueing the offer for later: keeping the offer
//! proxy alive across iterations of the dispatch loop is awkward (its
//! lifecycle is tied to the most recent Selection), and `pipe_receive`
//! interleaves cleanly with the wayland connection because all wayland
//! traffic is single-threaded — the compositor can buffer further events
//! while we drain the pipe.

use std::collections::HashMap;

use tracing::{debug, warn};
use uc_core::clipboard::SystemClipboardSnapshot;
use wayland_client::{
    backend::ObjectId,
    event_created_child,
    protocol::{wl_registry, wl_registry::WlRegistry, wl_seat, wl_seat::WlSeat},
    Connection, Dispatch, Proxy, QueueHandle,
};
use wayland_protocols_wlr::data_control::v1::client::{
    zwlr_data_control_device_v1::{self, ZwlrDataControlDeviceV1, EVT_DATA_OFFER_OPCODE},
    zwlr_data_control_manager_v1::{self, ZwlrDataControlManagerV1},
    zwlr_data_control_offer_v1::{self, ZwlrDataControlOfferV1},
    zwlr_data_control_source_v1::{self, ZwlrDataControlSourceV1},
};

use super::snapshot;

const WL_SEAT_VERSION: u32 = 7;
const ZWLR_DATA_CONTROL_MANAGER_VERSION: u32 = 2;

pub(super) struct State {
    pub(super) seat: Option<WlSeat>,
    pub(super) manager: Option<ZwlrDataControlManagerV1>,
    pub(super) device: Option<ZwlrDataControlDeviceV1>,

    /// Mime types collected per offer between the `data_offer` event (which
    /// births the offer proxy) and the `selection` event (which makes one of
    /// them the active one). Keyed by Wayland object id.
    offers_in_flight: HashMap<ObjectId, Vec<String>>,

    /// Snapshots produced by Selection event handlers, ready for the main
    /// loop to forward to the watcher.
    pub(super) pending_snapshots: Vec<SystemClipboardSnapshot>,
}

impl State {
    pub(super) fn new() -> Self {
        Self {
            seat: None,
            manager: None,
            device: None,
            offers_in_flight: HashMap::new(),
            pending_snapshots: Vec::new(),
        }
    }
}

impl Dispatch<WlRegistry, ()> for State {
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
                "wl_seat" => {
                    if state.seat.is_none() {
                        let v = version.min(WL_SEAT_VERSION);
                        state.seat = Some(registry.bind::<WlSeat, (), Self>(name, v, qh, ()));
                    }
                }
                "zwlr_data_control_manager_v1" => {
                    if state.manager.is_none() {
                        let v = version.min(ZWLR_DATA_CONTROL_MANAGER_VERSION);
                        state.manager = Some(registry.bind::<ZwlrDataControlManagerV1, (), Self>(
                            name,
                            v,
                            qh,
                            (),
                        ));
                    }
                }
                _ => {}
            }
        }
    }
}

impl Dispatch<WlSeat, ()> for State {
    fn event(
        _state: &mut Self,
        _seat: &WlSeat,
        _event: wl_seat::Event,
        _: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        // wl_seat events (capabilities, name) aren't relevant to clipboard
        // tracking; we just need the seat handle for binding the device.
    }
}

impl Dispatch<ZwlrDataControlManagerV1, ()> for State {
    fn event(
        _state: &mut Self,
        _manager: &ZwlrDataControlManagerV1,
        _event: zwlr_data_control_manager_v1::Event,
        _: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        // Manager has no events.
    }
}

impl Dispatch<ZwlrDataControlDeviceV1, ()> for State {
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
                let oid = id.id();
                debug!(?oid, "wayland: new data_offer");
                state.offers_in_flight.insert(oid, Vec::new());
            }
            zwlr_data_control_device_v1::Event::Selection { id } => {
                let Some(offer) = id else {
                    debug!("wayland: selection cleared");
                    return;
                };
                let oid = offer.id();
                let mimes = state.offers_in_flight.remove(&oid).unwrap_or_default();
                debug!(?oid, mime_count = mimes.len(), "wayland: selection");

                if mimes.is_empty() {
                    debug!("wayland: selection offer had no mimes");
                    offer.destroy();
                    return;
                }

                match snapshot::build_from_offer(conn, &offer, &mimes) {
                    Ok(snap) => state.pending_snapshots.push(snap),
                    Err(e) => warn!(error = %e, "wayland: snapshot capture failed"),
                }

                offer.destroy();
            }
            zwlr_data_control_device_v1::Event::PrimarySelection { id } => {
                // Primary selection (X11 middle-click semantics) is intentionally
                // not synced — clipboard sync should require an explicit copy.
                if let Some(offer) = id {
                    let oid = offer.id();
                    state.offers_in_flight.remove(&oid);
                    offer.destroy();
                }
            }
            zwlr_data_control_device_v1::Event::Finished => {
                debug!("wayland: data_control_device finished");
                state.device = None;
            }
            _ => {}
        }
    }

    // The `data_offer` event creates a fresh `ZwlrDataControlOfferV1` proxy.
    // wayland-client requires the parent's Dispatch impl to declare which
    // user data the new child gets, otherwise it panics on first delivery.
    event_created_child!(State, ZwlrDataControlDeviceV1, [
        EVT_DATA_OFFER_OPCODE => (ZwlrDataControlOfferV1, ()),
    ]);
}

impl Dispatch<ZwlrDataControlOfferV1, ()> for State {
    fn event(
        state: &mut Self,
        offer: &ZwlrDataControlOfferV1,
        event: zwlr_data_control_offer_v1::Event,
        _: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if let zwlr_data_control_offer_v1::Event::Offer { mime_type } = event {
            let oid = offer.id();
            if let Some(mimes) = state.offers_in_flight.get_mut(&oid) {
                mimes.push(mime_type);
            } else {
                debug!(
                    ?oid,
                    "wayland: offer event for unknown offer (already consumed?)"
                );
            }
        }
    }
}

impl Dispatch<ZwlrDataControlSourceV1, ()> for State {
    fn event(
        _state: &mut Self,
        _src: &ZwlrDataControlSourceV1,
        _event: zwlr_data_control_source_v1::Event,
        _: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        // Source events (Send / Cancelled) are produced when *we* own the
        // selection — Phase 2b will handle them to implement
        // `WaylandClipboard::write_snapshot`. Phase 2a is read-only.
    }
}
