package host

import (
	"context"
	"errors"
	"sync"
	"time"

	"github.com/uniclipboard/unic/app/desktop/internal/api"
	"github.com/uniclipboard/unic/engine"
	"github.com/uniclipboard/unic/internal/clip"
)

// clipboardHost connects the system clipboard to the engine, following the
// observation and application protocol of the engine contract (section 8):
// it polls the clipboard and reports changes, writes received content, and
// keeps the two from echoing each other by serializing both and reporting what
// it read back after each write.
type clipboardHost struct {
	e     *engine.Engine
	board Board
	poll  time.Duration
	hub   *hub
	logf  func(string, ...any)
	name  func(ctx context.Context, id engine.DeviceID) string

	mu    sync.Mutex // observation, application and explicit sends take turns
	epoch string
	seq   uint64
	seen  string         // hash of the clipboard as last read
	held  engine.LeaseID // lease on the received files now on the clipboard
	tried map[engine.ReceiptID]bool
}

func (c *clipboardHost) lost(err error) bool {
	var ee *engine.Error
	if errors.As(err, &ee) {
		switch ee.Code {
		case engine.CodeConflict, engine.CodeInvalidArgument, engine.CodeInvalidState, engine.CodeSuspended:
			return true
		}
	}
	return false
}

// importSnap leases the content of a snapshot. Content the engine cannot carry
// (a folder, over the size limit, only vendor formats) counts as an empty
// clipboard: it is not synced, and copying something else afterwards is a real
// change.
func (c *clipboardHost) importSnap(ctx context.Context, s clip.Snapshot) (engine.LeaseID, error) {
	in, err := contentInput(s)
	if err != nil {
		return "", nil
	}
	h, err := c.e.ImportContent(ctx, in)
	if err != nil {
		var ee *engine.Error
		if errors.As(err, &ee) {
			switch ee.Code {
			case engine.CodeLimitExceeded, engine.CodeUnsupported, engine.CodeInvalidArgument, engine.CodeSourceChanged:
				c.logf("not syncing the clipboard: %s", ee.Code)
				return "", nil
			}
		}
		return "", err
	}
	return h.LeaseID, nil
}

func (c *clipboardHost) release(l engine.LeaseID) {
	if l != "" {
		_ = c.e.ReleaseContent(context.Background(), l)
	}
}

// rebase starts a new observation epoch with the clipboard as it is now, which
// sends nothing. lease, if not empty, is the content of snap already imported.
func (c *clipboardHost) rebase(ctx context.Context, snap clip.Snapshot, lease engine.LeaseID, haveLease bool) error {
	if !haveLease {
		l, err := c.importSnap(ctx, snap)
		if err != nil {
			return err
		}
		lease = l
		defer c.release(lease)
	}
	c.epoch, c.seq = "e-"+randID(), 1
	_, err := c.e.ObserveClipboard(ctx, engine.Observation{Epoch: c.epoch, Sequence: 1, Mode: engine.ObserveBaseline, LeaseID: lease})
	if err != nil {
		return err
	}
	c.seen = snap.Hash()
	return nil
}

// tick looks at the clipboard once.
func (c *clipboardHost) tick(ctx context.Context) {
	c.mu.Lock()
	defer c.mu.Unlock()
	c.tickLocked(ctx)
}

func (c *clipboardHost) tickLocked(ctx context.Context) {
	s, err := c.board.Read()
	if err != nil {
		return // transient: the owner changed mid-read, or the board is locked
	}
	if c.epoch == "" {
		if err := c.rebase(ctx, s, "", false); err != nil {
			c.logf("cannot start observing the clipboard: %v", asCode(err))
		}
		return
	}
	h := s.Hash()
	if h == c.seen {
		return
	}
	c.seen = h
	lease, err := c.importSnap(ctx, s)
	if err != nil {
		c.logf("cannot read the clipboard into the engine: %v", asCode(err))
		return
	}
	defer c.release(lease)
	c.seq++
	res, err := c.e.ObserveClipboard(ctx, engine.Observation{Epoch: c.epoch, Sequence: c.seq, Mode: engine.ObserveChange, LeaseID: lease})
	switch {
	case err == nil:
		if res.Disposition == engine.DispSubmitted {
			c.hub.emit(api.Event{Kind: "sent", Info: describe(s)})
		}
	case c.lost(err):
		c.epoch = "" // start over with a fresh baseline
	default:
		c.logf("observing the clipboard failed: %v", asCode(err))
	}
}

// Run observes the clipboard and applies received content until ctx ends.
func (c *clipboardHost) Run(ctx context.Context) {
	c.tried = map[engine.ReceiptID]bool{}
	go c.applyLoop(ctx)
	t := time.NewTicker(c.poll)
	defer t.Stop()
	for {
		select {
		case <-ctx.Done():
			return
		case <-t.C:
			c.tick(ctx)
		}
	}
}

// applyLoop applies the newest received content whenever something arrives.
func (c *clipboardHost) applyLoop(ctx context.Context) {
	for ctx.Err() == nil {
		sub, err := c.e.SubscribeEvents(ctx)
		if err != nil {
			select {
			case <-ctx.Done():
				return
			case <-time.After(time.Second):
			}
			continue
		}
		c.tryApply(ctx)
		for {
			ev, err := sub.Next(ctx)
			if err != nil {
				break // resync_required or closed: subscribe again
			}
			if ev.Kind == engine.EventReceived {
				c.tryApply(ctx)
			}
		}
		sub.Close()
	}
}

