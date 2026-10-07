package main

import (
	"context"
	"encoding/json"
	"log"
	"net/url"
	"sync"
	"time"

	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/daemonclient"
)

// iconFeed turns daemon state into the tray icon's facts. Every fact is read from the daemon, either as a snapshot over HTTP (every
// refreshEvery, and straight away after an event that can change it) or from the daemon's own WebSocket events; nothing is assumed.
// The state-to-source table is in docs/architecture/gui-go-tray-icon.md.
type iconFeed struct {
	h    *HostService
	icon *trayIcon

	deliveryMu  sync.Mutex
	deliveryDue map[[2]string]struct{} // (entry, target) pairs whose delivery view must be read; repeats within a burst merge
	// deliveryUnresolved holds the pairs whose last read failed, with the time of the first failure. The refresh tick and a stream reconnect
	// put them back into deliveryDue until a read succeeds or deliveryGiveUp has passed, so a failed delivery is not lost to one bad read.
	deliveryUnresolved map[[2]string]time.Time
	deliveryWake       chan struct{} // capacity 1: wakes deliveryWorker

	refreshReq chan struct{} // capacity 1: snapshot reads run one at a time on run's goroutine, so an old answer cannot overwrite a newer one

	mu        sync.Mutex
	transfers map[string]time.Time // transfer id -> last progress seen
	ended     map[string]time.Time // transfer id -> when it ended; late progress for it is ignored for transferTombstone
	timer     *time.Timer          // fires when the oldest running transfer has lasted transferringAfter
	closed    bool
}

const (
	refreshEvery = 10 * time.Second
	// transferringAfter is the design's trigger for the transfer state: "a transfer in progress for more than 1 second".
	transferringAfter = time.Second
	// A running transfer that has been silent this long is treated as over; the daemon's terminal event was missed.
	transferStale = 15 * time.Second
	// transferTombstone is how long an ended transfer's id is remembered, so a progress event that overtakes its terminal event is not a new transfer.
	transferTombstone = 30 * time.Second
	// deliveryGiveUp is how long an unreadable delivery view is retried; maxUnresolvedDeliveries bounds how many are kept.
	deliveryGiveUp          = 5 * time.Minute
	maxUnresolvedDeliveries = 32
	// reconnectDelay is the pause before the event stream is opened again.
	reconnectDelay = 3 * time.Second
)

// iconTopics are the event streams the icon follows.
var iconTopics = []string{"file-transfer", "clipboard", "peers", "device-trust", "content-lock", "paired-devices"}

func newIconFeed(h *HostService, icon *trayIcon) *iconFeed {
	return &iconFeed{h: h, icon: icon, transfers: map[string]time.Time{}, ended: map[string]time.Time{}, deliveryDue: map[[2]string]struct{}{}, deliveryUnresolved: map[[2]string]time.Time{}, deliveryWake: make(chan struct{}, 1), refreshReq: make(chan struct{}, 1)}
}

// requestRefresh asks run for a snapshot read soon; requests that arrive while one is pending merge into it.
func (f *iconFeed) requestRefresh() {
	select {
	case f.refreshReq <- struct{}{}:
	default:
	}
}

// run reads the snapshot facts and follows the event stream until ctx ends.
func (f *iconFeed) run(ctx context.Context) {
	go f.stream(ctx)
	go f.deliveryWorker(ctx)
	f.refresh(ctx)
	ticker := time.NewTicker(refreshEvery)
	defer ticker.Stop()
	for {
		select {
		case <-ctx.Done():
			f.stop()
			return
		case <-ticker.C:
			f.refresh(ctx)
			f.requeueUnresolved()
			f.sweepTransfers()
		case <-f.refreshReq:
			f.refresh(ctx)
		}
	}
}

func (f *iconFeed) stop() {
	f.mu.Lock()
	f.closed = true
	if f.timer != nil {
		f.timer.Stop()
	}
	f.mu.Unlock()
	f.icon.close()
}

