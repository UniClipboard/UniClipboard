package host

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"sync"
	"time"

	"github.com/uniclipboard/unic/engine"
	"github.com/uniclipboard/unic/internal/clip"
	api "github.com/uniclipboard/unic/internal/localapi"
)

const adapterTTL = 10 * time.Second

type adapterState struct {
	mu       sync.Mutex
	n        *Node
	session  string
	until    time.Time
	leases   map[engine.LeaseID]bool
	ticket   engine.ApplyToken
	commands chan api.DesktopCommand
	waiting  map[string]chan api.DesktopResult
	ready    bool
	baseline bool
	readers  map[*engine.ContentReader]context.CancelFunc
	gate     chan struct{}
}

func (a *adapterState) init(n *Node) {
	a.n = n
	a.readers = map[*engine.ContentReader]context.CancelFunc{}
	a.gate = make(chan struct{}, 8)
	a.commands = make(chan api.DesktopCommand, 16)
	a.waiting = map[string]chan api.DesktopResult{}
	a.leases = map[engine.LeaseID]bool{}
}
func adapterError(code engine.Code) *engine.Error {
	return &engine.Error{Code: code, Message: "desktop adapter request failed"}
}
func (a *adapterState) resetLocked() {
	if a.session == "" {
		return
	}
	_ = a.n.e.UpdateHostCapabilities(context.Background(), engine.HostCapabilities{Observe: engine.AbilityUnavailable, Apply: engine.AbilityUnavailable})
	if a.ticket != "" {
		_ = a.n.e.ReportApplyResult(context.Background(), engine.ApplyResult{Token: a.ticket, Outcome: engine.ApplyOutcomeFailed, ErrorCode: "unavailable"})
	}
	for r, cancel := range a.readers {
		cancel()
		_ = r.Close()
		delete(a.readers, r)
	}
	for l := range a.leases {
		_ = a.n.e.ReleaseContent(context.Background(), l)
	}
	for id, ch := range a.waiting {
		ch <- api.DesktopResult{ID: id, Error: adapterError(engine.CodeUnavailable)}
	}
	a.session = ""
	a.ready = false
	a.baseline = false
	a.ticket = ""
	a.leases = map[engine.LeaseID]bool{}
	a.waiting = map[string]chan api.DesktopResult{}
	a.commands = make(chan api.DesktopCommand, 16)
	a.n.hub.emit(api.Event{Kind: "capabilities"})
}
func (a *adapterState) close() { a.mu.Lock(); defer a.mu.Unlock(); a.resetLocked() }
func (a *adapterState) watch(ctx context.Context) {
	t := time.NewTicker(time.Second)
	defer t.Stop()
	for {
		select {
		case <-ctx.Done():
			return
		case <-t.C:
			a.mu.Lock()
			if time.Now().After(a.until) {
				a.resetLocked()
			}
			a.mu.Unlock()
		}
	}
}
func (a *adapterState) validLocked(session string) bool {
	if a.session != "" && time.Now().After(a.until) {
		a.resetLocked()
	}
	return a.session != "" && a.session == session
}
func (a *adapterState) command(ctx context.Context, c api.DesktopCommand) (api.DesktopResult, error) {
	a.mu.Lock()
	if !a.validLocked(a.session) || !a.ready {
		a.mu.Unlock()
		return api.DesktopResult{}, adapterError(engine.CodeUnavailable)
	}
	if len(a.waiting) >= 16 {
		a.mu.Unlock()
		return api.DesktopResult{}, adapterError(engine.CodeResourceExhausted)
	}
	c.ID = randomID()
	ch := make(chan api.DesktopResult, 1)
	a.waiting[c.ID] = ch
	select {
	case a.commands <- c:
	default:
		delete(a.waiting, c.ID)
		a.mu.Unlock()
		return api.DesktopResult{}, adapterError(engine.CodeResourceExhausted)
	}
	a.mu.Unlock()
	timer := time.NewTimer(8 * time.Second)
	defer timer.Stop()
	defer func() { a.mu.Lock(); delete(a.waiting, c.ID); a.mu.Unlock() }()
	select {
	case out := <-ch:
		if out.Error != nil {
			return out, out.Error
		}
		return out, nil
	case <-ctx.Done():
		return api.DesktopResult{}, adapterError(engine.CodeTimeout)
	case <-timer.C:
		return api.DesktopResult{}, adapterError(engine.CodeTimeout)
	}
}
func (n *Node) AdapterCall(ctx context.Context, in api.AdapterRequest) api.AdapterReply {
	ctx, cancel := context.WithTimeout(ctx, 8*time.Second)
	defer cancel()
	var out any
	var err error
	a := &n.adapter
	select {
	case a.gate <- struct{}{}:
		defer func() { <-a.gate }()
	default:
		return api.AdapterReply{Error: adapterError(engine.CodeResourceExhausted)}
	}
	a.mu.Lock()
	defer a.mu.Unlock()
	decode := func(v any) bool {
		if json.Unmarshal(in.Value, v) != nil {
			err = adapterError(engine.CodeInvalidArgument)
			return false
		}
		return true
	}
	if in.Method == "register" {
		if a.session != "" && time.Now().After(a.until) {
			a.resetLocked()
		}
		if a.session != "" {
			err = adapterError(engine.CodeInUse)
		} else {
			a.session = randomID()
			a.until = time.Now().Add(adapterTTL)
			out = a.session
		}
	} else if !a.validLocked(in.Session) {
		err = adapterError(engine.CodeResourceExpired)
	} else {
		switch in.Method {
		case "activity":
			var ev api.Event
			if decode(&ev) {
				if ev.Kind != "sent" || (ev.Info != "text" && ev.Info != "image" && ev.Info != "1 file" && !validFileSummary(ev.Info)) {
					err = adapterError(engine.CodeInvalidArgument)
				} else {
					n.hub.emit(api.Event{Kind: "sent", Info: ev.Info})
				}
			}
		case "heartbeat":
			a.until = time.Now().Add(adapterTTL)
		case "close":
			a.resetLocked()
		case "capabilities":
			var c engine.HostCapabilities
			if decode(&c) {
				err = n.e.UpdateHostCapabilities(ctx, c)
			}
		case "ready":
			if !a.baseline {
				err = adapterError(engine.CodeInvalidState)
			} else {
				rs, e := n.e.ListReceived(ctx)
				if e != nil {
					err = e
				} else {
					ids := make([]engine.ReceiptID, 0, len(rs))
					for _, r := range rs {
						ids = append(ids, r.ReceiptID)
					}
					out = ids
					a.ready = true
				}
			}
		case "next":
			commands := a.commands
			a.mu.Unlock()
			select {
			case c := <-commands:
				out = c
			case <-ctx.Done():
				out = api.DesktopCommand{}
			}
			a.mu.Lock()
			if c, ok := out.(api.DesktopCommand); ok && c.ID != "" && a.waiting[c.ID] == nil {
				out = api.DesktopCommand{}
			}
			if !a.validLocked(in.Session) {
				err = adapterError(engine.CodeResourceExpired)
			}
		case "result":
			var r api.DesktopResult
			if decode(&r) {
				ch := a.waiting[r.ID]
				if ch == nil {
					err = adapterError(engine.CodeConflict)
				} else {
					delete(a.waiting, r.ID)
					ch <- r
				}
			}
		case "import":
			var s clip.Snapshot
			if decode(&s) {
				if len(a.leases) >= 128 {
					err = adapterError(engine.CodeResourceExhausted)
					break
				}
				var input engine.ContentInput
				input, err = contentInput(s)
				if err == nil {
					var h engine.ContentHandle
					h, err = n.e.ImportContent(ctx, input)
					if err == nil {
						a.leases[h.LeaseID] = true
						out = h
					}
				}
			}
		case "retain":
			var id engine.ContentID
			if decode(&id) {
				if len(a.leases) >= 128 {
					err = adapterError(engine.CodeResourceExhausted)
					break
				}
				var h engine.ContentHandle
				h, err = n.e.RetainContent(ctx, id)
				if err == nil {
					a.leases[h.LeaseID] = true
					out = h
				}
			}
		case "release":
			var l engine.LeaseID
			if decode(&l) {
				if !a.leases[l] {
					err = adapterError(engine.CodeResourceExpired)
				} else {
					err = n.e.ReleaseContent(ctx, l)
					delete(a.leases, l)
				}
			}
		case "observe":
			var o engine.Observation
			if decode(&o) {
				if o.LeaseID != "" && !a.leases[o.LeaseID] {
					err = adapterError(engine.CodeResourceExpired)
				} else {
					out, err = n.e.ObserveClipboard(ctx, o)
					if err == nil && o.Mode == engine.ObserveBaseline {
						a.baseline = true
					}
				}
			}
		case "apply":
			if len(a.leases) >= 128 {
				err = adapterError(engine.CodeResourceExhausted)
				break
			}
			var r engine.ApplyRequest
			if decode(&r) {
				var t engine.ApplyTicket
				t, err = n.e.BeginApply(ctx, r)
				if err == nil {
					a.ticket = t.Token
					a.leases[t.LeaseID] = true
					out = t
				}
			}
		case "apply-history":
			if len(a.leases) >= 128 {
				err = adapterError(engine.CodeResourceExhausted)
				break
			}
			var r engine.ApplyHistoryRequest
			if decode(&r) {
				var t engine.ApplyTicket
				t, err = n.e.BeginApplyHistory(ctx, r)
				if err == nil {
					a.ticket = t.Token
					a.leases[t.LeaseID] = true
					out = t
				}
			}
		case "report":
			var r engine.ApplyResult
			if decode(&r) {
				if r.Token != a.ticket || (r.BaselineLeaseID != "" && !a.leases[r.BaselineLeaseID]) {
					err = adapterError(engine.CodeConflict)
				} else {
					err = n.e.ReportApplyResult(ctx, r)
					if err == nil {
						a.ticket = ""
					}
				}
			}
		case "received":
			out, err = n.e.ListReceived(ctx)
		case "dismiss":
			var id engine.ReceiptID
			if decode(&id) {
				err = n.e.DismissReceived(ctx, id)
			}
		case "reconcile":
			var paths []string
			if decode(&paths) {
				err = n.e.ReconcileClipboardFiles(ctx, paths)
			}
		case "submit":
			var r engine.SubmitRequest
			if decode(&r) {
				if !a.leases[r.LeaseID] {
					err = adapterError(engine.CodeResourceExpired)
				} else {
					out, err = n.e.SubmitContent(ctx, r)
				}
			}
		case "restore-sync":
			var r struct {
				Token engine.ApplyToken
				Lease engine.LeaseID
			}
			if decode(&r) {
				if !a.leases[r.Lease] {
					err = adapterError(engine.CodeResourceExpired)
				} else {
					out, err = n.e.SyncRestoredHistory(ctx, r.Token, r.Lease)
				}
			}
		case "path", "reference", "read":
			var r struct {
				Lease engine.LeaseID
				Part  engine.PartID
			}
			if decode(&r) {
				if !a.leases[r.Lease] {
					err = adapterError(engine.CodeResourceExpired)
					break
				}
				switch in.Method {
				case "path":
					out, err = n.e.LocalFilePath(ctx, r.Lease, r.Part)
				case "reference":
					out, err = n.e.FileReference(ctx, r.Lease, r.Part)
				case "read":
					err = adapterError(engine.CodeUnsupported)
				}
			}
		default:
			err = adapterError(engine.CodeUnsupported)
		}
	}
	reply := api.AdapterReply{}
	if err != nil {
		var ee *engine.Error
		if errors.As(err, &ee) {
			reply.Error = ee
		} else {
			reply.Error = adapterError(engine.CodeUnavailable)
		}
	} else {
		reply.Value, _ = json.Marshal(out)
	}
	return reply
}

