package engine

import (
	"context"
	"log/slog"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"time"

	"github.com/uniclipboard/unic/internal/clip"
	"github.com/uniclipboard/unic/internal/peerbackend"
	"github.com/uniclipboard/unic/internal/store"
	"github.com/uniclipboard/unic/plugin"
)

// recvDirName is where incoming files are written, inside InboxDir. The engine
// owns the whole directory and empties it at startup and at Close.
const recvDirName = "unic-recv"

func (e *Engine) recvDir() string { return filepath.Join(e.opts.InboxDir, recvDirName) }

// receipt is an entry of the received list; it holds a reference on its content.
type receipt struct {
	ReceivedContent
	c       *contentObj
	backend PluginID // the backend it came through; its policy decides what can be applied
	expires time.Time
	locked  bool // being applied: must not be evicted (set by the apply flow)
}

// receiveRequest is what a backend tells Engine about an incoming clip before
// any content is read. source is the authenticated sender; origin is what the
// sender claims and must equal it, since nothing is forwarded.
type receiveRequest struct {
	backend  PluginID
	source   DeviceID
	channel  string // set instead of a device when the clip came from an external channel
	origin   string
	transfer string // sender-chosen transfer identity, shared by every backend of one commit
	view     string // hash of the exact view that is sent
	total    int64
	parts    []receivePart
}

type receivePart struct {
	file bool
	size int64
	mime string // formats only
}

// receiveTicket follows one admitted receive until it ends.
type receiveTicket struct {
	started       time.Time
	kind          DiagnosticContentKind
	backend       PluginID
	source        DeviceID
	channel       string
	key           store.DedupKey
	reserved      int64
	cacheReserved int64
}

func (e *Engine) dedupRecord(k store.DedupKey) store.Dedup {
	return store.Dedup{Key: k, NowMs: time.Now().UnixMilli(), TTLMs: e.res.SyncDedupTTL.Milliseconds(), Max: e.res.MaxSyncDedupEntries}
}

// allowsAny reports whether the policy lets at least one of the parts through.
func allowsAny(parts []receivePart, pol BackendSyncPolicy) bool {
	for _, p := range parts {
		switch {
		case p.file && pol.Files, !p.file && strings.HasPrefix(p.mime, "image/") && pol.Images,
			!p.file && !strings.HasPrefix(p.mime, "image/") && pol.Text:
			return true
		}
	}
	return false
}

// admitReceive decides whether an incoming clip is wanted before any byte of
// it is read: the engine must be running, sync on, the sender the clip's own
// origin, the backend's policy must allow some part, the same view must not
// have arrived (or be arriving) through another backend, and room must be
// found. Declining makes the backend read and drop the clip. Making room
// drops the oldest received entries that nobody is using.
func (e *Engine) admitReceive(req receiveRequest) (admission peerbackend.Admission, accepted bool) {
	started := time.Now()
	kind := DiagnosticContentKind("unknown")
	for _, p := range req.parts {
		k := DiagnosticContentKind("text")
		if p.file {
			k = "files"
		} else if strings.HasPrefix(p.mime, "image/") {
			k = "image"
		}
		if kind == "unknown" {
			kind = k
		} else if kind != k {
			kind = "mixed"
		}
	}
	outcome, code := DiagnosticResult("rejected"), CodeInvalidArgument
	defer func() {
		if !accepted {
			e.diagnostic(context.Background(), DiagnosticRecord{Stage: "receive", Result: outcome, Code: code, Backend: diagnosticBackend(req.backend), ContentKind: kind, Duration: time.Since(started)})
		}
	}()
	total := req.total
	if total == 0 || req.origin != string(req.source) || req.transfer == "" || req.view == "" {
		return peerbackend.Admission{}, false
	}
	srcKey := string(req.source)
	if req.channel != "" {
		srcKey = "channel:" + req.channel
	}
	key := store.DedupKey{Source: srcKey, Transfer: req.transfer, View: req.view}
	e.mu.Lock()
	ok := e.life == LifecycleRunning && e.runtime == RuntimeActive && e.settings.Value.Sync.Enabled
	pol := e.policyLocked(req.backend)
	_, arriving := e.dedupInflight[key]
	e.mu.Unlock()
	filesBlocked := false
	for _, p := range req.parts {
		filesBlocked = filesBlocked || (p.file && !pol.Files)
	}
	if !ok || arriving || filesBlocked || !allowsAny(req.parts, pol) {
		outcome, code = "policy_filtered", ""
		if arriving {
			outcome = "duplicate"
		}
		return peerbackend.Admission{}, false
	}
	if seen, err := e.st.HasDedup(key, time.Now().UnixMilli(), e.res.SyncDedupTTL.Milliseconds()); err != nil || seen {
		if seen {
			outcome, code = "duplicate", ""
		} else {
			outcome, code = "failed", CodeInternal
		}
		return peerbackend.Admission{}, false
	}
	e.mu.Lock()
	if _, arriving := e.dedupInflight[key]; arriving { // lost a race with another backend
		e.mu.Unlock()
		outcome, code = "duplicate", ""
		return peerbackend.Admission{}, false
	}
	e.dedupInflight[key] = struct{}{}
	e.mu.Unlock()
	release := func() {
		e.mu.Lock()
		delete(e.dedupInflight, key)
		e.mu.Unlock()
	}
	var fileBytes int64
	for _, p := range req.parts {
		if p.file {
			fileBytes += p.size
		}
	}
	cacheBytes := int64(0)
	if e.opts.StoreReceivedFile == nil && fileBytes > 0 {
		fits := e.reserveCachedFiles(fileBytes)
		if !fits {
			outcome, code = "rejected", CodeResourceExhausted
			release()
			e.commit(EventResourceWarning)
			return peerbackend.Admission{}, false
		}
		cacheBytes = fileBytes
	}
	if !e.reserveStored(total, true) {
		outcome, code = "rejected", CodeResourceExhausted
		if cacheBytes > 0 {
			e.cache.cancel(cacheBytes)
		}
		release()
		e.commit(EventResourceWarning)
		e.logf(slog.LevelWarn, "dropped incoming content", "reason", "storage_full")
		return peerbackend.Admission{}, false
	}
	t := receiveTicket{started: started, kind: kind, backend: req.backend, source: req.source, channel: req.channel, key: key, reserved: total, cacheReserved: cacheBytes}
	return peerbackend.Admission{
		Inbox: e.recvDir(),
		Done: func(snap clip.Snapshot, dir string, ok bool) {
			defer release() // the committed record, if any, replaces the placeholder
			e.delivered(t, snap, dir, ok)
		},
	}, true
}