// refresh reads the facts that are state rather than events: sync switch, LAN-only, content lock, device reachability.
func (f *iconFeed) refresh(ctx context.Context) {
	cctx, cancel := context.WithTimeout(ctx, 15*time.Second)
	defer cancel()
	var settings struct {
		Sync struct {
			SyncEnabled bool `json:"syncEnabled"`
		} `json:"sync"`
		Network struct {
			AllowRelayFallback bool `json:"allowRelayFallback"`
		} `json:"network"`
	}
	settingsErr := f.h.client.Get(cctx, "/settings", &settings)

	var lock struct {
		Unlocked bool `json:"unlocked"`
	}
	lockErr := f.h.client.Get(cctx, "/content-lock", &lock)
	var setup struct {
		HasCompleted bool `json:"hasCompleted"`
	}
	setupErr := f.h.client.Get(cctx, "/v2/setup/state", &setup)

	var devices []struct {
		Connected bool `json:"connected"`
	}
	devicesErr := f.h.client.Get(cctx, "/paired-devices", &devices)

	// What the user has to decide, as the device page reads it: a trust change to confirm, or a device waiting to be let in.
	var choices struct {
		DeviceTrust struct {
			CurrentChange   json.RawMessage `json:"currentChange"`
			InboundPairings []struct {
				Status string `json:"status"`
			} `json:"inboundPairings"`
		} `json:"deviceTrust"`
	}
	trustErr := f.h.client.Get(cctx, "/member/device-group-choices", &choices)

	before, after := f.icon.update(func(facts *iconFacts) {
		// A read that failed leaves its fact as it was: an unreachable daemon is not evidence of any state.
		if settingsErr == nil {
			facts.syncPaused = !settings.Sync.SyncEnabled
			facts.lanOnly = !settings.Network.AllowRelayFallback
		}
		if lockErr == nil && setupErr == nil {
			// Before the first space exists nothing is locked away: the lock only protects an existing history.
			facts.locked = setup.HasCompleted && !lock.Unlocked
		}
		if devicesErr == nil {
			reachable := false
			for _, d := range devices {
				reachable = reachable || d.Connected
			}
			facts.offline = len(devices) > 0 && !reachable
		}
		if trustErr == nil {
			pending := len(choices.DeviceTrust.CurrentChange) > 0 && string(choices.DeviceTrust.CurrentChange) != "null"
			for _, p := range choices.DeviceTrust.InboundPairings {
				pending = pending || p.Status == "awaiting_confirmation" || p.Status == "needs_attention"
			}
			facts.decisionPending = pending
		}
	})
	if before.base() != baseAttention && after.base() == baseAttention {
		f.icon.animate(animAttention)
	}
}

// stream follows the daemon's WebSocket until ctx ends, reconnecting after a failure.
func (f *iconFeed) stream(ctx context.Context) {
	for ctx.Err() == nil {
		if err := f.follow(ctx); err != nil && ctx.Err() == nil {
			log.Printf("tray icon: event stream: %v", err)
		}
		select {
		case <-ctx.Done():
			return
		case <-time.After(reconnectDelay):
		}
	}
}

func (f *iconFeed) follow(ctx context.Context) error {
	ws, err := f.h.client.DialWS(ctx)
	if err != nil {
		return err
	}
	defer ws.Close()
	if err := ws.Subscribe(ctx, iconTopics...); err != nil {
		return err
	}
	// The snapshot facts may have changed while no stream was open.
	f.requestRefresh()
	f.requeueUnresolved()
	for event := range ws.Events(ctx) {
		f.handle(ctx, event)
	}
	return nil
}

// handle maps one daemon event to the icon. The event names and payloads are the daemon's (crates/uc-daemon-contract constants).
func (f *iconFeed) handle(ctx context.Context, e daemonclient.Event) {
	switch e.Type {
	case "clipboard.new_content":
		var p struct {
			Origin string `json:"origin"`
		}
		if json.Unmarshal(e.Payload, &p) == nil && p.Origin == "remote" {
			if !f.userWatching() { // a focused window already shows the content
				f.icon.update(func(facts *iconFacts) { facts.newContent = true })
				f.icon.animate(animNewContent)
			}
		}
	case "file-transfer.progress":
		var p struct {
			TransferID string `json:"transferId"`
		}
		if json.Unmarshal(e.Payload, &p) == nil && p.TransferID != "" {
			f.transferSeen(p.TransferID)
		}
	case "file-transfer.status_changed":
		var p struct {
			TransferID string `json:"transferId"`
			Status     string `json:"status"`
		}
		if json.Unmarshal(e.Payload, &p) == nil && transferEnded(p.Status) {
			f.transferEnded(p.TransferID)
		}
	case "clipboard.delivery_status_changed":
		var p struct {
			EntryID        string `json:"entryId"`
			TargetDeviceID string `json:"targetDeviceId"`
		}
		if json.Unmarshal(e.Payload, &p) == nil {
			f.queueDelivery(p.EntryID, p.TargetDeviceID)
		}
	case "content_lock.changed", "device-trust.changed", "peers.changed", "peers.connectionChanged", "paired-devices.changed", "paired-devices.snapshot", "peers.snapshot":
		f.requestRefresh()
	}
}