type remoteDesktop struct{ n *Node }

func (r *remoteDesktop) Read(ctx context.Context) (clip.Snapshot, error) {
	o, e := r.n.adapter.command(ctx, api.DesktopCommand{Kind: "read"})
	return o.Snapshot, e
}
func (r *remoteDesktop) Send(ctx context.Context, s clip.Snapshot) error {
	_, e := r.n.adapter.command(ctx, api.DesktopCommand{Kind: "copy", Snapshot: s})
	return e
}
func (r *remoteDesktop) UseHistory(ctx context.Context, id engine.HistoryID) error {
	_, e := r.n.adapter.command(ctx, api.DesktopCommand{Kind: "apply", HistoryID: id})
	return e
}
func (r *remoteDesktop) RestoreHistory(ctx context.Context, id engine.HistoryID, request string) (engine.RestoreSyncResult, error) {
	o, e := r.n.adapter.command(ctx, api.DesktopCommand{Kind: "restore", HistoryID: id, RequestID: request})
	return o.Restore, e
}

func (n *Node) AdapterContent(ctx context.Context, s string, l engine.LeaseID, p engine.PartID) (api.HistoryContent, error) {
	a := &n.adapter
	a.mu.Lock()
	defer a.mu.Unlock()
	if !a.validLocked(s) || !a.leases[l] {
		return api.HistoryContent{}, apiErr(adapterError(engine.CodeResourceExpired), false)
	}
	if len(a.readers) >= 4 {
		return api.HistoryContent{}, apiErr(adapterError(engine.CodeResourceExhausted), false)
	}
	stream, cancel := context.WithTimeout(ctx, 8*time.Second)
	reader, err := n.e.ReadContent(stream, l, p)
	if err != nil {
		cancel()
		return api.HistoryContent{}, apiErr(err, false)
	}
	a.readers[reader] = cancel
	go func() { <-stream.Done(); _ = reader.Close() }()
	done := func() { cancel(); a.mu.Lock(); delete(a.readers, reader); a.mu.Unlock() }
	return api.HistoryContent{ReadCloser: releasing{reader, done}}, nil
}

func validFileSummary(s string) bool {
	var n int
	var rest string
	if _, err := fmt.Sscanf(s, "%d %s", &n, &rest); err != nil {
		return false
	}
	return n >= 2 && n <= 128 && s == fmt.Sprintf("%d files", n)
}
