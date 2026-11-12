package clipboardintegration

import (
	"context"
	"errors"
	"path/filepath"
	"sync"
	"time"

	"github.com/uniclipboard/unic/engine"
	"github.com/uniclipboard/unic/internal/clip"
	"github.com/uniclipboard/unic/internal/directory"
	api "github.com/uniclipboard/unic/internal/localapi"
)

// clipboardHost connects the system clipboard to the engine, following the
// observation and application protocol of the engine contract (section 8):
// it polls the clipboard and reports changes, writes received content, and
// keeps the two from echoing each other by serializing both and reporting what
// it read back after each write.
type Clipboard struct {
	e     Engine
	board Board
	poll  time.Duration
	emit  func(api.Event)
	logf  func(string, ...any)
	name  func(ctx context.Context, id engine.DeviceID) string
	// channelName names an external channel for the event log.
	channelName func(ctx context.Context, id string) string

	mu          sync.Mutex // observation, application and explicit sends take turns
	epoch       string
	seq         uint64
	seen        string // hash of the clipboard as last read
	heldPaths   []string
	held        engine.LeaseID // lease on the received files now on the clipboard
	tried       map[engine.ReceiptID]bool
	initialized bool
	restores    map[string]*restoreRecord
}

func (c *Clipboard) lost(err error) bool {
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
func (c *Clipboard) importSnap(ctx context.Context, s clip.Snapshot) (engine.LeaseID, error) {
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

func (c *Clipboard) release(l engine.LeaseID) {
	if l != "" {
		_ = c.e.ReleaseContent(context.Background(), l)
	}
}

// rebase starts a new observation epoch with the clipboard as it is now, which
// sends nothing. lease, if not empty, is the content of snap already imported.
func (c *Clipboard) rebase(ctx context.Context, snap clip.Snapshot, lease engine.LeaseID, haveLease bool) error {
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
	c.seen = snap.ObservationHash()
	return nil
}

// tick looks at the clipboard once.
func (c *Clipboard) tick(ctx context.Context) {
	c.mu.Lock()
	defer c.mu.Unlock()
	c.tickLocked(ctx)
}

func (c *Clipboard) tickLocked(ctx context.Context) {
	s, err := c.board.Read()
	if err != nil {
		return // transient: the owner changed mid-read, or the board is locked
	}
	var paths []string
	for _, f := range s.Files {
		paths = append(paths, f.Path)
	}
	_ = c.e.ReconcileClipboardFiles(ctx, paths)
	if c.seen != "" && s.ObservationHash() != c.seen && !sameHeldPaths(s, c.heldPaths) {
		c.release(c.held)
		c.held = ""
	}
	if c.epoch == "" {
		if err := c.rebase(ctx, s, "", false); err != nil {
			c.logf("cannot start observing the clipboard: %v", asCode(err))
		}
		return
	}
	h := s.ObservationHash()
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
			c.emit(api.Event{Kind: "sent", Info: describe(s)})
		}
	case c.lost(err):
		c.epoch = "" // start over with a fresh baseline
	default:
		c.logf("observing the clipboard failed: %v", asCode(err))
	}
}

// Run observes the clipboard and applies received content until ctx ends.
func (c *Clipboard) Run(ctx context.Context) {
	if !c.Ready() {
		t := time.NewTimer(c.poll)
		select {
		case <-ctx.Done():
			t.Stop()
			return
		case <-t.C:
		}
	}
	c.Initialize(ctx)
	t := time.NewTicker(c.poll)
	defer t.Stop()
	defer func() { c.mu.Lock(); defer c.mu.Unlock(); c.release(c.held); c.held = "" }()
	for {
		select {
		case <-ctx.Done():
			return
		case <-t.C:
			c.tryApply(ctx)
			c.tick(ctx)
		}
	}
}