// evictOldest drops the oldest received entry that is not in use, reporting
// whether there was one. With needSpace it only considers entries whose
// removal frees storage, i.e. whose content nobody else holds.
func (e *Engine) evictOldest(needSpace bool) bool {
	e.mu.Lock()
	var victim *receipt
	for i, rc := range e.recv {
		if needSpace {
			e.cs.mu.Lock()
			shared := rc.c.refs > 1
			e.cs.mu.Unlock()
			if shared {
				continue
			}
		}
		if !rc.locked {
			victim = rc
			e.recv = append(e.recv[:i:i], e.recv[i+1:]...)
			break
		}
	}
	if victim != nil {
		e.commitObjsLocked([]ObjectRef{{Type: "received", ID: string(victim.ReceiptID)}}, EventReceived)
		e.rescheduleRecvLocked()
	}
	e.mu.Unlock()
	if victim == nil {
		return false
	}
	e.cs.unref(victim.c)
	return true
}

// delivered turns a fully received clip into content and lists it. reserved is
// what Admit set aside.
func (e *Engine) delivered(t receiveTicket, snap clip.Snapshot, dir string, ok bool) {
	outcome, code := DiagnosticResult("failed"), CodeInternal
	defer func() {
		e.diagnostic(context.Background(), DiagnosticRecord{Stage: "receive", Result: outcome, Code: code, Backend: diagnosticBackend(t.backend), ContentKind: t.kind, Duration: time.Since(t.started)})
	}()
	peer, reserved := t.source, t.reserved
	cacheRemaining := t.cacheReserved
	if t.cacheReserved > 0 {
		defer func() { e.cache.cancel(cacheRemaining) }()
	}
	if t.channel != "" {
		peer = "" // a channel is not a device
	}
	giveBack := func(n int64) {
		e.cs.mu.Lock()
		e.cs.stored -= n
		e.cs.mu.Unlock()
	}
	e.mu.Lock()
	enabled := e.settings.Value.Sync.Enabled
	sy := e.policyLocked(t.backend)
	running := e.life == LifecycleRunning && e.runtime == RuntimeActive
	e.mu.Unlock()
	if !ok || !running || !enabled {
		if !running {
			outcome, code = "cancelled", CodeCancelled
		} else if !enabled {
			outcome, code = "policy_filtered", ""
		} else {
			code = CodeUnavailable
		}
		giveBack(reserved)
		if dir != "" {
			_ = os.RemoveAll(dir)
		}
		return
	}

	// Keep what the settings allow (0002): images, other formats ("text"), files.
	c := &contentObj{id: ContentID(e.newID()), dir: dir, refs: 1}
	n := 0
	next := func() PartID { n++; return PartID("p" + strconv.Itoa(n)) }
	for _, p := range snap.Parts {
		if strings.HasPrefix(p.Format, "image/") && !sy.Images || !strings.HasPrefix(p.Format, "image/") && !sy.Text {
			continue
		}
		size := int64(len(p.Data))
		c.parts = append(c.parts, &partRec{data: p.Data, desc: PartDescriptor{ID: next(), Kind: PartRepresentation, MIME: p.Format, Size: size}})
		c.total += size
	}
	if sy.Files {
		for _, f := range snap.Files {
			c.parts = append(c.parts, &partRec{path: f.Path, desc: PartDescriptor{ID: next(), Kind: PartFile, MIME: "application/octet-stream", Name: f.Name, Size: f.Size}})
			c.total += f.Size
		}
	} else if dir != "" {
		_ = os.RemoveAll(dir)
		c.dir = ""
	}
	c.owned = c.total
	if len(c.parts) == 0 {
		outcome, code = "policy_filtered", ""
		giveBack(reserved)
		if c.dir != "" {
			_ = os.RemoveAll(c.dir)
		}
		return
	}
	giveBack(reserved - c.owned)
	e.cs.mu.Lock()
	e.cs.contents[c.id] = c
	e.cs.mu.Unlock()
	e.mu.Lock()
	workCtx := e.runCtx
	e.mu.Unlock()
	origin := pipelineOrigin{kind: "remote_device", deviceID: peer}
	if t.channel != "" {
		origin = pipelineOrigin{kind: "external", pluginID: string(t.backend)}
	}
	processed, err := e.processContent(workCtx, pipelineReceived, c, origin)
	if err != nil {
		outcome, code = diagnosticOutcome(err)
		return // processContent released the input and any prepared replacement
	}
	c = processed.c
	defer e.cs.unref(c)
	oldOwned := c.owned
	defer func() {
		for _, p := range c.parts {
			if p.cache != "" {
				cacheRemaining -= p.desc.Size
			}
		}
	}()
	if ferr := e.finalizeReceivedFiles(workCtx, c); ferr != nil {
		outcome, code = diagnosticOutcome(ferr)
		giveBack(oldOwned - c.owned)
		return
	}
	giveBack(oldOwned - c.owned)
	if c.dir != "" {
		_ = os.RemoveAll(c.dir)
		c.dir = ""
	}
	e.cs.mu.Lock()
	desc := e.cs.descriptor(c)
	e.cs.mu.Unlock()

	// A list slot is reserved before anything is stored, so that a full list
	// cannot be discovered after the history record and dedup entry exist.
	live := func() bool {
		return e.life == LifecycleRunning && e.runtime == RuntimeActive && e.settings.Value.Sync.Enabled &&
			e.policyLocked(t.backend) == sy && workCtx.Err() == nil
	}
	allowed := func() (bool, error) { return e.allowedPeerOn(string(t.backend), string(peer)) }
	autoApply := false
	if t.channel != "" {
		allowed = func() (bool, error) { return e.channelAllowed(t.backend, t.channel) }
		if ch, cerr := e.st.Channel(t.channel); cerr == nil {
			autoApply = ch.AutoApply
		}
	}
	if !e.reserveRecvSlot(live, allowed) {
		outcome, code = "rejected", CodeResourceExhausted
		return
	}
	slot := true // a reserved slot is given back unless it is published
	defer func() {
		if slot {
			e.mu.Lock()
			e.recvReserved--
			e.mu.Unlock()
		}
	}()

	var hid HistoryID
	dedup := e.dedupRecord(t.key)
	if processed.policy.Persist {
		var perr *Error
		if hid, perr = e.persistHistory(workCtx, processed, &dedup); perr != nil {
			outcome, code = diagnosticOutcome(perr)
			return // no half-published entry; the receive fails without a record
		}
	} else if err := e.st.PutDedup(dedup); err != nil {
		return // the receive is not recorded as done, so it is not delivered either
	}
	now := time.Now()
	rc := &receipt{c: c, backend: t.backend, expires: now.Add(e.res.ReceivedTTL), ReceivedContent: ReceivedContent{
		ReceiptID: ReceiptID(e.newID()), Content: desc, PeerID: peer, ChannelID: t.channel, AutoApply: autoApply, ReceivedAtMs: now.UnixMilli(),
		ExpiresAtMs: now.Add(e.res.ReceivedTTL).UnixMilli(), ApplyState: ApplyAvailable, HistoryID: hid,
	}}
	// The commit point: state and trust may have moved on while the record was
	// written, in which case the record and the dedup entry are taken back.
	e.mu.Lock()
	trusted, trustErr := allowed()
	if !live() || trustErr != nil || !trusted {
		outcome, code = "rejected", CodePermissionDenied
		e.mu.Unlock()
		if hid != "" {
			e.removeHistory(hid)
		}
		_ = e.st.DeleteDedup(t.key)
		return
	}
	e.cs.mu.Lock()
	c.refs++ // Received has its own reference before processing ends
	e.cs.mu.Unlock()
	e.recvReserved--
	slot = false
	e.recv = append(e.recv, rc)
	e.commitObjsLocked([]ObjectRef{{Type: "received", ID: string(rc.ReceiptID)}}, EventReceived)
	e.rescheduleRecvLocked()
	e.mu.Unlock()
	if hid != "" {
		e.notifyObservers(plugin.EventCommitted, string(hid))
	}
	outcome, code = "succeeded", ""
}