// tryApply puts the newest received content on the clipboard, unless the
// clipboard moved on since it was last observed (then the user's newer copy
// wins).
func (c *clipboardHost) tryApply(ctx context.Context) {
	c.mu.Lock()
	defer c.mu.Unlock()
	rs, err := c.e.ListReceived(ctx)
	if err != nil || len(rs) == 0 {
		return
	}
	live := map[engine.ReceiptID]bool{}
	for _, r := range rs {
		live[r.ReceiptID] = true
	}
	for id := range c.tried {
		if !live[id] {
			delete(c.tried, id)
		}
	}
	r := rs[0] // newest
	if r.ApplyState != engine.ApplyAvailable || c.tried[r.ReceiptID] {
		return
	}
	c.tried[r.ReceiptID] = true

	// Pick up whatever the user copied since the last poll first.
	c.tickLocked(ctx) // starts an epoch if there is none, or reports a newer copy
	if c.epoch == "" {
		return
	}
	tk, err := c.e.BeginApply(ctx, engine.ApplyRequest{ReceiptID: r.ReceiptID, ExpectedEpoch: c.epoch, ExpectedSequence: c.seq})
	if err != nil {
		if c.lost(err) {
			c.epoch = ""
		}
		var ee *engine.Error
		if !(errors.As(err, &ee) && (ee.Code == engine.CodeSyncDisabled || ee.Code == engine.CodeConflict)) {
			c.logf("not applying received content: %v", asCode(err))
		}
		return
	}
	snap, hasFiles, err := c.snapshotOf(ctx, tk)
	if err == nil {
		err = c.board.Write(snap)
	}
	if err != nil {
		c.logf("write clipboard: %v", asCode(err))
		_ = c.e.ReportApplyResult(ctx, engine.ApplyResult{Token: tk.Token, Outcome: engine.ApplyOutcomeFailed, ErrorCode: "write_failed"})
		c.release(tk.LeaseID)
		return
	}

	// The system may add representations of its own to what was written; what
	// is on the clipboard now is the baseline, so that it does not bounce back.
	back, rerr := c.board.Read()
	var base engine.LeaseID
	own := false
	if rerr == nil {
		base, err = c.importSnap(ctx, back)
		own = err == nil
	}
	if !own {
		base = tk.LeaseID // could not read it back: the content itself will do
	}
	c.seq++
	err = c.e.ReportApplyResult(ctx, engine.ApplyResult{Token: tk.Token, Outcome: engine.ApplyOutcomeApplied, BaselineLeaseID: base, Epoch: c.epoch, Sequence: c.seq})
	if own {
		c.release(base)
	}
	if err != nil {
		// The user copied something while we wrote, or the engine voided the
		// ticket. Look at the clipboard as it really is.
		c.release(tk.LeaseID)
		c.epoch = ""
		return
	}
	if rerr == nil {
		c.seen = back.Hash()
	}
	// Files stay on the clipboard, so their lease is kept until replaced.
	old := c.held
	if hasFiles {
		c.held = tk.LeaseID
	} else {
		c.held = ""
		c.release(tk.LeaseID)
	}
	c.release(old)
	// The list entry has done its job. Without it, only the lease on files that
	// are on the clipboard keeps the content, as it did before the list existed.
	_ = c.e.DismissReceived(ctx, r.ReceiptID)
	c.hub.emit(api.Event{Kind: "received", Peer: c.name(ctx, r.PeerID), Info: describe(snap)})
}

// snapshotOf reads the ticket's content into a snapshot for the clipboard.
// Files win over the text some file managers put next to them.
func (c *clipboardHost) snapshotOf(ctx context.Context, tk engine.ApplyTicket) (s clip.Snapshot, hasFiles bool, err error) {
	h, err := c.e.RetainContent(ctx, tk.ContentID)
	if err != nil {
		return s, false, err
	}
	defer c.release(h.LeaseID)
	for _, p := range h.Content.Parts {
		if p.Kind != engine.PartFile {
			continue
		}
		path, err := c.e.LocalFilePath(ctx, tk.LeaseID, p.ID)
		if err != nil {
			return s, false, err
		}
		s.Files = append(s.Files, clip.File{Name: p.Name, Path: path, Size: p.Size})
	}
	if len(s.Files) > 0 {
		return s, true, nil
	}
	for _, p := range h.Content.Parts {
		data, err := readPart(ctx, c.e, h.LeaseID, p.ID)
		if err != nil {
			return s, false, err
		}
		s.Parts = append(s.Parts, clip.Part{Format: p.MIME, Data: data})
	}
	return s, false, nil
}

// Send puts s on the clipboard on behalf of the user (CLI, GUI) and sends it
// to the connected devices, once, even if the clipboard already held it. The
// write is read back and becomes the baseline before the send, so the poll
// does not report it a second time.
func (c *clipboardHost) Send(ctx context.Context, s clip.Snapshot) error {
	c.mu.Lock()
	defer c.mu.Unlock()
	if err := c.board.Write(s); err != nil {
		return err
	}
	back, err := c.board.Read()
	if err != nil {
		back = s
	}
	lease, err := c.importSnap(ctx, back)
	if err != nil {
		return err
	}
	defer c.release(lease)
	if err := c.rebase(ctx, back, lease, true); err != nil {
		return err
	}
	if lease == "" {
		return nil // nothing the engine can carry: the clipboard is set, nothing to send
	}
	_, err = c.e.SubmitContent(ctx, engine.SubmitRequest{RequestID: engine.RequestID("send-" + randID()), LeaseID: lease, Origin: engine.OriginExplicit})
	var ee *engine.Error
	switch {
	case err == nil:
		c.hub.emit(api.Event{Kind: "sent", Info: describe(back)})
	case errors.As(err, &ee) && (ee.Code == engine.CodeNoTargets || ee.Code == engine.CodeSyncDisabled):
		// Set locally; there is nobody to send to, or sync is paused.
	default:
		return err
	}
	return nil
}