// tryApply puts the newest received content on the clipboard, unless the
// clipboard moved on since it was last observed (then the user's newer copy
// wins).
func (c *Clipboard) tryApply(ctx context.Context) {
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
	if r.ChannelID != "" && !r.AutoApply {
		return // the user turned automatic writing off for this channel; it stays in the list and history
	}

	// Pick up whatever the user copied since the last poll first.
	before := c.seen
	c.tickLocked(ctx) // starts an epoch if there is none, or reports a newer copy
	if c.epoch == "" || (before != "" && before != c.seen) {
		return
	}
	tk, err := c.e.BeginApply(ctx, engine.ApplyRequest{ReceiptID: r.ReceiptID, ExpectedEpoch: c.epoch, ExpectedSequence: c.seq})
	if err != nil {
		if c.lost(err) {
			c.epoch = ""
		}
		var ee *engine.Error
		if !errors.As(err, &ee) || (ee.Code != engine.CodeSyncDisabled && ee.Code != engine.CodeConflict) {
			c.logf("not applying received content: %v", asCode(err))
		}
		return
	}
	snap, ok := c.writeTicket(ctx, tk)
	if !ok {
		return
	}
	// The list entry has done its job. Without it, only the lease on files that
	// are on the clipboard keeps the content, as it did before the list existed.
	_ = c.e.DismissReceived(ctx, r.ReceiptID)
	who := ""
	if r.ChannelID != "" {
		who = c.channelName(ctx, r.ChannelID)
	} else {
		who = c.name(ctx, r.PeerID)
	}
	c.emit(api.Event{Kind: "received", Peer: who, Info: describe(snap)})
}

// writeTicket writes the ticket's content to the clipboard and reports the
// result with what was read back as baseline, so the write does not bounce
// back. It returns false if the content could not be applied (the ticket is
// closed either way). c.mu held.
func (c *Clipboard) writeTicket(ctx context.Context, tk engine.ApplyTicket) (clip.Snapshot, bool) {
	snap, hasFiles, err := c.snapshotOf(ctx, tk)
	if err == nil {
		current, readErr := c.board.Read()
		if readErr != nil || current.ObservationHash() != c.seen {
			_ = c.e.ReportApplyResult(ctx, engine.ApplyResult{Token: tk.Token, Outcome: engine.ApplyOutcomeDeferred, ErrorCode: "conflict"})
			c.release(tk.LeaseID)
			if readErr == nil {
				c.tickLocked(ctx)
			}
			return snap, false
		}
		err = c.board.Write(snap)
	}
	if err != nil {
		c.logf("write clipboard: %v", asCode(err))
		_ = c.e.ReportApplyResult(ctx, engine.ApplyResult{Token: tk.Token, Outcome: engine.ApplyOutcomeFailed, ErrorCode: "write_failed"})
		c.release(tk.LeaseID)
		return snap, false
	}

	// The system may add representations of its own to what was written; what
	// is on the clipboard now is the baseline, so that it does not bounce back.
	back, rerr := c.board.Read()
	if rerr != nil || !sameWrittenSnapshot(snap, back) {
		_ = c.e.ReportApplyResult(ctx, engine.ApplyResult{Token: tk.Token, Outcome: engine.ApplyOutcomeFailed, ErrorCode: "conflict"})
		if hasFiles {
			c.release(c.held)
			c.held = tk.LeaseID
		} else {
			c.release(tk.LeaseID)
		}
		if rerr == nil {
			c.tickLocked(ctx)
		} else {
			c.epoch = ""
		}
		return snap, false
	}
	base, err := c.importSnap(ctx, back)
	if err != nil {
		_ = c.e.ReportApplyResult(ctx, engine.ApplyResult{Token: tk.Token, Outcome: engine.ApplyOutcomeFailed, ErrorCode: "unavailable"})
		c.release(tk.LeaseID)
		return snap, false
	}
	c.seq++
	err = c.e.ReportApplyResult(ctx, engine.ApplyResult{Token: tk.Token, Outcome: engine.ApplyOutcomeApplied, BaselineLeaseID: base, Epoch: c.epoch, Sequence: c.seq})
	c.release(base)
	if err != nil {
		// The user copied something while we wrote, or the engine voided the
		// ticket. Look at the clipboard as it really is.
		c.release(tk.LeaseID)
		c.epoch = ""
		return snap, false
	}
	if rerr == nil {
		c.seen = back.ObservationHash()
	}
	// Files stay on the clipboard, so their lease is kept until replaced.
	old := c.held
	if hasFiles {
		c.held = tk.LeaseID
		c.heldPaths = nil
		for _, f := range snap.Files {
			c.heldPaths = append(c.heldPaths, f.Path)
		}
	} else {
		c.held = ""
		c.release(tk.LeaseID)
	}
	c.release(old)
	return snap, true
}

// UseHistory puts a stored record on the clipboard. Like received content it
// is applied under the observation protocol, so the write is not taken for a
// new copy; unlike it, nothing is synced and no record is added.
type restoreRecord struct {
	history engine.HistoryID
	at      time.Time
	result  engine.RestoreSyncResult
	err     error
}