// reserveRecvSlot sets one place in the received list aside, making room by
// dropping the oldest entry nobody is applying. It fails, without side
// effects, if the engine moved on, the sender is no longer trusted or every
// entry is in use.
func (e *Engine) reserveRecvSlot(live func() bool, allowed func() (bool, error)) bool {
	for {
		e.mu.Lock()
		trusted, err := allowed()
		if !live() || err != nil || !trusted {
			e.mu.Unlock()
			return false
		}
		if len(e.recv)+e.recvReserved < e.res.MaxReceived {
			e.recvReserved++
			e.mu.Unlock()
			return true
		}
		e.mu.Unlock()
		if !e.evictOldest(false) {
			e.commit(EventResourceWarning)
			return false
		}
	}
}

// rescheduleRecvLocked arms the timer for the next expiry. e.mu must be held.
func (e *Engine) rescheduleRecvLocked() {
	if e.recvTimer != nil {
		e.recvTimer.Stop()
		e.recvTimer = nil
	}
	if len(e.recv) == 0 {
		return
	}
	next := e.recv[0].expires
	for _, r := range e.recv {
		if r.expires.Before(next) {
			next = r.expires
		}
	}
	e.recvTimer = time.AfterFunc(time.Until(next), e.expireReceived)
}

func (e *Engine) expireReceived() {
	now := time.Now()
	e.mu.Lock()
	var gone []*receipt
	keep := e.recv[:0:0]
	for _, r := range e.recv {
		if !r.locked && !now.Before(r.expires) {
			gone = append(gone, r)
		} else {
			keep = append(keep, r)
		}
	}
	e.recv = keep
	if len(gone) > 0 {
		var refs []ObjectRef
		for _, r := range gone {
			refs = append(refs, ObjectRef{Type: "received", ID: string(r.ReceiptID)})
		}
		e.commitObjsLocked(refs, EventReceived)
	}
	e.rescheduleRecvLocked()
	e.mu.Unlock()
	for _, r := range gone {
		e.cs.unref(r.c)
	}
}