func transferEnded(status string) bool {
	switch status {
	case "completed", "failed", "cancelled", "canceled":
		return true
	}
	return false
}

// raiseAttention sets an attention fact and plays the attention shake when it was not showing yet.
func (f *iconFeed) raiseAttention(set func(*iconFacts)) {
	before, after := f.icon.update(set)
	if before.base() != baseAttention && after.base() == baseAttention {
		f.icon.animate(animAttention)
	}
}

// queueDelivery asks for a delivery read. Reads run one at a time on deliveryWorker and a pair already waiting is not queued twice,
// so a burst of events costs a bounded number of requests and none is lost.
func (f *iconFeed) queueDelivery(entryID, targetID string) {
	if entryID == "" {
		return
	}
	f.deliveryMu.Lock()
	f.deliveryDue[[2]string{entryID, targetID}] = struct{}{}
	f.deliveryMu.Unlock()
	select {
	case f.deliveryWake <- struct{}{}:
	default:
	}
}

func (f *iconFeed) deliveryWorker(ctx context.Context) {
	for {
		select {
		case <-ctx.Done():
			return
		case <-f.deliveryWake:
		}
		f.deliveryMu.Lock()
		due := f.deliveryDue
		f.deliveryDue = map[[2]string]struct{}{}
		f.deliveryMu.Unlock()
		for pair := range due {
			if ctx.Err() != nil {
				return
			}
			err := f.deliveryChanged(ctx, pair[0], pair[1])
			f.deliveryMu.Lock()
			if err == nil {
				delete(f.deliveryUnresolved, pair)
			} else if _, known := f.deliveryUnresolved[pair]; !known {
				f.markUnresolvedLocked(pair)
				log.Printf("tray icon: delivery view of entry %s unreadable, will retry: %v", pair[0], err)
			}
			f.deliveryMu.Unlock()
		}
	}
}

// markUnresolvedLocked records the first failure of a pair; when the set is full the oldest pair is dropped. The caller holds deliveryMu.
func (f *iconFeed) markUnresolvedLocked(pair [2]string) {
	if len(f.deliveryUnresolved) >= maxUnresolvedDeliveries {
		var oldest [2]string
		var oldestAt time.Time
		for p, at := range f.deliveryUnresolved {
			if oldestAt.IsZero() || at.Before(oldestAt) {
				oldest, oldestAt = p, at
			}
		}
		delete(f.deliveryUnresolved, oldest)
	}
	f.deliveryUnresolved[pair] = time.Now()
}

// requeueUnresolved puts every pair whose read failed back into the work set, dropping those that have been failing for deliveryGiveUp.
// It runs on the refresh tick and after a stream reconnect, so there is no timer of its own and a pair is never queued twice.
func (f *iconFeed) requeueUnresolved() {
	f.deliveryMu.Lock()
	for pair, since := range f.deliveryUnresolved {
		if time.Since(since) > deliveryGiveUp {
			delete(f.deliveryUnresolved, pair)
			continue
		}
		f.deliveryDue[pair] = struct{}{}
	}
	pending := len(f.deliveryDue) > 0
	f.deliveryMu.Unlock()
	if pending {
		select {
		case f.deliveryWake <- struct{}{}:
		default:
		}
	}
}