// UseHistory applies an Action result locally. It never triggers restore sync.
func (c *Clipboard) UseHistory(ctx context.Context, id engine.HistoryID) error {
	_, err := c.useHistory(ctx, id, "", false)
	return err
}

func (c *Clipboard) RestoreHistory(ctx context.Context, id engine.HistoryID, request string) (engine.RestoreSyncResult, error) {
	return c.useHistory(ctx, id, request, true)
}

func (c *Clipboard) useHistory(ctx context.Context, id engine.HistoryID, request string, syncRestore bool) (engine.RestoreSyncResult, error) {
	c.mu.Lock()
	defer c.mu.Unlock()
	var out engine.RestoreSyncResult
	if syncRestore {
		if request == "" || len(request) > 128 {
			return out, &engine.Error{Code: engine.CodeInvalidArgument, Message: "restore request_id is required (at most 128 bytes)"}
		}
		if c.restores == nil {
			c.restores = map[string]*restoreRecord{}
		}
		now := time.Now()
		for key, r := range c.restores {
			if now.Sub(r.at) >= 30*time.Minute {
				delete(c.restores, key)
			}
		}
		if old := c.restores[request]; old != nil {
			if old.history != id {
				return out, &engine.Error{Code: engine.CodeConflict, Message: "restore request_id already names another item"}
			}
			return old.result, old.err
		}
		if len(c.restores) >= 1024 {
			return out, &engine.Error{Code: engine.CodeResourceExhausted, Message: "restore request cache is full"}
		}
	}
	out, err := c.applyHistory(ctx, id, syncRestore)
	if syncRestore {
		c.restores[request] = &restoreRecord{history: id, at: time.Now(), result: out, err: err}
	}
	return out, err
}

func (c *Clipboard) applyHistory(ctx context.Context, id engine.HistoryID, syncRestore bool) (engine.RestoreSyncResult, error) {
	var out engine.RestoreSyncResult
	c.tickLocked(ctx)
	if c.epoch == "" {
		return out, &engine.Error{Code: engine.CodeUnavailable, Message: "the clipboard is not available"}
	}
	tk, err := c.e.BeginApplyHistory(ctx, engine.ApplyHistoryRequest{HistoryID: id, UserRestore: syncRestore, ExpectedEpoch: c.epoch, ExpectedSequence: c.seq})
	if err != nil {
		if c.lost(err) {
			c.epoch = ""
		}
		return out, err
	}
	// Hold an independent lease because writeTicket releases text-only tickets.
	held, err := c.e.RetainContent(ctx, tk.ContentID)
	if err != nil {
		_ = c.e.ReportApplyResult(ctx, engine.ApplyResult{Token: tk.Token, Outcome: engine.ApplyOutcomeFailed})
		c.release(tk.LeaseID)
		return out, err
	}
	defer c.release(held.LeaseID)
	if _, ok := c.writeTicket(ctx, tk); !ok {
		return out, &engine.Error{Code: engine.CodeUnavailable, Message: "could not write the clipboard"}
	}
	out.LocalApplied, out.SyncDisposition = true, engine.SyncNotRequested
	if syncRestore {
		result, serr := c.e.SyncRestoredHistory(ctx, tk.Token, held.LeaseID)
		if serr != nil {
			out.SyncDisposition = engine.SyncFailed
			out.SyncError = &engine.Error{Code: engine.CodeUnavailable, Message: "local restore succeeded; sync could not start"}
			return out, nil
		}
		out = result
	}
	return out, nil
}