// receivedLocked lists the entries that have not expired, newest first.
func (e *Engine) receivedLocked(now time.Time) []ReceivedContent {
	out := make([]ReceivedContent, 0, len(e.recv))
	for i := len(e.recv) - 1; i >= 0; i-- {
		if r := e.recv[i]; r.locked || now.Before(r.expires) { // being applied keeps an entry listed
			out = append(out, r.ReceivedContent)
		}
	}
	return out
}

// ListReceived returns what arrived and is still listed, newest first.
func (e *Engine) ListReceived(ctx context.Context) ([]ReceivedContent, error) {
	release, gerr := e.enter(ctx)
	if gerr != nil {
		return nil, gerr
	}
	defer release()
	e.mu.Lock()
	rs := e.receivedLocked(time.Now())
	e.mu.Unlock()
	for i := range rs {
		rs[i].Content.Parts = append([]PartDescriptor{}, rs[i].Content.Parts...)
	}
	return rs, nil
}

// DismissReceived removes an entry from the list. Dismissing one that is gone
// succeeds. Content a host has retained stays available to it.
func (e *Engine) DismissReceived(ctx context.Context, id ReceiptID) error {
	release, gerr := e.enter(ctx)
	if gerr != nil {
		return gerr
	}
	defer release()
	e.mu.Lock()
	var gone *receipt
	for i, r := range e.recv {
		if r.ReceiptID == id {
			if r.locked {
				e.mu.Unlock()
				return newErr(CodeInUse, "the content is being applied")
			}
			gone = r
			e.recv = append(e.recv[:i:i], e.recv[i+1:]...)
			break
		}
	}
	if gone != nil {
		e.commitObjsLocked([]ObjectRef{{Type: "received", ID: string(id)}}, EventReceived)
		e.rescheduleRecvLocked()
	}
	e.mu.Unlock()
	if gone != nil {
		e.cs.unref(gone.c)
	}
	return nil
}
