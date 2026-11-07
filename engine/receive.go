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
	"github.com/uniclipboard/unic/internal/session"
	"github.com/uniclipboard/unic/internal/wire"
)

// recvDirName is where incoming files are written, inside InboxDir. The engine
// owns the whole directory and empties it at startup and at Close.
const recvDirName = "unic-recv"

func (e *Engine) recvDir() string { return filepath.Join(e.opts.InboxDir, recvDirName) }

// receipt is an entry of the received list; it holds a reference on its content.
type receipt struct {
	ReceivedContent
	c       *contentObj
	expires time.Time
	locked  bool // being applied: must not be evicted (set by the apply flow)
}

// receiver adapts the engine to the session layer's Receiver.
type receiver struct{ e *Engine }

// Admit reserves room for an incoming clip, making room by dropping the oldest
// received entries that nobody is using. When there is none the clip is read
// and discarded, which keeps the connection in step.
func (r receiver) Admit(peer string, m wire.Clip) (session.Admission, bool) {
	e := r.e
	var total int64
	for _, p := range m.Parts {
		total += p.Size
	}
	if total == 0 {
		return session.Admission{}, false
	}
	e.mu.Lock()
	ok := e.life == LifecycleRunning && e.runtime == RuntimeActive
	max := e.res.MaxStoredBytes
	e.mu.Unlock()
	if !ok {
		return session.Admission{}, false
	}
	reserve := func() bool {
		e.cs.mu.Lock()
		defer e.cs.mu.Unlock()
		if e.cs.stored+total > max {
			return false
		}
		e.cs.stored += total
		return true
	}
	for !reserve() {
		if !e.evictOldest(true) {
			e.commit(EventResourceWarning)
			e.logf(slog.LevelWarn, "dropped incoming content", "reason", "storage_full")
			return session.Admission{}, false
		}
	}
	return session.Admission{
		Inbox: e.recvDir(),
		Done: func(snap clip.Snapshot, dir string, ok bool) {
			e.delivered(DeviceID(peer), total, snap, dir, ok)
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
func (e *Engine) delivered(peer DeviceID, reserved int64, snap clip.Snapshot, dir string, ok bool) {
	giveBack := func(n int64) {
		e.cs.mu.Lock()
		e.cs.stored -= n
		e.cs.mu.Unlock()
	}
	e.mu.Lock()
	sy := e.settings.Value.Sync
	running := e.life == LifecycleRunning && e.runtime == RuntimeActive
	e.mu.Unlock()
	if !ok || !running || !sy.Enabled {
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
	processed, err := e.processContent(workCtx, pipelineReceived, c, pipelineOrigin{kind: "remote_device", deviceID: peer})
	if err != nil {
		return // processContent released the input and any prepared replacement
	}
	c = processed.c
	defer e.cs.unref(c)
	e.cs.mu.Lock()
	desc := e.cs.descriptor(c)
	e.cs.mu.Unlock()

	// Persist after the cheap admission checks and before publishing; if the
	// state moves on meanwhile the record is rolled back below.
	var hid HistoryID
	if processed.policy.Persist {
		e.mu.Lock()
		ok := e.life == LifecycleRunning && e.runtime == RuntimeActive && e.settings.Value.Sync == sy && workCtx.Err() == nil
		e.mu.Unlock()
		if !ok {
			return
		}
		var perr *Error
		if hid, perr = e.persistHistory(workCtx, processed); perr != nil {
			return // no half-published entry; the receive fails without a record
		}
	}
	rollback := func() {
		if hid != "" {
			e.removeHistory(hid)
		}
	}
	now := time.Now()
	rc := &receipt{c: c, expires: now.Add(e.res.ReceivedTTL), ReceivedContent: ReceivedContent{
		ReceiptID: ReceiptID(e.newID()), Content: desc, PeerID: peer, ReceivedAtMs: now.UnixMilli(),
		ExpiresAtMs: now.Add(e.res.ReceivedTTL).UnixMilli(), ApplyState: ApplyAvailable, HistoryID: hid,
	}}
	// Make room in the list; if every entry is in use, the new one loses.
	for {
		e.mu.Lock()
		if e.life != LifecycleRunning || e.runtime != RuntimeActive || e.settings.Value.Sync != sy || workCtx.Err() != nil {
			e.mu.Unlock()
			rollback()
			return
		}
		trusted, trustErr := e.st.HasPeer(string(peer))
		if trustErr != nil || !trusted {
			e.mu.Unlock()
			rollback()
			return
		}
		if len(e.recv) < e.res.MaxReceived {
			e.cs.mu.Lock()
			c.refs++ // Received has its own reference before processing ends
			e.cs.mu.Unlock()
			e.recv = append(e.recv, rc)
			e.commitObjsLocked([]ObjectRef{{Type: "received", ID: string(rc.ReceiptID)}}, EventReceived)
			e.rescheduleRecvLocked()
			e.mu.Unlock()
			return
		}
		e.mu.Unlock()
		if !e.evictOldest(false) {
			e.commit(EventResourceWarning)
			rollback()
			return
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