// snapshotOf reads the ticket's content into a snapshot for the clipboard.
// Files win over the text some file managers put next to them.
func (c *Clipboard) snapshotOf(ctx context.Context, tk engine.ApplyTicket) (s clip.Snapshot, hasFiles bool, err error) {
	h, err := c.e.RetainContent(ctx, tk.ContentID)
	if err != nil {
		return s, false, err
	}
	defer c.release(h.LeaseID)
	for _, p := range h.Content.Parts {
		if p.MIME == directory.MIME {
			provider, ok := c.e.(interface {
				DirectoryPaths(context.Context, engine.LeaseID) ([]string, error)
			})
			if !ok {
				return s, false, &engine.Error{Code: engine.CodeUnsupported, Message: "directory apply unavailable"}
			}
			var paths []string
			if host, ok := c.e.(interface {
				DirectoryReferences(context.Context, engine.LeaseID) ([]string, error)
			}); ok {
				refs, err := host.DirectoryReferences(ctx, tk.LeaseID)
				if err != nil {
					return s, false, err
				}
				for _, ref := range refs {
					path, err := desktopFilePath(ref)
					if err != nil {
						return s, false, err
					}
					paths = append(paths, path)
				}
			}
			if len(paths) == 0 {
				paths, err = provider.DirectoryPaths(ctx, tk.LeaseID)
			}
			if err != nil {
				return s, false, err
			}
			for _, path := range paths {
				s.Files = append(s.Files, clip.File{Name: filepath.Base(path), Path: path})
			}
			return s, true, nil
		}
	}
	for _, p := range h.Content.Parts {
		if p.Kind != engine.PartFile {
			continue
		}
		var path string
		var err error
		if p.Referenced {
			var ref string
			ref, err = c.e.FileReference(ctx, tk.LeaseID, p.ID)
			if err == nil {
				path, err = desktopFilePath(ref)
			}
		} else {
			path, err = c.e.LocalFilePath(ctx, tk.LeaseID, p.ID)
		}
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
func (c *Clipboard) Send(ctx context.Context, s clip.Snapshot) error {
	c.mu.Lock()
	defer c.mu.Unlock()
	if err := c.board.Write(s); err != nil {
		return err
	}
	back, err := c.board.Read()
	if err != nil {
		c.epoch = ""
		return &engine.Error{Code: engine.CodeUnavailable, Message: "clipboard read-back failed"}
	}
	if !sameWrittenSnapshot(s, back) {
		c.tickLocked(ctx)
		return &engine.Error{Code: engine.CodeConflict, Message: "clipboard changed during copy"}
	}
	lease, err := c.importSnap(ctx, back)
	if err != nil {
		return err
	}
	defer c.release(lease)
	// Import the new snapshot before releasing the old clipboard lease: both
	// snapshots may refer to the same managed file. Rebase hides this write
	// from polling, so polling cannot release the replaced lease for us.
	paths := make([]string, 0, len(back.Files))
	for _, f := range back.Files {
		paths = append(paths, f.Path)
	}
	_ = c.e.ReconcileClipboardFiles(ctx, paths)
	if back.ObservationHash() != c.seen {
		c.release(c.held)
		c.held = ""
	}
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
		c.emit(api.Event{Kind: "sent", Info: describe(back)})
	case errors.As(err, &ee) && (ee.Code == engine.CodeNoTargets || ee.Code == engine.CodeSyncDisabled):
		// Set locally; there is nobody to send to, or sync is paused.
	default:
		return err
	}
	return nil
}

func (c *Clipboard) Initialize(ctx context.Context) {
	c.mu.Lock()
	defer c.mu.Unlock()
	if c.initialized {
		return
	}
	c.initialized = true
	c.tried = map[engine.ReceiptID]bool{}
	rs, err := c.e.ListReceived(ctx)
	if err != nil {
		c.initialized = false
		return
	}
	for _, r := range rs {
		c.tried[r.ReceiptID] = true
	}
	c.tickLocked(ctx)
}
func (c *Clipboard) Ready() bool { c.mu.Lock(); defer c.mu.Unlock(); return c.epoch != "" }

func sameWrittenSnapshot(want, back clip.Snapshot) bool {
	if len(want.Files) > 0 {
		if len(want.Files) != len(back.Files) {
			return false
		}
		for i, f := range want.Files {
			if f.Path != back.Files[i].Path {
				return false
			}
		}
		return true
	}
	for _, p := range want.Parts {
		found := false
		for _, q := range back.Parts {
			if p.Format == q.Format && string(p.Data) == string(q.Data) {
				found = true
			}
		}
		if !found {
			return false
		}
	}
	return true
}

func (c *Clipboard) IgnoreReceipts(ids []engine.ReceiptID) {
	c.mu.Lock()
	defer c.mu.Unlock()
	for _, id := range ids {
		c.tried[id] = true
	}
}

func sameHeldPaths(s clip.Snapshot, paths []string) bool {
	if len(paths) == 0 || len(paths) != len(s.Files) {
		return false
	}
	for i, f := range s.Files {
		if f.Path != paths[i] {
			return false
		}
	}
	return true
}