// deliveryChanged reads what happened to one entry's delivery to one device; the event only says that something did.
func (f *iconFeed) deliveryChanged(ctx context.Context, entryID, targetID string) error {
	if entryID == "" {
		return nil
	}
	e2eTrayIconDeliveryRead(entryID, targetID)
	cctx, cancel := context.WithTimeout(ctx, 10*time.Second)
	defer cancel()
	var view struct {
		Deliveries []struct {
			TargetDeviceID string `json:"targetDeviceId"`
			Status         struct {
				Tag string `json:"tag"`
			} `json:"status"`
		} `json:"deliveries"`
	}
	if err := f.h.client.Get(cctx, "/clipboard/entries/"+url.PathEscape(entryID)+"/delivery", &view); err != nil {
		return err
	}
	delivered := false
	for _, d := range view.Deliveries {
		if targetID != "" && d.TargetDeviceID != targetID {
			continue
		}
		switch d.Status.Tag {
		case "failed": // a failure anywhere outranks a delivery to another device
			if !f.userWatching() {
				f.raiseAttention(func(facts *iconFacts) { facts.sendFailed = true })
			}
			return nil
		case "delivered":
			delivered = true
		}
	}
	if delivered {
		f.icon.animate(animSent)
	}
	return nil
}

// userWatching reports that the main window has the focus: the user sees new content and delivery results themselves.
func (f *iconFeed) userWatching() bool {
	w, ok := f.h.app.Window.GetByName("main")
	return ok && w.IsVisible() && w.IsFocused()
}

// userLooked is the user opening a window or the quick panel: what asked for their attention has been seen.
func (f *iconFeed) userLooked() {
	f.icon.update(func(facts *iconFacts) {
		facts.newContent = false
		facts.sendFailed = false
	})
}

// transferSeen records progress. The transfer state starts only when one transfer has run for transferringAfter.
func (f *iconFeed) transferSeen(id string) {
	f.mu.Lock()
	defer f.mu.Unlock()
	if f.closed {
		return
	}
	if at, gone := f.ended[id]; gone && time.Since(at) < transferTombstone {
		return // progress that arrived after the terminal event
	}
	_, known := f.transfers[id]
	f.transfers[id] = time.Now()
	if !known && f.timer == nil {
		f.timer = time.AfterFunc(transferringAfter, f.transferLongEnough)
	}
}

// transferLongEnough runs under f.mu through the whole decision, so a transfer that ends meanwhile cannot be followed by a stale "transferring".
// f.mu is always taken before the icon's lock, never the other way round.
func (f *iconFeed) transferLongEnough() {
	f.mu.Lock()
	f.timer = nil
	if f.closed || len(f.transfers) == 0 {
		f.mu.Unlock()
		return
	}
	before, after := f.icon.update(func(facts *iconFacts) { facts.transferring = true })
	f.mu.Unlock()
	if !before.transferring && after.transferring {
		f.icon.animate(animTransferring)
		f.mu.Lock() // the transfer may have ended while the animation was being started
		if len(f.transfers) == 0 {
			f.icon.stopAnimation(animTransferring)
		}
		f.mu.Unlock()
	}
}

func (f *iconFeed) transferEnded(id string) {
	f.mu.Lock()
	if id != "" {
		f.ended[id] = time.Now()
	}
	delete(f.transfers, id)
	if len(f.transfers) == 0 {
		f.clearTransferringLocked()
	}
	f.mu.Unlock()
}

// sweepTransfers drops transfers that went silent without a terminal event, and forgets old tombstones.
func (f *iconFeed) sweepTransfers() {
	f.mu.Lock()
	defer f.mu.Unlock()
	for id, seen := range f.transfers {
		if time.Since(seen) > transferStale {
			delete(f.transfers, id)
		}
	}
	for id, at := range f.ended {
		if time.Since(at) > transferTombstone {
			delete(f.ended, id)
		}
	}
	if len(f.transfers) == 0 {
		f.clearTransferringLocked()
	}
}

// clearTransferringLocked ends the transfer state; the caller holds f.mu.
func (f *iconFeed) clearTransferringLocked() {
	if f.timer != nil {
		f.timer.Stop()
		f.timer = nil
	}
	f.icon.update(func(facts *iconFacts) { facts.transferring = false })
	f.icon.stopAnimation(animTransferring)
}
